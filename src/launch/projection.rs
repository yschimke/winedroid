use std::ffi::c_void;
use std::path::{Path, PathBuf};

use crate::activity_runtime::{settle_runtime_view_frames_extended, ActivityRuntimeWorker};
use crate::android_framework;
use crate::dex_run;
use crate::error::{Result, WinedroidError};
use crate::host_bridge;
use crate::host_window;
use crate::input::InputKey;
use crate::layout_render;
use crate::render_metrics;
use crate::trace;

use super::diagnostics::view_snapshot_detail_lines;

pub(super) struct ActivityHostContext {
    pub(super) activity_runtime: ActivityRuntimeWorker,
    pub(super) apk_path: PathBuf,
    pub(super) resources: Option<Vec<u8>>,
    pub(super) package_name: String,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) unsafe fn click_activity_runtime(context: *const c_void, view_id: i32) -> Result<i32> {
    let activity_runtime = activity_runtime_from_context(context);
    let handled = activity_runtime.click_view_id(view_id)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn click_activity_preference(
    context: *const c_void,
    preference_key: &str,
) -> Result<i32> {
    let activity_runtime = activity_runtime_from_context(context);
    let handled = activity_runtime.click_preference(preference_key)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn click_activity_touch(context: *const c_void, x: u32, y: u32) -> Result<i32> {
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let handled = activity_runtime.click_touch(x, android_touch_coordinate_y(y))?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn touch_activity_hold(
    context: *const c_void,
    x: u32,
    y: u32,
    hold_ms: u64,
) -> Result<i32> {
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let handled = activity_runtime.touch_hold(x, android_touch_coordinate_y(y), hold_ms)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn touch_activity_swipe(
    context: *const c_void,
    start: (u32, u32),
    end: (u32, u32),
    duration_ms: u64,
    steps: Option<u32>,
) -> Result<i32> {
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let handled = activity_runtime.touch_swipe(
        (start.0, android_touch_coordinate_y(start.1)),
        (end.0, android_touch_coordinate_y(end.1)),
        duration_ms,
        steps,
    )?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn rotary_activity_scroll(
    context: *const c_void,
    axis_scroll: f32,
) -> Result<i32> {
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let handled = activity_runtime.rotary_scroll(axis_scroll)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

fn android_touch_coordinate_y(y: u32) -> u32 {
    y
}

pub(super) unsafe fn focus_activity_view(context: *const c_void, view_id: i32) -> Result<i32> {
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let handled = activity_runtime.focus_view(view_id)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn key_activity_input(
    context: *const c_void,
    characters: &str,
    key: InputKey,
) -> Result<i32> {
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let handled = activity_runtime.key_input(characters, key)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn click_activity_dialog_button(
    context: *const c_void,
    dialog_index: usize,
    button_index: usize,
) -> Result<i32> {
    let activity_runtime = activity_runtime_from_context(context);
    let handled = activity_runtime.click_dialog_button(dialog_index, button_index)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) unsafe fn click_activity_dialog_item(
    context: *const c_void,
    dialog_index: usize,
    item_index: usize,
) -> Result<i32> {
    let activity_runtime = activity_runtime_from_context(context);
    let handled = activity_runtime.click_dialog_item(dialog_index, item_index)?;
    run_pending_activity_host_actions(activity_runtime)?;

    Ok(handled)
}

pub(super) fn run_pending_activity_host_actions(
    activity_runtime: &ActivityRuntimeWorker,
) -> Result<()> {
    let actions = activity_runtime.take_host_actions()?;

    host_bridge::run_host_actions(&actions)
}

pub(super) unsafe fn read_activity_view_text(
    context: *const c_void,
    view_id: i32,
) -> Result<Option<String>> {
    let activity_runtime = activity_runtime_from_context(context);
    activity_runtime.view_text(view_id)
}

pub(super) unsafe fn read_activity_surface(
    context: *const c_void,
) -> Result<host_window::ActivitySurfaceState> {
    // cite: frameworks-base ViewRootImpl.java#scheduleTraversals, doTraversal
    build_activity_surface(context)
}

pub(super) unsafe fn read_activity_overlays(
    context: *const c_void,
) -> Result<Vec<host_window::ActivityOverlay>> {
    let activity_runtime = activity_runtime_from_context(context);
    Ok(activity_runtime
        .activity_overlays()?
        .into_iter()
        .map(activity_overlay)
        .collect())
}

pub(super) unsafe fn set_activity_frame_interval(
    context: *const c_void,
    frame_interval_nanos: i64,
) -> Result<()> {
    let activity_runtime = activity_runtime_from_context(context);
    activity_runtime.set_frame_interval_nanos(frame_interval_nanos)
}

const FRAME_BUDGET_NANOS: i64 = 16_666_666;

pub(super) unsafe fn drive_activity_frame(
    context: *const c_void,
    skipped_intervals: i64,
) -> Result<host_window::FrameDriveOutcome> {
    let activity_runtime = activity_runtime_from_context(context);
    activity_runtime.advance_frame_clock(skipped_intervals)?;

    let jank = trace::enabled("jank");
    let frame_start = (jank || render_metrics::enabled()).then(std::time::Instant::now);

    let settle_start = jank.then(std::time::Instant::now);
    let view_changed = settle_activity_frame(context)?;
    let settle_nanos = settle_start.map(|start| start.elapsed().as_nanos() as i64);

    activity_runtime.finish_driven_frame()?;
    let wants_more_frames = activity_runtime.wants_frames()?;
    let main_thread_idle = activity_runtime.main_thread_idle()?;
    let next_wake_delay_millis = activity_runtime.next_wake_delay_millis()?;

    let surface_start = jank.then(std::time::Instant::now);
    let surface = if view_changed {
        Some(build_activity_surface(context)?)
    } else {
        None
    };
    let surface_nanos = surface_start.map(|start| start.elapsed().as_nanos() as i64);

    if let Some(frame_start) = frame_start {
        let total = frame_start.elapsed().as_nanos() as i64;
        let frame_janked = (view_changed && total > FRAME_BUDGET_NANOS) || skipped_intervals > 0;
        render_metrics::record_frame(
            Some(frame_start),
            skipped_intervals,
            view_changed,
            frame_janked,
        );
        if frame_janked {
            let over = total as f64 / FRAME_BUDGET_NANOS as f64;
            if jank {
                trace::event(
                    "jank",
                    format!(
                        "frame {:.2}ms ({over:.1}x budget) settle={:.2}ms surface={:.2}ms \
                     dropped_before={skipped_intervals}",
                        total as f64 / 1_000_000.0,
                        settle_nanos.unwrap_or(0) as f64 / 1_000_000.0,
                        surface_nanos.unwrap_or(0) as f64 / 1_000_000.0,
                    ),
                );
            }
        }
    }

    Ok(host_window::FrameDriveOutcome {
        surface,
        wants_more_frames,
        main_thread_idle,
        next_wake_delay_millis,
    })
}

unsafe fn settle_activity_frame(context: *const c_void) -> Result<bool> {
    let _span = trace::span("ui", "refresh runtime view frames");
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let view_changed =
        settle_runtime_view_frames_extended(activity_runtime, context.width, context.height)?;
    activity_runtime.dispatch_view_tree_draw_listeners()?;
    Ok(view_changed)
}

unsafe fn build_activity_surface(
    context: *const c_void,
) -> Result<host_window::ActivitySurfaceState> {
    let render_metric_start = render_metrics::start_timing();
    let _span = trace::span("render", "refresh activity surface");
    let context = &*(context as *const ActivityHostContext);
    let activity_runtime = &context.activity_runtime;
    let activity_start = {
        let _span = trace::span("runtime", "read refreshed activity snapshot");
        activity_runtime.activity_start()?
    };
    let dialog_overlays = activity_runtime.dialog_overlays()?;
    let dialog_content_layouts = activity_runtime.dialog_content_layouts()?;
    let activity_view_snapshot = activity_snapshot_without_dialog_layouts(
        &activity_start.view_snapshot,
        &dialog_overlays,
        &dialog_content_layouts,
    );
    let floating_panel = floating_activity_panel(
        &context.apk_path,
        context.resources.as_deref(),
        &activity_view_snapshot,
        context.width,
        context.height,
    );
    let (layout_width, layout_height) = match floating_panel {
        Some(panel) => (panel.width, panel.height),
        None => (context.width, context.height),
    };

    let layout_summary = {
        let _span = trace::span("render", "summarize refreshed activity layouts");
        layout_render::summarize_activity_layouts(
            &context.apk_path,
            &activity_view_snapshot,
            layout_width,
            layout_height,
        )?
    };
    register_declared_on_click_handlers(activity_runtime, &layout_summary)?;
    let mut text_elements = activity_text_elements(&layout_summary, layout_width, layout_height);
    refresh_activity_text_elements(activity_runtime, &mut text_elements)?;
    apply_activity_image_resources(
        &context.apk_path,
        &activity_view_snapshot,
        &mut text_elements,
    )?;
    apply_activity_text_cursors(&activity_view_snapshot, &mut text_elements);
    normalize_activity_text_projection(&mut text_elements, layout_width, layout_height);
    expand_scrollable_text_elements(
        &mut text_elements,
        &layout_summary.scrollable_view_ids,
        layout_height,
    );
    let preference_snapshot = activity_runtime.preference_snapshot()?;
    let activity_theme = layout_render::layout_theme_from_snapshot(&activity_view_snapshot);
    append_preference_text_elements(
        &mut text_elements,
        &preference_snapshot,
        &context.apk_path,
        &activity_theme,
        layout_width,
        layout_height,
    )?;
    append_runtime_view_root_elements(
        &mut text_elements,
        &activity_view_snapshot,
        layout_width,
        layout_height,
    );
    let dialogs = activity_dialog_overlays(
        &context.apk_path,
        &activity_start.view_snapshot,
        &dialog_overlays,
        layout_width,
        layout_height,
    )?;

    let action_bar = if floating_panel.is_some() {
        None
    } else {
        build_activity_action_bar(
            &context.apk_path,
            context.resources.as_deref(),
            &activity_view_snapshot,
        )?
    };
    if action_bar.as_ref().is_some_and(|bar| bar.is_decor) {
        inset_elements_below_action_bar(&mut text_elements, host_window::ACTION_BAR_HEIGHT);
    }

    let mut overlays =
        read_activity_overlays(context as *const ActivityHostContext as *const c_void)?;
    if let Some(overflow) =
        action_bar_overflow_overlay(&activity_view_snapshot, action_bar.as_ref())
    {
        overlays.push(overflow);
    }

    if let Some(panel) = floating_panel {
        offset_elements_into_panel(&mut text_elements, panel, context.height);
        offset_overlays_into_panel(&mut overlays, panel);
    }

    let surface = host_window::ActivitySurfaceState {
        message: active_activity_message(&context.package_name, &activity_start, &layout_summary),
        text_elements,
        runtime_canvas_frames: activity_runtime_canvas_frames(&activity_start.view_snapshot),
        overlays,
        dialogs,
        action_bar,
        floating_panel,
        window_background: activity_window_background_color(
            &context.apk_path,
            context.resources.as_deref(),
            &activity_view_snapshot,
        ),
        resource_id_names: layout_render::cached_resource_id_names(&context.apk_path)?
            .as_ref()
            .clone(),
    };
    render_metrics::record_surface_read_elapsed(render_metric_start);
    Ok(surface)
}

fn floating_activity_panel(
    apk_path: &Path,
    resources: Option<&[u8]>,
    snapshot: &dex_run::DexViewSnapshot,
    window_width: u32,
    window_height: u32,
) -> Option<host_window::ActivityRect> {
    let is_floating = resources
        .map(|resources| layout_render::activity_is_floating_from_resources(resources, snapshot))
        .unwrap_or_else(|| layout_render::activity_is_floating(apk_path, snapshot));
    if !is_floating {
        return None;
    }

    let horizontal_margin = 24.min(window_width / 8);
    let vertical_margin = 72.min(window_height / 6);
    Some(host_window::ActivityRect {
        x: horizontal_margin,
        y: vertical_margin,
        width: window_width.saturating_sub(horizontal_margin * 2).max(1),
        height: window_height.saturating_sub(vertical_margin * 2).max(1),
    })
}

fn offset_elements_into_panel(
    elements: &mut [host_window::ActivityTextElement],
    panel: host_window::ActivityRect,
    window_height: u32,
) {
    let offset_y = i64::from(window_height)
        .saturating_sub(i64::from(panel.y))
        .saturating_sub(i64::from(panel.height)) as i32;
    for element in elements {
        element.x = element.x.saturating_add(panel.x);
        element.y = element.y.saturating_add(offset_y);
        if let Some(clip) = &mut element.clip {
            clip.x = clip.x.saturating_add(panel.x);
            clip.y = (clip.y as i64 + i64::from(offset_y)).max(0) as u32;
        }
    }
}

fn offset_overlays_into_panel(
    overlays: &mut [host_window::ActivityOverlay],
    _panel: host_window::ActivityRect,
) {
    let _ = overlays;
}

pub(super) fn activity_snapshot_without_dialog_layouts(
    snapshot: &dex_run::DexViewSnapshot,
    dialogs: &[dex_run::DexDialogOverlay],
    dialog_content_layouts: &[i32],
) -> dex_run::DexViewSnapshot {
    let mut dialog_layouts = dialogs
        .iter()
        .flat_map(|dialog| dialog.content_layouts.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    dialog_layouts.extend(dialog_content_layouts.iter().copied());

    if dialog_layouts.is_empty() {
        return snapshot.clone();
    }

    let mut filtered = snapshot.clone();
    filtered
        .content_layouts
        .retain(|layout_id| !dialog_layouts.contains(layout_id));
    filtered
        .inflated_layouts
        .retain(|layout_id| !dialog_layouts.contains(layout_id));
    filtered
}

pub(super) unsafe fn activity_runtime_from_context<'a>(
    context: *const c_void,
) -> &'a ActivityRuntimeWorker {
    let context = &*(context as *const ActivityHostContext);
    &context.activity_runtime
}

pub(super) fn activity_overlay(
    overlay: dex_run::DexActivityOverlay,
) -> host_window::ActivityOverlay {
    host_window::ActivityOverlay {
        anchor_view_id: overlay.anchor_view_id,
        items: overlay
            .items
            .into_iter()
            .map(|item| host_window::ActivityOverlayItem {
                item_id: item.item_id,
                text: item.text,
                enabled: item.enabled,
            })
            .collect(),
    }
}

pub(super) fn activity_dialog_overlays(
    apk_path: &Path,
    activity_snapshot: &dex_run::DexViewSnapshot,
    dialogs: &[dex_run::DexDialogOverlay],
    window_width: u32,
    window_height: u32,
) -> Result<Vec<host_window::ActivityDialogOverlay>> {
    let content_width = window_width.saturating_sub(96).max(160);
    let content_height = window_height.saturating_sub(260).max(120);
    let mut overlays = Vec::new();

    for dialog in dialogs {
        let dialog_snapshot = dex_run::DexViewSnapshot {
            content_layouts: dialog.content_layouts.clone(),
            inflated_layouts: Vec::new(),
            theme_styles: activity_snapshot.theme_styles.clone(),
            resource_sdk_version: activity_snapshot.resource_sdk_version,
            resource_ui_mode: activity_snapshot.resource_ui_mode,
            resource_display_width: activity_snapshot.resource_display_width,
            resource_display_height: activity_snapshot.resource_display_height,
            resource_screen_round: activity_snapshot.resource_screen_round,
            runtime_view_roots: Vec::new(),
            runtime_canvas_frames: Vec::new(),
            view_lookup_ids: Vec::new(),
            view_texts: activity_snapshot.view_texts.clone(),
            view_text_cursors: activity_snapshot.view_text_cursors.clone(),
            view_images: activity_snapshot.view_images.clone(),
            view_visibilities: activity_snapshot.view_visibilities.clone(),
            click_listeners: activity_snapshot.click_listeners.clone(),
            declared_on_click_handlers: activity_snapshot.declared_on_click_handlers.clone(),
            adapter_rows: Vec::new(),
            dynamic_children: Vec::new(),
            activity_overlays: Vec::new(),
            dialog_overlays: Vec::new(),
            action_bar_title: None,
            action_bar_nav_icon: None,
            action_bar_overflow_open: false,
            action_bar_present: false,
            action_bar_home_as_up: false,
            options_menu: Vec::new(),
        };
        let summary = layout_render::summarize_activity_layouts(
            apk_path,
            &dialog_snapshot,
            content_width,
            content_height,
        )?;
        let mut text_elements = activity_text_elements(&summary, content_width, content_height);
        apply_activity_image_resources(apk_path, &dialog_snapshot, &mut text_elements)?;
        apply_activity_text_cursors(&dialog_snapshot, &mut text_elements);

        overlays.push(host_window::ActivityDialogOverlay {
            title: dialog.title.clone(),
            content_layouts: dialog.content_layouts.clone(),
            text_elements,
            items: dialog
                .items
                .iter()
                .map(|item| host_window::ActivityDialogItem {
                    text: item.text.clone(),
                    enabled: item.enabled,
                    selected: item.selected,
                })
                .collect(),
            item_scroll_offset: 0,
            buttons: dialog
                .buttons
                .iter()
                .map(|button| host_window::ActivityDialogButton {
                    text: button.text.clone(),
                    enabled: button.enabled,
                })
                .collect(),
        });
    }

    Ok(overlays)
}

pub(super) fn expand_scrollable_text_elements(
    elements: &mut [host_window::ActivityTextElement],
    scrollable_view_ids: &std::collections::BTreeSet<u32>,
    window_height: u32,
) {
    if scrollable_view_ids.is_empty() {
        return;
    }

    for element in elements.iter_mut() {
        if element.kind != host_window::ActivityTextKind::Label
            || element.clip.is_some()
            || element.text.trim().is_empty()
        {
            continue;
        }
        let Some(view_id) = element.view_id.and_then(|id| u32::try_from(id).ok()) else {
            continue;
        };
        if !scrollable_view_ids.contains(&view_id) {
            continue;
        }

        let content_height = crate::skia_renderer::measure_label_text_height(
            &element.text,
            element.width,
            crate::skia_renderer::SCROLLABLE_TEXT_FONT_SIZE,
        );
        let top = i64::from(window_height)
            .saturating_sub(i64::from(element.y))
            .saturating_sub(i64::from(element.height))
            .max(0) as u32;
        let viewport_height = window_height.saturating_sub(top);
        if content_height <= viewport_height {
            continue;
        }

        element.height = content_height;
        element.y = i64::from(window_height)
            .saturating_sub(i64::from(content_height))
            .saturating_sub(i64::from(top))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        element.clip = Some(host_window::ActivityRect {
            x: element.x,
            y: 0,
            width: element.width,
            height: viewport_height,
        });
    }
}

pub(super) fn refresh_activity_text_elements(
    activity_runtime: &ActivityRuntimeWorker,
    elements: &mut [host_window::ActivityTextElement],
) -> Result<()> {
    for element in elements {
        let Some(view_id) = element.view_id else {
            continue;
        };
        let Some(text) = activity_runtime.view_text(view_id)? else {
            continue;
        };

        element.text = text;
    }

    Ok(())
}

pub(super) fn apply_activity_image_resources(
    apk_path: &Path,
    snapshot: &dex_run::DexViewSnapshot,
    elements: &mut [host_window::ActivityTextElement],
) -> Result<()> {
    let theme = layout_render::layout_theme_from_snapshot(snapshot);
    for image in &snapshot.view_images {
        let drawable =
            layout_render::load_drawable_resource(apk_path, image.drawable_resource_id, &theme)?;
        let Some(element) = elements
            .iter_mut()
            .find(|element| element.view_id == Some(image.view_id))
        else {
            continue;
        };

        element.text.clear();
        element.image = Some(activity_image(&drawable, 0));
    }

    Ok(())
}

pub(super) fn apply_activity_text_cursors(
    snapshot: &dex_run::DexViewSnapshot,
    elements: &mut [host_window::ActivityTextElement],
) {
    for cursor in &snapshot.view_text_cursors {
        let Some(element) = elements
            .iter_mut()
            .find(|element| element.view_id == Some(cursor.view_id) && element.editable)
        else {
            continue;
        };

        element.cursor = Some(host_window::ActivityTextCursor {
            selection_start: cursor.selection_start,
            selection_end: cursor.selection_end,
            visible: cursor.cursor_visible,
        });
    }
}

pub(super) fn normalize_activity_text_projection(
    elements: &mut [host_window::ActivityTextElement],
    window_width: u32,
    window_height: u32,
) {
    normalize_navigation_toolbar(elements, window_width, window_height);
    promote_compound_label_rows(elements, window_width);
}

pub(super) fn normalize_navigation_toolbar(
    elements: &mut [host_window::ActivityTextElement],
    window_width: u32,
    window_height: u32,
) {
    let Some(title_index) = navigation_title_element_index(elements, window_height) else {
        return;
    };
    let Some(up_index) = navigation_up_element_index(elements, title_index, window_height) else {
        return;
    };

    let toolbar_height = 56;
    let toolbar_y = host_y_from_top(window_height, 0, toolbar_height);
    let title_width = window_width.saturating_sub(60);

    let up_element = &mut elements[up_index];
    up_element.kind = host_window::ActivityTextKind::Label;
    up_element.x = 0;
    up_element.y = toolbar_y;
    up_element.width = 48;
    up_element.height = toolbar_height;
    up_element.alignment = host_window::ActivityTextAlignment::Center;

    let title_element = &mut elements[title_index];
    title_element.kind = host_window::ActivityTextKind::Label;
    title_element.x = 52;
    title_element.y = toolbar_y;
    title_element.width = title_width;
    title_element.height = toolbar_height;
    title_element.alignment = host_window::ActivityTextAlignment::Start;
}

pub(super) fn navigation_title_element_index(
    elements: &[host_window::ActivityTextElement],
    window_height: u32,
) -> Option<usize> {
    elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            element.kind == host_window::ActivityTextKind::Label
                && element.image.is_none()
                && !element.text.trim().is_empty()
                && screen_top_from_host_y(window_height, element.y, element.height) < 140
        })
        .min_by_key(|(_, element)| screen_top_from_host_y(window_height, element.y, element.height))
        .map(|(index, _)| index)
}

pub(super) fn navigation_up_element_index(
    elements: &[host_window::ActivityTextElement],
    title_index: usize,
    window_height: u32,
) -> Option<usize> {
    let title = &elements[title_index];
    let title_center_y = host_center_y(title);

    elements
        .iter()
        .enumerate()
        .filter(|(index, element)| {
            *index != title_index
                && element.kind == host_window::ActivityTextKind::Label
                && element.image.is_some()
                && element.text.trim().is_empty()
                && element.width.saturating_mul(2) <= element.height.saturating_mul(3)
                && element.x < title.x
                && screen_top_from_host_y(window_height, element.y, element.height) < 160
                && host_center_y(element).saturating_sub(title_center_y).abs() <= 72
        })
        .min_by_key(|(_, element)| element.x)
        .map(|(index, _)| index)
}

pub(super) fn promote_compound_label_rows(
    elements: &mut [host_window::ActivityTextElement],
    window_width: u32,
) {
    let row_width = window_width.saturating_sub(48);
    if row_width == 0 {
        return;
    }

    for element in elements {
        if !is_compound_label_row(element) {
            continue;
        }

        let row_height = 48;
        let center_y = host_center_y(element);

        element.kind = host_window::ActivityTextKind::ListItem;
        element.x = 24;
        element.y = center_y.saturating_sub(i32::try_from(row_height / 2).unwrap_or(0));
        element.width = row_width;
        element.height = row_height;
        element.alignment = host_window::ActivityTextAlignment::Start;
    }
}

pub(super) fn is_compound_label_row(element: &host_window::ActivityTextElement) -> bool {
    element.kind == host_window::ActivityTextKind::Label
        && element.image.is_some()
        && !element.text.trim().is_empty()
        && element.preference_key.is_none()
}

pub(super) fn host_center_y(element: &host_window::ActivityTextElement) -> i32 {
    element
        .y
        .saturating_add(i32::try_from(element.height / 2).unwrap_or(0))
}

pub(super) fn active_activity_message(
    package_name: &str,
    activity_start: &dex_run::DexActivityStart,
    summary: &layout_render::LayoutRenderSummary,
) -> String {
    let mut lines = vec![
        package_name.to_owned(),
        activity_start.class_descriptor.clone(),
        "managed onCreate completed".to_owned(),
    ];
    lines.extend(view_snapshot_detail_lines(&activity_start.view_snapshot));
    lines.extend(summary.lines.clone());
    lines.join("\n")
}

pub(super) fn register_declared_on_click_handlers(
    activity_runtime: &ActivityRuntimeWorker,
    summary: &layout_render::LayoutRenderSummary,
) -> Result<()> {
    for element in &summary.text_elements {
        let Some(view_id) = activity_view_id(element.view_id) else {
            continue;
        };
        let Some(method_name) = &element.on_click else {
            continue;
        };

        activity_runtime.register_declared_on_click_handler(view_id, method_name)?;
    }

    Ok(())
}

pub(super) fn register_layout_text_values(
    activity_runtime: &ActivityRuntimeWorker,
    summary: &layout_render::LayoutRenderSummary,
) -> Result<()> {
    for element in &summary.text_elements {
        // cite: frameworks-base TextView.java#getText
        if element.text.is_empty() {
            continue;
        }

        let Some(view_id) = activity_view_id(element.view_id) else {
            continue;
        };

        activity_runtime.register_view_text(view_id, &element.text)?;
    }

    Ok(())
}

pub(super) fn register_layout_view_visibilities(
    activity_runtime: &ActivityRuntimeWorker,
    summary: &layout_render::LayoutRenderSummary,
) -> Result<()> {
    for visibility in &summary.view_visibilities {
        let view_id = i32::try_from(visibility.view_id).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "layout view visibility ID does not fit Android int\nview id: {}",
                    visibility.view_id
                ),
                error,
            )
        })?;

        activity_runtime.register_view_visibility(view_id, visibility.visibility)?;
    }

    Ok(())
}

pub(super) fn activity_text_elements(
    summary: &layout_render::LayoutRenderSummary,
    window_width: u32,
    window_height: u32,
) -> Vec<host_window::ActivityTextElement> {
    let mut elements = Vec::new();
    let row_height = 34;
    let label_height = 26;
    let top_margin = 44;
    let side_margin = 24;

    for source in &summary.text_elements {
        if let Some(frame) = source.frame {
            if frame.width != 0 && frame.height != 0 {
                let y = host_y_from_top(window_height, frame.y, frame.height);

                elements.push(host_window::ActivityTextElement {
                    text: source.text.clone(),
                    kind: activity_text_kind(source.kind),
                    view_id: activity_view_id(source.view_id),
                    preference_key: None,
                    accessory: None,
                    image: source
                        .drawable
                        .as_ref()
                        .map(|drawable| activity_image(drawable, source.drawable_padding)),
                    text_size: source.text_size,
                    text_padding_left: source.text_padding_left,
                    text_padding_right: source.text_padding_right,
                    text_color: source.text_color,
                    background_color: source.background_color,
                    background_corner_radius: source.background_corner_radius,
                    background_border_color: source.background_border_color,
                    background_border_width: source.background_border_width,
                    elevation_px: source.elevation_px,
                    alignment: activity_text_alignment(source.alignment),
                    editable: source.editable,
                    cursor: None,
                    x: frame.x,
                    y,
                    width: frame.width,
                    height: frame.height,
                    clip: source
                        .clip
                        .map(|clip| layout_frame_to_host_clip(clip, window_height)),
                });
                continue;
            }
        }

        let row = elements.len() as u32;
        let Some(bottom_y) = window_height
            .checked_sub(top_margin)
            .and_then(|value| value.checked_sub((row + 1) * row_height))
        else {
            break;
        };

        if bottom_y < side_margin {
            break;
        }

        let x = side_margin + (source.depth as u32).saturating_mul(8).min(72);
        let width = window_width.saturating_sub(x + side_margin);

        if width == 0 {
            break;
        }

        elements.push(host_window::ActivityTextElement {
            text: source.text.clone(),
            kind: activity_text_kind(source.kind),
            view_id: activity_view_id(source.view_id),
            preference_key: None,
            accessory: None,
            image: source
                .drawable
                .as_ref()
                .map(|drawable| activity_image(drawable, source.drawable_padding)),
            text_size: source.text_size,
            text_padding_left: source.text_padding_left,
            text_padding_right: source.text_padding_right,
            text_color: source.text_color,
            background_color: source.background_color,
            background_corner_radius: source.background_corner_radius,
            background_border_color: source.background_border_color,
            background_border_width: source.background_border_width,
            elevation_px: source.elevation_px,
            alignment: activity_text_alignment(source.alignment),
            editable: source.editable,
            cursor: None,
            x,
            y: i32::try_from(bottom_y).unwrap_or(i32::MAX),
            width,
            height: label_height,
            clip: None,
        });
    }

    elements
}

pub(super) fn append_preference_text_elements(
    elements: &mut Vec<host_window::ActivityTextElement>,
    preferences: &[dex_run::DexPreferenceSnapshot],
    apk_path: &Path,
    theme: &layout_render::LayoutTheme,
    window_width: u32,
    window_height: u32,
) -> Result<()> {
    if preferences.is_empty() {
        return Ok(());
    }

    let bottom_margin = 28;
    let mut top_y = preference_row_start_top(elements, window_height);
    let viewport = preference_row_viewport(top_y, bottom_margin, window_width, window_height);
    let colors = preference_row_colors(elements, window_width, window_height);

    for preference in preferences {
        if preference.title.trim().is_empty() {
            continue;
        }

        let row_height = if preference.summary.is_some() { 58 } else { 42 };
        let x = 0;
        let width = window_width;
        let image = preference_row_image(apk_path, preference, theme, colors.text)?;

        elements.push(host_window::ActivityTextElement {
            text: preference_row_text(preference),
            kind: host_window::ActivityTextKind::ListItem,
            view_id: None,
            preference_key: preference_row_click_key(preference),
            accessory: preference_row_accessory(preference),
            image,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: colors.text,
            background_color: colors.background,
            alignment: host_window::ActivityTextAlignment::Start,
            editable: false,
            cursor: None,
            x,
            y: host_y_from_top(window_height, top_y, row_height),
            width,
            height: row_height,
            clip: Some(viewport),
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        });

        top_y += row_height;
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PreferenceRowColors {
    background: Option<u32>,
    text: Option<u32>,
}

fn preference_row_colors(
    elements: &[host_window::ActivityTextElement],
    window_width: u32,
    window_height: u32,
) -> PreferenceRowColors {
    PreferenceRowColors {
        background: dominant_background_color(elements),
        text: foreground_text_color(elements, window_width, window_height),
    }
}

fn dominant_background_color(elements: &[host_window::ActivityTextElement]) -> Option<u32> {
    elements
        .iter()
        .filter(|element| is_projected_background_element(element))
        .filter_map(|element| {
            let color = element.background_color?;
            let area = u64::from(element.width) * u64::from(element.height);
            Some((area, color))
        })
        .max_by_key(|(area, _color)| *area)
        .map(|(_area, color)| color)
}

fn is_projected_background_element(element: &host_window::ActivityTextElement) -> bool {
    element.kind == host_window::ActivityTextKind::Label
        && element.text.is_empty()
        && element.image.is_none()
        && element.accessory.is_none()
        && element.cursor.is_none()
}

fn foreground_text_color(
    elements: &[host_window::ActivityTextElement],
    window_width: u32,
    window_height: u32,
) -> Option<u32> {
    elements
        .iter()
        .filter(|element| !element.text.trim().is_empty())
        .filter_map(|element| {
            let color = element.text_color?;
            let top = screen_top_from_host_y(window_height, element.y, element.height);
            let distance_from_left = element.x.min(window_width.saturating_sub(element.x));
            Some((top, distance_from_left, color))
        })
        .min_by_key(|(top, distance_from_left, _color)| (*top, *distance_from_left))
        .map(|(_top, _distance_from_left, color)| color)
}

pub(super) fn append_runtime_view_root_elements(
    elements: &mut Vec<host_window::ActivityTextElement>,
    snapshot: &dex_run::DexViewSnapshot,
    window_width: u32,
    window_height: u32,
) {
    if !elements.is_empty() {
        return;
    }

    let Some(root) = snapshot.runtime_view_roots.last() else {
        return;
    };

    elements.push(host_window::ActivityTextElement {
        text: runtime_view_root_text(root),
        kind: host_window::ActivityTextKind::RuntimeViewRoot,
        view_id: None,
        preference_key: None,
        accessory: None,
        image: None,
        text_size: None,
        text_padding_left: 0,
        text_padding_right: 0,
        text_color: None,
        background_color: None,
        alignment: host_window::ActivityTextAlignment::Start,
        editable: false,
        cursor: None,
        x: 0,
        y: 0,
        width: window_width,
        height: window_height,
        clip: None,
        background_corner_radius: 0,
        background_border_color: None,
        background_border_width: 0,
        elevation_px: 0,
    });
}

pub(super) fn activity_runtime_canvas_frames(
    snapshot: &dex_run::DexViewSnapshot,
) -> Vec<host_window::ActivityRuntimeCanvasFrame> {
    snapshot
        .runtime_canvas_frames
        .iter()
        .map(|frame| host_window::ActivityRuntimeCanvasFrame {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
            ops: frame.ops.iter().map(activity_runtime_canvas_op).collect(),
        })
        .collect()
}

pub(super) fn activity_runtime_canvas_op(
    op: &dex_run::DexRuntimeCanvasOp,
) -> host_window::ActivityRuntimeCanvasOp {
    match op {
        dex_run::DexRuntimeCanvasOp::Save => host_window::ActivityRuntimeCanvasOp::Save,
        dex_run::DexRuntimeCanvasOp::Restore => host_window::ActivityRuntimeCanvasOp::Restore,
        dex_run::DexRuntimeCanvasOp::Translate { dx, dy } => {
            host_window::ActivityRuntimeCanvasOp::Translate { dx: *dx, dy: *dy }
        }
        dex_run::DexRuntimeCanvasOp::Scale { sx, sy } => {
            host_window::ActivityRuntimeCanvasOp::Scale { sx: *sx, sy: *sy }
        }
        dex_run::DexRuntimeCanvasOp::Rotate { degrees } => {
            host_window::ActivityRuntimeCanvasOp::Rotate { degrees: *degrees }
        }
        dex_run::DexRuntimeCanvasOp::ConcatMatrix { values } => {
            host_window::ActivityRuntimeCanvasOp::ConcatMatrix { values: *values }
        }
        dex_run::DexRuntimeCanvasOp::ClipRect {
            left,
            top,
            right,
            bottom,
            op,
        } => host_window::ActivityRuntimeCanvasOp::ClipRect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            op: activity_runtime_canvas_clip_op(op),
        },
        dex_run::DexRuntimeCanvasOp::ClipPath { ops, op } => {
            host_window::ActivityRuntimeCanvasOp::ClipPath {
                ops: ops.iter().map(activity_runtime_canvas_path_op).collect(),
                op: activity_runtime_canvas_clip_op(op),
            }
        }
        dex_run::DexRuntimeCanvasOp::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            blend_mode,
            stroke_width,
            stroke_cap,
        } => host_window::ActivityRuntimeCanvasOp::Line {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
            color: *color,
            blend_mode: activity_runtime_canvas_blend_mode(blend_mode),
            stroke_width: *stroke_width,
            stroke_cap: activity_runtime_canvas_stroke_cap(stroke_cap),
        },
        dex_run::DexRuntimeCanvasOp::Rect {
            left,
            top,
            right,
            bottom,
            color,
            blend_mode,
            paint_style,
        } => host_window::ActivityRuntimeCanvasOp::Rect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            color: *color,
            blend_mode: activity_runtime_canvas_blend_mode(blend_mode),
            paint_style: activity_runtime_canvas_paint_style(paint_style),
        },
        dex_run::DexRuntimeCanvasOp::Circle {
            cx,
            cy,
            radius,
            color,
            blend_mode,
            paint_style,
            stroke_width,
        } => host_window::ActivityRuntimeCanvasOp::Circle {
            cx: *cx,
            cy: *cy,
            radius: *radius,
            color: *color,
            blend_mode: activity_runtime_canvas_blend_mode(blend_mode),
            paint_style: activity_runtime_canvas_paint_style(paint_style),
            stroke_width: *stroke_width,
        },
        dex_run::DexRuntimeCanvasOp::RoundRect {
            left,
            top,
            right,
            bottom,
            rx,
            ry,
            color,
            blend_mode,
            paint_style,
        } => host_window::ActivityRuntimeCanvasOp::RoundRect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            rx: *rx,
            ry: *ry,
            color: *color,
            blend_mode: activity_runtime_canvas_blend_mode(blend_mode),
            paint_style: activity_runtime_canvas_paint_style(paint_style),
        },
        dex_run::DexRuntimeCanvasOp::Path {
            ops,
            color,
            blend_mode,
            paint_style,
        } => host_window::ActivityRuntimeCanvasOp::Path {
            ops: ops.iter().map(activity_runtime_canvas_path_op).collect(),
            color: *color,
            blend_mode: activity_runtime_canvas_blend_mode(blend_mode),
            paint_style: activity_runtime_canvas_paint_style(paint_style),
        },
        dex_run::DexRuntimeCanvasOp::Text {
            text,
            x,
            y,
            color,
            text_size,
        } => host_window::ActivityRuntimeCanvasOp::Text {
            text: text.clone(),
            x: *x,
            y: *y,
            color: *color,
            text_size: *text_size,
        },
        dex_run::DexRuntimeCanvasOp::Bitmap {
            left,
            top,
            right,
            bottom,
            source_left,
            source_top,
            source_right,
            source_bottom,
            bitmap_width,
            bitmap_height,
            color,
            blend_mode,
            color_filter,
            alpha_mask,
            encoded_image,
            ops,
        } => host_window::ActivityRuntimeCanvasOp::Bitmap {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            source_left: *source_left,
            source_top: *source_top,
            source_right: *source_right,
            source_bottom: *source_bottom,
            bitmap_width: *bitmap_width,
            bitmap_height: *bitmap_height,
            color: *color,
            blend_mode: activity_runtime_canvas_blend_mode(blend_mode),
            color_filter: color_filter
                .as_ref()
                .map(activity_runtime_canvas_color_filter),
            alpha_mask: *alpha_mask,
            encoded_image: encoded_image.clone(),
            ops: ops.iter().map(activity_runtime_canvas_op).collect(),
        },
        dex_run::DexRuntimeCanvasOp::RenderNode {
            left,
            top,
            right,
            bottom,
            alpha,
            scale_x,
            scale_y,
            clip_to_bounds,
            ops,
        } => host_window::ActivityRuntimeCanvasOp::RenderNode {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            alpha: *alpha,
            scale_x: *scale_x,
            scale_y: *scale_y,
            clip_to_bounds: *clip_to_bounds,
            ops: ops.iter().map(activity_runtime_canvas_op).collect(),
        },
    }
}

fn activity_runtime_canvas_clip_op(
    op: &dex_run::DexRuntimeCanvasClipOp,
) -> host_window::ActivityRuntimeCanvasClipOp {
    match op {
        dex_run::DexRuntimeCanvasClipOp::Intersect => {
            host_window::ActivityRuntimeCanvasClipOp::Intersect
        }
        dex_run::DexRuntimeCanvasClipOp::Difference => {
            host_window::ActivityRuntimeCanvasClipOp::Difference
        }
    }
}

fn activity_runtime_canvas_blend_mode(
    blend_mode: &dex_run::DexRuntimeCanvasBlendMode,
) -> host_window::ActivityRuntimeCanvasBlendMode {
    match blend_mode {
        dex_run::DexRuntimeCanvasBlendMode::Clear => {
            host_window::ActivityRuntimeCanvasBlendMode::Clear
        }
        dex_run::DexRuntimeCanvasBlendMode::Src => host_window::ActivityRuntimeCanvasBlendMode::Src,
        dex_run::DexRuntimeCanvasBlendMode::Dst => host_window::ActivityRuntimeCanvasBlendMode::Dst,
        dex_run::DexRuntimeCanvasBlendMode::SrcOver => {
            host_window::ActivityRuntimeCanvasBlendMode::SrcOver
        }
        dex_run::DexRuntimeCanvasBlendMode::DstOver => {
            host_window::ActivityRuntimeCanvasBlendMode::DstOver
        }
        dex_run::DexRuntimeCanvasBlendMode::SrcIn => {
            host_window::ActivityRuntimeCanvasBlendMode::SrcIn
        }
        dex_run::DexRuntimeCanvasBlendMode::DstIn => {
            host_window::ActivityRuntimeCanvasBlendMode::DstIn
        }
        dex_run::DexRuntimeCanvasBlendMode::SrcOut => {
            host_window::ActivityRuntimeCanvasBlendMode::SrcOut
        }
        dex_run::DexRuntimeCanvasBlendMode::DstOut => {
            host_window::ActivityRuntimeCanvasBlendMode::DstOut
        }
        dex_run::DexRuntimeCanvasBlendMode::SrcAtop => {
            host_window::ActivityRuntimeCanvasBlendMode::SrcAtop
        }
        dex_run::DexRuntimeCanvasBlendMode::DstAtop => {
            host_window::ActivityRuntimeCanvasBlendMode::DstAtop
        }
        dex_run::DexRuntimeCanvasBlendMode::Xor => host_window::ActivityRuntimeCanvasBlendMode::Xor,
        dex_run::DexRuntimeCanvasBlendMode::Plus => {
            host_window::ActivityRuntimeCanvasBlendMode::Plus
        }
        dex_run::DexRuntimeCanvasBlendMode::Modulate => {
            host_window::ActivityRuntimeCanvasBlendMode::Modulate
        }
        dex_run::DexRuntimeCanvasBlendMode::Screen => {
            host_window::ActivityRuntimeCanvasBlendMode::Screen
        }
        dex_run::DexRuntimeCanvasBlendMode::Overlay => {
            host_window::ActivityRuntimeCanvasBlendMode::Overlay
        }
        dex_run::DexRuntimeCanvasBlendMode::Darken => {
            host_window::ActivityRuntimeCanvasBlendMode::Darken
        }
        dex_run::DexRuntimeCanvasBlendMode::Lighten => {
            host_window::ActivityRuntimeCanvasBlendMode::Lighten
        }
        dex_run::DexRuntimeCanvasBlendMode::ColorDodge => {
            host_window::ActivityRuntimeCanvasBlendMode::ColorDodge
        }
        dex_run::DexRuntimeCanvasBlendMode::ColorBurn => {
            host_window::ActivityRuntimeCanvasBlendMode::ColorBurn
        }
        dex_run::DexRuntimeCanvasBlendMode::HardLight => {
            host_window::ActivityRuntimeCanvasBlendMode::HardLight
        }
        dex_run::DexRuntimeCanvasBlendMode::SoftLight => {
            host_window::ActivityRuntimeCanvasBlendMode::SoftLight
        }
        dex_run::DexRuntimeCanvasBlendMode::Difference => {
            host_window::ActivityRuntimeCanvasBlendMode::Difference
        }
        dex_run::DexRuntimeCanvasBlendMode::Exclusion => {
            host_window::ActivityRuntimeCanvasBlendMode::Exclusion
        }
        dex_run::DexRuntimeCanvasBlendMode::Multiply => {
            host_window::ActivityRuntimeCanvasBlendMode::Multiply
        }
        dex_run::DexRuntimeCanvasBlendMode::Hue => host_window::ActivityRuntimeCanvasBlendMode::Hue,
        dex_run::DexRuntimeCanvasBlendMode::Saturation => {
            host_window::ActivityRuntimeCanvasBlendMode::Saturation
        }
        dex_run::DexRuntimeCanvasBlendMode::Color => {
            host_window::ActivityRuntimeCanvasBlendMode::Color
        }
        dex_run::DexRuntimeCanvasBlendMode::Luminosity => {
            host_window::ActivityRuntimeCanvasBlendMode::Luminosity
        }
    }
}

fn activity_runtime_canvas_color_filter(
    color_filter: &dex_run::DexRuntimeCanvasColorFilter,
) -> host_window::ActivityRuntimeCanvasColorFilter {
    host_window::ActivityRuntimeCanvasColorFilter {
        color: color_filter.color,
        blend_mode: activity_runtime_canvas_blend_mode(&color_filter.blend_mode),
    }
}

fn activity_runtime_canvas_paint_style(
    paint_style: &dex_run::DexRuntimeCanvasPaintStyle,
) -> host_window::ActivityRuntimeCanvasPaintStyle {
    match paint_style {
        dex_run::DexRuntimeCanvasPaintStyle::Fill => {
            host_window::ActivityRuntimeCanvasPaintStyle::Fill
        }
        dex_run::DexRuntimeCanvasPaintStyle::Stroke => {
            host_window::ActivityRuntimeCanvasPaintStyle::Stroke
        }
        dex_run::DexRuntimeCanvasPaintStyle::FillAndStroke => {
            host_window::ActivityRuntimeCanvasPaintStyle::FillAndStroke
        }
    }
}

fn activity_runtime_canvas_stroke_cap(
    stroke_cap: &dex_run::DexRuntimeCanvasStrokeCap,
) -> host_window::ActivityRuntimeCanvasStrokeCap {
    match stroke_cap {
        dex_run::DexRuntimeCanvasStrokeCap::Butt => {
            host_window::ActivityRuntimeCanvasStrokeCap::Butt
        }
        dex_run::DexRuntimeCanvasStrokeCap::Round => {
            host_window::ActivityRuntimeCanvasStrokeCap::Round
        }
        dex_run::DexRuntimeCanvasStrokeCap::Square => {
            host_window::ActivityRuntimeCanvasStrokeCap::Square
        }
    }
}

fn activity_runtime_canvas_path_op(
    op: &dex_run::DexRuntimeCanvasPathOp,
) -> host_window::ActivityRuntimeCanvasPathOp {
    match op {
        dex_run::DexRuntimeCanvasPathOp::MoveTo { x, y } => {
            host_window::ActivityRuntimeCanvasPathOp::MoveTo { x: *x, y: *y }
        }
        dex_run::DexRuntimeCanvasPathOp::LineTo { x, y } => {
            host_window::ActivityRuntimeCanvasPathOp::LineTo { x: *x, y: *y }
        }
        dex_run::DexRuntimeCanvasPathOp::RLineTo { dx, dy } => {
            host_window::ActivityRuntimeCanvasPathOp::RLineTo { dx: *dx, dy: *dy }
        }
        dex_run::DexRuntimeCanvasPathOp::QuadTo { x1, y1, x2, y2 } => {
            host_window::ActivityRuntimeCanvasPathOp::QuadTo {
                x1: *x1,
                y1: *y1,
                x2: *x2,
                y2: *y2,
            }
        }
        dex_run::DexRuntimeCanvasPathOp::CubicTo {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
        } => host_window::ActivityRuntimeCanvasPathOp::CubicTo {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
            x3: *x3,
            y3: *y3,
        },
        dex_run::DexRuntimeCanvasPathOp::RCubicTo {
            dx1,
            dy1,
            dx2,
            dy2,
            dx3,
            dy3,
        } => host_window::ActivityRuntimeCanvasPathOp::RCubicTo {
            dx1: *dx1,
            dy1: *dy1,
            dx2: *dx2,
            dy2: *dy2,
            dx3: *dx3,
            dy3: *dy3,
        },
        dex_run::DexRuntimeCanvasPathOp::BooleanOp { left, right, op } => {
            host_window::ActivityRuntimeCanvasPathOp::BooleanOp {
                left: left.iter().map(activity_runtime_canvas_path_op).collect(),
                right: right.iter().map(activity_runtime_canvas_path_op).collect(),
                op: activity_runtime_canvas_path_boolean_op(op),
            }
        }
        dex_run::DexRuntimeCanvasPathOp::Rect {
            left,
            top,
            right,
            bottom,
            clockwise,
        } => host_window::ActivityRuntimeCanvasPathOp::Rect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            clockwise: *clockwise,
        },
        dex_run::DexRuntimeCanvasPathOp::RoundRect {
            left,
            top,
            right,
            bottom,
            radii,
            clockwise,
        } => host_window::ActivityRuntimeCanvasPathOp::RoundRect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            radii: radii.clone(),
            clockwise: *clockwise,
        },
        dex_run::DexRuntimeCanvasPathOp::Close => host_window::ActivityRuntimeCanvasPathOp::Close,
    }
}

fn activity_runtime_canvas_path_boolean_op(
    op: &dex_run::DexRuntimeCanvasPathBooleanOp,
) -> host_window::ActivityRuntimeCanvasPathBooleanOp {
    match op {
        dex_run::DexRuntimeCanvasPathBooleanOp::Difference => {
            host_window::ActivityRuntimeCanvasPathBooleanOp::Difference
        }
        dex_run::DexRuntimeCanvasPathBooleanOp::Intersect => {
            host_window::ActivityRuntimeCanvasPathBooleanOp::Intersect
        }
        dex_run::DexRuntimeCanvasPathBooleanOp::Union => {
            host_window::ActivityRuntimeCanvasPathBooleanOp::Union
        }
        dex_run::DexRuntimeCanvasPathBooleanOp::Xor => {
            host_window::ActivityRuntimeCanvasPathBooleanOp::Xor
        }
        dex_run::DexRuntimeCanvasPathBooleanOp::ReverseDifference => {
            host_window::ActivityRuntimeCanvasPathBooleanOp::ReverseDifference
        }
    }
}

pub(super) fn runtime_view_root_text(root: &dex_run::DexRuntimeViewRoot) -> String {
    format!("Runtime View root\n{}", root.class_descriptor)
}

fn layout_frame_to_host_clip(
    clip: layout_render::LayoutFrame,
    window_height: u32,
) -> host_window::ActivityRect {
    host_window::ActivityRect {
        x: clip.x,
        y: window_height
            .saturating_sub(clip.y)
            .saturating_sub(clip.height),
        width: clip.width,
        height: clip.height,
    }
}

pub(super) fn host_y_from_top(window_height: u32, top_y: u32, height: u32) -> i32 {
    i64::from(window_height)
        .saturating_sub(i64::from(top_y))
        .saturating_sub(i64::from(height))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

pub(super) fn screen_top_from_host_y(window_height: u32, y: i32, height: u32) -> u32 {
    let top = i64::from(window_height)
        .saturating_sub(i64::from(y))
        .saturating_sub(i64::from(height));

    if top <= 0 {
        return 0;
    }

    u32::try_from(top).unwrap_or(u32::MAX)
}

pub(super) fn preference_row_viewport(
    top_y: u32,
    bottom_margin: u32,
    window_width: u32,
    window_height: u32,
) -> host_window::ActivityRect {
    host_window::ActivityRect {
        x: 0,
        y: bottom_margin,
        width: window_width,
        height: window_height
            .saturating_sub(top_y)
            .saturating_sub(bottom_margin),
    }
}

pub(super) fn preference_row_start_top(
    elements: &[host_window::ActivityTextElement],
    window_height: u32,
) -> u32 {
    let mut bottom = 0;

    for element in elements {
        if !element_reserves_preference_row_space(element) {
            continue;
        }

        let top = screen_top_from_host_y(window_height, element.y, element.height);
        bottom = bottom.max(top.saturating_add(element.height));
    }

    bottom.saturating_add(8)
}

fn element_reserves_preference_row_space(element: &host_window::ActivityTextElement) -> bool {
    !element.text.trim().is_empty()
        || element.image.is_some()
        || element.cursor.is_some()
        || element.editable
        || element.accessory.is_some()
}

pub(super) fn preference_row_text(preference: &dex_run::DexPreferenceSnapshot) -> String {
    if let Some(summary) = &preference.summary {
        return format!("{}\n{}", preference.title, summary);
    }

    if let Some(value) = preference_list_value_label(preference) {
        return format!("{}\n{}", preference.title, value);
    }

    preference.title.clone()
}

pub(super) fn preference_list_value_label(
    preference: &dex_run::DexPreferenceSnapshot,
) -> Option<String> {
    let dex_run::DexPreferenceKind::List {
        value,
        entries,
        entry_values,
    } = &preference.kind
    else {
        return None;
    };
    let value = value.as_deref()?;

    for (entry, entry_value) in entries.iter().zip(entry_values) {
        if entry_value == value {
            return Some(entry.clone());
        }
    }

    Some(value.to_owned())
}

pub(super) fn preference_row_accessory(
    preference: &dex_run::DexPreferenceSnapshot,
) -> Option<host_window::ActivityTextAccessory> {
    match preference.kind {
        dex_run::DexPreferenceKind::Switch { checked } => {
            Some(host_window::ActivityTextAccessory::Switch { checked })
        }
        dex_run::DexPreferenceKind::List { .. } => {
            Some(host_window::ActivityTextAccessory::ListValue)
        }
        dex_run::DexPreferenceKind::Category | dex_run::DexPreferenceKind::Plain => None,
    }
}

pub(super) fn preference_row_click_key(
    preference: &dex_run::DexPreferenceSnapshot,
) -> Option<String> {
    match preference.kind {
        dex_run::DexPreferenceKind::Category => None,
        dex_run::DexPreferenceKind::Switch { .. }
        | dex_run::DexPreferenceKind::List { .. }
        | dex_run::DexPreferenceKind::Plain => Some(preference.runtime_key.clone()),
    }
}

pub(super) fn preference_row_image(
    apk_path: &Path,
    preference: &dex_run::DexPreferenceSnapshot,
    theme: &layout_render::LayoutTheme,
    tint_color: Option<u32>,
) -> Result<Option<host_window::ActivityImage>> {
    let Some(resource_id) = preference.icon_resource_id else {
        return Ok(None);
    };

    let mut image = load_activity_image_resource(apk_path, resource_id, theme)?;
    if tint_color.is_some() {
        image.tint_color = tint_color;
    }

    Ok(Some(image))
}

pub(super) fn build_activity_action_bar(
    apk_path: &Path,
    resources: Option<&[u8]>,
    snapshot: &dex_run::DexViewSnapshot,
) -> Result<Option<host_window::ActivityActionBar>> {
    let is_decor = resources
        .map(|resources| {
            layout_render::activity_has_decor_action_bar_from_resources(resources, snapshot)
        })
        .unwrap_or_else(|| layout_render::activity_has_decor_action_bar(apk_path, snapshot));
    if !is_decor && snapshot.options_menu.is_empty() {
        return Ok(None);
    }

    const SHOW_AS_ACTION_IF_ROOM_OR_ALWAYS: i32 = 0x3;
    let theme = layout_render::layout_theme_from_snapshot(snapshot);
    let mut items = Vec::with_capacity(snapshot.options_menu.len());
    for item in &snapshot.options_menu {
        let shows_as_action = item.show_as_action & SHOW_AS_ACTION_IF_ROOM_OR_ALWAYS != 0;
        let icon_resource_id = item.icon_resource.and_then(|id| u32::try_from(id).ok());
        // cite: androidx ActionMenuPresenter.java#flagActionItems
        let image = if !shows_as_action {
            None
        } else if let Some(resource_id) = icon_resource_id {
            Some(load_activity_image_resource(apk_path, resource_id, &theme)?)
        } else {
            None
        };
        let overflow = image.is_none();
        items.push(host_window::ActivityActionItem {
            item_id: item.item_id,
            title: item.title.clone(),
            enabled: item.enabled,
            image,
            overflow,
        });
    }

    // cite: androidx ToolbarWidgetWrapper.java#setWindowTitle
    let title = snapshot.action_bar_title.clone().unwrap_or_default();
    let background_color =
        layout_render::resolve_theme_color_attribute(apk_path, snapshot, "colorPrimary")?;
    let navigation_icon = snapshot
        .action_bar_nav_icon
        .map(|resource_id| load_activity_image_resource(apk_path, resource_id, &theme))
        .transpose()?;
    let navigation_is_up = snapshot.action_bar_home_as_up && navigation_icon.is_none();
    Ok(Some(host_window::ActivityActionBar {
        title,
        items,
        background_color,
        navigation_icon,
        navigation_is_up,
        is_decor,
    }))
}

fn activity_window_background_color(
    apk_path: &Path,
    resources: Option<&[u8]>,
    snapshot: &dex_run::DexViewSnapshot,
) -> Option<u32> {
    resources
        .and_then(|resources| {
            layout_render::activity_window_background_color_from_resources(resources, snapshot)
        })
        .or_else(|| layout_render::activity_window_background_color(apk_path, snapshot))
}

pub(super) fn action_bar_overflow_overlay(
    snapshot: &dex_run::DexViewSnapshot,
    action_bar: Option<&host_window::ActivityActionBar>,
) -> Option<host_window::ActivityOverlay> {
    if !snapshot.action_bar_overflow_open {
        return None;
    }
    let action_bar = action_bar?;
    let items: Vec<host_window::ActivityOverlayItem> = action_bar
        .items
        .iter()
        .filter(|item| item.overflow)
        .map(|item| host_window::ActivityOverlayItem {
            item_id: item.item_id,
            text: item.title.clone(),
            enabled: item.enabled,
        })
        .collect();
    if items.is_empty() {
        return None;
    }

    Some(host_window::ActivityOverlay {
        anchor_view_id: Some(host_window::ACTION_BAR_OVERFLOW_TOGGLE_ID),
        items,
    })
}

pub(super) fn inset_elements_below_action_bar(
    elements: &mut [host_window::ActivityTextElement],
    inset: u32,
) {
    for element in elements {
        element.y -= inset as i32;
        if let Some(clip) = &mut element.clip {
            let shift = inset.min(clip.y);
            clip.y -= shift;
            clip.height = clip.height.saturating_sub(inset - shift);
        }
    }
}

pub(super) fn activity_image(
    drawable: &layout_render::LayoutDrawable,
    padding_after: u32,
) -> host_window::ActivityImage {
    host_window::ActivityImage {
        source: drawable.path.clone(),
        data: drawable.data.clone(),
        resources: drawable.resources.clone(),
        theme: drawable.theme.clone(),
        tint_color: drawable.tint_color,
        padding_after,
    }
}

fn load_activity_image_resource(
    apk_path: &Path,
    resource_id: u32,
    theme: &layout_render::LayoutTheme,
) -> Result<host_window::ActivityImage> {
    if android_framework::is_framework_package_resource_id(resource_id) {
        // cite: frameworks-base ResourcesImpl.java#loadDrawable, loadDrawableForCookie
        return Ok(missing_framework_drawable_image(resource_id, theme));
    }

    let drawable = layout_render::load_drawable_resource(apk_path, resource_id, theme)?;
    Ok(activity_image(&drawable, 0))
}

fn missing_framework_drawable_image(
    resource_id: u32,
    theme: &layout_render::LayoutTheme,
) -> host_window::ActivityImage {
    crate::missing::record_from_error_message(&format!(
        "framework drawable resource 0x{resource_id:08x} is not bundled with the app\nnext missing primitive: framework drawable 0x{resource_id:08x}"
    ));

    host_window::ActivityImage {
        source: host_window::MISSING_FRAMEWORK_DRAWABLE_SOURCE.to_owned(),
        data: Vec::new(),
        resources: Vec::new(),
        theme: theme.clone(),
        tint_color: None,
        padding_after: 0,
    }
}

pub(super) fn activity_text_kind(
    kind: layout_render::LayoutTextKind,
) -> host_window::ActivityTextKind {
    match kind {
        layout_render::LayoutTextKind::Label => host_window::ActivityTextKind::Label,
        layout_render::LayoutTextKind::Button => host_window::ActivityTextKind::Button,
    }
}

pub(super) fn activity_text_alignment(
    alignment: layout_render::LayoutTextAlignment,
) -> host_window::ActivityTextAlignment {
    match alignment {
        layout_render::LayoutTextAlignment::Start => host_window::ActivityTextAlignment::Start,
        layout_render::LayoutTextAlignment::Center => host_window::ActivityTextAlignment::Center,
        layout_render::LayoutTextAlignment::End => host_window::ActivityTextAlignment::End,
    }
}

pub(super) fn activity_view_id(view_id: Option<u32>) -> Option<i32> {
    view_id.and_then(|value| i32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        activity_image, android_touch_coordinate_y, load_activity_image_resource,
        preference_row_image,
    };

    // cite: frameworks-base ResourcesImpl.java#loadDrawable, loadDrawableForCookie
    // cite: frameworks-base Resources.java#resolveAttribute
    #[test]
    fn resources_impl_selected_drawable_config_and_applied_theme_reach_activity_image() {
        let resource_config =
            crate::resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let theme = crate::layout_render::LayoutTheme {
            styles: vec![crate::layout_render::LayoutThemeStyle {
                resource_id: 0x7f02_0000,
                force: true,
            }],
            config: resource_config,
        };
        let drawable = crate::layout_render::LayoutDrawable {
            resource_id: 0x7f01_0000,
            path: "res/drawable-land/icon.xml".to_owned(),
            data: Vec::new(),
            resources: Vec::new(),
            theme: theme.clone(),
            tint_color: None,
        };

        let image = activity_image(&drawable, 0);

        assert_eq!(image.theme, theme);
    }

    // cite: frameworks-base ResourcesImpl.java#loadDrawableForCookie
    #[test]
    fn resources_impl_preference_drawable_load_failure_is_not_treated_as_no_image() {
        let preference = crate::dex_run::DexPreferenceSnapshot {
            runtime_key: "object:1".to_owned(),
            key: Some("preference".to_owned()),
            title: "Preference".to_owned(),
            summary: None,
            icon_resource_id: Some(0x7f01_0000),
            class_descriptor: "Landroidx/preference/Preference;".to_owned(),
            kind: crate::dex_run::DexPreferenceKind::Plain,
            depth: 0,
        };

        let error = preference_row_image(
            Path::new("/winedroid-test-missing-preference-icon.apk"),
            &preference,
            &crate::layout_render::LayoutTheme {
                styles: Vec::new(),
                config: crate::resource_table::ResourceResolutionConfig::default(),
            },
            None,
        )
        .expect_err("selected preference icon failure should propagate");

        assert!(error.to_string().contains("failed to open APK"));
    }

    // cite: frameworks-base ResourcesImpl.java#loadDrawable, loadDrawableForCookie
    #[test]
    fn resources_impl_unavailable_framework_drawable_is_an_explicit_error() {
        let error = load_activity_image_resource(
            Path::new("/unused.apk"),
            0x0108_0000,
            &crate::layout_render::LayoutTheme {
                styles: Vec::new(),
                config: crate::resource_table::ResourceResolutionConfig::default(),
            },
        )
        .expect_err("unavailable framework drawable should fail");

        assert!(error
            .to_string()
            .contains("next missing primitive: framework drawable resource loading"));
    }

    #[test]
    fn rendered_point_y_is_already_an_android_touch_coordinate() {
        assert_eq!(android_touch_coordinate_y(0), 0);
        assert_eq!(android_touch_coordinate_y(725), 725);
    }
}
