use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::automation::{
    AssertKind, AutomationAction, AutomationScript, DumpKind, KeyName, TapTarget,
};
use crate::dex_interpreter::{drain_opcode_step_histogram, reset_opcode_step_histogram};
use crate::error::{Result, WinedroidError};
use crate::frame_clock::FrameClock;
use crate::host_window::{
    activity_dialog_content_rect, activity_dialog_frame, text_element_view_click_target,
    ActivityClickHandler, ActivityDialogOverlay, ActivitySurfaceState, ActivityTextElement,
    ActivityTextKind, ActivityWindowRequest, ACTIVITY_DIALOG_ITEM_HEIGHT,
};
use crate::input::InputKey;
use crate::skia_renderer;

pub(in crate::host_window) const POST_CLICK_IDLE_REFRESH_LIMIT: usize = 24;
pub(in crate::host_window) const DEFERRED_INPUT_FRAME_DRIVE_LIMIT: usize = 1;
pub(in crate::host_window) const NANOS_PER_SECOND_F64: f64 = 1_000_000_000.0;
pub(in crate::host_window) const MAX_EVENT_WAKE_SECONDS: f64 = 0.05;
pub(in crate::host_window) const MAX_IDLE_WAKE_SECONDS: f64 = 1.0;
pub(in crate::host_window) const FALLBACK_FRAMES_PER_SECOND: u32 = 60;

thread_local! {
    pub(in crate::host_window) static ACTIVE_CLICK_HANDLER: RefCell<Option<ActivityClickHandler>> = const { RefCell::new(None) };
    pub(in crate::host_window) static NEXT_WAKE_DELAY_MS: Cell<Option<i64>> = const { Cell::new(None) };
    pub(in crate::host_window) static MAIN_THREAD_IDLE: Cell<bool> = const { Cell::new(false) };
    pub(in crate::host_window) static AUTOMATION_FRAME_PACER: RefCell<Option<(FrameClock, Instant)>> = const { RefCell::new(None) };
    pub(in crate::host_window) static ACTIVE_RENDER_SURFACE: RefCell<Option<ActivitySurfaceState>> = const { RefCell::new(None) };
    pub(in crate::host_window) static ACTIVE_RENDER_SURFACE_SIZE: RefCell<Option<(u32, u32)>> = const { RefCell::new(None) };
    static ACTIVE_DISPLAY_ROUND: Cell<bool> = const { Cell::new(false) };
    pub(in crate::host_window) static ACTIVE_RENDER_IDENTITY: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    pub(in crate::host_window) static ACTIVE_RENDER_SCROLL_Y: RefCell<i32> = const { RefCell::new(0) };
    pub(in crate::host_window) static ACTIVE_DIALOG_ITEM_SCROLL_OFFSET: RefCell<usize> = const { RefCell::new(0) };
    pub(in crate::host_window) static PENDING_RENDER_SCROLL_Y: RefCell<Option<i32>> = const { RefCell::new(None) };
    pub(in crate::host_window) static LAST_CLICK_RESULT: RefCell<Option<NativeClickResult>> = const { RefCell::new(None) };
    pub(in crate::host_window) static PENDING_IDLE_SURFACE_REFRESHES: Cell<usize> = const { Cell::new(0) };
    pub(in crate::host_window) static SCREENSHOT_DIR: RefCell<Option<crate::external_storage::ScreenshotDirectory>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::host_window) struct NativeTextUpdate {
    pub(in crate::host_window) view_id: i32,
    pub(in crate::host_window) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(in crate::host_window) struct NativeRefreshResult {
    pub(in crate::host_window) text_updates: Vec<NativeTextUpdate>,
    pub(in crate::host_window) surface_changed: bool,
    pub(in crate::host_window) structure_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::host_window) enum NativeClickResult {
    Handled(bool),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::host_window) enum RenderedClickTarget {
    View(i32),
    FocusView(i32),
    Preference(String),
    DialogButton {
        dialog_index: usize,
        button_index: usize,
    },
    DialogItem {
        dialog_index: usize,
        item_index: usize,
    },
    DialogSurface {
        dialog_index: usize,
    },
    Point {
        x: u32,
        y: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::host_window) enum PostInputRefresh {
    ReadCommittedSurface,
    DeferToScheduledFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::host_window) enum RenderScrollResult {
    ConsumedChanged,
    ConsumedUnchanged,
    NotConsumed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::host_window) enum InteractivePointerReleaseAction {
    RuntimeSwipe,
    ProjectedScroll(i32),
    Click(RenderedClickTarget),
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::host_window) struct InteractivePointerMotionState {
    pub(in crate::host_window) moved_beyond_slop: bool,
    pub(in crate::host_window) vertical_moved_beyond_slop: bool,
    vertical_scroll_slop_sign: i8,
    pub(in crate::host_window) target_retained: bool,
}

impl InteractivePointerMotionState {
    pub(in crate::host_window) fn new() -> Self {
        Self {
            moved_beyond_slop: false,
            vertical_moved_beyond_slop: false,
            vertical_scroll_slop_sign: 0,
            target_retained: true,
        }
    }

    pub(in crate::host_window) fn update(
        &mut self,
        start: (f64, f64),
        current: (f64, f64),
        touch_slop: f64,
        initial_target: Option<&RenderedClickTarget>,
        surface_width: u32,
        surface_height: u32,
    ) {
        let horizontal_moved = (current.0 - start.0).abs() > touch_slop;
        let vertical_moved = (current.1 - start.1).abs() > touch_slop;
        self.moved_beyond_slop |= horizontal_moved || vertical_moved;
        // cite: frameworks-base ScrollView.java#onInterceptTouchEvent, onTouchEvent
        if vertical_moved && self.vertical_scroll_slop_sign == 0 {
            self.vertical_scroll_slop_sign = if start.1 > current.1 { 1 } else { -1 };
        }
        self.vertical_moved_beyond_slop |= vertical_moved;
        // cite: frameworks-base View.java#onTouchEvent, pointInView
        if (horizontal_moved || vertical_moved)
            && !initial_target.is_some_and(|target| {
                rendered_touch_target_contains_point(
                    target,
                    current,
                    touch_slop,
                    surface_width,
                    surface_height,
                )
            })
        {
            self.target_retained = false;
        }
    }
}

pub(in crate::host_window) trait SessionSurfaceHost {
    fn request_redraw(&self);
    fn refresh_native_text(&self, handler: &ActivityClickHandler) -> Result<Vec<NativeTextUpdate>>;
}

pub(in crate::host_window) trait SessionLoopHost:
    SessionSurfaceHost
{
    fn pump_events(&self, wake_seconds: f64) -> Result<()>;
    fn after_pump(&self) -> Result<()>;
    fn is_alive(&self) -> bool;
}

pub(in crate::host_window) struct ActiveClickHandlerGuard;

impl ActiveClickHandlerGuard {
    pub(in crate::host_window) fn install(click_handler: Option<ActivityClickHandler>) -> Self {
        ACTIVE_CLICK_HANDLER.with(|handler| {
            *handler.borrow_mut() = click_handler;
        });

        Self
    }
}

impl Drop for ActiveClickHandlerGuard {
    fn drop(&mut self) {
        ACTIVE_CLICK_HANDLER.with(|handler| {
            *handler.borrow_mut() = None;
        });
    }
}

pub(in crate::host_window) struct ActiveRenderSurfaceGuard;

impl ActiveRenderSurfaceGuard {
    pub(in crate::host_window) fn install(initial_scroll_y: i32) -> Self {
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = None;
        });
        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            *scroll_y.borrow_mut() = 0;
        });
        reset_active_dialog_item_scroll();
        PENDING_RENDER_SCROLL_Y.with(|scroll_y| {
            let initial_scroll_y = initial_scroll_y.max(0);
            *scroll_y.borrow_mut() = (initial_scroll_y > 0).then_some(initial_scroll_y);
        });

        Self
    }
}

impl Drop for ActiveRenderSurfaceGuard {
    fn drop(&mut self) {
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = None;
        });
        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            *scroll_y.borrow_mut() = 0;
        });
        reset_active_dialog_item_scroll();
        PENDING_RENDER_SCROLL_Y.with(|scroll_y| {
            *scroll_y.borrow_mut() = None;
        });
    }
}

pub(in crate::host_window) fn set_screenshot_dir(
    dir: Option<crate::external_storage::ScreenshotDirectory>,
) {
    SCREENSHOT_DIR.with(|screenshot_dir| *screenshot_dir.borrow_mut() = dir);
}

pub(in crate::host_window) fn window_message(request: &ActivityWindowRequest) -> String {
    let mut lines = vec![
        request.package_name.clone(),
        request.activity_name.clone(),
        "managed onCreate completed".to_owned(),
    ];
    lines.extend(request.detail_lines.clone());
    lines.join("\n")
}

pub(in crate::host_window) fn install_render_surface_state(
    request: &ActivityWindowRequest,
) -> Result<()> {
    let surface = ActivitySurfaceState {
        message: window_message(request),
        text_elements: request.text_elements.clone(),
        runtime_canvas_frames: request.runtime_canvas_frames.clone(),
        overlays: request.overlays.clone(),
        dialogs: request.dialogs.clone(),
        action_bar: request.action_bar.clone(),
        floating_panel: None,
        window_background: request.window_background,
        resource_id_names: request.resource_id_names.clone(),
    };

    ACTIVE_RENDER_SURFACE_SIZE.with(|size| {
        *size.borrow_mut() = Some((request.width, request.height));
    });
    ACTIVE_DISPLAY_ROUND.with(|round| round.set(request.display_round));
    ACTIVE_RENDER_IDENTITY.with(|identity| {
        *identity.borrow_mut() =
            Some((request.package_name.clone(), request.activity_name.clone()));
    });
    dump_render_surface_if_requested(request.width, request.height, &surface)?;
    dump_ui_if_requested(request.width, request.height, &surface)?;
    crate::host_window::trace_activity_surface_bounds(
        "initial",
        request.width,
        request.height,
        &surface,
    );
    ACTIVE_RENDER_SURFACE.with(|active_surface| {
        *active_surface.borrow_mut() = Some(surface);
    });
    reset_active_dialog_item_scroll();
    reconcile_active_render_scroll(request.height);
    reconcile_active_dialog_item_scroll(request.width, request.height);
    Ok(())
}

pub(in crate::host_window) fn active_display_round() -> bool {
    ACTIVE_DISPLAY_ROUND.with(Cell::get)
}

pub(in crate::host_window) fn present_on_display_panel(
    mut image: skia_renderer::SkiaSurfaceImage,
) -> Result<skia_renderer::SkiaSurfaceImage> {
    if active_display_round() {
        skia_renderer::mask_round_display(&mut image)?;
    }
    Ok(image)
}

pub(in crate::host_window) fn display_panel_contains(
    point: (f64, f64),
    surface_width: u32,
    surface_height: u32,
) -> bool {
    !active_display_round()
        || crate::host_window::round_display_contains(point, surface_width, surface_height)
}

fn dump_render_surface_if_requested(
    width: u32,
    height: u32,
    surface: &ActivitySurfaceState,
) -> Result<()> {
    if env::var_os("WINEDROID_RENDER_DUMP_PNG").is_none() {
        return Ok(());
    }

    let image = present_on_display_panel(skia_renderer::render_activity_surface(
        width,
        height,
        &surface.message,
        &surface.text_elements,
        &surface.runtime_canvas_frames,
        &surface.overlays,
        &surface.dialogs,
        surface.action_bar.as_ref(),
        surface.floating_panel,
        surface.window_background,
    )?)?;
    dump_renderer_png_if_requested(&image)
}

pub(in crate::host_window) fn dump_active_render_surface_if_requested() -> Result<()> {
    if env::var_os("WINEDROID_RENDER_DUMP_PNG").is_none() {
        return Ok(());
    }

    let Some((width, height)) = ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow()) else {
        return Ok(());
    };
    let Some(surface) = ACTIVE_RENDER_SURFACE.with(|surface| surface.borrow().clone()) else {
        return Ok(());
    };
    let surface = scrolled_render_surface(&surface, width, height);
    let image = present_on_display_panel(skia_renderer::render_activity_surface(
        width,
        height,
        &surface.message,
        &surface.text_elements,
        &surface.runtime_canvas_frames,
        &surface.overlays,
        &surface.dialogs,
        surface.action_bar.as_ref(),
        surface.floating_panel,
        surface.window_background,
    )?)?;

    dump_renderer_png_if_requested(&image)
}

pub(in crate::host_window) fn render_active_surface_for_metrics() {
    if !crate::render_metrics::enabled() {
        return;
    }
    let started_at = crate::render_metrics::start_timing();
    let result = render_active_surface_image_for_metrics();
    crate::render_metrics::record_surface_present_elapsed(started_at);
    if let Err(error) = result {
        crate::trace::event("render", format!("headless render metric failed: {error}"));
    }
}

fn render_active_surface_image_for_metrics() -> Result<()> {
    let Some((width, height)) = ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow()) else {
        return Ok(());
    };
    let Some(surface) = ACTIVE_RENDER_SURFACE.with(|surface| surface.borrow().clone()) else {
        return Ok(());
    };
    let surface = scrolled_render_surface(&surface, width, height);
    let _image = skia_renderer::render_activity_surface(
        width,
        height,
        &surface.message,
        &surface.text_elements,
        &surface.runtime_canvas_frames,
        &surface.overlays,
        &surface.dialogs,
        surface.action_bar.as_ref(),
        surface.floating_panel,
        surface.window_background,
    )?;
    Ok(())
}

pub(in crate::host_window) fn dump_renderer_png_if_requested(
    image: &skia_renderer::SkiaSurfaceImage,
) -> Result<()> {
    let Ok(path) = env::var("WINEDROID_RENDER_DUMP_PNG") else {
        return Ok(());
    };

    fs::write(&path, &image.png).map_err(|error| {
        WinedroidError::with_source(
            format!("failed to write Skia renderer dump PNG to `{path}`"),
            error,
        )
    })
}

fn dump_ui_if_requested(width: u32, height: u32, surface: &ActivitySurfaceState) -> Result<()> {
    let Some(path) = env::var_os("WINEDROID_UI_DUMP") else {
        return Ok(());
    };
    let (package, _activity) = ACTIVE_RENDER_IDENTITY
        .with(|identity| identity.borrow().clone())
        .unwrap_or_else(|| ("unknown".to_owned(), "unknown".to_owned()));
    let dump = crate::host_window::activity_surface_ui_dump(width, height, &package, surface);
    let path = PathBuf::from(path);
    fs::write(&path, &dump).map_err(|error| {
        WinedroidError::with_source(
            format!("failed to write WINEDROID_UI_DUMP to `{}`", path.display()),
            error,
        )
    })
}

pub(in crate::host_window) fn initial_render_scroll_y() -> Result<i32> {
    let value = match env::var("WINEDROID_RENDER_SCROLL_Y") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(0),
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_RENDER_SCROLL_Y",
                error,
            ));
        }
    };

    let parsed = value.parse::<i32>().map_err(|error| {
        WinedroidError::with_source(
            format!("WINEDROID_RENDER_SCROLL_Y must be a nonnegative integer: {value}"),
            error,
        )
    })?;

    Ok(parsed.max(0))
}

pub(in crate::host_window) fn scrolled_render_surface(
    surface: &ActivitySurfaceState,
    surface_width: u32,
    surface_height: u32,
) -> ActivitySurfaceState {
    let scroll_y = ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| *scroll_y.borrow());
    let max_scroll_y = max_render_scroll_y(surface, surface_height);
    let scroll_y = scroll_y.clamp(0, max_scroll_y);
    let dialog_item_scroll_offset =
        ACTIVE_DIALOG_ITEM_SCROLL_OFFSET.with(|offset| *offset.borrow());
    let mut scrolled = surface.clone();

    for element in &mut scrolled.text_elements {
        if !is_scrollable_render_element(element) {
            continue;
        }

        element.y = element.y.saturating_add(scroll_y);
    }

    if let Some(dialog) = scrolled.dialogs.last_mut() {
        let max_offset = max_dialog_item_scroll_offset(surface_width, surface_height, dialog);
        dialog.item_scroll_offset = dialog_item_scroll_offset.min(max_offset);
    }

    scrolled
}

pub(in crate::host_window) fn adjust_active_render_scroll(
    delta_y: i32,
    surface_height: u32,
) -> RenderScrollResult {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let Some(surface) = surface.as_ref() else {
            return RenderScrollResult::NotConsumed;
        };
        let max_scroll_y = max_render_scroll_y(surface, surface_height);
        if max_scroll_y == 0 {
            ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
                *scroll_y.borrow_mut() = 0;
            });
            return RenderScrollResult::NotConsumed;
        }

        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            let mut current = scroll_y.borrow_mut();
            let next = current.saturating_add(delta_y).clamp(0, max_scroll_y);
            if next == *current {
                return RenderScrollResult::ConsumedUnchanged;
            }

            *current = next;
            crate::trace::event("click", format!("Host renderer scroll offset y: {next}"));
            RenderScrollResult::ConsumedChanged
        })
    })
}

pub(in crate::host_window) fn adjust_active_dialog_item_scroll(
    delta_y: i32,
    point: (f64, f64),
    surface_width: u32,
    surface_height: u32,
) -> RenderScrollResult {
    if point.0 < 0.0 || point.1 < 0.0 {
        return RenderScrollResult::NotConsumed;
    }

    let x = point.0 as u32;
    let y = point.1 as u32;
    if x >= surface_width || y >= surface_height {
        return RenderScrollResult::NotConsumed;
    }

    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let Some(surface) = surface.as_ref() else {
            return RenderScrollResult::NotConsumed;
        };
        let Some(dialog) = surface.dialogs.last() else {
            return RenderScrollResult::NotConsumed;
        };

        if dialog.items.is_empty() {
            return RenderScrollResult::ConsumedUnchanged;
        }

        let dialog_frame = activity_dialog_frame(surface_width, surface_height, dialog);
        if !dialog_frame.contains_host_point(x, y) {
            return RenderScrollResult::ConsumedUnchanged;
        }

        let max_offset = max_dialog_item_scroll_offset(surface_width, surface_height, dialog);
        if max_offset == 0 {
            ACTIVE_DIALOG_ITEM_SCROLL_OFFSET.with(|offset| {
                *offset.borrow_mut() = 0;
            });
            return RenderScrollResult::ConsumedUnchanged;
        }

        ACTIVE_DIALOG_ITEM_SCROLL_OFFSET.with(|offset| {
            let mut current = offset.borrow_mut();
            let next = next_dialog_item_scroll_offset(*current, delta_y, max_offset);
            if next == *current {
                return RenderScrollResult::ConsumedUnchanged;
            }

            *current = next;
            crate::trace::event(
                "click",
                format!("Host renderer dialog scroll item offset: {next}"),
            );
            RenderScrollResult::ConsumedChanged
        })
    })
}

fn next_dialog_item_scroll_offset(current: usize, delta_y: i32, max_offset: usize) -> usize {
    let row_delta = dialog_scroll_row_delta(delta_y);
    if row_delta == 0 {
        return current;
    }

    if row_delta > 0 {
        return current.saturating_add(row_delta as usize).min(max_offset);
    }

    current.saturating_sub(row_delta.unsigned_abs() as usize)
}

fn dialog_scroll_row_delta(delta_y: i32) -> i32 {
    let magnitude = delta_y.unsigned_abs();
    let rows = (magnitude / ACTIVITY_DIALOG_ITEM_HEIGHT)
        .max(1)
        .min(i32::MAX as u32) as i32;

    if delta_y > 0 {
        rows
    } else {
        -rows
    }
}

fn max_dialog_item_scroll_offset(
    surface_width: u32,
    surface_height: u32,
    dialog: &ActivityDialogOverlay,
) -> usize {
    if dialog.items.is_empty() {
        return 0;
    }

    let content = activity_dialog_content_rect(surface_width, surface_height, dialog);
    if content.height == 0 {
        return 0;
    }

    let visible_rows = content.height.div_ceil(ACTIVITY_DIALOG_ITEM_HEIGHT) as usize;
    dialog.items.len().saturating_sub(visible_rows.max(1))
}

pub(in crate::host_window) fn reset_active_dialog_item_scroll() {
    ACTIVE_DIALOG_ITEM_SCROLL_OFFSET.with(|offset| {
        *offset.borrow_mut() = 0;
    });
}

fn reconcile_active_dialog_item_scroll(surface_width: u32, surface_height: u32) {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let Some(surface) = surface.as_ref() else {
            reset_active_dialog_item_scroll();
            return;
        };
        let Some(dialog) = surface.dialogs.last() else {
            reset_active_dialog_item_scroll();
            return;
        };

        let max_offset = max_dialog_item_scroll_offset(surface_width, surface_height, dialog);
        ACTIVE_DIALOG_ITEM_SCROLL_OFFSET.with(|offset| {
            let mut offset = offset.borrow_mut();
            *offset = (*offset).min(max_offset);
        });
    });
}

fn reconcile_active_render_scroll(surface_height: u32) {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let Some(surface) = surface.as_ref() else {
            return;
        };
        let max_scroll_y = max_render_scroll_y(surface, surface_height);

        let pending_scroll_y = PENDING_RENDER_SCROLL_Y.with(|scroll_y| *scroll_y.borrow());
        if let Some(pending_scroll_y) = pending_scroll_y {
            if max_scroll_y > 0 {
                ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
                    *scroll_y.borrow_mut() = pending_scroll_y.clamp(0, max_scroll_y);
                });
                PENDING_RENDER_SCROLL_Y.with(|scroll_y| {
                    *scroll_y.borrow_mut() = None;
                });
                return;
            }
        }

        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            let mut scroll_y = scroll_y.borrow_mut();
            *scroll_y = (*scroll_y).clamp(0, max_scroll_y);
        });
    });
}

fn max_render_scroll_y(surface: &ActivitySurfaceState, surface_height: u32) -> i32 {
    let surface_height = i32::try_from(surface_height).unwrap_or(i32::MAX);
    let mut max_scroll_y = 0;

    for element in &surface.text_elements {
        if !is_scrollable_render_element(element) {
            continue;
        }
        let Some(clip) = element.clip else {
            continue;
        };
        let content_bottom = surface_height.saturating_sub(element.y);
        let clip_bottom = surface_height.saturating_sub(i32::try_from(clip.y).unwrap_or(i32::MAX));
        max_scroll_y = max_scroll_y.max(content_bottom.saturating_sub(clip_bottom));
    }

    max_scroll_y
}

fn is_scrollable_render_element(element: &ActivityTextElement) -> bool {
    matches!(
        element.kind,
        ActivityTextKind::ListItem | ActivityTextKind::Label
    ) && element.clip.is_some()
}

pub(in crate::host_window) fn session_click_surface_size() -> Option<(u32, u32)> {
    ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow())
}

pub(in crate::host_window) fn rendered_click_target_at(
    point: (f64, f64),
    surface_width: u32,
    surface_height: u32,
) -> Option<RenderedClickTarget> {
    if point.0 < 0.0 || point.1 < 0.0 {
        return None;
    }

    if !display_panel_contains(point, surface_width, surface_height) {
        return None;
    }

    let x = point.0 as u32;
    let y = point.1 as u32;
    if x >= surface_width || y >= surface_height {
        return None;
    }

    let hit_y = surface_height.saturating_sub(y);

    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let surface = surface.as_ref()?;
        let surface = scrolled_render_surface(surface, surface_width, surface_height);

        for (dialog_index, dialog) in surface.dialogs.iter().enumerate().rev() {
            let content = activity_dialog_content_rect(surface_width, surface_height, dialog);
            let frames = crate::host_window::activity_dialog_button_frames(
                surface_width,
                surface_height,
                dialog_index,
                dialog,
            );
            if let Some(frame) = frames
                .iter()
                .rev()
                .find(|frame| frame.button.enabled && frame.frame.contains_host_point(x, hit_y))
            {
                return Some(RenderedClickTarget::DialogButton {
                    dialog_index: frame.dialog_index,
                    button_index: frame.button_index,
                });
            }

            let frames = crate::host_window::activity_dialog_item_frames(
                surface_width,
                surface_height,
                dialog_index,
                dialog,
            );
            if let Some(frame) = frames.iter().rev().find(|frame| {
                frame.item.enabled
                    && content.contains_host_point(x, hit_y)
                    && frame.frame.contains_host_point(x, hit_y)
            }) {
                return Some(RenderedClickTarget::DialogItem {
                    dialog_index: frame.dialog_index,
                    item_index: frame.item_index,
                });
            }

            return Some(RenderedClickTarget::DialogSurface { dialog_index });
        }

        for overlay in surface.overlays.iter().rev() {
            let frames = crate::host_window::activity_overlay_item_frames(
                surface_width,
                surface_height,
                &surface.text_elements,
                overlay,
            );
            if let Some(frame) = frames
                .iter()
                .rev()
                .find(|frame| frame.item.enabled && frame.frame.contains_host_point(x, hit_y))
            {
                return Some(RenderedClickTarget::View(frame.item.item_id));
            }
        }

        if surface.overlays.iter().any(|overlay| {
            overlay.anchor_view_id == Some(crate::host_window::ACTION_BAR_OVERFLOW_TOGGLE_ID)
        }) {
            return Some(RenderedClickTarget::View(
                crate::host_window::ACTION_BAR_OVERFLOW_TOGGLE_ID,
            ));
        }

        if let Some(action_bar) = &surface.action_bar {
            let bar_top = surface_height.saturating_sub(crate::host_window::ACTION_BAR_HEIGHT);
            if hit_y >= bar_top {
                if (action_bar.navigation_icon.is_some() || action_bar.navigation_is_up) && x < 56 {
                    return Some(RenderedClickTarget::View(
                        crate::host_window::ANDROID_R_ID_HOME,
                    ));
                }
                let slot = 48_u32;
                let mut right = surface_width;
                if action_bar.items.iter().any(|item| item.overflow) {
                    right = right.saturating_sub(slot);
                    if x >= right && x < right + slot {
                        return Some(RenderedClickTarget::View(
                            crate::host_window::ACTION_BAR_OVERFLOW_TOGGLE_ID,
                        ));
                    }
                }
                for item in action_bar.items.iter().filter(|item| item.image.is_some()) {
                    right = right.saturating_sub(slot);
                    if x >= right && x < right + slot {
                        return Some(RenderedClickTarget::View(item.item_id));
                    }
                }
                return None;
            }
        }

        surface
            .text_elements
            .iter()
            .rev()
            .find(|element| {
                element.kind == ActivityTextKind::ListItem
                    && element.preference_key.is_some()
                    && element.contains_host_point(x, hit_y)
            })
            .and_then(|element| element.preference_key.clone())
            .map(RenderedClickTarget::Preference)
            .or_else(|| {
                surface
                    .text_elements
                    .iter()
                    .rev()
                    .find(|element| {
                        text_element_view_click_target(element).is_some()
                            && element.contains_host_point(x, hit_y)
                    })
                    .and_then(|element| {
                        let view_id = text_element_view_click_target(element)?;
                        if element.editable {
                            Some(RenderedClickTarget::FocusView(view_id))
                        } else {
                            Some(RenderedClickTarget::View(view_id))
                        }
                    })
            })
            .or_else(|| {
                if surface.runtime_canvas_frames.is_empty() {
                    return None;
                }

                surface
                    .text_elements
                    .iter()
                    .rev()
                    .find(|element| {
                        element.kind == ActivityTextKind::RuntimeViewRoot
                            && element.contains_host_point(x, hit_y)
                    })
                    .map(|_| RenderedClickTarget::Point { x, y })
            })
    })
}

fn rendered_touch_targets_match(
    initial: Option<&RenderedClickTarget>,
    current: Option<&RenderedClickTarget>,
) -> bool {
    // cite: frameworks-base ViewGroup.java#dispatchTouchEvent
    match (initial, current) {
        (Some(RenderedClickTarget::Point { .. }), Some(RenderedClickTarget::Point { .. })) => true,
        (Some(initial), Some(current)) => initial == current,
        (None, None) => true,
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct RenderedPointerBounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl RenderedPointerBounds {
    fn contains_with_slop(self, point: (f64, f64), touch_slop: f64) -> bool {
        // cite: frameworks-base View.java#pointInView
        point.0 >= self.left - touch_slop
            && point.0 < self.right + touch_slop
            && point.1 >= self.top - touch_slop
            && point.1 < self.bottom + touch_slop
    }
}

fn rendered_touch_target_contains_point(
    target: &RenderedClickTarget,
    point: (f64, f64),
    touch_slop: f64,
    surface_width: u32,
    surface_height: u32,
) -> bool {
    if matches!(
        target,
        RenderedClickTarget::Point { .. } | RenderedClickTarget::DialogSurface { .. }
    ) {
        return RenderedPointerBounds {
            left: 0.0,
            top: 0.0,
            right: f64::from(surface_width),
            bottom: f64::from(surface_height),
        }
        .contains_with_slop(point, touch_slop);
    }

    let bounds = ACTIVE_RENDER_SURFACE.with(|active_surface| {
        let active_surface = active_surface.borrow();
        let surface = active_surface.as_ref()?;
        let surface = scrolled_render_surface(surface, surface_width, surface_height);
        rendered_touch_target_bounds(target, &surface, surface_width, surface_height)
    });
    if let Some(bounds) = bounds {
        return bounds.contains_with_slop(point, touch_slop);
    }

    let current = rendered_click_target_at(point, surface_width, surface_height);
    rendered_touch_targets_match(Some(target), current.as_ref())
}

fn rendered_touch_target_bounds(
    target: &RenderedClickTarget,
    surface: &ActivitySurfaceState,
    surface_width: u32,
    surface_height: u32,
) -> Option<RenderedPointerBounds> {
    match target {
        RenderedClickTarget::View(view_id) | RenderedClickTarget::FocusView(view_id) => {
            rendered_view_bounds(*view_id, surface, surface_width, surface_height)
        }
        RenderedClickTarget::Preference(preference_key) => surface
            .text_elements
            .iter()
            .rev()
            .find(|element| element.preference_key.as_ref() == Some(preference_key))
            .map(|element| text_element_pointer_bounds(element, surface_height)),
        RenderedClickTarget::DialogButton {
            dialog_index,
            button_index,
        } => {
            let dialog = surface.dialogs.get(*dialog_index)?;
            crate::host_window::activity_dialog_button_frames(
                surface_width,
                surface_height,
                *dialog_index,
                dialog,
            )
            .into_iter()
            .find(|frame| frame.button_index == *button_index)
            .map(|frame| activity_rect_pointer_bounds(frame.frame, surface_height))
        }
        RenderedClickTarget::DialogItem {
            dialog_index,
            item_index,
        } => {
            let dialog = surface.dialogs.get(*dialog_index)?;
            crate::host_window::activity_dialog_item_frames(
                surface_width,
                surface_height,
                *dialog_index,
                dialog,
            )
            .into_iter()
            .find(|frame| frame.item_index == *item_index)
            .map(|frame| activity_rect_pointer_bounds(frame.frame, surface_height))
        }
        RenderedClickTarget::DialogSurface { .. } => Some(RenderedPointerBounds {
            left: 0.0,
            top: 0.0,
            right: f64::from(surface_width),
            bottom: f64::from(surface_height),
        }),
        RenderedClickTarget::Point { .. } => surface
            .text_elements
            .iter()
            .rev()
            .find(|element| element.kind == ActivityTextKind::RuntimeViewRoot)
            .map(|element| text_element_pointer_bounds(element, surface_height))
            .or(Some(RenderedPointerBounds {
                left: 0.0,
                top: 0.0,
                right: f64::from(surface_width),
                bottom: f64::from(surface_height),
            })),
    }
}

fn rendered_view_bounds(
    view_id: i32,
    surface: &ActivitySurfaceState,
    surface_width: u32,
    surface_height: u32,
) -> Option<RenderedPointerBounds> {
    for overlay in surface.overlays.iter().rev() {
        if let Some(frame) = crate::host_window::activity_overlay_item_frames(
            surface_width,
            surface_height,
            &surface.text_elements,
            overlay,
        )
        .into_iter()
        .rev()
        .find(|frame| frame.item.item_id == view_id)
        {
            return Some(activity_rect_pointer_bounds(frame.frame, surface_height));
        }
    }

    if let Some(bounds) = action_bar_view_bounds(view_id, surface, surface_width) {
        return Some(bounds);
    }

    surface
        .text_elements
        .iter()
        .rev()
        .find(|element| text_element_view_click_target(element) == Some(view_id))
        .map(|element| text_element_pointer_bounds(element, surface_height))
}

fn action_bar_view_bounds(
    view_id: i32,
    surface: &ActivitySurfaceState,
    surface_width: u32,
) -> Option<RenderedPointerBounds> {
    let action_bar = surface.action_bar.as_ref()?;
    if view_id == crate::host_window::ANDROID_R_ID_HOME
        && (action_bar.navigation_icon.is_some() || action_bar.navigation_is_up)
    {
        return Some(RenderedPointerBounds {
            left: 0.0,
            top: 0.0,
            right: 56.0,
            bottom: f64::from(crate::host_window::ACTION_BAR_HEIGHT),
        });
    }

    let slot = 48_u32;
    let mut right = surface_width;
    if action_bar.items.iter().any(|item| item.overflow) {
        right = right.saturating_sub(slot);
        if view_id == crate::host_window::ACTION_BAR_OVERFLOW_TOGGLE_ID {
            return Some(RenderedPointerBounds {
                left: f64::from(right),
                top: 0.0,
                right: f64::from(right.saturating_add(slot)),
                bottom: f64::from(crate::host_window::ACTION_BAR_HEIGHT),
            });
        }
    }
    for item in action_bar.items.iter().filter(|item| item.image.is_some()) {
        right = right.saturating_sub(slot);
        if item.item_id == view_id {
            return Some(RenderedPointerBounds {
                left: f64::from(right),
                top: 0.0,
                right: f64::from(right.saturating_add(slot)),
                bottom: f64::from(crate::host_window::ACTION_BAR_HEIGHT),
            });
        }
    }
    None
}

fn activity_rect_pointer_bounds(
    rect: crate::host_window::ActivityRect,
    surface_height: u32,
) -> RenderedPointerBounds {
    RenderedPointerBounds {
        left: f64::from(rect.x),
        top: f64::from(surface_height.saturating_sub(rect.y.saturating_add(rect.height))),
        right: f64::from(rect.x.saturating_add(rect.width)),
        bottom: f64::from(surface_height.saturating_sub(rect.y)),
    }
}

fn text_element_pointer_bounds(
    element: &ActivityTextElement,
    surface_height: u32,
) -> RenderedPointerBounds {
    let top = i64::from(surface_height)
        .saturating_sub(i64::from(element.y))
        .saturating_sub(i64::from(element.height));
    let bottom = i64::from(surface_height).saturating_sub(i64::from(element.y));
    RenderedPointerBounds {
        left: f64::from(element.x),
        top: top as f64,
        right: f64::from(element.x.saturating_add(element.width)),
        bottom: bottom as f64,
    }
}

pub(in crate::host_window) fn interactive_pointer_release_action(
    start: (f64, f64),
    end: (f64, f64),
    touch_slop: f64,
    motion: InteractivePointerMotionState,
    target: Option<RenderedClickTarget>,
    surface_width: u32,
    surface_height: u32,
) -> InteractivePointerReleaseAction {
    ACTIVE_RENDER_SURFACE.with(|active_surface| {
        let active_surface = active_surface.borrow();
        let Some(surface) = active_surface.as_ref() else {
            return InteractivePointerReleaseAction::Ignore;
        };

        let scroll_delta_y =
            pointer_scroll_delta_y(start, end, touch_slop, motion.vertical_scroll_slop_sign);
        if let Some(dialog) = surface.dialogs.last() {
            // cite: frameworks-base WindowManagerGlobal.java#addView
            // cite: frameworks-base ScrollView.java#onInterceptTouchEvent, onTouchEvent
            if motion.vertical_moved_beyond_slop
                && dialog_scroll_contains_point(dialog, start, surface_width, surface_height)
            {
                return InteractivePointerReleaseAction::ProjectedScroll(scroll_delta_y);
            }
            return projected_pointer_release_action(motion, target);
        }

        // cite: frameworks-base WindowManagerGlobal.java#addView
        if !surface.overlays.is_empty() {
            return projected_pointer_release_action(motion, target);
        }

        // cite: frameworks-base ViewGroup.java#dispatchTouchEvent
        if surface.action_bar.is_some()
            && start.1 >= 0.0
            && start.1 < f64::from(crate::host_window::ACTION_BAR_HEIGHT)
        {
            return projected_pointer_release_action(motion, target);
        }

        // cite: frameworks-base ViewRootImpl.java#processPointerEvent
        // cite: frameworks-base View.java#dispatchPointerEvent
        if motion.moved_beyond_slop && !surface.runtime_canvas_frames.is_empty() {
            return InteractivePointerReleaseAction::RuntimeSwipe;
        }

        // cite: frameworks-base ScrollView.java#onInterceptTouchEvent, onTouchEvent
        if motion.vertical_moved_beyond_slop
            && render_scroll_contains_point(surface, start, surface_width, surface_height)
        {
            return InteractivePointerReleaseAction::ProjectedScroll(scroll_delta_y);
        }

        projected_pointer_release_action(motion, target)
    })
}

fn projected_pointer_release_action(
    motion: InteractivePointerMotionState,
    target: Option<RenderedClickTarget>,
) -> InteractivePointerReleaseAction {
    // cite: frameworks-base View.java#onTouchEvent, pointInView
    if !motion.moved_beyond_slop || motion.target_retained {
        if let Some(target) = target {
            return InteractivePointerReleaseAction::Click(target);
        }
    }

    InteractivePointerReleaseAction::Ignore
}

fn pointer_scroll_delta_y(
    start: (f64, f64),
    end: (f64, f64),
    touch_slop: f64,
    vertical_scroll_slop_sign: i8,
) -> i32 {
    // cite: frameworks-base ScrollView.java#onTouchEvent
    let delta_y =
        start.1 - end.1 - f64::from(vertical_scroll_slop_sign).clamp(-1.0, 1.0) * touch_slop;
    delta_y
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn dialog_scroll_contains_point(
    dialog: &ActivityDialogOverlay,
    point: (f64, f64),
    surface_width: u32,
    surface_height: u32,
) -> bool {
    if max_dialog_item_scroll_offset(surface_width, surface_height, dialog) == 0 {
        return false;
    }
    let Some((x, hit_y)) = render_hit_point(point, surface_width, surface_height) else {
        return false;
    };
    activity_dialog_content_rect(surface_width, surface_height, dialog)
        .contains_host_point(x, hit_y)
}

fn render_scroll_contains_point(
    surface: &ActivitySurfaceState,
    point: (f64, f64),
    surface_width: u32,
    surface_height: u32,
) -> bool {
    if max_render_scroll_y(surface, surface_height) == 0 {
        return false;
    }
    let Some((x, hit_y)) = render_hit_point(point, surface_width, surface_height) else {
        return false;
    };
    surface.text_elements.iter().any(|element| {
        is_scrollable_render_element(element)
            && element
                .clip
                .is_some_and(|clip| clip.contains_host_point(x, hit_y))
    })
}

fn render_hit_point(
    point: (f64, f64),
    surface_width: u32,
    surface_height: u32,
) -> Option<(u32, u32)> {
    if point.0 < 0.0 || point.1 < 0.0 {
        return None;
    }
    let x = point.0 as u32;
    let y = point.1 as u32;
    if x >= surface_width || y >= surface_height {
        return None;
    }
    Some((x, surface_height.saturating_sub(y)))
}

pub(in crate::host_window) fn click_rendered_view(
    host: &dyn SessionSurfaceHost,
    view_id: i32,
    label: &str,
) {
    click_rendered_target(host, RenderedClickTarget::View(view_id), label);
}

pub(in crate::host_window) fn click_rendered_point(
    host: &dyn SessionSurfaceHost,
    x: u32,
    y: u32,
    label: &str,
) -> Result<()> {
    let Some((width, height)) = session_click_surface_size() else {
        return Err(WinedroidError::new(
            "renderer point autoclick has no active render surface size",
        ));
    };
    let Some(target) = rendered_click_target_at((x as f64, y as f64), width, height) else {
        return Err(WinedroidError::new(format!(
            "renderer point autoclick did not hit a rendered target\npoint: {x}:{y}"
        )));
    };

    click_rendered_target(host, target, label);
    Ok(())
}

pub(in crate::host_window) fn click_rendered_target(
    host: &dyn SessionSurfaceHost,
    target: RenderedClickTarget,
    label: &str,
) {
    let result = ACTIVE_CLICK_HANDLER.with(|handler| {
        let Some(handler) = *handler.borrow() else {
            return Err(WinedroidError::new(
                "renderer click has no active Activity click handler",
            ));
        };

        let handled = match &target {
            RenderedClickTarget::View(view_id) => handler.click_view_id(*view_id)?,
            RenderedClickTarget::FocusView(view_id) => handler.focus_view(*view_id)?,
            RenderedClickTarget::Preference(preference_key) => {
                handler.click_preference(preference_key)?
            }
            RenderedClickTarget::DialogButton {
                dialog_index,
                button_index,
            } => handler.click_dialog_button(*dialog_index, *button_index)?,
            RenderedClickTarget::DialogItem {
                dialog_index,
                item_index,
            } => handler.click_dialog_item(*dialog_index, *item_index)?,
            RenderedClickTarget::DialogSurface { .. } => 0,
            RenderedClickTarget::Point { x, y } => handler.click_touch(*x, *y)?,
        };
        let post_input_refresh = post_input_refresh_for_target(&target);
        let mut refresh = NativeRefreshResult::default();
        if handled != 0 {
            refresh = refresh_after_click(host, handler, post_input_refresh)?;
        }

        Ok((handled, post_input_refresh, refresh))
    });

    match result {
        Ok((handled, post_input_refresh, refresh)) => {
            request_redraw_after_post_input(host, post_input_refresh);
            LAST_CLICK_RESULT.with(|result| {
                *result.borrow_mut() = Some(NativeClickResult::Handled(handled != 0));
            });
            if crate::trace::enabled("click") {
                println!("{label}");
                print_click_target(target);
                println!("  handled: {}", handled != 0);
                print_refresh_result(&refresh);
                if let Some(overlays) = display_active_overlays() {
                    println!("  overlays: {overlays}");
                }
                if let Some(dialogs) = display_active_dialogs() {
                    println!("  dialogs: {dialogs}");
                }
                if let Some(cursors) = display_active_cursors() {
                    println!("  cursors: {cursors}");
                }
            }
        }
        Err(error) => {
            LAST_CLICK_RESULT.with(|result| {
                *result.borrow_mut() = Some(NativeClickResult::Error(error.to_string()));
            });
            eprintln!("{label} failed");
            print_click_target_error(target);
            eprintln!("  error: {error}");
        }
    }
}

pub(in crate::host_window) fn session_key_input(
    host: &dyn SessionSurfaceHost,
    characters: &str,
    key: InputKey,
) -> Result<()> {
    let handler = ACTIVE_CLICK_HANDLER
        .with(|handler| *handler.borrow())
        .ok_or_else(|| WinedroidError::new("key event has no active Activity click handler"))?;

    let handled = handler.key_input(characters, key)?;
    if handled != 0 {
        refresh_after_click(host, handler, PostInputRefresh::DeferToScheduledFrame)?;
    }

    Ok(())
}

pub(in crate::host_window) fn scroll_active_surface(
    host: &dyn SessionSurfaceHost,
    delta_y: i32,
    point: (f64, f64),
    surface_width: u32,
    surface_height: u32,
) -> bool {
    match adjust_active_dialog_item_scroll(delta_y, point, surface_width, surface_height) {
        RenderScrollResult::ConsumedChanged => {
            host.request_redraw();
            return true;
        }
        RenderScrollResult::ConsumedUnchanged => return false,
        RenderScrollResult::NotConsumed => {}
    }

    match adjust_active_render_scroll(delta_y, surface_height) {
        RenderScrollResult::ConsumedChanged => {
            host.request_redraw();
            true
        }
        RenderScrollResult::ConsumedUnchanged | RenderScrollResult::NotConsumed => false,
    }
}

fn session_touch_hold(host: &dyn SessionSurfaceHost, x: u32, y: u32, hold_ms: u64) -> Result<()> {
    let Some((width, height)) = session_click_surface_size() else {
        return Err(WinedroidError::new(
            "tap hold has no active render surface size",
        ));
    };
    let target = rendered_click_target_at((x as f64, y as f64), width, height);
    match target {
        Some(RenderedClickTarget::Point { x, y }) => {
            let handler = ACTIVE_CLICK_HANDLER.with(|handler| *handler.borrow()).ok_or_else(
                || WinedroidError::new("tap hold has no active Activity click handler"),
            )?;
            let handled = handler.touch_hold(x, y, hold_ms)?;
            let mut refresh = NativeRefreshResult::default();
            if handled != 0 {
                refresh = refresh_after_click(host, handler, PostInputRefresh::DeferToScheduledFrame)?;
            }
            if crate::trace::enabled("click") {
                println!("Host automation tap hold");
                println!("  point: {x}:{y}");
                println!("  hold ms: {hold_ms}");
                println!("  handled: {}", handled != 0);
                print_refresh_result(&refresh);
            }
            Ok(())
        }
        Some(_) => Err(WinedroidError::new(format!(
            "tap hold only dispatches to runtime canvas targets\npoint: {x}:{y}\nnext missing primitive: View long-press dispatch"
        ))),
        None => Err(WinedroidError::new(format!(
            "tap hold did not hit a rendered target\npoint: {x}:{y}"
        ))),
    }
}

pub(in crate::host_window) fn session_touch_swipe(
    host: &dyn SessionSurfaceHost,
    start: (u32, u32),
    end: (u32, u32),
    duration_ms: u64,
    steps: Option<u32>,
) -> Result<()> {
    let handler = ACTIVE_CLICK_HANDLER
        .with(|handler| *handler.borrow())
        .ok_or_else(|| WinedroidError::new("swipe has no active Activity click handler"))?;

    let handled = handler.touch_swipe(start, end, duration_ms, steps)?;
    let mut refresh = NativeRefreshResult::default();
    if handled != 0 {
        refresh = refresh_after_click(host, handler, PostInputRefresh::DeferToScheduledFrame)?;
    }
    if crate::trace::enabled("click") {
        println!("Host automation swipe");
        println!("  start: {}:{}", start.0, start.1);
        println!("  end: {}:{}", end.0, end.1);
        println!("  duration ms: {duration_ms}");
        println!("  handled: {}", handled != 0);
        print_refresh_result(&refresh);
    }

    Ok(())
}

pub(in crate::host_window) fn print_click_target(target: RenderedClickTarget) {
    match target {
        RenderedClickTarget::View(view_id) => println!("  view id: {view_id}"),
        RenderedClickTarget::FocusView(view_id) => {
            println!("  focus view id: {view_id}");
        }
        RenderedClickTarget::Preference(preference_key) => {
            println!("  preference key: {preference_key}");
        }
        RenderedClickTarget::DialogButton {
            dialog_index,
            button_index,
        } => {
            println!("  dialog index: {dialog_index}");
            println!("  button index: {button_index}");
        }
        RenderedClickTarget::DialogItem {
            dialog_index,
            item_index,
        } => {
            println!("  dialog index: {dialog_index}");
            println!("  item index: {item_index}");
        }
        RenderedClickTarget::DialogSurface { dialog_index } => {
            println!("  dialog index: {dialog_index}");
            println!("  dialog surface");
        }
        RenderedClickTarget::Point { x, y } => {
            println!("  point: {x}:{y}");
        }
    }
}

pub(in crate::host_window) fn print_click_target_error(target: RenderedClickTarget) {
    match target {
        RenderedClickTarget::View(view_id) => eprintln!("  view id: {view_id}"),
        RenderedClickTarget::FocusView(view_id) => eprintln!("  focus view id: {view_id}"),
        RenderedClickTarget::Preference(preference_key) => {
            eprintln!("  preference key: {preference_key}");
        }
        RenderedClickTarget::DialogButton {
            dialog_index,
            button_index,
        } => {
            eprintln!("  dialog index: {dialog_index}");
            eprintln!("  button index: {button_index}");
        }
        RenderedClickTarget::DialogItem {
            dialog_index,
            item_index,
        } => {
            eprintln!("  dialog index: {dialog_index}");
            eprintln!("  item index: {item_index}");
        }
        RenderedClickTarget::DialogSurface { dialog_index } => {
            eprintln!("  dialog index: {dialog_index}");
            eprintln!("  dialog surface");
        }
        RenderedClickTarget::Point { x, y } => {
            eprintln!("  point: {x}:{y}");
        }
    }
}

pub(in crate::host_window) fn refresh_after_click(
    host: &dyn SessionSurfaceHost,
    handler: ActivityClickHandler,
    post_input_refresh: PostInputRefresh,
) -> Result<NativeRefreshResult> {
    // cite: frameworks-base ViewRootImpl.java#scheduleTraversals, doTraversal
    let mut refresh = NativeRefreshResult {
        text_updates: host.refresh_native_text(&handler)?,
        surface_changed: false,
        structure_changed: false,
    };
    if post_input_refresh == PostInputRefresh::ReadCommittedSurface {
        let committed_refresh = apply_refreshed_surface(handler.read_surface()?)?;
        refresh.text_updates.extend(committed_refresh.text_updates);
        refresh.surface_changed = committed_refresh.surface_changed;
        refresh.structure_changed = committed_refresh.structure_changed;
    }
    schedule_post_input_idle_surface_refreshes(post_input_refresh);

    Ok(refresh)
}

fn request_redraw_after_post_input(
    host: &dyn SessionSurfaceHost,
    post_input_refresh: PostInputRefresh,
) {
    if post_input_refresh == PostInputRefresh::ReadCommittedSurface {
        host.request_redraw();
    }
}

fn post_input_refresh_for_target(target: &RenderedClickTarget) -> PostInputRefresh {
    match target {
        RenderedClickTarget::Point { .. } => PostInputRefresh::DeferToScheduledFrame,
        RenderedClickTarget::View(_)
        | RenderedClickTarget::FocusView(_)
        | RenderedClickTarget::Preference(_)
        | RenderedClickTarget::DialogButton { .. }
        | RenderedClickTarget::DialogItem { .. }
        | RenderedClickTarget::DialogSurface { .. } => PostInputRefresh::ReadCommittedSurface,
    }
}

pub(in crate::host_window) fn apply_refreshed_surface(
    next_surface: ActivitySurfaceState,
) -> Result<NativeRefreshResult> {
    let mut refresh = NativeRefreshResult::default();
    let Some((surface_width, surface_height)) =
        ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow())
    else {
        return Err(WinedroidError::new(
            "refreshed Activity surface has no installed display size",
        ));
    };

    ACTIVE_RENDER_SURFACE.with(|surface| -> Result<()> {
        let mut surface = surface.borrow_mut();
        let Some(current_surface) = surface.as_ref() else {
            reset_active_dialog_item_scroll();
            crate::host_window::trace_activity_surface_bounds(
                "refreshed",
                surface_width,
                surface_height,
                &next_surface,
            );
            *surface = Some(next_surface);
            return Ok(());
        };
        refresh.surface_changed = current_surface != &next_surface;
        refresh.structure_changed =
            activity_surface_structure_changed(current_surface, &next_surface);
        let dialogs_changed = current_surface.dialogs != next_surface.dialogs;

        for element in &next_surface.text_elements {
            let Some(view_id) = element.view_id else {
                continue;
            };
            let old_text = current_surface
                .text_elements
                .iter()
                .find(|old| old.view_id == Some(view_id))
                .map(|old| old.text.as_str());
            if old_text == Some(element.text.as_str()) {
                continue;
            }

            refresh.text_updates.push(NativeTextUpdate {
                view_id,
                text: element.text.clone(),
            });
        }

        if dialogs_changed {
            reset_active_dialog_item_scroll();
        }
        crate::host_window::trace_activity_surface_bounds(
            "refreshed",
            surface_width,
            surface_height,
            &next_surface,
        );
        *surface = Some(next_surface);

        Ok(())
    })?;
    reconcile_active_render_scroll(surface_height);
    reconcile_active_dialog_item_scroll(surface_width, surface_height);
    dump_active_render_surface_if_requested()?;

    Ok(refresh)
}

pub(in crate::host_window) fn schedule_post_input_idle_surface_refreshes(
    post_input_refresh: PostInputRefresh,
) {
    // cite: frameworks-base ViewRootImpl.java#scheduleTraversals, doTraversal
    let refresh_count = match post_input_refresh {
        PostInputRefresh::ReadCommittedSurface => POST_CLICK_IDLE_REFRESH_LIMIT,
        PostInputRefresh::DeferToScheduledFrame => DEFERRED_INPUT_FRAME_DRIVE_LIMIT,
    };
    PENDING_IDLE_SURFACE_REFRESHES.with(|refreshes| {
        refreshes.set(refresh_count);
    });
}

pub(in crate::host_window) fn take_pending_idle_surface_refresh() -> bool {
    PENDING_IDLE_SURFACE_REFRESHES.with(|refreshes| {
        let remaining = refreshes.get();
        if remaining == 0 {
            return false;
        }

        refreshes.set(remaining - 1);
        true
    })
}

pub(in crate::host_window) fn pending_idle_surface_refreshes() -> usize {
    PENDING_IDLE_SURFACE_REFRESHES.with(|refreshes| refreshes.get())
}

pub(in crate::host_window) fn refresh_idle_render_surface() -> bool {
    take_pending_idle_surface_refresh()
}

fn activity_surface_structure_changed(
    current: &ActivitySurfaceState,
    next: &ActivitySurfaceState,
) -> bool {
    if current.text_elements != next.text_elements
        || current.overlays != next.overlays
        || current.dialogs != next.dialogs
    {
        return true;
    }

    if current.runtime_canvas_frames.len() != next.runtime_canvas_frames.len() {
        return true;
    }

    current
        .runtime_canvas_frames
        .iter()
        .zip(next.runtime_canvas_frames.iter())
        .any(|(current, next)| {
            current.width != next.width || current.height != next.height || current.ops != next.ops
        })
}

fn display_text_updates(updates: &[NativeTextUpdate]) -> String {
    updates
        .iter()
        .map(|update| format!("{} -> {:?}", update.view_id, update.text))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(in crate::host_window) fn print_refresh_result(refresh: &NativeRefreshResult) {
    if !refresh.text_updates.is_empty() {
        println!(
            "  text updates: {}",
            display_text_updates(&refresh.text_updates)
        );
    }
    if refresh.surface_changed || refresh.structure_changed || refresh.text_updates.is_empty() {
        println!("  surface changed: {}", refresh.surface_changed);
        println!("  structure changed: {}", refresh.structure_changed);
    }
}

pub(in crate::host_window) fn display_active_overlays() -> Option<String> {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let surface = surface.as_ref()?;
        if surface.overlays.is_empty() {
            return None;
        }

        let overlays = surface
            .overlays
            .iter()
            .flat_map(|overlay| overlay.items.iter())
            .map(|item| format!("{} -> {:?}", item.item_id, item.text))
            .collect::<Vec<_>>();

        if overlays.is_empty() {
            None
        } else {
            Some(overlays.join(", "))
        }
    })
}

pub(in crate::host_window) fn display_active_dialogs() -> Option<String> {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let surface = surface.as_ref()?;
        let dialogs = surface
            .dialogs
            .iter()
            .map(|dialog| {
                let title = dialog.title.as_deref().unwrap_or("");
                let buttons = dialog
                    .buttons
                    .iter()
                    .map(|button| button.text.as_str())
                    .collect::<Vec<_>>()
                    .join("|");
                let items = dialog
                    .items
                    .iter()
                    .map(|item| item.text.as_str())
                    .collect::<Vec<_>>()
                    .join("|");
                format!(
                    "title={title:?} layouts={:?} elements={} items={items:?} buttons={buttons:?}",
                    dialog.content_layouts,
                    dialog.text_elements.len()
                )
            })
            .collect::<Vec<_>>();

        if dialogs.is_empty() {
            None
        } else {
            Some(dialogs.join(", "))
        }
    })
}

pub(in crate::host_window) fn display_active_cursors() -> Option<String> {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        let surface = surface.borrow();
        let surface = surface.as_ref()?;
        let cursors = surface
            .text_elements
            .iter()
            .filter_map(|element| {
                let view_id = element.view_id?;
                let cursor = element.cursor.as_ref()?;
                Some(format!(
                    "{} -> {}..{} visible={}",
                    view_id, cursor.selection_start, cursor.selection_end, cursor.visible
                ))
            })
            .collect::<Vec<_>>();

        if cursors.is_empty() {
            None
        } else {
            Some(cursors.join(", "))
        }
    })
}

pub(in crate::host_window) fn apply_runtime_frame_interval(interval_nanos: i64) -> Result<()> {
    let Some(handler) = ACTIVE_CLICK_HANDLER.with(|handler| *handler.borrow()) else {
        return Ok(());
    };
    handler.set_frame_interval(interval_nanos)
}

pub(in crate::host_window) fn initialize_automation_frame_pacer(interval_nanos: i64) {
    AUTOMATION_FRAME_PACER.with(|pacer| {
        *pacer.borrow_mut() = Some((FrameClock::new(interval_nanos), Instant::now()));
    });
}

pub(in crate::host_window) fn drive_session_frame_paced(
    host: &dyn SessionSurfaceHost,
) -> Result<bool> {
    let pace = AUTOMATION_FRAME_PACER.with(|pacer| {
        let mut pacer = pacer.borrow_mut();
        match pacer.as_mut() {
            Some((clock, started_at)) => clock.poll(started_at.elapsed().as_nanos() as i64),
            None => Some(crate::frame_clock::FramePace {
                elapsed_intervals: 1,
            }),
        }
    });
    let Some(pace) = pace else {
        return Ok(true);
    };

    drive_session_frame(host, pace.skipped_intervals())
}

pub(in crate::host_window) fn drive_session_frame(
    host: &dyn SessionSurfaceHost,
    skipped_intervals: i64,
) -> Result<bool> {
    let Some(handler) = ACTIVE_CLICK_HANDLER.with(|handler| *handler.borrow()) else {
        return Ok(false);
    };

    let outcome = handler.drive_frame(skipped_intervals)?;
    NEXT_WAKE_DELAY_MS.with(|cell| cell.set(outcome.next_wake_delay_millis));
    MAIN_THREAD_IDLE.with(|cell| cell.set(outcome.main_thread_idle));
    if let Some(surface) = outcome.surface {
        let refresh = apply_refreshed_surface(surface)?;
        if refresh.surface_changed {
            host.request_redraw();
        }
    }

    Ok(outcome.wants_more_frames)
}

pub(in crate::host_window) fn idle_wake_seconds(base_wake_seconds: f64) -> f64 {
    // cite: frameworks-base ViewRootImpl.java#scheduleTraversals
    // cite: androidx AndroidUiDispatcher.android.kt#postFrameCallback
    if pending_idle_surface_refreshes() > 0 {
        return base_wake_seconds;
    }

    match NEXT_WAKE_DELAY_MS.with(|cell| cell.get()) {
        Some(0) => base_wake_seconds,
        Some(delay_ms) => {
            ((delay_ms as f64) / 1000.0).clamp(base_wake_seconds, MAX_IDLE_WAKE_SECONDS)
        }
        None => MAX_IDLE_WAKE_SECONDS,
    }
}

pub(in crate::host_window) fn should_autoclose(
    started_at: Instant,
    autoclose_after: Option<Duration>,
) -> bool {
    match autoclose_after {
        Some(limit) => started_at.elapsed() >= limit,
        None => false,
    }
}

pub(in crate::host_window) fn automation_interactive_input() -> Result<bool> {
    let value = match env::var("WINEDROID_AUTOMATION_INTERACTIVE") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(false),
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_AUTOMATION_INTERACTIVE",
                error,
            ));
        }
    };

    Ok(value == "1" || value.eq_ignore_ascii_case("true"))
}

pub(in crate::host_window) fn autoclose_after() -> Result<Option<Duration>> {
    let value = match env::var("WINEDROID_WINDOW_AUTOCLOSE_MS") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_WINDOW_AUTOCLOSE_MS",
                error,
            ));
        }
    };
    let millis = value.parse::<u64>().map_err(|error| {
        WinedroidError::with_source(
            format!("WINEDROID_WINDOW_AUTOCLOSE_MS is not a valid millisecond value: {value}"),
            error,
        )
    })?;

    Ok(Some(Duration::from_millis(millis)))
}

pub(in crate::host_window) fn save_screenshot_png(png: &[u8]) -> Result<Option<PathBuf>> {
    let Some(dir) = SCREENSHOT_DIR.with(|dir| dir.borrow().clone()) else {
        return Ok(None);
    };

    let path = dir.save(&screenshot_file_name(), png)?;

    Ok(Some(path))
}

fn screenshot_file_name() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_millis())
        .unwrap_or(0);

    format!("Screenshot_{millis}.png")
}

pub(in crate::host_window) fn automation_script_from_env() -> Result<
    Option<(
        AutomationScript,
        Option<PathBuf>,
        Option<PathBuf>,
        Option<PathBuf>,
        Option<PathBuf>,
        Option<PathBuf>,
    )>,
> {
    let path = match env::var("WINEDROID_AUTOMATION_SCRIPT") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_AUTOMATION_SCRIPT",
                error,
            ));
        }
    };

    let contents = fs::read_to_string(&path).map_err(|error| {
        WinedroidError::with_source(format!("failed to read automation script `{path}`"), error)
    })?;
    let script = AutomationScript::parse(&contents)?;

    let log_path = match env::var("WINEDROID_AUTOMATION_LOG") {
        Ok(value) => Some(PathBuf::from(value)),
        Err(env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_AUTOMATION_LOG",
                error,
            ));
        }
    };

    let opcode_log_path = match env::var("WINEDROID_AUTOMATION_OPCODE_LOG") {
        Ok(value) => Some(PathBuf::from(value)),
        Err(env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_AUTOMATION_OPCODE_LOG",
                error,
            ));
        }
    };

    let usage_log_path = match env::var("WINEDROID_AUTOMATION_USAGE_LOG") {
        Ok(value) => Some(PathBuf::from(value)),
        Err(env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_AUTOMATION_USAGE_LOG",
                error,
            ));
        }
    };

    let render_log_path = match env::var("WINEDROID_AUTOMATION_RENDER_LOG") {
        Ok(value) => Some(PathBuf::from(value)),
        Err(env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_AUTOMATION_RENDER_LOG",
                error,
            ));
        }
    };

    let method_log_path = match env::var("WINEDROID_AUTOMATION_METHOD_LOG") {
        Ok(value) => Some(PathBuf::from(value)),
        Err(env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_AUTOMATION_METHOD_LOG",
                error,
            ));
        }
    };

    Ok(Some((
        script,
        log_path,
        opcode_log_path,
        usage_log_path,
        render_log_path,
        method_log_path,
    )))
}

pub(in crate::host_window) fn run_automation_script(
    host: &dyn SessionLoopHost,
    script: AutomationScript,
    autoclose_after: Option<Duration>,
    interval_nanos: i64,
    log_path: Option<PathBuf>,
    opcode_log_path: Option<PathBuf>,
    usage_log_path: Option<PathBuf>,
    render_log_path: Option<PathBuf>,
    method_log_path: Option<PathBuf>,
) -> Result<()> {
    let started_at = Instant::now();
    apply_runtime_frame_interval(interval_nanos)?;
    initialize_automation_frame_pacer(interval_nanos);
    crate::render_metrics::set_enabled(render_log_path.is_some());
    crate::method_metrics::set_enabled(method_log_path.is_some());
    let wake_seconds = (interval_nanos as f64 / NANOS_PER_SECOND_F64).min(MAX_EVENT_WAKE_SECONDS);

    let _ = drive_session_frame_paced(host)?;

    let mut log_lines = vec!["step,t_inject_ms,action,latency_ms,result".to_owned()];
    let mut opcode_log_lines =
        vec!["step,t_inject_ms,action,latency_ms,result,opcode_total,top_opcodes".to_owned()];
    let mut usage_log_lines = vec![
        "step,t_inject_ms,action,latency_ms,result,user_us,system_us,instructions,cycles"
            .to_owned(),
    ];
    let mut render_log_lines = vec![
        "step,t_inject_ms,action,latency_ms,result,frame_count,work_frame_count,dropped_frame_count,jank_frame_count,frame_total_us,frame_max_us,traversal_count,traversal_us,measure_layout_count,measure_layout_us,draw_count,draw_us,retained_replay_count,retained_replay_us,surface_read_count,surface_read_us,surface_present_count,surface_present_us,host_redraw_requests"
            .to_owned(),
    ];
    let mut method_log_lines = vec![
        "step,t_inject_ms,action,latency_ms,result,call_count,call_us,field_op_count,field_op_us,array_op_count,array_op_us,branch_count,branch_us,object_alloc_count,object_alloc_us,builtin_call_count,builtin_call_us,render_callback_count,render_callback_us"
            .to_owned(),
    ];
    let mut step_finished_at = started_at;
    let mut failure: Option<WinedroidError> = None;

    for (index, step) in script.steps.iter().enumerate() {
        let due = step_finished_at + Duration::from_millis(step.delay_ms);
        pump_session_until(host, wake_seconds, due)?;
        if !host.is_alive() {
            break;
        }

        let t_inject_ms = started_at.elapsed().as_millis() as u64;
        let action_text = describe_automation_action(&step.action);
        if opcode_log_path.is_some() {
            reset_opcode_step_histogram();
        }
        if render_log_path.is_some() {
            crate::render_metrics::reset();
        }
        if method_log_path.is_some() {
            crate::method_metrics::reset();
        }
        let usage_before = if usage_log_path.is_some() {
            capture_automation_usage()
        } else {
            None
        };
        let outcome = execute_automation_step(host, wake_seconds, &step.action);
        let usage_after = if usage_log_path.is_some() {
            capture_automation_usage()
        } else {
            None
        };
        let opcode_snapshot = opcode_log_path
            .as_ref()
            .and_then(|_| drain_opcode_step_histogram(256));
        let render_snapshot = render_log_path
            .as_ref()
            .map(|_| crate::render_metrics::drain());
        let method_snapshot = method_log_path
            .as_ref()
            .map(|_| crate::method_metrics::drain());
        let usage_delta = match (usage_before, usage_after) {
            (Some(before), Some(after)) => Some(after.delta_since(before)),
            _ => None,
        };

        let (latency_ms, result_text) = match &outcome {
            Ok(latency_ms) => (*latency_ms, "ok".to_owned()),
            Err(error) => (0, format!("error: {error}")),
        };
        if outcome.is_err() || crate::trace::enabled("automation") {
            println!(
                "[automation] step {} t={t_inject_ms}ms {action_text} latency={latency_ms}ms -> {result_text}",
                index + 1
            );
        }
        log_lines.push(format!(
            "{},{t_inject_ms},{action_text},{latency_ms},{result_text}",
            index + 1
        ));
        if opcode_log_path.is_some() {
            let (opcode_total, top_opcodes) = match opcode_snapshot {
                Some(snapshot) => (
                    snapshot.total,
                    format_opcode_step_entries(&snapshot.entries),
                ),
                None => (0, String::new()),
            };
            opcode_log_lines.push(format!(
                "{},{t_inject_ms},{action_text},{latency_ms},{result_text},{opcode_total},{}",
                index + 1,
                csv_field(&top_opcodes)
            ));
        }
        if usage_log_path.is_some() {
            let (user_us, system_us, instructions, cycles) = match usage_delta {
                Some(delta) => (
                    Some(delta.user_us),
                    Some(delta.system_us),
                    delta.instructions,
                    delta.cycles,
                ),
                None => (None, None, None, None),
            };
            usage_log_lines.push(format!(
                "{},{t_inject_ms},{action_text},{latency_ms},{result_text},{},{},{},{}",
                index + 1,
                optional_u64_csv_field(user_us),
                optional_u64_csv_field(system_us),
                optional_u64_csv_field(instructions),
                optional_u64_csv_field(cycles),
            ));
        }
        if let Some(metrics) = render_snapshot {
            render_log_lines.push(format!(
                "{},{t_inject_ms},{action_text},{latency_ms},{result_text},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                index + 1,
                metrics.frame_count,
                metrics.work_frame_count,
                metrics.dropped_frame_count,
                metrics.jank_frame_count,
                metrics.frame_total_us,
                metrics.frame_max_us,
                metrics.traversal_count,
                metrics.traversal_us,
                metrics.measure_layout_count,
                metrics.measure_layout_us,
                metrics.draw_count,
                metrics.draw_us,
                metrics.retained_replay_count,
                metrics.retained_replay_us,
                metrics.surface_read_count,
                metrics.surface_read_us,
                metrics.surface_present_count,
                metrics.surface_present_us,
                metrics.host_redraw_requests,
            ));
        }
        if let Some(metrics) = method_snapshot {
            method_log_lines.push(format!(
                "{},{t_inject_ms},{action_text},{latency_ms},{result_text},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                index + 1,
                metrics.call_count,
                metrics.call_us,
                metrics.field_op_count,
                metrics.field_op_us,
                metrics.array_op_count,
                metrics.array_op_us,
                metrics.branch_count,
                metrics.branch_us,
                metrics.object_alloc_count,
                metrics.object_alloc_us,
                metrics.builtin_call_count,
                metrics.builtin_call_us,
                metrics.render_callback_count,
                metrics.render_callback_us,
            ));
        }

        step_finished_at = Instant::now();

        if let Err(error) = outcome {
            failure = Some(WinedroidError::new(format!(
                "automation step {} ({action_text}) failed: {error}",
                index + 1
            )));
            break;
        }
    }

    if let Some(log_path) = &log_path {
        let body = format!("{}\n", log_lines.join("\n"));
        fs::write(log_path, body).map_err(|error| {
            WinedroidError::with_source(
                format!("failed to write automation log `{}`", log_path.display()),
                error,
            )
        })?;
    }

    if let Some(opcode_log_path) = &opcode_log_path {
        let body = format!("{}\n", opcode_log_lines.join("\n"));
        fs::write(opcode_log_path, body).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to write automation opcode log `{}`",
                    opcode_log_path.display()
                ),
                error,
            )
        })?;
    }

    if let Some(usage_log_path) = &usage_log_path {
        let body = format!("{}\n", usage_log_lines.join("\n"));
        fs::write(usage_log_path, body).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to write automation usage log `{}`",
                    usage_log_path.display()
                ),
                error,
            )
        })?;
    }

    if let Some(render_log_path) = &render_log_path {
        let body = format!("{}\n", render_log_lines.join("\n"));
        fs::write(render_log_path, body).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to write automation render log `{}`",
                    render_log_path.display()
                ),
                error,
            )
        })?;
    }

    if let Some(method_log_path) = &method_log_path {
        let body = format!("{}\n", method_log_lines.join("\n"));
        fs::write(method_log_path, body).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to write automation method log `{}`",
                    method_log_path.display()
                ),
                error,
            )
        })?;
    }

    crate::render_metrics::set_enabled(false);
    crate::method_metrics::set_enabled(false);

    if let Some(error) = failure {
        return Err(error);
    }

    while host.is_alive() && !should_autoclose(started_at, autoclose_after) {
        pump_session_once(host, wake_seconds)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct AutomationUsageSnapshot {
    user_us: u64,
    system_us: u64,
    instructions: Option<u64>,
    cycles: Option<u64>,
}

#[derive(Clone, Copy)]
struct AutomationUsageDelta {
    user_us: u64,
    system_us: u64,
    instructions: Option<u64>,
    cycles: Option<u64>,
}

impl AutomationUsageSnapshot {
    fn delta_since(self, before: Self) -> AutomationUsageDelta {
        AutomationUsageDelta {
            user_us: self.user_us.saturating_sub(before.user_us),
            system_us: self.system_us.saturating_sub(before.system_us),
            instructions: optional_saturating_sub(self.instructions, before.instructions),
            cycles: optional_saturating_sub(self.cycles, before.cycles),
        }
    }
}

fn optional_saturating_sub(after: Option<u64>, before: Option<u64>) -> Option<u64> {
    match (after, before) {
        (Some(after), Some(before)) => Some(after.saturating_sub(before)),
        _ => None,
    }
}

fn optional_u64_csv_field(value: Option<u64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn capture_getrusage_usage() -> Option<AutomationUsageSnapshot> {
    let mut usage = MaybeUninit::<libc::rusage>::zeroed();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let usage = unsafe { usage.assume_init() };
    Some(AutomationUsageSnapshot {
        user_us: timeval_to_micros(usage.ru_utime),
        system_us: timeval_to_micros(usage.ru_stime),
        instructions: None,
        cycles: None,
    })
}

fn timeval_to_micros(value: libc::timeval) -> u64 {
    let seconds = u64::try_from(value.tv_sec).unwrap_or(0);
    let micros = u64::try_from(value.tv_usec).unwrap_or(0);
    seconds.saturating_mul(1_000_000).saturating_add(micros)
}

#[cfg(not(target_os = "macos"))]
fn capture_automation_usage() -> Option<AutomationUsageSnapshot> {
    capture_getrusage_usage()
}

#[cfg(target_os = "macos")]
const RUSAGE_INFO_V4: libc::c_int = 4;

#[cfg(target_os = "macos")]
static MACH_TIMEBASE: OnceLock<Option<MachTimebaseInfo>> = OnceLock::new();

#[cfg(target_os = "macos")]
#[repr(C)]
#[allow(dead_code)]
struct RUsageInfoV4 {
    ri_uuid: [u8; 16],
    ri_user_time: u64,
    ri_system_time: u64,
    ri_pkg_idle_wkups: u64,
    ri_interrupt_wkups: u64,
    ri_pageins: u64,
    ri_wired_size: u64,
    ri_resident_size: u64,
    ri_phys_footprint: u64,
    ri_proc_start_abstime: u64,
    ri_proc_exit_abstime: u64,
    ri_child_user_time: u64,
    ri_child_system_time: u64,
    ri_child_pkg_idle_wkups: u64,
    ri_child_interrupt_wkups: u64,
    ri_child_pageins: u64,
    ri_child_elapsed_abstime: u64,
    ri_diskio_bytesread: u64,
    ri_diskio_byteswritten: u64,
    ri_cpu_time_qos_default: u64,
    ri_cpu_time_qos_maintenance: u64,
    ri_cpu_time_qos_background: u64,
    ri_cpu_time_qos_utility: u64,
    ri_cpu_time_qos_legacy: u64,
    ri_cpu_time_qos_user_initiated: u64,
    ri_cpu_time_qos_user_interactive: u64,
    ri_billed_system_time: u64,
    ri_serviced_system_time: u64,
    ri_logical_writes: u64,
    ri_lifetime_max_phys_footprint: u64,
    ri_instructions: u64,
    ri_cycles: u64,
    ri_billed_energy: u64,
    ri_serviced_energy: u64,
    ri_interval_max_phys_footprint: u64,
    ri_runnable_time: u64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn proc_pid_rusage(
        pid: libc::c_int,
        flavor: libc::c_int,
        buffer: *mut libc::c_void,
    ) -> libc::c_int;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> libc::c_int;
}

#[cfg(target_os = "macos")]
fn capture_automation_usage() -> Option<AutomationUsageSnapshot> {
    let mut info = MaybeUninit::<RUsageInfoV4>::zeroed();
    let result = unsafe {
        proc_pid_rusage(
            libc::getpid(),
            RUSAGE_INFO_V4,
            info.as_mut_ptr().cast::<libc::c_void>(),
        )
    };
    if result != 0 {
        return capture_getrusage_usage();
    }
    let info = unsafe { info.assume_init() };
    let user_us = mach_ticks_to_micros(info.ri_user_time)?;
    let system_us = mach_ticks_to_micros(info.ri_system_time)?;
    Some(AutomationUsageSnapshot {
        user_us,
        system_us,
        instructions: Some(info.ri_instructions),
        cycles: Some(info.ri_cycles),
    })
}

#[cfg(target_os = "macos")]
fn mach_ticks_to_micros(ticks: u64) -> Option<u64> {
    let timebase = mach_timebase_info_cached()?;
    let nanos = (ticks as u128).saturating_mul(timebase.numer as u128) / timebase.denom as u128;
    Some((nanos / 1_000).min(u64::MAX as u128) as u64)
}

#[cfg(target_os = "macos")]
fn mach_timebase_info_cached() -> Option<MachTimebaseInfo> {
    *MACH_TIMEBASE.get_or_init(|| {
        let mut info = MaybeUninit::<MachTimebaseInfo>::zeroed();
        let result = unsafe { mach_timebase_info(info.as_mut_ptr()) };
        if result != 0 {
            return None;
        }
        let info = unsafe { info.assume_init() };
        if info.denom == 0 {
            return None;
        }
        Some(info)
    })
}

fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn format_opcode_step_entries(
    entries: &[crate::dex_interpreter::OpcodeStepHistogramEntry],
) -> String {
    entries
        .iter()
        .map(|entry| format!("0x{:02x}:{}:{}", entry.opcode, entry.name, entry.count))
        .collect::<Vec<_>>()
        .join(";")
}

fn execute_automation_step(
    host: &dyn SessionLoopHost,
    wake_seconds: f64,
    action: &AutomationAction,
) -> Result<u64> {
    match action {
        AutomationAction::Tap { target, hold_ms } => {
            let injected_at = Instant::now();
            let (x, y) = resolve_automation_tap_target(target)?;
            if *hold_ms > 0 {
                session_touch_hold(host, x, y, *hold_ms)?;
            } else {
                click_rendered_point(host, x, y, "Host automation tap")?;
            }
            Ok(injected_at.elapsed().as_millis() as u64)
        }
        AutomationAction::Scroll { delta_y, point } => {
            let injected_at = Instant::now();
            let Some((width, height)) = session_click_surface_size() else {
                return Err(WinedroidError::new(
                    "scroll has no active render surface size",
                ));
            };
            let point = point.unwrap_or((width / 2, height / 2));
            let changed = scroll_active_surface(
                host,
                *delta_y,
                (point.0 as f64, point.1 as f64),
                width,
                height,
            );
            if crate::trace::enabled("click") {
                println!("Host automation scroll");
                println!("  delta y: {delta_y}");
                println!("  point: {}:{}", point.0, point.1);
                println!("  changed: {changed}");
            }
            Ok(injected_at.elapsed().as_millis() as u64)
        }
        AutomationAction::WaitIdle { timeout_ms } => {
            let injected_at = Instant::now();
            let deadline = injected_at + Duration::from_millis(*timeout_ms);
            loop {
                drive_session_frame_for_wait(host)?;
                if MAIN_THREAD_IDLE.with(|cell| cell.get()) && pending_idle_surface_refreshes() == 0
                {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(WinedroidError::new(
                        "waitidle timed out before the runtime settled",
                    ));
                }
                host.pump_events(wake_seconds)?;
            }
            Ok(injected_at.elapsed().as_millis() as u64)
        }
        AutomationAction::WaitText { text, timeout_ms } => {
            let injected_at = Instant::now();
            let deadline = injected_at + Duration::from_millis(*timeout_ms);
            loop {
                drive_session_frame_for_wait(host)?;
                if active_surface_contains_text(text) {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(WinedroidError::new(format!(
                        "waittext `{text}` timed out before it appeared"
                    )));
                }
                host.pump_events(wake_seconds)?;
            }
            Ok(injected_at.elapsed().as_millis() as u64)
        }
        AutomationAction::Assert { kind } => {
            assert_automation_condition(kind)?;
            Ok(0)
        }
        AutomationAction::Screenshot { path } => {
            let target = path
                .clone()
                .unwrap_or_else(|| PathBuf::from("automation-screenshot.png"));
            screenshot_active_surface(&target)?;
            Ok(0)
        }
        AutomationAction::Dump { kind, path } => {
            match kind {
                DumpKind::Ui => dump_active_surface_ui(path.as_deref())?,
                DumpKind::Bounds | DumpKind::Surface => dump_active_surface_bounds(),
            }
            Ok(0)
        }
        AutomationAction::Swipe {
            start,
            end,
            duration_ms,
            steps,
        } => {
            let injected_at = Instant::now();
            session_touch_swipe(host, *start, *end, *duration_ms, *steps)?;
            Ok(injected_at.elapsed().as_millis() as u64)
        }
        AutomationAction::Pointer { .. } => Err(automation_not_implemented(
            "pointer down/move/up",
            "stateful automation pointer sessions",
        )),
        AutomationAction::Key { name } => {
            let injected_at = Instant::now();
            let (characters, key) = automation_key_event(name)?;
            session_key_input(host, characters, key)?;
            Ok(injected_at.elapsed().as_millis() as u64)
        }
        AutomationAction::Text { text } => {
            let injected_at = Instant::now();
            session_key_input(host, text, InputKey::Text)?;
            Ok(injected_at.elapsed().as_millis() as u64)
        }
    }
}

fn resolve_automation_tap_target(target: &TapTarget) -> Result<(u32, u32)> {
    match target {
        TapTarget::Point { x, y } => Ok((*x, *y)),
        TapTarget::Text(text) => {
            resolve_tap_node_center(&format!("text {text:?}"), |node| node.text == *text)
        }
        TapTarget::ViewId(view_id) => {
            let expected = expected_tap_resource_id(view_id)?;
            resolve_tap_node_center(&format!("id `{view_id}`"), move |node| match &expected {
                TapResourceIdMatch::Exact(name) => node.resource_id == *name,
                TapResourceIdMatch::Suffix(suffix) => node.resource_id.ends_with(suffix),
            })
        }
    }
}

enum TapResourceIdMatch {
    Exact(String),
    Suffix(String),
}

fn expected_tap_resource_id(view_id: &str) -> Result<TapResourceIdMatch> {
    let numeric = if let Some(hex) = view_id.strip_prefix("0x") {
        Some(u32::from_str_radix(hex, 16).map_err(|_| {
            WinedroidError::new(format!("tap id `{view_id}` is not a valid hex view id"))
        })? as i32)
    } else if view_id.chars().all(|c| c.is_ascii_digit()) {
        Some(view_id.parse::<i64>().map_err(|_| {
            WinedroidError::new(format!("tap id `{view_id}` is not a valid view id"))
        })? as i32)
    } else {
        None
    };

    if let Some(numeric) = numeric {
        let surface = ACTIVE_RENDER_SURFACE.with(|surface| surface.borrow().clone());
        let name = surface
            .as_ref()
            .and_then(|surface| surface.resource_id_names.get(&numeric).cloned())
            .unwrap_or_else(|| format!("0x{:08x}", numeric as u32));
        return Ok(TapResourceIdMatch::Exact(name));
    }

    if view_id.contains(":id/") {
        return Ok(TapResourceIdMatch::Exact(view_id.to_owned()));
    }

    Ok(TapResourceIdMatch::Suffix(format!(":id/{view_id}")))
}

fn resolve_tap_node_center(
    description: &str,
    matcher: impl Fn(&crate::host_window::UiDumpNode) -> bool,
) -> Result<(u32, u32)> {
    let Some((width, height)) = ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow()) else {
        return Err(WinedroidError::new(format!(
            "tap {description} has no active render surface"
        )));
    };
    let Some(surface) = ACTIVE_RENDER_SURFACE.with(|surface| surface.borrow().clone()) else {
        return Err(WinedroidError::new(format!(
            "tap {description} has no active render surface"
        )));
    };
    let surface = scrolled_render_surface(&surface, width, height);
    let root = crate::host_window::build_activity_surface_dump_tree(width, height, &surface);

    let Some(node) = find_topmost_dump_node(&root, &matcher) else {
        return Err(WinedroidError::new(format!(
            "tap {description} matched no on-screen node"
        )));
    };

    Ok((
        node.left.saturating_add(node.right) / 2,
        node.top.saturating_add(node.bottom) / 2,
    ))
}

fn find_topmost_dump_node<'a>(
    node: &'a crate::host_window::UiDumpNode,
    matcher: &impl Fn(&crate::host_window::UiDumpNode) -> bool,
) -> Option<&'a crate::host_window::UiDumpNode> {
    for child in node.children.iter().rev() {
        if let Some(found) = find_topmost_dump_node(child, matcher) {
            return Some(found);
        }
    }

    if matcher(node) {
        return Some(node);
    }

    None
}

fn drive_session_frame_for_wait(host: &dyn SessionSurfaceHost) -> Result<()> {
    // cite: frameworks-base ViewRootImpl.java#scheduleTraversals, doTraversal
    if refresh_idle_render_surface() {
        let _ = drive_session_frame(host, 0)?;
        return Ok(());
    }

    let _ = drive_session_frame_paced(host)?;
    Ok(())
}

pub(in crate::host_window) fn automation_not_implemented(
    what: &str,
    primitive: &str,
) -> WinedroidError {
    WinedroidError::new(format!(
        "automation `{what}` is not implemented yet\nnext missing primitive: {primitive}"
    ))
}

pub(in crate::host_window) fn automation_key_event(
    name: &KeyName,
) -> Result<(&'static str, InputKey)> {
    match name {
        KeyName::Enter => Ok(("\r", InputKey::Enter)),
        KeyName::Del => Ok(("", InputKey::DeleteBackward)),
        KeyName::Tab => Ok(("\t", InputKey::Text)),
        KeyName::Space => Ok((" ", InputKey::Text)),
        KeyName::Back | KeyName::Home | KeyName::Menu | KeyName::VolumeUp | KeyName::VolumeDown => {
            Err(automation_not_implemented(
                "navigation/volume key events",
                "Android navigation/volume KeyEvent dispatch",
            ))
        }
    }
}

fn assert_automation_condition(kind: &AssertKind) -> Result<()> {
    match kind {
        AssertKind::Text(text) => {
            if active_surface_contains_text(text) {
                Ok(())
            } else {
                Err(WinedroidError::new(format!(
                    "assert text `{text}` was not present on the surface"
                )))
            }
        }
        AssertKind::NoText(text) => {
            if active_surface_contains_text(text) {
                Err(WinedroidError::new(format!(
                    "assert notext `{text}` was present on the surface"
                )))
            } else {
                Ok(())
            }
        }
        AssertKind::View(view_id) => {
            if active_surface_contains_view(*view_id) {
                Ok(())
            } else {
                Err(WinedroidError::new(format!(
                    "assert view {view_id} was not present on the surface"
                )))
            }
        }
        AssertKind::Display(text) => {
            if active_surface_has_exact_text(text) {
                Ok(())
            } else {
                Err(WinedroidError::new(format!(
                    "assert display `{text}` did not exactly match any text element"
                )))
            }
        }
    }
}

fn active_surface_contains_text(needle: &str) -> bool {
    active_surface_matches_text(|text| text.contains(needle))
}

fn active_surface_has_exact_text(needle: &str) -> bool {
    active_surface_matches_text(|text| text == needle)
}

fn active_surface_matches_text(matches: impl Fn(&str) -> bool) -> bool {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        surface.borrow().as_ref().is_some_and(|surface| {
            surface
                .text_elements
                .iter()
                .any(|element| matches(&element.text))
                || surface
                    .overlays
                    .iter()
                    .flat_map(|overlay| overlay.items.iter())
                    .any(|item| matches(&item.text))
                || surface.dialogs.iter().any(|dialog| {
                    dialog.title.as_deref().is_some_and(&matches)
                        || dialog
                            .text_elements
                            .iter()
                            .any(|element| matches(&element.text))
                        || dialog.items.iter().any(|item| matches(&item.text))
                        || dialog.buttons.iter().any(|button| matches(&button.text))
                })
                || surface
                    .runtime_canvas_frames
                    .iter()
                    .any(|frame| canvas_ops_match_text(&frame.ops, &matches))
        })
    })
}

fn canvas_ops_match_text(
    ops: &[crate::host_window::ActivityRuntimeCanvasOp],
    matches: &impl Fn(&str) -> bool,
) -> bool {
    ops.iter().any(|op| match op {
        crate::host_window::ActivityRuntimeCanvasOp::Text { text, .. } => matches(text),
        crate::host_window::ActivityRuntimeCanvasOp::Bitmap { ops, .. }
        | crate::host_window::ActivityRuntimeCanvasOp::RenderNode { ops, .. } => {
            canvas_ops_match_text(ops, matches)
        }
        _ => false,
    })
}

fn active_surface_contains_view(view_id: i32) -> bool {
    ACTIVE_RENDER_SURFACE.with(|surface| {
        surface.borrow().as_ref().is_some_and(|surface| {
            surface
                .text_elements
                .iter()
                .any(|element| element.view_id == Some(view_id))
        })
    })
}

fn screenshot_active_surface(path: &Path) -> Result<()> {
    let Some((width, height)) = ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow()) else {
        return Err(WinedroidError::new(
            "automation screenshot has no active render surface size",
        ));
    };
    let Some(surface) = ACTIVE_RENDER_SURFACE.with(|surface| surface.borrow().clone()) else {
        return Err(WinedroidError::new(
            "automation screenshot has no active render surface",
        ));
    };
    let surface = scrolled_render_surface(&surface, width, height);
    let image = present_on_display_panel(skia_renderer::render_activity_surface(
        width,
        height,
        &surface.message,
        &surface.text_elements,
        &surface.runtime_canvas_frames,
        &surface.overlays,
        &surface.dialogs,
        surface.action_bar.as_ref(),
        surface.floating_panel,
        surface.window_background,
    )?)?;
    fs::write(path, &image.png).map_err(|error| {
        WinedroidError::with_source(
            format!("failed to write automation screenshot `{}`", path.display()),
            error,
        )
    })
}

pub(in crate::host_window) fn dump_active_surface_bounds() {
    let size = ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow());
    let surface = ACTIVE_RENDER_SURFACE.with(|surface| surface.borrow().clone());
    if let (Some((width, height)), Some(surface)) = (size, surface) {
        let surface = scrolled_render_surface(&surface, width, height);
        crate::host_window::trace_activity_surface_bounds(
            "automation dump",
            width,
            height,
            &surface,
        );
        crate::host_window::trace_activity_surface_runtime_canvas_ops("automation dump", &surface);
    }
}

pub(in crate::host_window) fn active_surface_ui_dump() -> Option<String> {
    let (width, height) = ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow())?;
    let surface = ACTIVE_RENDER_SURFACE.with(|surface| surface.borrow().clone())?;
    let surface = scrolled_render_surface(&surface, width, height);
    let (package, _activity) = ACTIVE_RENDER_IDENTITY
        .with(|identity| identity.borrow().clone())
        .unwrap_or_else(|| ("unknown".to_owned(), "unknown".to_owned()));
    Some(crate::host_window::activity_surface_ui_dump(
        width, height, &package, &surface,
    ))
}

pub(in crate::host_window) fn dump_active_surface_ui(path: Option<&Path>) -> Result<()> {
    let Some(dump) = active_surface_ui_dump() else {
        return Ok(());
    };
    match path {
        Some(path) => fs::write(path, &dump).map_err(|error| {
            WinedroidError::with_source(
                format!("failed to write UI dump to `{}`", path.display()),
                error,
            )
        }),
        None => {
            print!("{dump}");
            Ok(())
        }
    }
}

fn describe_automation_action(action: &AutomationAction) -> String {
    match action {
        AutomationAction::Tap { target, hold_ms } => {
            let described = match target {
                TapTarget::Point { x, y } => format!("tap {x}:{y}"),
                TapTarget::Text(text) => format!("tap text {text:?}"),
                TapTarget::ViewId(view_id) => format!("tap id {view_id}"),
            };
            if *hold_ms > 0 {
                format!("{described} hold {hold_ms}ms")
            } else {
                described
            }
        }
        AutomationAction::Swipe {
            start,
            end,
            duration_ms,
            ..
        } => format!(
            "swipe {}:{} {}:{} {duration_ms}ms",
            start.0, start.1, end.0, end.1
        ),
        AutomationAction::Pointer {
            kind,
            pointer,
            x,
            y,
            ..
        } => format!("pointer {kind:?} ptr{pointer} {x}:{y}"),
        AutomationAction::Scroll { delta_y, point } => match point {
            Some((x, y)) => format!("scroll {delta_y} {x}:{y}"),
            None => format!("scroll {delta_y}"),
        },
        AutomationAction::Key { name } => format!("key {name:?}"),
        AutomationAction::Text { text } => format!("text {text:?}"),
        AutomationAction::WaitIdle { timeout_ms } => format!("waitidle {timeout_ms}ms"),
        AutomationAction::WaitText { text, timeout_ms } => {
            format!("waittext {text:?} {timeout_ms}ms")
        }
        AutomationAction::Assert { kind } => format!("assert {kind:?}"),
        AutomationAction::Screenshot { .. } => "screenshot".to_owned(),
        AutomationAction::Dump { .. } => "dump".to_owned(),
    }
}

pub(in crate::host_window) fn pump_session_until(
    host: &dyn SessionLoopHost,
    wake_seconds: f64,
    deadline: Instant,
) -> Result<()> {
    while host.is_alive() && Instant::now() < deadline {
        pump_session_once(host, wake_seconds)?;
    }
    Ok(())
}

pub(in crate::host_window) fn pump_session_once(
    host: &dyn SessionLoopHost,
    wake_seconds: f64,
) -> Result<()> {
    host.pump_events(wake_seconds)?;
    let _ = refresh_idle_render_surface();
    let _ = drive_session_frame_paced(host)?;
    host.after_pump()?;
    Ok(())
}

pub(in crate::host_window) fn drain_idle_surface_refreshes(
    host: &dyn SessionSurfaceHost,
) -> Result<()> {
    if ACTIVE_CLICK_HANDLER.with(|handler| handler.borrow().is_none()) {
        return Ok(());
    }

    let mut refresh_count = 0usize;
    while take_pending_idle_surface_refresh() {
        // cite: frameworks-base ViewRootImpl.java#scheduleTraversals, doTraversal
        let wants_more_frames = drive_session_frame(host, 0)?;
        refresh_count = refresh_count.saturating_add(1);

        if !wants_more_frames {
            break;
        }
    }

    if refresh_count > 0 {
        crate::trace::event(
            "ui",
            format!("host autoclick drained {refresh_count} idle surface refreshes"),
        );
    }

    Ok(())
}

pub(in crate::host_window) fn auto_click_target_error_detail(
    target: RenderedClickTarget,
) -> String {
    match target {
        RenderedClickTarget::View(view_id) => format!("view id: {view_id}"),
        RenderedClickTarget::FocusView(view_id) => format!("focus view id: {view_id}"),
        RenderedClickTarget::Preference(preference_key) => {
            format!("preference key: {preference_key}")
        }
        RenderedClickTarget::DialogButton {
            dialog_index,
            button_index,
        } => format!("dialog index: {dialog_index}\nbutton index: {button_index}"),
        RenderedClickTarget::DialogItem {
            dialog_index,
            item_index,
        } => format!("dialog index: {dialog_index}\nitem index: {item_index}"),
        RenderedClickTarget::DialogSurface { dialog_index } => {
            format!("dialog index: {dialog_index}\ndialog surface")
        }
        RenderedClickTarget::Point { x, y } => format!("point: {x}:{y}"),
    }
}

pub(in crate::host_window) fn perform_session_auto_click_target(
    host: &dyn SessionSurfaceHost,
    target: RenderedClickTarget,
) -> Result<()> {
    LAST_CLICK_RESULT.with(|result| {
        *result.borrow_mut() = None;
    });

    match &target {
        RenderedClickTarget::View(view_id) => {
            click_rendered_view(host, *view_id, "Host renderer autoclick");
        }
        RenderedClickTarget::Point { x, y } => {
            click_rendered_point(host, *x, *y, "Host renderer autoclick")?;
        }
        _ => {
            click_rendered_target(host, target.clone(), "Host renderer autoclick");
        }
    }

    let result = LAST_CLICK_RESULT.with(|result| result.borrow_mut().take());
    match result {
        Some(NativeClickResult::Handled(_)) => Ok(()),
        Some(NativeClickResult::Error(error)) => Err(WinedroidError::new(format!(
            "host autoclick failed\n{}\nerror: {error}",
            auto_click_target_error_detail(target)
        ))),
        None => Err(WinedroidError::new(format!(
            "host autoclick did not reach target/action\n{}",
            auto_click_target_error_detail(target)
        ))),
    }
}

pub(in crate::host_window) fn perform_session_auto_clicks(
    host: &dyn SessionSurfaceHost,
    targets: &[RenderedClickTarget],
) -> Result<()> {
    for target in targets {
        perform_session_auto_click_target(host, target.clone())?;
        drain_idle_surface_refreshes(host)?;
    }

    Ok(())
}

pub(in crate::host_window) fn auto_click_targets() -> Result<Vec<RenderedClickTarget>> {
    let ordered_targets = ordered_auto_click_targets()?;
    let view_ids = auto_click_view_ids()?;
    let dialog_buttons = auto_click_dialog_buttons()?;

    if let Some(targets) = ordered_targets {
        if !view_ids.is_empty() || !dialog_buttons.is_empty() {
            return Err(WinedroidError::new(
                    "set either WINEDROID_WINDOW_AUTOCLICK_TARGETS or the legacy autoclick env vars, not both",
                ));
        }

        return Ok(targets);
    }

    let mut targets = Vec::new();
    targets.extend(view_ids.into_iter().map(RenderedClickTarget::View));
    targets.extend(
        dialog_buttons
            .into_iter()
            .map(
                |(dialog_index, button_index)| RenderedClickTarget::DialogButton {
                    dialog_index,
                    button_index,
                },
            ),
    );

    Ok(targets)
}

fn ordered_auto_click_targets() -> Result<Option<Vec<RenderedClickTarget>>> {
    let value = match env::var("WINEDROID_WINDOW_AUTOCLICK_TARGETS") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_WINDOW_AUTOCLICK_TARGETS",
                error,
            ));
        }
    };
    let value = value.trim();

    if value.is_empty() {
        return Err(WinedroidError::new(
            "WINEDROID_WINDOW_AUTOCLICK_TARGETS is empty",
        ));
    }

    let mut targets = Vec::new();
    for item in value.split(',') {
        targets.push(parse_auto_click_target(item.trim())?);
    }

    Ok(Some(targets))
}

fn parse_auto_click_target(value: &str) -> Result<RenderedClickTarget> {
    if value.is_empty() {
        return Err(WinedroidError::new(
            "WINEDROID_WINDOW_AUTOCLICK_TARGETS contains an empty target",
        ));
    }

    let Some((kind, argument)) = value.split_once(':') else {
        return Err(WinedroidError::new(format!(
                "WINEDROID_WINDOW_AUTOCLICK_TARGETS target must use view:id, preference:runtime-key, dialog:dialog_index:button_index, dialog-item:dialog_index:item_index, or point:x:y: {value}"
            )));
    };

    match kind {
        "view" => Ok(RenderedClickTarget::View(
            parse_positive_auto_click_view_id("WINEDROID_WINDOW_AUTOCLICK_TARGETS", argument)?,
        )),
        "preference" => parse_auto_click_preference_target(argument),
        "dialog" => parse_auto_click_dialog_target(argument),
        "dialog-item" => parse_auto_click_dialog_item_target(argument),
        "point" => parse_auto_click_point_target(argument),
        _ => Err(WinedroidError::new(format!(
            "WINEDROID_WINDOW_AUTOCLICK_TARGETS contains unsupported target kind: {kind}"
        ))),
    }
}

fn parse_auto_click_preference_target(value: &str) -> Result<RenderedClickTarget> {
    if value.is_empty() {
        return Err(WinedroidError::new(
            "WINEDROID_WINDOW_AUTOCLICK_TARGETS preference target is empty",
        ));
    }

    Ok(RenderedClickTarget::Preference(value.to_owned()))
}

fn parse_auto_click_dialog_target(value: &str) -> Result<RenderedClickTarget> {
    let Some((dialog_index, button_index)) = value.split_once(':') else {
        return Err(WinedroidError::new(format!(
                "WINEDROID_WINDOW_AUTOCLICK_TARGETS dialog target must use dialog:dialog_index:button_index: dialog:{value}"
            )));
    };
    let dialog_index = parse_auto_click_dialog_button_index(
        "WINEDROID_WINDOW_AUTOCLICK_TARGETS",
        "dialog index",
        dialog_index.trim(),
    )?;
    let button_index = parse_auto_click_dialog_button_index(
        "WINEDROID_WINDOW_AUTOCLICK_TARGETS",
        "button index",
        button_index.trim(),
    )?;

    Ok(RenderedClickTarget::DialogButton {
        dialog_index,
        button_index,
    })
}

fn parse_auto_click_dialog_item_target(value: &str) -> Result<RenderedClickTarget> {
    let Some((dialog_index, item_index)) = value.split_once(':') else {
        return Err(WinedroidError::new(format!(
                "WINEDROID_WINDOW_AUTOCLICK_TARGETS dialog item target must use dialog-item:dialog_index:item_index: dialog-item:{value}"
            )));
    };
    let dialog_index = parse_auto_click_dialog_button_index(
        "WINEDROID_WINDOW_AUTOCLICK_TARGETS",
        "dialog index",
        dialog_index.trim(),
    )?;
    let item_index = parse_auto_click_dialog_button_index(
        "WINEDROID_WINDOW_AUTOCLICK_TARGETS",
        "item index",
        item_index.trim(),
    )?;

    Ok(RenderedClickTarget::DialogItem {
        dialog_index,
        item_index,
    })
}

fn parse_auto_click_point_target(value: &str) -> Result<RenderedClickTarget> {
    let Some((x, y)) = value.split_once(':') else {
        return Err(WinedroidError::new(format!(
            "WINEDROID_WINDOW_AUTOCLICK_TARGETS point target must use point:x:y: point:{value}"
        )));
    };
    let x = parse_auto_click_point_coordinate("WINEDROID_WINDOW_AUTOCLICK_TARGETS", "x", x.trim())?;
    let y = parse_auto_click_point_coordinate("WINEDROID_WINDOW_AUTOCLICK_TARGETS", "y", y.trim())?;

    Ok(RenderedClickTarget::Point { x, y })
}

fn parse_auto_click_point_coordinate(env_name: &str, label: &str, value: &str) -> Result<u32> {
    if value.is_empty() {
        return Err(WinedroidError::new(format!(
            "{env_name} contains an empty {label} coordinate"
        )));
    }

    value.parse::<u32>().map_err(|error| {
        WinedroidError::with_source(
            format!("{env_name} contains an invalid {label} coordinate: {value}"),
            error,
        )
    })
}

fn auto_click_view_ids() -> Result<Vec<i32>> {
    let single_value = match env::var("WINEDROID_WINDOW_AUTOCLICK_VIEW_ID") {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_WINDOW_AUTOCLICK_VIEW_ID",
                error,
            ));
        }
    };
    let sequence_value = match env::var("WINEDROID_WINDOW_AUTOCLICK_VIEW_IDS") {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_WINDOW_AUTOCLICK_VIEW_IDS",
                error,
            ));
        }
    };

    if single_value.is_some() && sequence_value.is_some() {
        return Err(WinedroidError::new(
                "set only one autoclick env var: WINEDROID_WINDOW_AUTOCLICK_VIEW_ID or WINEDROID_WINDOW_AUTOCLICK_VIEW_IDS",
            ));
    }

    if let Some(value) = single_value {
        return Ok(vec![parse_positive_auto_click_view_id(
            "WINEDROID_WINDOW_AUTOCLICK_VIEW_ID",
            &value,
        )?]);
    }

    let Some(value) = sequence_value else {
        return Ok(Vec::new());
    };

    if value.trim().is_empty() {
        return Err(WinedroidError::new(
            "WINEDROID_WINDOW_AUTOCLICK_VIEW_IDS is empty",
        ));
    }

    let mut view_ids = Vec::new();
    for item in value.split(',') {
        view_ids.push(parse_positive_auto_click_view_id(
            "WINEDROID_WINDOW_AUTOCLICK_VIEW_IDS",
            item.trim(),
        )?);
    }

    Ok(view_ids)
}

fn parse_positive_auto_click_view_id(env_name: &str, value: &str) -> Result<i32> {
    if value.is_empty() {
        return Err(WinedroidError::new(format!(
            "{env_name} contains an empty view id"
        )));
    }

    let view_id = parse_view_id(value).map_err(|error| {
        WinedroidError::with_source(
            format!("{env_name} contains an invalid view id: {value}"),
            error,
        )
    })?;

    if view_id <= 0 {
        return Err(WinedroidError::new(format!(
            "{env_name} view ids must be positive: {view_id}"
        )));
    }

    Ok(view_id)
}

fn auto_click_dialog_buttons() -> Result<Vec<(usize, usize)>> {
    let value = match env::var("WINEDROID_WINDOW_AUTOCLICK_DIALOG_BUTTON") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(Vec::new()),
        Err(error) => {
            return Err(WinedroidError::with_source(
                "failed to read WINEDROID_WINDOW_AUTOCLICK_DIALOG_BUTTON",
                error,
            ));
        }
    };
    let value = value.trim();

    if value.is_empty() {
        return Err(WinedroidError::new(
            "WINEDROID_WINDOW_AUTOCLICK_DIALOG_BUTTON is empty",
        ));
    }

    let Some((dialog_index, button_index)) = value.split_once(':') else {
        return Err(WinedroidError::new(
            "WINEDROID_WINDOW_AUTOCLICK_DIALOG_BUTTON must use dialog_index:button_index",
        ));
    };
    let dialog_index = parse_auto_click_dialog_button_index(
        "WINEDROID_WINDOW_AUTOCLICK_DIALOG_BUTTON",
        "dialog index",
        dialog_index.trim(),
    )?;
    let button_index = parse_auto_click_dialog_button_index(
        "WINEDROID_WINDOW_AUTOCLICK_DIALOG_BUTTON",
        "button index",
        button_index.trim(),
    )?;

    Ok(vec![(dialog_index, button_index)])
}

fn parse_auto_click_dialog_button_index(env_name: &str, label: &str, value: &str) -> Result<usize> {
    if value.is_empty() {
        return Err(WinedroidError::new(format!(
            "{env_name} contains an empty {label}"
        )));
    }

    value.parse::<usize>().map_err(|error| {
        WinedroidError::with_source(
            format!("{env_name} contains an invalid {label}: {value}"),
            error,
        )
    })
}

fn parse_view_id(value: &str) -> std::result::Result<i32, std::num::ParseIntError> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        i32::from_str_radix(hex, 16)
    } else {
        value.parse::<i32>()
    }
}

#[cfg(test)]
mod scheduled_surface_tests {
    use super::*;
    use crate::host_window::FrameDriveOutcome;
    use std::ffi::c_void;

    struct TestHost {
        redraws: Cell<usize>,
    }

    struct TestCallbacks {
        read_surface_calls: Cell<usize>,
        drive_frame_calls: Cell<usize>,
    }

    impl SessionSurfaceHost for TestHost {
        fn request_redraw(&self) {
            self.redraws.set(self.redraws.get() + 1);
        }

        fn refresh_native_text(
            &self,
            _handler: &ActivityClickHandler,
        ) -> Result<Vec<NativeTextUpdate>> {
            Ok(vec![NativeTextUpdate {
                view_id: 7,
                text: "typed".to_owned(),
            }])
        }
    }

    #[test]
    fn aosp_viewroot_scheduletraversals_refresh_after_click_defers_surface_read_to_frame() {
        reset_scheduled_surface_test_state();
        let host = TestHost {
            redraws: Cell::new(0),
        };
        let callbacks = TestCallbacks {
            read_surface_calls: Cell::new(0),
            drive_frame_calls: Cell::new(0),
        };
        let handler = test_click_handler(&callbacks);

        let refresh = refresh_after_click(&host, handler, PostInputRefresh::DeferToScheduledFrame)
            .expect("post-click refresh should defer");

        assert_eq!(callbacks.read_surface_calls.get(), 0);
        assert_eq!(callbacks.drive_frame_calls.get(), 0);
        assert_eq!(
            pending_idle_surface_refreshes(),
            DEFERRED_INPUT_FRAME_DRIVE_LIMIT
        );
        assert_eq!(refresh.text_updates.len(), 1);
        assert!(!refresh.surface_changed);
        assert!(!refresh.structure_changed);
        reset_scheduled_surface_test_state();
    }

    #[test]
    fn aosp_viewroot_scheduletraversals_refresh_after_click_reads_committed_surface_without_frame()
    {
        reset_scheduled_surface_test_state();
        let host = TestHost {
            redraws: Cell::new(0),
        };
        let callbacks = TestCallbacks {
            read_surface_calls: Cell::new(0),
            drive_frame_calls: Cell::new(0),
        };
        let handler = test_click_handler(&callbacks);
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(test_surface("old"));
        });

        let refresh = refresh_after_click(&host, handler, PostInputRefresh::ReadCommittedSurface)
            .expect("post-click refresh should read");

        assert_eq!(callbacks.read_surface_calls.get(), 1);
        assert_eq!(callbacks.drive_frame_calls.get(), 0);
        assert_eq!(
            pending_idle_surface_refreshes(),
            POST_CLICK_IDLE_REFRESH_LIMIT
        );
        assert!(refresh.surface_changed);
        assert!(!refresh.structure_changed);
        ACTIVE_RENDER_SURFACE.with(|surface| {
            let surface = surface.borrow();
            assert_eq!(
                surface.as_ref().map(|surface| surface.message.as_str()),
                Some("read")
            );
        });
        reset_scheduled_surface_test_state();
    }

    #[test]
    fn aosp_viewroot_scheduletraversals_deferred_input_does_not_redraw_before_frame_commit() {
        reset_scheduled_surface_test_state();
        let host = TestHost {
            redraws: Cell::new(0),
        };

        request_redraw_after_post_input(&host, PostInputRefresh::DeferToScheduledFrame);
        assert_eq!(host.redraws.get(), 0);

        request_redraw_after_post_input(&host, PostInputRefresh::ReadCommittedSurface);
        assert_eq!(host.redraws.get(), 1);
        reset_scheduled_surface_test_state();
    }

    #[test]
    fn aosp_viewroot_dotraversal_drive_session_frame_commits_changed_surface() {
        reset_scheduled_surface_test_state();
        let host = TestHost {
            redraws: Cell::new(0),
        };
        let callbacks = TestCallbacks {
            read_surface_calls: Cell::new(0),
            drive_frame_calls: Cell::new(0),
        };
        let handler = test_click_handler(&callbacks);
        let _guard = ActiveClickHandlerGuard::install(Some(handler));

        let wants_more = drive_session_frame(&host, 0).expect("frame drive should commit surface");

        assert!(!wants_more);
        assert_eq!(callbacks.drive_frame_calls.get(), 1);
        assert_eq!(callbacks.read_surface_calls.get(), 0);
        assert_eq!(host.redraws.get(), 0);
        ACTIVE_RENDER_SURFACE.with(|surface| {
            let surface = surface.borrow();
            assert_eq!(
                surface.as_ref().map(|surface| surface.message.as_str()),
                Some("driven")
            );
        });
        reset_scheduled_surface_test_state();
    }

    fn reset_scheduled_surface_test_state() {
        PENDING_IDLE_SURFACE_REFRESHES.with(|refreshes| refreshes.set(0));
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = None;
        });
        ACTIVE_RENDER_SURFACE_SIZE.with(|size| {
            *size.borrow_mut() = Some((420, 760));
        });
        NEXT_WAKE_DELAY_MS.with(|cell| cell.set(None));
        MAIN_THREAD_IDLE.with(|cell| cell.set(false));
    }

    fn test_click_handler(callbacks: &TestCallbacks) -> ActivityClickHandler {
        unsafe {
            ActivityClickHandler::new(
                (callbacks as *const TestCallbacks).cast(),
                test_click_view_id,
                test_click_preference,
                test_click_touch,
                test_touch_hold,
                test_touch_swipe,
                test_click_dialog_button,
                test_click_dialog_item,
                test_focus_view,
                test_key_input,
                test_read_view_text,
                test_read_surface,
                test_set_frame_interval,
                test_drive_frame,
            )
        }
    }

    unsafe fn test_click_view_id(_context: *const c_void, _view_id: i32) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_click_preference(_context: *const c_void, _key: &str) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_click_touch(_context: *const c_void, _x: u32, _y: u32) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_touch_hold(
        _context: *const c_void,
        _x: u32,
        _y: u32,
        _hold_ms: u64,
    ) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_touch_swipe(
        _context: *const c_void,
        _start: (u32, u32),
        _end: (u32, u32),
        _duration_ms: u64,
        _steps: Option<u32>,
    ) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_click_dialog_button(
        _context: *const c_void,
        _dialog_index: usize,
        _button_index: usize,
    ) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_click_dialog_item(
        _context: *const c_void,
        _dialog_index: usize,
        _item_index: usize,
    ) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_focus_view(_context: *const c_void, _view_id: i32) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_key_input(
        _context: *const c_void,
        _characters: &str,
        _key: InputKey,
    ) -> Result<i32> {
        Ok(1)
    }

    unsafe fn test_read_view_text(
        _context: *const c_void,
        _view_id: i32,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    unsafe fn test_read_surface(context: *const c_void) -> Result<ActivitySurfaceState> {
        let callbacks = &*(context as *const TestCallbacks);
        callbacks
            .read_surface_calls
            .set(callbacks.read_surface_calls.get() + 1);
        Ok(test_surface("read"))
    }

    unsafe fn test_set_frame_interval(
        _context: *const c_void,
        _frame_interval_nanos: i64,
    ) -> Result<()> {
        Ok(())
    }

    unsafe fn test_drive_frame(
        context: *const c_void,
        _skipped_intervals: i64,
    ) -> Result<FrameDriveOutcome> {
        let callbacks = &*(context as *const TestCallbacks);
        callbacks
            .drive_frame_calls
            .set(callbacks.drive_frame_calls.get() + 1);
        Ok(FrameDriveOutcome {
            surface: Some(test_surface("driven")),
            wants_more_frames: false,
            main_thread_idle: true,
            next_wake_delay_millis: None,
        })
    }

    fn test_surface(message: &str) -> ActivitySurfaceState {
        ActivitySurfaceState {
            message: message.to_owned(),
            text_elements: Vec::new(),
            runtime_canvas_frames: Vec::new(),
            overlays: Vec::new(),
            dialogs: Vec::new(),
            action_bar: None,
            floating_panel: None,
            window_background: None,
            resource_id_names: std::collections::BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod render_scroll_tests {
    use super::*;
    use crate::host_window::{
        ActivityDialogItem, ActivityRect, ActivityRuntimeCanvasFrame, ActivityTextAlignment,
    };

    #[test]
    fn max_render_scroll_uses_clipped_list_content_extent() {
        reset_render_scroll_state();
        let surface = scrollable_surface();

        assert_eq!(max_render_scroll_y(&surface, 200), 60);
    }

    #[test]
    fn scrolled_render_surface_moves_only_clipped_list_items() {
        reset_render_scroll_state();
        let surface = scrollable_surface();
        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            *scroll_y.borrow_mut() = 60;
        });

        let scrolled = scrolled_render_surface(&surface, 200, 200);

        assert_eq!(scrolled.text_elements[0].y, 170);
        assert_eq!(scrolled.text_elements[1].y, 20);
        assert_eq!(scrolled.text_elements[2].y, 40);
        reset_render_scroll_state();
    }

    #[test]
    fn pending_scroll_waits_for_a_scrollable_surface() {
        reset_render_scroll_state();
        PENDING_RENDER_SCROLL_Y.with(|scroll_y| {
            *scroll_y.borrow_mut() = Some(40);
        });
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Main".to_owned(),
                text_elements: Vec::new(),
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        reconcile_active_render_scroll(200);

        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            assert_eq!(*scroll_y.borrow(), 0);
        });
        PENDING_RENDER_SCROLL_Y.with(|scroll_y| {
            assert_eq!(*scroll_y.borrow(), Some(40));
        });

        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(scrollable_surface());
        });
        reconcile_active_render_scroll(200);

        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            assert_eq!(*scroll_y.borrow(), 40);
        });
        PENDING_RENDER_SCROLL_Y.with(|scroll_y| {
            assert_eq!(*scroll_y.borrow(), None);
        });
        reset_render_scroll_state();
    }

    #[test]
    fn active_dialog_surface_consumes_empty_clicks() {
        reset_render_scroll_state();
        let dialog = modal_dialog(Vec::new());
        let frame = activity_dialog_frame(420, 760, &dialog);
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Dialog".to_owned(),
                text_elements: vec![text_element(ActivityTextKind::Button, 100, None)],
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: vec![dialog],
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let target = rendered_click_target_at(
            (
                frame.x.saturating_add(8) as f64,
                frame.y.saturating_add(8) as f64,
            ),
            420,
            760,
        );

        assert_eq!(
            target,
            Some(RenderedClickTarget::DialogSurface { dialog_index: 0 })
        );
        reset_render_scroll_state();
    }

    #[test]
    fn active_dialog_scrim_consumes_background_clicks() {
        reset_render_scroll_state();
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Dialog".to_owned(),
                text_elements: vec![text_element(ActivityTextKind::Button, 100, None)],
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: vec![modal_dialog(Vec::new())],
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let target = rendered_click_target_at((4.0, 4.0), 420, 760);

        assert_eq!(
            target,
            Some(RenderedClickTarget::DialogSurface { dialog_index: 0 })
        );
        reset_render_scroll_state();
    }

    #[test]
    fn background_only_elements_do_not_hide_clickable_elements() {
        reset_render_scroll_state();
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Calculator".to_owned(),
                text_elements: vec![
                    ActivityTextElement {
                        text: "7".to_owned(),
                        kind: ActivityTextKind::Button,
                        view_id: Some(2131296745),
                        preference_key: None,
                        accessory: None,
                        image: None,
                        text_size: None,
                        text_padding_left: 0,
                        text_padding_right: 0,
                        text_color: None,
                        background_color: None,
                        alignment: ActivityTextAlignment::Center,
                        editable: false,
                        cursor: None,
                        x: 8,
                        y: 356,
                        width: 101,
                        height: 101,
                        clip: None,
                        background_corner_radius: 0,
                        background_border_color: None,
                        background_border_width: 0,
                        elevation_px: 0,
                    },
                    ActivityTextElement {
                        text: String::new(),
                        kind: ActivityTextKind::Label,
                        view_id: Some(2131296519),
                        preference_key: None,
                        accessory: None,
                        image: None,
                        text_size: None,
                        text_padding_left: 0,
                        text_padding_right: 0,
                        text_color: None,
                        background_color: Some(0xffee_f0ff),
                        alignment: ActivityTextAlignment::Start,
                        editable: false,
                        cursor: None,
                        x: 0,
                        y: 198,
                        width: 420,
                        height: 511,
                        clip: None,
                        background_corner_radius: 0,
                        background_border_color: None,
                        background_border_width: 0,
                        elevation_px: 0,
                    },
                ],
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let target = rendered_click_target_at((58.0, 353.0), 420, 760);

        assert_eq!(target, Some(RenderedClickTarget::View(2131296745)));
        reset_render_scroll_state();
    }

    #[test]
    fn point_click_resolves_to_the_button_drawn_under_it_not_the_mirrored_row() {
        reset_render_scroll_state();
        let stacked_button = |text: &str, view_id: i32, y: i32| ActivityTextElement {
            text: text.to_owned(),
            kind: ActivityTextKind::Button,
            view_id: Some(view_id),
            preference_key: None,
            accessory: None,
            image: None,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: None,
            alignment: ActivityTextAlignment::Center,
            editable: false,
            cursor: None,
            x: 109,
            y,
            width: 101,
            height: 101,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Calculator".to_owned(),
                text_elements: vec![
                    stacked_button("(", 2131296676, 404),
                    stacked_button("8", 2131296466, 303),
                ],
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let target = rendered_click_target_at((159.0, 406.0), 420, 760);
        assert_eq!(target, Some(RenderedClickTarget::View(2131296466)));

        let target = rendered_click_target_at((159.0, 305.0), 420, 760);
        assert_eq!(target, Some(RenderedClickTarget::View(2131296676)));
        reset_render_scroll_state();
    }

    #[test]
    fn runtime_canvas_root_receives_point_clicks() {
        reset_render_scroll_state();
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Meditation Timer".to_owned(),
                text_elements: vec![ActivityTextElement {
                    text: "Runtime View root\nLxt;".to_owned(),
                    kind: ActivityTextKind::RuntimeViewRoot,
                    view_id: None,
                    preference_key: None,
                    accessory: None,
                    image: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: ActivityTextAlignment::Start,
                    editable: false,
                    cursor: None,
                    x: 0,
                    y: 0,
                    width: 420,
                    height: 760,
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                }],
                runtime_canvas_frames: vec![ActivityRuntimeCanvasFrame {
                    x: 0,
                    y: 0,
                    width: 420,
                    height: 760,
                    ops: Vec::new(),
                }],
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let target = rendered_click_target_at((388.0, 725.0), 420, 760);

        assert_eq!(target, Some(RenderedClickTarget::Point { x: 388, y: 725 }));
        reset_render_scroll_state();
    }

    #[test]
    fn aosp_view_on_touch_event_move_inside_projected_button_still_clicks() {
        reset_render_scroll_state();
        let mut button = text_element(ActivityTextKind::Button, 20, None);
        button.view_id = Some(7);
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Calculator".to_owned(),
                text_elements: vec![button],
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });
        let target = rendered_click_target_at((50.0, 165.0), 100, 200);

        let action = interactive_pointer_release_action(
            (50.0, 165.0),
            (62.0, 165.0),
            8.0,
            pointer_motion(true, false, true),
            target,
            100,
            200,
        );

        assert_eq!(
            action,
            InteractivePointerReleaseAction::Click(RenderedClickTarget::View(7))
        );
        reset_render_scroll_state();
    }

    #[test]
    fn aosp_view_on_touch_event_move_outside_projected_button_cancels_click() {
        reset_render_scroll_state();
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Calculator".to_owned(),
                text_elements: Vec::new(),
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let action = interactive_pointer_release_action(
            (50.0, 165.0),
            (150.0, 165.0),
            8.0,
            pointer_motion(true, false, false),
            Some(RenderedClickTarget::View(7)),
            200,
            200,
        );

        assert_eq!(action, InteractivePointerReleaseAction::Ignore);
        reset_render_scroll_state();
    }

    #[test]
    fn aosp_scrollview_on_intercept_touch_event_routes_projected_vertical_drag_to_scroll() {
        reset_render_scroll_state();
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(scrollable_surface());
        });

        let action = interactive_pointer_release_action(
            (50.0, 100.0),
            (50.0, 80.0),
            8.0,
            pointer_motion(true, true, true),
            Some(RenderedClickTarget::View(7)),
            200,
            200,
        );

        assert_eq!(action, InteractivePointerReleaseAction::ProjectedScroll(12));
        reset_render_scroll_state();
    }

    #[test]
    fn aosp_viewroot_process_pointer_event_routes_runtime_drag_independent_of_projected_target() {
        reset_render_scroll_state();
        let mut surface = scrollable_surface();
        surface.runtime_canvas_frames = vec![ActivityRuntimeCanvasFrame {
            x: 0,
            y: 0,
            width: 200,
            height: 200,
            ops: Vec::new(),
        }];
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(surface);
        });

        let action = interactive_pointer_release_action(
            (50.0, 100.0),
            (50.0, 80.0),
            8.0,
            pointer_motion(true, true, true),
            Some(RenderedClickTarget::View(7)),
            200,
            200,
        );

        assert_eq!(action, InteractivePointerReleaseAction::RuntimeSwipe);
        reset_render_scroll_state();
    }

    #[test]
    fn aosp_window_manager_dialog_blocks_drag_from_underlying_runtime_viewroot() {
        reset_render_scroll_state();
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Dialog".to_owned(),
                text_elements: Vec::new(),
                runtime_canvas_frames: vec![ActivityRuntimeCanvasFrame {
                    x: 0,
                    y: 0,
                    width: 420,
                    height: 760,
                    ops: Vec::new(),
                }],
                overlays: Vec::new(),
                dialogs: vec![modal_dialog(Vec::new())],
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let action = interactive_pointer_release_action(
            (200.0, 380.0),
            (200.0, 350.0),
            8.0,
            pointer_motion(true, true, true),
            Some(RenderedClickTarget::DialogSurface { dialog_index: 0 }),
            420,
            760,
        );

        assert_eq!(
            action,
            InteractivePointerReleaseAction::Click(RenderedClickTarget::DialogSurface {
                dialog_index: 0
            })
        );
        reset_render_scroll_state();
    }

    #[test]
    fn aosp_viewgroup_touch_target_keeps_runtime_root_across_pointer_coordinates() {
        assert!(rendered_touch_targets_match(
            Some(&RenderedClickTarget::Point { x: 10, y: 20 }),
            Some(&RenderedClickTarget::Point { x: 40, y: 80 })
        ));
        assert!(!rendered_touch_targets_match(
            Some(&RenderedClickTarget::View(1)),
            Some(&RenderedClickTarget::View(2))
        ));
    }

    #[test]
    fn aosp_view_on_touch_event_motion_state_keeps_slop_and_target_cancellation_sticky() {
        reset_render_scroll_state();
        let mut seven = text_element(ActivityTextKind::Button, 20, None);
        seven.view_id = Some(1);
        seven.x = 8;
        seven.width = 101;
        let mut eight = text_element(ActivityTextKind::Button, 20, None);
        eight.view_id = Some(2);
        eight.x = 109;
        eight.width = 101;
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Calculator".to_owned(),
                text_elements: vec![seven, eight],
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });
        let initial = RenderedClickTarget::View(1);
        let mut motion = InteractivePointerMotionState::new();
        assert_eq!(
            rendered_click_target_at((114.0, 165.0), 220, 200),
            Some(RenderedClickTarget::View(2))
        );

        motion.update(
            (105.0, 165.0),
            (114.0, 165.0),
            8.0,
            Some(&initial),
            220,
            200,
        );
        assert_eq!(motion, pointer_motion(true, false, true));

        motion.update(
            (105.0, 165.0),
            (105.0, 153.0),
            8.0,
            Some(&initial),
            220,
            200,
        );
        assert_eq!(motion, pointer_motion(true, true, true));

        motion.update(
            (105.0, 165.0),
            (105.0, 165.0),
            8.0,
            Some(&initial),
            220,
            200,
        );
        assert_eq!(motion, pointer_motion(true, true, true));

        motion.update(
            (105.0, 165.0),
            (118.0, 165.0),
            8.0,
            Some(&initial),
            220,
            200,
        );
        assert_eq!(motion, pointer_motion(true, true, false));
        reset_render_scroll_state();
    }

    #[test]
    fn active_dialog_items_scroll_without_activity_scroll() {
        reset_render_scroll_state();
        let dialog = modal_dialog(
            (0..28)
                .map(|index| ActivityDialogItem {
                    text: index.to_string(),
                    enabled: true,
                    selected: false,
                })
                .collect(),
        );
        let frame = activity_dialog_frame(420, 760, &dialog);
        let surface = ActivitySurfaceState {
            message: "Settings".to_owned(),
            text_elements: vec![text_element(
                ActivityTextKind::ListItem,
                -40,
                Some(scroll_clip()),
            )],
            runtime_canvas_frames: Vec::new(),
            overlays: Vec::new(),
            dialogs: vec![dialog],
            action_bar: None,
            floating_panel: None,
            window_background: None,
            resource_id_names: std::collections::BTreeMap::new(),
        };
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(surface.clone());
        });

        let result = adjust_active_dialog_item_scroll(
            48,
            (
                frame.x.saturating_add(20) as f64,
                frame.y.saturating_add(frame.height / 2) as f64,
            ),
            420,
            760,
        );
        let scrolled = scrolled_render_surface(&surface, 420, 760);

        assert_eq!(result, RenderScrollResult::ConsumedChanged);
        assert_eq!(scrolled.dialogs[0].item_scroll_offset, 1);
        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            assert_eq!(*scroll_y.borrow(), 0);
        });
        reset_render_scroll_state();
    }

    #[test]
    fn active_non_list_dialog_consumes_scroll_without_activity_scroll() {
        reset_render_scroll_state();
        ACTIVE_RENDER_SURFACE.with(|active_surface| {
            *active_surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Support Us".to_owned(),
                text_elements: vec![text_element(
                    ActivityTextKind::ListItem,
                    -40,
                    Some(scroll_clip()),
                )],
                runtime_canvas_frames: Vec::new(),
                overlays: Vec::new(),
                dialogs: vec![modal_dialog(Vec::new())],
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        let result = adjust_active_dialog_item_scroll(48, (200.0, 380.0), 420, 760);

        assert_eq!(result, RenderScrollResult::ConsumedUnchanged);
        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            assert_eq!(*scroll_y.borrow(), 0);
        });
        reset_render_scroll_state();
    }

    fn scrollable_surface() -> ActivitySurfaceState {
        ActivitySurfaceState {
            message: "Settings".to_owned(),
            text_elements: vec![
                text_element(ActivityTextKind::ListItem, 110, Some(scroll_clip())),
                text_element(ActivityTextKind::ListItem, -40, Some(scroll_clip())),
                text_element(ActivityTextKind::Label, 40, None),
            ],
            runtime_canvas_frames: Vec::new(),
            overlays: Vec::new(),
            dialogs: Vec::new(),
            action_bar: None,
            floating_panel: None,
            window_background: None,
            resource_id_names: std::collections::BTreeMap::new(),
        }
    }

    fn modal_dialog(items: Vec<ActivityDialogItem>) -> ActivityDialogOverlay {
        ActivityDialogOverlay {
            title: Some("Dialog".to_owned()),
            content_layouts: Vec::new(),
            text_elements: Vec::new(),
            items,
            item_scroll_offset: 0,
            buttons: Vec::new(),
        }
    }

    fn text_element(
        kind: ActivityTextKind,
        y: i32,
        clip: Option<ActivityRect>,
    ) -> ActivityTextElement {
        ActivityTextElement {
            text: "Row".to_owned(),
            kind,
            view_id: None,
            preference_key: None,
            accessory: None,
            image: None,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
            alignment: ActivityTextAlignment::Start,
            editable: false,
            cursor: None,
            x: 0,
            y,
            width: 100,
            height: 30,
            clip,
        }
    }

    fn scroll_clip() -> ActivityRect {
        ActivityRect {
            x: 0,
            y: 20,
            width: 100,
            height: 100,
        }
    }

    fn pointer_motion(
        moved_beyond_slop: bool,
        vertical_moved_beyond_slop: bool,
        target_retained: bool,
    ) -> InteractivePointerMotionState {
        InteractivePointerMotionState {
            moved_beyond_slop,
            vertical_moved_beyond_slop,
            vertical_scroll_slop_sign: i8::from(vertical_moved_beyond_slop),
            target_retained,
        }
    }

    fn reset_render_scroll_state() {
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = None;
        });
        ACTIVE_RENDER_SCROLL_Y.with(|scroll_y| {
            *scroll_y.borrow_mut() = 0;
        });
        reset_active_dialog_item_scroll();
        PENDING_RENDER_SCROLL_Y.with(|scroll_y| {
            *scroll_y.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
mod surface_text_tests {
    use super::*;
    use crate::host_window::{ActivityRuntimeCanvasFrame, ActivityRuntimeCanvasOp};

    #[test]
    fn surface_text_search_includes_runtime_canvas_text_ops() {
        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = Some(ActivitySurfaceState {
                message: "Compose".to_owned(),
                text_elements: Vec::new(),
                runtime_canvas_frames: vec![ActivityRuntimeCanvasFrame {
                    x: 0,
                    y: 0,
                    width: 420,
                    height: 760,
                    ops: vec![ActivityRuntimeCanvasOp::RenderNode {
                        left: 0.0,
                        top: 0.0,
                        right: 420.0,
                        bottom: 760.0,
                        alpha: 1.0,
                        scale_x: 1.0,
                        scale_y: 1.0,
                        clip_to_bounds: true,
                        ops: vec![ActivityRuntimeCanvasOp::Text {
                            text: "Create".to_owned(),
                            x: 10.0,
                            y: 20.0,
                            color: 0xff00_0000,
                            text_size: 14.0,
                        }],
                    }],
                }],
                overlays: Vec::new(),
                dialogs: Vec::new(),
                action_bar: None,
                floating_panel: None,
                window_background: None,
                resource_id_names: std::collections::BTreeMap::new(),
            });
        });

        assert!(active_surface_contains_text("Create"));
        assert!(!active_surface_contains_text("Delete"));

        ACTIVE_RENDER_SURFACE.with(|surface| {
            *surface.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
mod screenshot_tests {
    use super::*;

    #[test]
    fn screenshot_file_name_is_timestamped_png() {
        let name = screenshot_file_name();
        assert!(name.starts_with("Screenshot_"), "unexpected name: {name}");
        assert!(name.ends_with(".png"), "unexpected name: {name}");
    }

    #[test]
    fn save_writes_png_into_configured_dir_and_skips_without_one() {
        SCREENSHOT_DIR.with(|dir| *dir.borrow_mut() = None);
        assert_eq!(
            save_screenshot_png(b"PNG").expect("save should not error"),
            None
        );

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "winedroid-screenshot-test-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("screenshot directory should exist");
        let screenshot_dir = crate::external_storage::ScreenshotDirectory::from_host_root(&dir)
            .expect("screenshot capability should open");
        SCREENSHOT_DIR.with(|slot| *slot.borrow_mut() = Some(screenshot_dir));

        let saved = save_screenshot_png(b"PNGDATA")
            .expect("save should succeed")
            .expect("a path should be returned when a directory is configured");
        assert!(saved.starts_with(&dir));
        assert_eq!(
            fs::read(&saved).expect("screenshot should exist"),
            b"PNGDATA"
        );

        SCREENSHOT_DIR.with(|slot| *slot.borrow_mut() = None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn save_uses_the_held_screenshot_root_after_visible_replacement() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "winedroid-screenshot-swap-test-{}-{nanos}",
            std::process::id()
        ));
        let visible = root.join("visible");
        let moved = root.join("moved");
        let outside = root.join("outside");
        fs::create_dir_all(&visible).expect("visible root should exist");
        fs::create_dir_all(&outside).expect("outside root should exist");
        let screenshot_dir = crate::external_storage::ScreenshotDirectory::from_host_root(&visible)
            .expect("screenshot capability should open");
        fs::rename(&visible, &moved).expect("visible root should move");
        std::os::unix::fs::symlink(&outside, &visible)
            .expect("replacement root should be a symlink");
        SCREENSHOT_DIR.with(|slot| *slot.borrow_mut() = Some(screenshot_dir));

        save_screenshot_png(b"PNGDATA").expect("save should succeed");

        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        let saved = fs::read_dir(&moved)
            .unwrap()
            .next()
            .expect("held root should contain screenshot")
            .unwrap()
            .path();
        assert_eq!(fs::read(saved).unwrap(), b"PNGDATA");
        SCREENSHOT_DIR.with(|slot| *slot.borrow_mut() = None);
        fs::remove_file(&visible).ok();
        fs::remove_dir_all(&root).ok();
    }
}
