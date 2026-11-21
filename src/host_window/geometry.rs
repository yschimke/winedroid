use super::*;

pub fn activity_overlay_item_frames(
    surface_width: u32,
    surface_height: u32,
    elements: &[ActivityTextElement],
    overlay: &ActivityOverlay,
) -> Vec<ActivityOverlayItemFrame> {
    if overlay.items.is_empty() {
        return Vec::new();
    }

    let frame = activity_overlay_frame(surface_width, surface_height, elements, overlay);
    overlay
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| ActivityOverlayItemFrame {
            item: item.clone(),
            frame: ActivityRect {
                x: frame.x,
                y: frame
                    .y
                    .saturating_add((index as u32).saturating_mul(ACTIVITY_OVERLAY_ITEM_HEIGHT)),
                width: frame.width,
                height: ACTIVITY_OVERLAY_ITEM_HEIGHT,
            },
        })
        .collect()
}

pub fn activity_overlay_frame(
    surface_width: u32,
    surface_height: u32,
    elements: &[ActivityTextElement],
    overlay: &ActivityOverlay,
) -> ActivityRect {
    let available_width = surface_width.saturating_sub(ACTIVITY_OVERLAY_MARGIN * 2);
    let width = ACTIVITY_OVERLAY_WIDTH.min(available_width).max(1);
    let item_count = overlay.items.len() as u32;
    let height = item_count
        .saturating_mul(ACTIVITY_OVERLAY_ITEM_HEIGHT)
        .max(1);
    if overlay.anchor_view_id == Some(ACTION_BAR_OVERFLOW_TOGGLE_ID) {
        let x = surface_width
            .saturating_sub(width)
            .saturating_sub(ACTIVITY_OVERLAY_MARGIN);
        let y = surface_height
            .saturating_sub(ACTION_BAR_HEIGHT)
            .saturating_sub(height);
        return ActivityRect {
            x,
            y,
            width,
            height,
        };
    }

    if overlay.anchor_view_id == Some(ANDROID_R_ID_HOME) {
        let x = ACTIVITY_OVERLAY_MARGIN.min(surface_width.saturating_sub(width));
        let y = surface_height
            .saturating_sub(ACTION_BAR_HEIGHT)
            .saturating_sub(height);
        return ActivityRect {
            x,
            y,
            width,
            height,
        };
    }

    let fallback_x = surface_width
        .saturating_sub(width)
        .saturating_sub(ACTIVITY_OVERLAY_MARGIN);
    let fallback_y = ACTIVITY_OVERLAY_MARGIN;
    let anchor = overlay.anchor_view_id.and_then(|view_id| {
        elements
            .iter()
            .find(|element| element.view_id == Some(view_id))
    });
    let mut x = fallback_x;
    let mut y = fallback_y;

    if let Some(anchor) = anchor {
        let anchor_y = u32::try_from(anchor.y.max(0)).unwrap_or(0);
        x = anchor.x.saturating_add(anchor.width).saturating_sub(width);
        y = anchor_y
            .saturating_add(anchor.height)
            .saturating_add(ACTIVITY_OVERLAY_MARGIN / 2);
    }

    let max_x = surface_width.saturating_sub(width);
    let max_y = surface_height.saturating_sub(height);
    x = x.min(max_x);

    if y > max_y && anchor.is_some() {
        if let Some(anchor) = anchor {
            let anchor_y = u32::try_from(anchor.y.max(0)).unwrap_or(0);
            y = anchor_y.saturating_sub(height.saturating_add(ACTIVITY_OVERLAY_MARGIN / 2));
        }
    }
    y = y.min(max_y);

    ActivityRect {
        x,
        y,
        width,
        height,
    }
}

pub fn activity_dialog_frame(
    surface_width: u32,
    surface_height: u32,
    dialog: &ActivityDialogOverlay,
) -> ActivityRect {
    let width = surface_width
        .saturating_sub(ACTIVITY_DIALOG_HORIZONTAL_MARGIN * 2)
        .min(ACTIVITY_DIALOG_MAX_WIDTH);
    let content_height = activity_dialog_content_height(dialog);
    let title_height = if dialog.title.is_some() { 56 } else { 24 };
    let button_height = if dialog.buttons.is_empty() { 24 } else { 64 };
    let wanted_height = content_height
        .saturating_add(title_height)
        .saturating_add(button_height)
        .saturating_add(24);
    let height = wanted_height.max(ACTIVITY_DIALOG_MIN_HEIGHT).min(
        surface_height
            .saturating_sub(ACTIVITY_DIALOG_VERTICAL_MARGIN * 2)
            .max(160),
    );

    ActivityRect {
        x: surface_width.saturating_sub(width) / 2,
        y: surface_height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub fn activity_dialog_button_frames(
    surface_width: u32,
    surface_height: u32,
    dialog_index: usize,
    dialog: &ActivityDialogOverlay,
) -> Vec<ActivityDialogButtonFrame> {
    let dialog_frame = activity_dialog_frame(surface_width, surface_height, dialog);
    let mut right = dialog_frame
        .x
        .saturating_add(dialog_frame.width)
        .saturating_sub(ACTIVITY_DIALOG_BUTTON_RIGHT_MARGIN);
    let y = dialog_frame
        .y
        .saturating_add(ACTIVITY_DIALOG_BUTTON_BOTTOM_MARGIN);
    let mut frames = Vec::new();

    for (button_index, button) in dialog.buttons.iter().enumerate().rev() {
        let width = dialog_button_width(&button.text);
        let x = right.saturating_sub(width);
        frames.push(ActivityDialogButtonFrame {
            dialog_index,
            button_index,
            button: button.clone(),
            frame: ActivityRect {
                x,
                y,
                width,
                height: ACTIVITY_DIALOG_BUTTON_HEIGHT,
            },
        });
        right = x.saturating_sub(8);
    }

    frames.reverse();
    frames
}

pub fn activity_dialog_item_frames(
    surface_width: u32,
    surface_height: u32,
    dialog_index: usize,
    dialog: &ActivityDialogOverlay,
) -> Vec<ActivityDialogItemFrame> {
    if dialog.items.is_empty() {
        return Vec::new();
    }

    let content = activity_dialog_content_rect(surface_width, surface_height, dialog);
    if content.width == 0 || content.height == 0 {
        return Vec::new();
    }

    let mut frames = Vec::new();
    let max_visible_items = content.height.div_ceil(ACTIVITY_DIALOG_ITEM_HEIGHT);

    for (visible_index, (item_index, item)) in dialog
        .items
        .iter()
        .enumerate()
        .skip(dialog.item_scroll_offset)
        .enumerate()
    {
        let visible_offset = u32::try_from(visible_index).unwrap_or(u32::MAX);
        if visible_offset >= max_visible_items {
            break;
        }

        let offset_from_top = visible_offset.saturating_mul(ACTIVITY_DIALOG_ITEM_HEIGHT);
        if offset_from_top >= content.height {
            break;
        }
        let available_height = content.height.saturating_sub(offset_from_top);
        let height = ACTIVITY_DIALOG_ITEM_HEIGHT.min(available_height);
        let y = content
            .y
            .saturating_add(content.height)
            .saturating_sub(offset_from_top)
            .saturating_sub(height);

        frames.push(ActivityDialogItemFrame {
            dialog_index,
            item_index,
            item: item.clone(),
            frame: ActivityRect {
                x: content.x,
                y,
                width: content.width,
                height,
            },
        });
    }

    frames
}

pub fn activity_dialog_content_rect(
    surface_width: u32,
    surface_height: u32,
    dialog: &ActivityDialogOverlay,
) -> ActivityRect {
    let frame = activity_dialog_frame(surface_width, surface_height, dialog);
    let title_height = if dialog.title.is_some() { 56 } else { 22 };
    let button_height = if dialog.buttons.is_empty() { 24 } else { 64 };

    ActivityRect {
        x: frame.x.saturating_add(20),
        y: frame.y.saturating_add(button_height),
        width: frame.width.saturating_sub(40),
        height: frame
            .height
            .saturating_sub(title_height)
            .saturating_sub(button_height)
            .saturating_sub(12),
    }
}

fn activity_dialog_content_height(dialog: &ActivityDialogOverlay) -> u32 {
    let text_height = dialog
        .text_elements
        .iter()
        .map(|element| {
            u32::try_from(element.y.max(0))
                .unwrap_or(0)
                .saturating_add(element.height)
        })
        .max()
        .unwrap_or(0);
    let item_height = u32::try_from(dialog.items.len())
        .unwrap_or(u32::MAX)
        .saturating_mul(ACTIVITY_DIALOG_ITEM_HEIGHT);

    text_height.max(item_height).max(80).min(420)
}

fn dialog_button_width(text: &str) -> u32 {
    let character_count = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
    character_count.saturating_mul(9).saturating_add(24).max(48)
}

pub(crate) fn round_display_contains(point: (f64, f64), width: u32, height: u32) -> bool {
    let radius = f64::from(width.min(height)) / 2.0;
    let dx = point.0 - f64::from(width) / 2.0;
    let dy = point.1 - f64::from(height) / 2.0;
    dx * dx + dy * dy <= radius * radius
}
