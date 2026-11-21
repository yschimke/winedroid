use std::collections::BTreeSet;
use std::sync::OnceLock;

#[cfg(not(target_os = "emscripten"))]
use skia_safe::FontStyle;
use skia_safe::{
    canvas, paint, surfaces, AlphaType, BlurStyle, Color, ColorType, Data, EncodedImageFormat,
    Font, FontMgr, Image, ImageInfo, MaskFilter, Matrix, Paint, Path, Point, Point3, RRect, Rect,
    Typeface,
};

use crate::android_framework;
use crate::binary_xml::{self, XmlEvent, XmlStartElement, XmlValue};
use crate::error::{Result, WinedroidError};
use crate::host_window::{
    activity_dialog_button_frames, activity_dialog_content_rect, activity_dialog_frame,
    activity_dialog_item_frames, activity_overlay_frame, activity_overlay_item_frames,
    ActivityActionBar, ActivityDialogOverlay, ActivityImage, ActivityOverlay, ActivityRect,
    ActivityRuntimeCanvasFrame, ActivityRuntimeCanvasOp, ActivityTextAccessory,
    ActivityTextAlignment, ActivityTextElement, ActivityTextKind, ACTION_BAR_HEIGHT,
    ACTIVITY_DIALOG_ITEM_HEIGHT, ACTIVITY_OVERLAY_ITEM_HEIGHT, MISSING_FRAMEWORK_DRAWABLE_SOURCE,
};
use crate::render_metrics;
use crate::resource_table::{self, ResourcePrimitive};

const BACKGROUND: Color = Color::from_argb(255, 244, 246, 248);
const FLOATING_SCRIM: u32 = 0x99_00_00_00;
const FLOATING_PANEL_FILL: u32 = 0xff_fa_fa_fa;
const FALLBACK_CONTROL_FILL: Color = Color::from_argb(255, 255, 255, 255);
const FALLBACK_CONTROL_STROKE: Color = Color::from_argb(255, 206, 212, 222);
const FALLBACK_CONTROL_TEXT: Color = Color::from_argb(255, 15, 23, 42);
const LABEL_TEXT: Color = Color::from_argb(255, 32, 34, 38);
const OVERFLOW_ONLY_FOREGROUND: u32 = 0xff20_2226;
const TEXT_CURSOR: Color = Color::from_argb(255, 32, 34, 38);
const LIST_ITEM_FILL: Color = Color::from_argb(255, 255, 255, 255);
const LIST_ITEM_PRIMARY_TEXT: Color = Color::from_argb(255, 28, 32, 38);
const LIST_ITEM_SECONDARY_TEXT: Color = Color::from_argb(255, 93, 101, 112);
const LIST_ITEM_DIVIDER: Color = Color::from_argb(255, 224, 229, 236);
const SWITCH_TRACK_ON: Color = Color::from_argb(255, 70, 120, 220);
const SWITCH_TRACK_OFF: Color = Color::from_argb(255, 183, 190, 202);
const SWITCH_THUMB: Color = Color::from_argb(255, 255, 255, 255);
const LIST_VALUE_CHEVRON: Color = Color::from_argb(255, 112, 122, 138);
const POPUP_FILL: Color = Color::from_argb(255, 255, 255, 255);
const POPUP_STROKE: Color = Color::from_argb(255, 196, 202, 212);
const POPUP_DIVIDER: Color = Color::from_argb(255, 226, 232, 240);
const POPUP_TEXT: Color = Color::from_argb(255, 24, 28, 36);
const POPUP_DISABLED_TEXT: Color = Color::from_argb(255, 148, 158, 172);
// cite: frameworks-base PhoneWindow.java#Window_backgroundDimAmount
const DIALOG_SCRIM: Color = Color::from_argb(128, 0, 0, 0);
const DIALOG_FILL: Color = Color::from_argb(255, 255, 255, 255);
const DIALOG_STROKE: Color = Color::from_argb(255, 210, 216, 226);
const DIALOG_TITLE: Color = Color::from_argb(255, 20, 24, 32);
const DIALOG_BUTTON_TEXT: Color = Color::from_argb(255, 37, 99, 235);

// cite: frameworks-base themes.xml#lightY, lightZ, lightRadius, ambientShadowAlpha, spotShadowAlpha
// cite: frameworks-base dimens.xml#light_y, light_z, light_radius, ambient_shadow_alpha, spot_shadow_alpha
const SHADOW_LIGHT_Y: f32 = 0.0;
const SHADOW_LIGHT_Z: f32 = 500.0;
const SHADOW_LIGHT_RADIUS: f32 = 800.0;
const SHADOW_AMBIENT_ALPHA: f32 = 0.039;
const SHADOW_SPOT_ALPHA: f32 = 0.19;
// cite: frameworks-base styles_material.xml#Widget.Material.ActionBar
// cite: frameworks-base dimens_material.xml#action_bar_elevation_material
const DECOR_ACTION_BAR_ELEVATION_PX: f32 = 4.0;
// cite: frameworks-base themes_material.xml#windowElevation
// cite: frameworks-base dimens_material.xml#floating_window_z
const FLOATING_WINDOW_ELEVATION_PX: f32 = 16.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkiaSurfaceImage {
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
    pub pixels: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkiaSurfaceOutput {
    Png,
    Pixels,
    PixelsAndPng,
}

pub fn render_activity_surface(
    width: u32,
    height: u32,
    message: &str,
    elements: &[ActivityTextElement],
    runtime_canvas_frames: &[ActivityRuntimeCanvasFrame],
    overlays: &[ActivityOverlay],
    dialogs: &[ActivityDialogOverlay],
    action_bar: Option<&ActivityActionBar>,
    floating_panel: Option<ActivityRect>,
    window_background: Option<u32>,
) -> Result<SkiaSurfaceImage> {
    render_activity_surface_scaled(
        width,
        height,
        message,
        elements,
        runtime_canvas_frames,
        overlays,
        dialogs,
        action_bar,
        floating_panel,
        window_background,
        1.0,
    )
}

pub fn render_activity_surface_scaled(
    width: u32,
    height: u32,
    message: &str,
    elements: &[ActivityTextElement],
    runtime_canvas_frames: &[ActivityRuntimeCanvasFrame],
    overlays: &[ActivityOverlay],
    dialogs: &[ActivityDialogOverlay],
    action_bar: Option<&ActivityActionBar>,
    floating_panel: Option<ActivityRect>,
    window_background: Option<u32>,
    pixel_scale: f32,
) -> Result<SkiaSurfaceImage> {
    render_activity_surface_scaled_internal(
        width,
        height,
        message,
        elements,
        runtime_canvas_frames,
        overlays,
        dialogs,
        action_bar,
        floating_panel,
        window_background,
        pixel_scale,
        SkiaSurfaceOutput::Png,
    )
}

pub fn render_activity_surface_pixels_scaled(
    width: u32,
    height: u32,
    message: &str,
    elements: &[ActivityTextElement],
    runtime_canvas_frames: &[ActivityRuntimeCanvasFrame],
    overlays: &[ActivityOverlay],
    dialogs: &[ActivityDialogOverlay],
    action_bar: Option<&ActivityActionBar>,
    floating_panel: Option<ActivityRect>,
    window_background: Option<u32>,
    pixel_scale: f32,
) -> Result<SkiaSurfaceImage> {
    render_activity_surface_scaled_internal(
        width,
        height,
        message,
        elements,
        runtime_canvas_frames,
        overlays,
        dialogs,
        action_bar,
        floating_panel,
        window_background,
        pixel_scale,
        SkiaSurfaceOutput::Pixels,
    )
}

pub fn render_activity_surface_pixels_and_png_scaled(
    width: u32,
    height: u32,
    message: &str,
    elements: &[ActivityTextElement],
    runtime_canvas_frames: &[ActivityRuntimeCanvasFrame],
    overlays: &[ActivityOverlay],
    dialogs: &[ActivityDialogOverlay],
    action_bar: Option<&ActivityActionBar>,
    floating_panel: Option<ActivityRect>,
    window_background: Option<u32>,
    pixel_scale: f32,
) -> Result<SkiaSurfaceImage> {
    render_activity_surface_scaled_internal(
        width,
        height,
        message,
        elements,
        runtime_canvas_frames,
        overlays,
        dialogs,
        action_bar,
        floating_panel,
        window_background,
        pixel_scale,
        SkiaSurfaceOutput::PixelsAndPng,
    )
}

fn render_activity_surface_scaled_internal(
    width: u32,
    height: u32,
    message: &str,
    elements: &[ActivityTextElement],
    runtime_canvas_frames: &[ActivityRuntimeCanvasFrame],
    overlays: &[ActivityOverlay],
    dialogs: &[ActivityDialogOverlay],
    action_bar: Option<&ActivityActionBar>,
    floating_panel: Option<ActivityRect>,
    window_background: Option<u32>,
    pixel_scale: f32,
    output: SkiaSurfaceOutput,
) -> Result<SkiaSurfaceImage> {
    if width == 0 || height == 0 {
        return Err(WinedroidError::new(format!(
            "Skia render surface requires nonzero dimensions\nsize: {width}x{height}"
        )));
    }

    let (surface_width, surface_height, scale) =
        scaled_surface_dimensions(width, height, pixel_scale);
    let image_info = ImageInfo::new(
        (surface_width, surface_height),
        ColorType::BGRA8888,
        AlphaType::Premul,
        None,
    );
    let row_bytes = surface_width as usize * 4;
    let mut pixel_bytes = vec![0u8; row_bytes * surface_height as usize];
    let mut surface = surfaces::wrap_pixels(&image_info, &mut pixel_bytes, row_bytes, None)
        .ok_or_else(|| {
            WinedroidError::new(format!(
                "failed to create Skia raster surface\nsize: {surface_width}x{surface_height}"
            ))
        })?;

    {
        let canvas = surface.canvas();
        let theme_colors = surface_theme_colors(elements);
        // cite: frameworks-base PhoneWindow.java#Window_windowBackground
        canvas.clear(
            theme_colors
                .background_color()
                .or(window_background.map(android_argb_color))
                .unwrap_or(BACKGROUND),
        );
        canvas.scale((scale, scale));

        if let Some(panel) = floating_panel {
            canvas.draw_rect(
                Rect::from_xywh(0.0, 0.0, width as f32, height as f32),
                &fill_paint(android_argb_color(FLOATING_SCRIM)),
            );
            let panel_rect = Rect::from_xywh(
                panel.x as f32,
                panel.y as f32,
                panel.width as f32,
                panel.height as f32,
            );
            let panel_color = theme_colors
                .background_color()
                .unwrap_or(android_argb_color(FLOATING_PANEL_FILL));
            canvas.draw_round_rect(panel_rect, 6.0, 6.0, &fill_paint(panel_color));
            canvas.save();
            canvas.clip_rect(panel_rect, None, Some(true));
        }

        if elements.is_empty() {
            if runtime_canvas_frames.is_empty() {
                draw_bootstrap_message(canvas, message);
            } else {
                let frame = Rect::from_xywh(0.0, 0.0, width as f32, height as f32);
                for runtime_canvas_frame in runtime_canvas_frames {
                    draw_runtime_canvas_frame(canvas, &frame, width, height, runtime_canvas_frame);
                }
            }
        } else {
            // cite: frameworks-base ViewOutlineProvider.java#BACKGROUND
            for element in elements {
                if element.elevation_px == 0 || element.background_color.is_none() {
                    continue;
                }
                let frame = element_rect(element, height);
                draw_elevation_shadow(
                    canvas,
                    width,
                    height,
                    frame,
                    element.background_corner_radius as f32,
                    element.elevation_px as f32,
                );
            }
            for element in elements {
                if !is_background_only_element(element) {
                    continue;
                }
                draw_text_element(
                    canvas,
                    width,
                    height,
                    element,
                    runtime_canvas_frames,
                    theme_colors,
                )?;
            }
            for element in elements {
                if is_background_only_element(element) {
                    continue;
                }
                draw_text_element(
                    canvas,
                    width,
                    height,
                    element,
                    runtime_canvas_frames,
                    theme_colors,
                )?;
            }
        }

        for overlay in overlays {
            draw_activity_overlay(canvas, width, height, elements, overlay, theme_colors);
        }

        if floating_panel.is_some() {
            canvas.restore();
        }

        if let Some(action_bar) = action_bar {
            draw_action_bar(canvas, width, height, action_bar, theme_colors)?;
        }

        for dialog in dialogs {
            draw_dialog_overlay(canvas, width, height, dialog, theme_colors)?;
        }
    }

    let png = match output {
        SkiaSurfaceOutput::Png | SkiaSurfaceOutput::PixelsAndPng => {
            encode_surface_png(&mut surface)?
        }
        SkiaSurfaceOutput::Pixels => Vec::new(),
    };
    drop(surface);
    let pixels = match output {
        SkiaSurfaceOutput::Pixels | SkiaSurfaceOutput::PixelsAndPng => {
            opaque_softbuffer_pixels(&pixel_bytes, BACKGROUND)
        }
        SkiaSurfaceOutput::Png => Vec::new(),
    };

    Ok(SkiaSurfaceImage {
        width: surface_width as u32,
        height: surface_height as u32,
        png,
        pixels,
    })
}

fn opaque_softbuffer_pixels(pixel_bytes: &[u8], backdrop: Color) -> Vec<u32> {
    let mut pixels = Vec::with_capacity(pixel_bytes.len() / 4);
    let backdrop_blue = backdrop.b();
    let backdrop_green = backdrop.g();
    let backdrop_red = backdrop.r();

    for pixel in pixel_bytes.chunks_exact(4) {
        let alpha = pixel[3];
        let blue = composite_premultiplied_channel(pixel[0], alpha, backdrop_blue);
        let green = composite_premultiplied_channel(pixel[1], alpha, backdrop_green);
        let red = composite_premultiplied_channel(pixel[2], alpha, backdrop_red);
        pixels.push(u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue));
    }

    pixels
}

// cite: frameworks-base ColorUtils.java#compositeColors, compositeComponent
fn composite_premultiplied_channel(source: u8, source_alpha: u8, backdrop: u8) -> u8 {
    let inverse_alpha = 255 - u32::from(source_alpha);
    let backdrop_component = u32::from(backdrop) * inverse_alpha / 255;
    (u32::from(source) + backdrop_component) as u8
}

fn encode_surface_png(surface: &mut skia_safe::Surface) -> Result<Vec<u8>> {
    let image = surface.image_snapshot();
    let encoded = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .ok_or_else(|| WinedroidError::new("failed to encode Skia renderer surface as PNG"))?;
    Ok(encoded.as_bytes().to_vec())
}

fn scaled_surface_dimensions(width: u32, height: u32, pixel_scale: f32) -> (i32, i32, f32) {
    let scale = if pixel_scale.is_finite() && pixel_scale > 0.0 {
        pixel_scale
    } else {
        1.0
    };
    let surface_width = ((width as f32 * scale).round() as i32).max(1);
    let surface_height = ((height as f32 * scale).round() as i32).max(1);

    (surface_width, surface_height, scale)
}

const SCREENSHOT_FLASH_SHRINK_MS: u32 = 400;
const SCREENSHOT_FLASH_HOLD_MS: u32 = 400;
const SCREENSHOT_FLASH_RISE_MS: u32 = 500;
pub const SCREENSHOT_FLASH_DURATION_MS: u32 =
    SCREENSHOT_FLASH_SHRINK_MS + SCREENSHOT_FLASH_HOLD_MS + SCREENSHOT_FLASH_RISE_MS;
const SCREENSHOT_FLASH_MIN_SCALE: f32 = 0.5;
const SCREENSHOT_FLASH_BORDER: f32 = 10.0;
const SCREENSHOT_FLASH_CORNER_RADIUS: f32 = 8.0;
const SCREENSHOT_FLASH_RISE_ABOVE: f32 = 1000.0;
const SCREENSHOT_FLASH_SHADOW_OFFSET_Y: f32 = 6.0;
const SCREENSHOT_FLASH_SHADOW_BLUR: f32 = 8.0;

pub fn render_screenshot_flash_pixels_scaled(
    width: u32,
    height: u32,
    background_pixels: &[u32],
    snapshot_png: &[u8],
    elapsed_ms: u32,
    pixel_scale: f32,
) -> Result<SkiaSurfaceImage> {
    if width == 0 || height == 0 {
        return Err(WinedroidError::new(format!(
            "screenshot flash requires nonzero dimensions\nsize: {width}x{height}"
        )));
    }

    let snapshot = Image::from_encoded(Data::new_copy(snapshot_png))
        .ok_or_else(|| WinedroidError::new("failed to decode screenshot flash snapshot PNG"))?;
    let (surface_width, surface_height, scale) =
        scaled_surface_dimensions(width, height, pixel_scale);
    let image_info = ImageInfo::new(
        (surface_width, surface_height),
        ColorType::BGRA8888,
        AlphaType::Premul,
        None,
    );
    let row_bytes = surface_width as usize * 4;
    let expected_pixels = surface_width as usize * surface_height as usize;
    if background_pixels.len() != expected_pixels {
        return Err(WinedroidError::new(format!(
            "screenshot flash background pixel count mismatch\nexpected: {expected_pixels}\nactual: {}",
            background_pixels.len()
        )));
    }
    let mut pixel_bytes = softbuffer_pixels_to_opaque_bgra(background_pixels);
    let mut surface = surfaces::wrap_pixels(&image_info, &mut pixel_bytes, row_bytes, None)
        .ok_or_else(|| {
            WinedroidError::new(format!(
                "failed to create Skia screenshot flash surface\nsize: {surface_width}x{surface_height}"
            ))
        })?;

    {
        let canvas = surface.canvas();
        canvas.scale((scale, scale));

        if elapsed_ms < SCREENSHOT_FLASH_DURATION_MS {
            draw_screenshot_flash_frame(canvas, width, height, &snapshot, elapsed_ms);
        }
    }

    drop(surface);
    let pixels = opaque_softbuffer_pixels(&pixel_bytes, BACKGROUND);

    Ok(SkiaSurfaceImage {
        width: surface_width as u32,
        height: surface_height as u32,
        png: Vec::new(),
        pixels,
    })
}

pub fn mask_round_display(image: &mut SkiaSurfaceImage) -> Result<()> {
    if image.width == 0 || image.height == 0 {
        return Err(WinedroidError::new(format!(
            "round display mask requires nonzero dimensions\nsize: {}x{}",
            image.width, image.height
        )));
    }

    let image_info = ImageInfo::new(
        (image.width as i32, image.height as i32),
        ColorType::BGRA8888,
        AlphaType::Premul,
        None,
    );
    let row_bytes = image.width as usize * 4;

    if !image.pixels.is_empty() {
        let source_bytes = softbuffer_pixels_to_opaque_bgra(&image.pixels);
        let source = skia_safe::images::raster_from_data(
            &image_info,
            Data::new_copy(&source_bytes),
            row_bytes,
        )
        .ok_or_else(|| WinedroidError::new("failed to wrap frame pixels for round display mask"))?;
        let (masked_bytes, _) = draw_round_display_mask(&image_info, row_bytes, &source, false)?;
        image.pixels = opaque_softbuffer_pixels(&masked_bytes, Color::BLACK);
    }

    if !image.png.is_empty() {
        let source = Image::from_encoded(Data::new_copy(&image.png)).ok_or_else(|| {
            WinedroidError::new("failed to decode frame PNG for round display mask")
        })?;
        let (_, png) = draw_round_display_mask(&image_info, row_bytes, &source, true)?;
        image.png = png;
    }

    Ok(())
}

fn draw_round_display_mask(
    image_info: &ImageInfo,
    row_bytes: usize,
    source: &Image,
    encode_png: bool,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let width = image_info.width();
    let height = image_info.height();
    let mut pixel_bytes = vec![0u8; row_bytes * height as usize];
    let mut surface = surfaces::wrap_pixels(image_info, &mut pixel_bytes, row_bytes, None)
        .ok_or_else(|| {
            WinedroidError::new(format!(
                "failed to create Skia round display surface\nsize: {width}x{height}"
            ))
        })?;

    {
        let canvas = surface.canvas();
        canvas.clear(Color::BLACK);
        let diameter = width.min(height) as f32;
        let panel = Rect::from_xywh(
            (width as f32 - diameter) / 2.0,
            (height as f32 - diameter) / 2.0,
            diameter,
            diameter,
        );
        canvas.save();
        canvas.clip_rrect(RRect::new_oval(panel), skia_safe::ClipOp::Intersect, true);
        canvas.draw_image(source, (0, 0), None);
        canvas.restore();
    }

    let png = if encode_png {
        encode_surface_png(&mut surface)?
    } else {
        Vec::new()
    };
    drop(surface);
    Ok((pixel_bytes, png))
}

fn softbuffer_pixels_to_opaque_bgra(pixels: &[u32]) -> Vec<u8> {
    let mut pixel_bytes = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        pixel_bytes.push(*pixel as u8);
        pixel_bytes.push((*pixel >> 8) as u8);
        pixel_bytes.push((*pixel >> 16) as u8);
        pixel_bytes.push(255);
    }
    pixel_bytes
}

fn draw_screenshot_flash_frame(
    canvas: &skia_safe::Canvas,
    width: u32,
    height: u32,
    snapshot: &Image,
    elapsed_ms: u32,
) {
    let scale = screenshot_flash_scale(elapsed_ms);
    let rise = screenshot_flash_rise(elapsed_ms, height);

    let inner_width = width as f32 * scale;
    let inner_height = height as f32 * scale;
    let inner_x = (width as f32 - inner_width) / 2.0;
    let inner_y = (height as f32 - inner_height) / 2.0 - rise;
    let inner = Rect::from_xywh(inner_x, inner_y, inner_width, inner_height);
    let border = Rect::from_xywh(
        inner_x - SCREENSHOT_FLASH_BORDER,
        inner_y - SCREENSHOT_FLASH_BORDER,
        inner_width + 2.0 * SCREENSHOT_FLASH_BORDER,
        inner_height + 2.0 * SCREENSHOT_FLASH_BORDER,
    );

    let shadow = Rect::from_xywh(
        border.left(),
        border.top() + SCREENSHOT_FLASH_SHADOW_OFFSET_Y,
        border.width(),
        border.height(),
    );
    let mut shadow_paint = fill_paint(Color::from_argb(70, 0, 0, 0));
    if let Some(blur) = MaskFilter::blur(BlurStyle::Normal, SCREENSHOT_FLASH_SHADOW_BLUR, false) {
        shadow_paint.set_mask_filter(blur);
    }
    canvas.draw_round_rect(
        shadow,
        SCREENSHOT_FLASH_CORNER_RADIUS,
        SCREENSHOT_FLASH_CORNER_RADIUS,
        &shadow_paint,
    );

    canvas.draw_round_rect(
        border,
        SCREENSHOT_FLASH_CORNER_RADIUS,
        SCREENSHOT_FLASH_CORNER_RADIUS,
        &fill_paint(Color::WHITE),
    );

    let src = Rect::from_xywh(0.0, 0.0, snapshot.width() as f32, snapshot.height() as f32);
    let mut image_paint = Paint::default();
    image_paint.set_anti_alias(true);
    canvas.draw_image_rect(
        snapshot,
        Some((&src, canvas::SrcRectConstraint::Strict)),
        inner,
        &image_paint,
    );
}

fn screenshot_flash_scale(elapsed_ms: u32) -> f32 {
    if elapsed_ms >= SCREENSHOT_FLASH_SHRINK_MS {
        return SCREENSHOT_FLASH_MIN_SCALE;
    }

    let progress = elapsed_ms as f32 / SCREENSHOT_FLASH_SHRINK_MS as f32;
    1.0 - (1.0 - SCREENSHOT_FLASH_MIN_SCALE) * progress
}

fn screenshot_flash_rise(elapsed_ms: u32, height: u32) -> f32 {
    let rise_start = SCREENSHOT_FLASH_SHRINK_MS + SCREENSHOT_FLASH_HOLD_MS;
    let rise_distance = height as f32 + SCREENSHOT_FLASH_RISE_ABOVE;

    if elapsed_ms <= rise_start {
        return 0.0;
    }
    if elapsed_ms >= SCREENSHOT_FLASH_DURATION_MS {
        return rise_distance;
    }

    let progress = (elapsed_ms - rise_start) as f32 / SCREENSHOT_FLASH_RISE_MS as f32;
    rise_distance * progress
}

fn is_background_only_element(element: &ActivityTextElement) -> bool {
    element.kind == ActivityTextKind::Label
        && element.background_color.is_some()
        && element.text.is_empty()
        && element.image.is_none()
        && element.cursor.is_none()
}

#[cfg(test)]
fn surface_background_color(elements: &[ActivityTextElement]) -> Option<Color> {
    surface_background_argb(elements).map(android_argb_color)
}

fn surface_background_argb(elements: &[ActivityTextElement]) -> Option<u32> {
    dominant_element_background_argb(elements, is_background_only_element).or_else(|| {
        dominant_element_background_argb(elements, |element| {
            element.kind == ActivityTextKind::ListItem
        })
    })
}

fn dominant_element_background_argb(
    elements: &[ActivityTextElement],
    predicate: impl Fn(&ActivityTextElement) -> bool,
) -> Option<u32> {
    elements
        .iter()
        .filter(|element| predicate(element))
        .filter_map(|element| {
            let color = element.background_color?;
            let area = u64::from(element.width) * u64::from(element.height);
            Some((area, color))
        })
        .max_by_key(|(area, _color)| *area)
        .map(|(_area, color)| color)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SurfaceThemeColors {
    background: Option<u32>,
    foreground: Option<u32>,
}

impl SurfaceThemeColors {
    fn background_color(self) -> Option<Color> {
        self.background.map(android_argb_color)
    }

    fn foreground_color_or(self, fallback: Color) -> Color {
        self.foreground.map(android_argb_color).unwrap_or(fallback)
    }

    fn secondary_foreground_color_or(self, fallback: Color) -> Color {
        self.foreground
            .map(secondary_android_argb_color)
            .unwrap_or(fallback)
    }

    fn disabled_foreground_color_or(self, fallback: Color) -> Color {
        self.foreground
            .map(disabled_android_argb_color)
            .unwrap_or(fallback)
    }

    fn divider_color_or(self, fallback: Color) -> Color {
        self.background
            .map(divider_android_argb_color)
            .unwrap_or(fallback)
    }
}

fn surface_theme_colors(elements: &[ActivityTextElement]) -> SurfaceThemeColors {
    SurfaceThemeColors {
        background: surface_background_argb(elements),
        foreground: surface_foreground_argb(elements),
    }
}

fn surface_foreground_argb(elements: &[ActivityTextElement]) -> Option<u32> {
    let mut best = None;

    for element in elements {
        if element.text.trim().is_empty() {
            continue;
        }
        let Some(color) = element.text_color else {
            continue;
        };

        match best {
            Some((best_y, _, _)) if best_y > element.y => {}
            Some((best_y, best_x, _)) if best_y == element.y && best_x <= element.x => {}
            _ => best = Some((element.y, element.x, color)),
        }
    }

    best.map(|(_, _, color)| color)
}

fn popup_fill_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.background_color().unwrap_or(POPUP_FILL)
}

fn popup_stroke_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.divider_color_or(POPUP_STROKE)
}

fn popup_divider_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.divider_color_or(POPUP_DIVIDER)
}

fn popup_text_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.foreground_color_or(POPUP_TEXT)
}

fn popup_disabled_text_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.disabled_foreground_color_or(POPUP_DISABLED_TEXT)
}

fn dialog_fill_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.background_color().unwrap_or(DIALOG_FILL)
}

fn dialog_stroke_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.divider_color_or(DIALOG_STROKE)
}

fn dialog_divider_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.divider_color_or(LIST_ITEM_DIVIDER)
}

fn dialog_title_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.foreground_color_or(DIALOG_TITLE)
}

fn dialog_text_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.foreground_color_or(LIST_ITEM_PRIMARY_TEXT)
}

fn dialog_disabled_text_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.disabled_foreground_color_or(POPUP_DISABLED_TEXT)
}

fn dialog_button_text_color(theme_colors: SurfaceThemeColors) -> Color {
    theme_colors.secondary_foreground_color_or(DIALOG_BUTTON_TEXT)
}

fn draw_text_element(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    element: &ActivityTextElement,
    runtime_canvas_frames: &[ActivityRuntimeCanvasFrame],
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    if let Some(clip) = element.clip {
        canvas.save();
        canvas.clip_rect(
            activity_rect_to_skia_rect(clip, surface_height),
            None,
            Some(false),
        );
    }

    let result = match element.kind {
        ActivityTextKind::Label => {
            draw_label(canvas, surface_width, surface_height, element, theme_colors)
        }
        ActivityTextKind::Button => draw_button(canvas, surface_height, element, theme_colors),
        ActivityTextKind::ListItem => draw_list_item(canvas, surface_height, element, theme_colors),
        ActivityTextKind::RuntimeViewRoot => draw_runtime_view_root(
            canvas,
            surface_width,
            surface_height,
            element,
            runtime_canvas_frames,
        ),
    };

    if element.clip.is_some() {
        canvas.restore();
    }

    result
}

fn draw_activity_overlay(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    elements: &[ActivityTextElement],
    overlay: &ActivityOverlay,
    theme_colors: SurfaceThemeColors,
) {
    if overlay.items.is_empty() {
        return;
    }

    let frame = activity_overlay_frame(surface_width, surface_height, elements, overlay);
    let rect = activity_rect_to_skia_rect(frame, surface_height);
    // cite: frameworks-base themes_material.xml#windowElevation
    draw_elevation_shadow(
        canvas,
        surface_width,
        surface_height,
        rect,
        6.0,
        FLOATING_WINDOW_ELEVATION_PX,
    );
    let mut paint = fill_paint(popup_fill_color(theme_colors));
    canvas.draw_round_rect(rect, 6.0, 6.0, &paint);

    paint.set_style(paint::Style::Stroke);
    paint.set_stroke_width(1.0);
    paint.set_color(popup_stroke_color(theme_colors));
    canvas.draw_round_rect(rect, 6.0, 6.0, &paint);

    let item_frames =
        activity_overlay_item_frames(surface_width, surface_height, elements, overlay);
    for (index, item_frame) in item_frames.iter().enumerate() {
        draw_activity_overlay_item(canvas, surface_height, item_frame, theme_colors);

        if index + 1 < item_frames.len() {
            let item_rect = activity_rect_to_skia_rect(item_frame.frame, surface_height);
            let divider_y = item_rect.bottom();
            let divider_paint = fill_paint(popup_divider_color(theme_colors));
            canvas.draw_line(
                Point::new(item_rect.left() + 12.0, divider_y),
                Point::new(item_rect.right() - 12.0, divider_y),
                &divider_paint,
            );
        }
    }
}

fn draw_activity_overlay_item(
    canvas: &skia_safe::Canvas,
    surface_height: u32,
    item_frame: &crate::host_window::ActivityOverlayItemFrame,
    theme_colors: SurfaceThemeColors,
) {
    let rect = activity_rect_to_skia_rect(item_frame.frame, surface_height);
    let text = clean_render_text(&item_frame.item.text);
    let font = ui_font(15.0, false);
    let color = if item_frame.item.enabled {
        popup_text_color(theme_colors)
    } else {
        popup_disabled_text_color(theme_colors)
    };
    let paint = fill_paint(color);
    let baseline = rect.top() + (ACTIVITY_OVERLAY_ITEM_HEIGHT as f32 / 2.0) + 5.0;

    canvas.draw_str(
        &text,
        Point::new(rect.left() + 16.0, baseline),
        &font,
        &paint,
    );
}

fn draw_dialog_overlay(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    dialog: &ActivityDialogOverlay,
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    let scrim = Rect::from_xywh(0.0, 0.0, surface_width as f32, surface_height as f32);
    canvas.draw_rect(scrim, &fill_paint(DIALOG_SCRIM));

    let frame = activity_dialog_frame(surface_width, surface_height, dialog);
    let rect = activity_rect_to_skia_rect(frame, surface_height);
    // cite: frameworks-base themes_material.xml#windowElevation
    draw_elevation_shadow(
        canvas,
        surface_width,
        surface_height,
        rect,
        8.0,
        FLOATING_WINDOW_ELEVATION_PX,
    );
    let mut paint = fill_paint(dialog_fill_color(theme_colors));
    canvas.draw_round_rect(rect, 8.0, 8.0, &paint);

    paint.set_style(paint::Style::Stroke);
    paint.set_stroke_width(1.0);
    paint.set_color(dialog_stroke_color(theme_colors));
    canvas.draw_round_rect(rect, 8.0, 8.0, &paint);

    draw_dialog_title(canvas, rect, dialog.title.as_deref(), theme_colors);
    draw_dialog_content(canvas, surface_width, surface_height, dialog, theme_colors)?;
    draw_dialog_buttons(canvas, surface_width, surface_height, dialog, theme_colors);

    Ok(())
}

fn draw_dialog_title(
    canvas: &skia_safe::Canvas,
    rect: Rect,
    title: Option<&str>,
    theme_colors: SurfaceThemeColors,
) {
    let Some(title) = title else {
        return;
    };

    let font = ui_font(19.0, true);
    let paint = fill_paint(dialog_title_color(theme_colors));
    canvas.draw_str(
        clean_render_text(title),
        Point::new(rect.left() + 22.0, rect.top() + 34.0),
        &font,
        &paint,
    );
}

fn draw_dialog_content(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    dialog: &ActivityDialogOverlay,
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    let content = activity_dialog_content_rect(surface_width, surface_height, dialog);

    if content.width == 0 || content.height == 0 {
        return Ok(());
    }

    let clip = activity_rect_to_skia_rect(content, surface_height);
    canvas.save();
    canvas.clip_rect(clip, None, Some(false));

    if dialog.items.is_empty() {
        for element in &dialog.text_elements {
            let mut offset = offset_dialog_element(element, content.x, content.y);
            if offset.text_color.is_none() {
                offset.text_color = theme_colors.foreground;
            }
            draw_text_element(
                canvas,
                content.width,
                surface_height,
                &offset,
                &[],
                theme_colors,
            )?;
        }
    } else {
        draw_dialog_items(canvas, surface_width, surface_height, dialog, theme_colors)?;
    }

    canvas.restore();
    Ok(())
}

fn offset_dialog_element(
    element: &ActivityTextElement,
    offset_x: u32,
    offset_y: u32,
) -> ActivityTextElement {
    let mut offset = element.clone();
    offset.x = offset.x.saturating_add(offset_x);
    offset.y = offset
        .y
        .saturating_add(i32::try_from(offset_y).unwrap_or(i32::MAX));
    offset
}

fn draw_dialog_items(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    dialog: &ActivityDialogOverlay,
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    let frames = activity_dialog_item_frames(surface_width, surface_height, 0, dialog);

    for (index, frame) in frames.iter().enumerate() {
        draw_dialog_item(canvas, surface_height, frame, theme_colors);

        if index + 1 < frames.len() {
            let rect = activity_rect_to_skia_rect(frame.frame, surface_height);
            let divider = fill_paint(dialog_divider_color(theme_colors));
            canvas.draw_line(
                Point::new(rect.left() + 12.0, rect.bottom()),
                Point::new(rect.right() - 12.0, rect.bottom()),
                &divider,
            );
        }
    }

    Ok(())
}

fn draw_dialog_item(
    canvas: &skia_safe::Canvas,
    surface_height: u32,
    item_frame: &crate::host_window::ActivityDialogItemFrame,
    theme_colors: SurfaceThemeColors,
) {
    let rect = activity_rect_to_skia_rect(item_frame.frame, surface_height);
    let fill = fill_paint(dialog_fill_color(theme_colors));
    canvas.draw_rect(rect, &fill);

    let text = clean_render_text(&item_frame.item.text);
    let font = ui_font(15.0, false);
    let paint = fill_paint(if item_frame.item.enabled {
        dialog_text_color(theme_colors)
    } else {
        dialog_disabled_text_color(theme_colors)
    });
    let baseline = rect.top() + (ACTIVITY_DIALOG_ITEM_HEIGHT as f32 / 2.0) + 5.0;

    canvas.save();
    canvas.clip_rect(
        Rect::new(rect.left(), rect.top(), rect.right() - 40.0, rect.bottom()),
        None,
        Some(false),
    );
    canvas.draw_str(
        &text,
        Point::new(rect.left() + 14.0, baseline),
        &font,
        &paint,
    );
    canvas.restore();

    if item_frame.item.selected {
        draw_dialog_selected_check(canvas, &rect, theme_colors);
    }
}

fn draw_dialog_selected_check(
    canvas: &skia_safe::Canvas,
    rect: &Rect,
    theme_colors: SurfaceThemeColors,
) {
    let center_x = rect.right() - 24.0;
    let center_y = rect.top() + rect.height() / 2.0;
    let paint = stroke_paint(dialog_button_text_color(theme_colors), 2.0);

    canvas.draw_line(
        Point::new(center_x - 8.0, center_y),
        Point::new(center_x - 2.0, center_y + 6.0),
        &paint,
    );
    canvas.draw_line(
        Point::new(center_x - 2.0, center_y + 6.0),
        Point::new(center_x + 9.0, center_y - 7.0),
        &paint,
    );
}

fn draw_dialog_buttons(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    dialog: &ActivityDialogOverlay,
    theme_colors: SurfaceThemeColors,
) {
    if dialog.buttons.is_empty() {
        return;
    }

    let font = ui_font(14.0, true);
    let paint = fill_paint(dialog_button_text_color(theme_colors));
    let button_frames = activity_dialog_button_frames(surface_width, surface_height, 0, dialog);

    for button_frame in button_frames {
        if !button_frame.button.enabled {
            continue;
        }

        let text = clean_render_text(&button_frame.button.text);
        let button_rect = activity_rect_to_skia_rect(button_frame.frame, surface_height);
        let baseline = button_rect.top() + 25.0;
        canvas.draw_str(
            text,
            Point::new(button_rect.left() + 12.0, baseline),
            &font,
            &paint,
        );
    }
}

fn draw_bootstrap_message(canvas: &skia_safe::Canvas, message: &str) {
    let paint = fill_paint(LABEL_TEXT);
    let font = ui_font(14.0, false);
    let mut y = 28.0;

    for line in message.lines().take(24) {
        canvas.draw_str(line, Point::new(18.0, y), &font, &paint);
        y += 20.0;
    }
}

fn draw_action_bar(
    canvas: &skia_safe::Canvas,
    width: u32,
    height: u32,
    action_bar: &ActivityActionBar,
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    let bar_height = ACTION_BAR_HEIGHT as f32;
    let background_argb = action_bar_background_argb(action_bar, theme_colors);
    let foreground_argb = if action_bar.is_decor || !action_bar.title.is_empty() {
        on_action_bar_argb(background_argb)
    } else {
        OVERFLOW_ONLY_FOREGROUND
    };

    if action_bar.is_decor || !action_bar.title.is_empty() {
        let bar_frame = Rect::from_xywh(0.0, 0.0, width as f32, bar_height);
        // cite: frameworks-base styles_material.xml#Widget.Material.ActionBar
        draw_elevation_shadow(
            canvas,
            width,
            height,
            bar_frame,
            0.0,
            DECOR_ACTION_BAR_ELEVATION_PX,
        );
        canvas.draw_rect(bar_frame, &fill_paint(android_argb_color(background_argb)));

        let mut title_x = 16.0_f32;
        if let Some(icon) = &action_bar.navigation_icon {
            let icon_size = 26.0_f32;
            let rect = Rect::from_xywh(12.0, (bar_height - icon_size) / 2.0, icon_size, icon_size);
            draw_button_image_with_fallback_tint(canvas, &rect, icon, Some(foreground_argb))?;
            title_x = 56.0;
        } else if action_bar.navigation_is_up {
            draw_up_chevron(
                canvas,
                Rect::from_xywh(12.0, (bar_height - 26.0) / 2.0, 26.0, 26.0),
                android_argb_color(foreground_argb),
            );
            title_x = 56.0;
        }

        if !action_bar.title.is_empty() {
            let title_font = ui_font(20.0, true);
            canvas.draw_str(
                clean_render_text(&action_bar.title),
                Point::new(title_x, bar_height / 2.0 + 7.0),
                &title_font,
                &fill_paint(android_argb_color(foreground_argb)),
            );
        }
    }

    let slot = 48.0_f32;
    let icon = 26.0_f32;
    let mut right = width as f32;

    if action_bar.items.iter().any(|item| item.overflow) {
        right -= slot;
        draw_overflow_indicator(
            canvas,
            right + slot / 2.0,
            bar_height / 2.0,
            android_argb_color(foreground_argb),
        );
    }

    for item in action_bar.items.iter().filter(|item| !item.overflow) {
        right -= slot;
        let rect = Rect::from_xywh(
            right + (slot - icon) / 2.0,
            (bar_height - icon) / 2.0,
            icon,
            icon,
        );
        if let Some(image) = &item.image {
            draw_button_image_with_fallback_tint(canvas, &rect, image, Some(foreground_argb))?;
        }
    }

    Ok(())
}

fn action_bar_background_argb(
    action_bar: &ActivityActionBar,
    theme_colors: SurfaceThemeColors,
) -> u32 {
    if let Some(color) = action_bar.background_color {
        return color;
    }
    let base = theme_colors.background.unwrap_or(0xff44_46_48);
    darken_argb(base, 0.86)
}

fn darken_argb(argb: u32, factor: f32) -> u32 {
    let scale = |shift: u32| {
        let channel = ((argb >> shift) & 0xff) as f32 * factor;
        (channel.round() as u32).min(255) << shift
    };
    (argb & 0xff00_0000) | scale(16) | scale(8) | scale(0)
}

fn on_action_bar_argb(background_argb: u32) -> u32 {
    let red = ((background_argb >> 16) & 0xff) as f32;
    let green = ((background_argb >> 8) & 0xff) as f32;
    let blue = (background_argb & 0xff) as f32;
    let luminance = 0.299 * red + 0.587 * green + 0.114 * blue;
    if luminance > 140.0 {
        0xff20_2024
    } else {
        0xffff_ffff
    }
}

fn draw_up_chevron(canvas: &skia_safe::Canvas, rect: Rect, color: Color) {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(skia_safe::paint::Style::Stroke);
    paint.set_stroke_width(2.4);
    paint.set_stroke_cap(skia_safe::paint::Cap::Round);
    paint.set_stroke_join(skia_safe::paint::Join::Round);
    paint.set_color(color);

    let mid_y = rect.center_y();
    let tip_x = rect.left() + rect.width() * 0.34;
    let back_x = rect.left() + rect.width() * 0.66;
    let arm = rect.height() * 0.22;

    canvas.draw_line(Point::new(tip_x, mid_y), Point::new(back_x, mid_y), &paint);
    canvas.draw_line(
        Point::new(tip_x, mid_y),
        Point::new(tip_x + arm, mid_y - arm),
        &paint,
    );
    canvas.draw_line(
        Point::new(tip_x, mid_y),
        Point::new(tip_x + arm, mid_y + arm),
        &paint,
    );
}

fn draw_overflow_indicator(canvas: &skia_safe::Canvas, center_x: f32, center_y: f32, color: Color) {
    let paint = fill_paint(color);
    for offset in [-7.0_f32, 0.0, 7.0] {
        canvas.draw_circle(Point::new(center_x, center_y + offset), 2.0, &paint);
    }
}

fn draw_runtime_view_root(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    element: &ActivityTextElement,
    runtime_canvas_frames: &[ActivityRuntimeCanvasFrame],
) -> Result<()> {
    if element.width == 0 || element.height == 0 {
        return Ok(());
    }

    let frame = label_rect(element, surface_width, surface_height);
    let surface_fill = fill_paint(Color::from_argb(255, 250, 250, 250));
    canvas.draw_rect(frame, &surface_fill);

    if !runtime_canvas_frames.is_empty() {
        for runtime_canvas_frame in runtime_canvas_frames {
            draw_runtime_canvas_frame(
                canvas,
                &frame,
                surface_width,
                surface_height,
                runtime_canvas_frame,
            );
        }
        return Ok(());
    }

    let top_bar = Rect::from_xywh(frame.left(), frame.top(), frame.width(), 64.0);
    let top_bar_fill = fill_paint(Color::from_argb(255, 255, 255, 255));
    canvas.draw_rect(top_bar, &top_bar_fill);

    let divider = fill_paint(LIST_ITEM_DIVIDER);
    canvas.draw_line(
        Point::new(frame.left(), top_bar.bottom()),
        Point::new(frame.right(), top_bar.bottom()),
        &divider,
    );

    let lines = element
        .text
        .lines()
        .map(clean_render_text)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let title = lines.first().map(String::as_str).unwrap_or("Runtime View");
    let detail = lines.get(1).map(String::as_str).unwrap_or("");
    let title_font = ui_font(19.0, true);
    let detail_font = ui_font(12.0, false);
    let title_paint = fill_paint(LIST_ITEM_PRIMARY_TEXT);
    let detail_paint = fill_paint(LIST_ITEM_SECONDARY_TEXT);
    let text_x = frame.left() + 24.0;

    canvas.draw_str(
        title,
        Point::new(text_x, frame.top() + 37.0),
        &title_font,
        &title_paint,
    );

    if !detail.is_empty() {
        canvas.draw_str(
            detail,
            Point::new(text_x, frame.top() + 54.0),
            &detail_font,
            &detail_paint,
        );
    }

    Ok(())
}

fn draw_runtime_canvas_frame(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    window_width: u32,
    window_height: u32,
    runtime_canvas_frame: &ActivityRuntimeCanvasFrame,
) {
    if runtime_canvas_frame.width == 0
        || runtime_canvas_frame.height == 0
        || window_width == 0
        || window_height == 0
    {
        return;
    }

    let window_to_frame_x = frame.width() / window_width as f32;
    let window_to_frame_y = frame.height() / window_height as f32;
    let target = Rect::from_xywh(
        frame.left() + runtime_canvas_frame.x as f32 * window_to_frame_x,
        frame.top() + runtime_canvas_frame.y as f32 * window_to_frame_y,
        runtime_canvas_frame.width as f32 * window_to_frame_x,
        runtime_canvas_frame.height as f32 * window_to_frame_y,
    );
    let save_count = canvas.save();
    canvas.clip_rect(target, None, Some(false));
    canvas.translate((target.left(), target.top()));
    canvas.scale((window_to_frame_x, window_to_frame_y));
    let render_metric_start = render_metrics::start_timing();
    draw_runtime_canvas_ops(canvas, &runtime_canvas_frame.ops);
    render_metrics::record_retained_replay_elapsed(render_metric_start);
    canvas.restore_to_count(save_count);
}

fn draw_runtime_canvas_ops(canvas: &skia_safe::Canvas, ops: &[ActivityRuntimeCanvasOp]) {
    for op in ops {
        draw_runtime_canvas_op(canvas, op);
    }
}

fn draw_runtime_canvas_op(canvas: &skia_safe::Canvas, op: &ActivityRuntimeCanvasOp) {
    match op {
        ActivityRuntimeCanvasOp::Save => {
            canvas.save();
        }
        ActivityRuntimeCanvasOp::Restore => {
            canvas.restore();
        }
        ActivityRuntimeCanvasOp::Translate { dx, dy } => {
            canvas.translate((*dx, *dy));
        }
        ActivityRuntimeCanvasOp::Scale { sx, sy } => {
            canvas.scale((*sx, *sy));
        }
        ActivityRuntimeCanvasOp::Rotate { degrees } => {
            canvas.rotate(*degrees, None);
        }
        ActivityRuntimeCanvasOp::ConcatMatrix { values } => {
            let matrix = Matrix::new_all(
                values[0], values[1], values[2], values[3], values[4], values[5], values[6],
                values[7], values[8],
            );
            canvas.concat(&matrix);
        }
        ActivityRuntimeCanvasOp::ClipRect {
            left,
            top,
            right,
            bottom,
            op,
        } => {
            let rect = Rect::new(*left, *top, *right, *bottom);
            canvas.clip_rect(rect, runtime_canvas_clip_op(op), Some(false));
        }
        ActivityRuntimeCanvasOp::ClipPath { ops, op } => {
            let path = runtime_canvas_path(ops);
            canvas.clip_path(&path, runtime_canvas_clip_op(op), Some(true));
        }
        ActivityRuntimeCanvasOp::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            blend_mode,
            stroke_width,
            stroke_cap,
        } => {
            let mut paint = fill_paint(android_argb_color(*color));
            paint.set_style(paint::Style::Stroke);
            paint.set_stroke_width(*stroke_width);
            apply_runtime_canvas_blend_mode(&mut paint, blend_mode);
            apply_runtime_canvas_stroke_cap(&mut paint, stroke_cap);
            canvas.draw_line(Point::new(*x1, *y1), Point::new(*x2, *y2), &paint);
        }
        ActivityRuntimeCanvasOp::Rect {
            left,
            top,
            right,
            bottom,
            color,
            blend_mode,
            paint_style,
        } => {
            let rect = Rect::new(*left, *top, *right, *bottom);
            let mut paint = fill_paint(android_argb_color(*color));
            apply_runtime_canvas_blend_mode(&mut paint, blend_mode);
            apply_runtime_canvas_paint_style(&mut paint, paint_style);
            canvas.draw_rect(rect, &paint);
        }
        ActivityRuntimeCanvasOp::Circle {
            cx,
            cy,
            radius,
            color,
            blend_mode,
            paint_style,
            stroke_width,
        } => {
            let mut paint = fill_paint(android_argb_color(*color));
            paint.set_stroke_width((*stroke_width).max(0.0));
            apply_runtime_canvas_blend_mode(&mut paint, blend_mode);
            apply_runtime_canvas_paint_style(&mut paint, paint_style);
            canvas.draw_circle(Point::new(*cx, *cy), *radius, &paint);
        }
        ActivityRuntimeCanvasOp::RoundRect {
            left,
            top,
            right,
            bottom,
            rx,
            ry,
            color,
            blend_mode,
            paint_style,
        } => {
            let rect = Rect::new(*left, *top, *right, *bottom);
            let mut paint = fill_paint(android_argb_color(*color));
            apply_runtime_canvas_blend_mode(&mut paint, blend_mode);
            apply_runtime_canvas_paint_style(&mut paint, paint_style);
            canvas.draw_round_rect(rect, *rx, *ry, &paint);
        }
        ActivityRuntimeCanvasOp::Path {
            ops,
            color,
            blend_mode,
            paint_style,
        } => {
            let path = runtime_canvas_path(ops);
            let mut paint = fill_paint(android_argb_color(*color));
            apply_runtime_canvas_blend_mode(&mut paint, blend_mode);
            apply_runtime_canvas_paint_style(&mut paint, paint_style);
            canvas.draw_path(&path, &paint);
        }
        ActivityRuntimeCanvasOp::Text {
            text,
            x,
            y,
            color,
            text_size,
        } => {
            if text.is_empty() {
                return;
            }

            let paint = fill_paint(android_argb_color(*color));
            let font = ui_font((*text_size).max(1.0), false);
            canvas.draw_str(text, Point::new(*x, *y), &font, &paint);
        }
        ActivityRuntimeCanvasOp::Bitmap {
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
        } => draw_runtime_canvas_bitmap(
            canvas,
            RuntimeCanvasBitmap {
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
                blend_mode: *blend_mode,
                color_filter: *color_filter,
                alpha_mask: *alpha_mask,
                encoded_image: encoded_image.as_deref(),
                ops,
            },
        ),
        ActivityRuntimeCanvasOp::RenderNode {
            left,
            top,
            right,
            bottom,
            alpha,
            scale_x,
            scale_y,
            clip_to_bounds,
            ops,
        } => {
            // cite: frameworks-base RenderNode.java#setPosition, setAlpha, setScaleX, setScaleY, setClipToBounds
            let save_count = canvas.save();
            let width = (right - left).max(0.0);
            let height = (bottom - top).max(0.0);
            canvas.translate((*left, *top));
            if *clip_to_bounds && width > 0.0 && height > 0.0 {
                canvas.clip_rect(Rect::from_xywh(0.0, 0.0, width, height), None, Some(false));
            }
            if *alpha < 1.0 {
                let mut layer_paint = Paint::default();
                layer_paint.set_alpha_f(alpha.clamp(0.0, 1.0));
                if width > 0.0 && height > 0.0 {
                    let bounds = Rect::from_xywh(0.0, 0.0, width, height);
                    canvas.save_layer(
                        &canvas::SaveLayerRec::default()
                            .bounds(&bounds)
                            .paint(&layer_paint),
                    );
                } else {
                    canvas.save_layer(&canvas::SaveLayerRec::default().paint(&layer_paint));
                }
            }
            if *scale_x != 1.0 || *scale_y != 1.0 {
                canvas.scale((*scale_x, *scale_y));
            }
            draw_runtime_canvas_ops(canvas, ops);
            canvas.restore_to_count(save_count);
        }
    }
}

fn runtime_canvas_clip_op(
    op: &crate::host_window::ActivityRuntimeCanvasClipOp,
) -> skia_safe::ClipOp {
    match op {
        crate::host_window::ActivityRuntimeCanvasClipOp::Intersect => skia_safe::ClipOp::Intersect,
        crate::host_window::ActivityRuntimeCanvasClipOp::Difference => {
            skia_safe::ClipOp::Difference
        }
    }
}

fn apply_runtime_canvas_blend_mode(
    paint: &mut Paint,
    blend_mode: &crate::host_window::ActivityRuntimeCanvasBlendMode,
) {
    paint.set_blend_mode(runtime_canvas_skia_blend_mode(blend_mode));
}

fn runtime_canvas_skia_blend_mode(
    blend_mode: &crate::host_window::ActivityRuntimeCanvasBlendMode,
) -> skia_safe::BlendMode {
    use crate::host_window::ActivityRuntimeCanvasBlendMode;

    // cite: frameworks-base BlendMode.java#CLEAR, SRC, DST, SRC_OVER, DST_OVER, SRC_IN, DST_IN, SRC_OUT, DST_OUT, SRC_ATOP, DST_ATOP, XOR, PLUS, MODULATE, SCREEN, OVERLAY, DARKEN, LIGHTEN, COLOR_DODGE, COLOR_BURN, HARD_LIGHT, SOFT_LIGHT, DIFFERENCE, EXCLUSION, MULTIPLY, HUE, SATURATION, COLOR, LUMINOSITY
    match blend_mode {
        ActivityRuntimeCanvasBlendMode::Clear => skia_safe::BlendMode::Clear,
        ActivityRuntimeCanvasBlendMode::Src => skia_safe::BlendMode::Src,
        ActivityRuntimeCanvasBlendMode::Dst => skia_safe::BlendMode::Dst,
        ActivityRuntimeCanvasBlendMode::SrcOver => skia_safe::BlendMode::SrcOver,
        ActivityRuntimeCanvasBlendMode::DstOver => skia_safe::BlendMode::DstOver,
        ActivityRuntimeCanvasBlendMode::SrcIn => skia_safe::BlendMode::SrcIn,
        ActivityRuntimeCanvasBlendMode::DstIn => skia_safe::BlendMode::DstIn,
        ActivityRuntimeCanvasBlendMode::SrcOut => skia_safe::BlendMode::SrcOut,
        ActivityRuntimeCanvasBlendMode::DstOut => skia_safe::BlendMode::DstOut,
        ActivityRuntimeCanvasBlendMode::SrcAtop => skia_safe::BlendMode::SrcATop,
        ActivityRuntimeCanvasBlendMode::DstAtop => skia_safe::BlendMode::DstATop,
        ActivityRuntimeCanvasBlendMode::Xor => skia_safe::BlendMode::Xor,
        ActivityRuntimeCanvasBlendMode::Plus => skia_safe::BlendMode::Plus,
        ActivityRuntimeCanvasBlendMode::Modulate => skia_safe::BlendMode::Modulate,
        ActivityRuntimeCanvasBlendMode::Screen => skia_safe::BlendMode::Screen,
        ActivityRuntimeCanvasBlendMode::Overlay => skia_safe::BlendMode::Overlay,
        ActivityRuntimeCanvasBlendMode::Darken => skia_safe::BlendMode::Darken,
        ActivityRuntimeCanvasBlendMode::Lighten => skia_safe::BlendMode::Lighten,
        ActivityRuntimeCanvasBlendMode::ColorDodge => skia_safe::BlendMode::ColorDodge,
        ActivityRuntimeCanvasBlendMode::ColorBurn => skia_safe::BlendMode::ColorBurn,
        ActivityRuntimeCanvasBlendMode::HardLight => skia_safe::BlendMode::HardLight,
        ActivityRuntimeCanvasBlendMode::SoftLight => skia_safe::BlendMode::SoftLight,
        ActivityRuntimeCanvasBlendMode::Difference => skia_safe::BlendMode::Difference,
        ActivityRuntimeCanvasBlendMode::Exclusion => skia_safe::BlendMode::Exclusion,
        ActivityRuntimeCanvasBlendMode::Multiply => skia_safe::BlendMode::Multiply,
        ActivityRuntimeCanvasBlendMode::Hue => skia_safe::BlendMode::Hue,
        ActivityRuntimeCanvasBlendMode::Saturation => skia_safe::BlendMode::Saturation,
        ActivityRuntimeCanvasBlendMode::Color => skia_safe::BlendMode::Color,
        ActivityRuntimeCanvasBlendMode::Luminosity => skia_safe::BlendMode::Luminosity,
    }
}

fn apply_runtime_canvas_paint_style(
    paint: &mut Paint,
    paint_style: &crate::host_window::ActivityRuntimeCanvasPaintStyle,
) {
    match paint_style {
        crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill => {
            paint.set_style(paint::Style::Fill);
        }
        crate::host_window::ActivityRuntimeCanvasPaintStyle::Stroke => {
            paint.set_style(paint::Style::Stroke);
        }
        crate::host_window::ActivityRuntimeCanvasPaintStyle::FillAndStroke => {
            paint.set_style(paint::Style::StrokeAndFill);
        }
    }
}

fn apply_runtime_canvas_stroke_cap(
    paint: &mut Paint,
    stroke_cap: &crate::host_window::ActivityRuntimeCanvasStrokeCap,
) {
    match stroke_cap {
        crate::host_window::ActivityRuntimeCanvasStrokeCap::Butt => {
            paint.set_stroke_cap(paint::Cap::Butt);
        }
        crate::host_window::ActivityRuntimeCanvasStrokeCap::Round => {
            paint.set_stroke_cap(paint::Cap::Round);
        }
        crate::host_window::ActivityRuntimeCanvasStrokeCap::Square => {
            paint.set_stroke_cap(paint::Cap::Square);
        }
    }
}

fn runtime_canvas_path(ops: &[crate::host_window::ActivityRuntimeCanvasPathOp]) -> skia_safe::Path {
    let mut builder = skia_safe::PathBuilder::new();

    for op in ops {
        match op {
            crate::host_window::ActivityRuntimeCanvasPathOp::MoveTo { x, y } => {
                builder.move_to(Point::new(*x, *y));
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::LineTo { x, y } => {
                builder.line_to(Point::new(*x, *y));
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::RLineTo { dx, dy } => {
                builder.r_line_to((*dx, *dy));
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::QuadTo { x1, y1, x2, y2 } => {
                builder.quad_to(Point::new(*x1, *y1), Point::new(*x2, *y2));
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                builder.cubic_to(
                    Point::new(*x1, *y1),
                    Point::new(*x2, *y2),
                    Point::new(*x3, *y3),
                );
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::RCubicTo {
                dx1,
                dy1,
                dx2,
                dy2,
                dx3,
                dy3,
            } => {
                builder.r_cubic_to((*dx1, *dy1), (*dx2, *dy2), (*dx3, *dy3));
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::BooleanOp { left, right, op } => {
                let left_path = runtime_canvas_path(left);
                let right_path = runtime_canvas_path(right);
                let result = left_path
                    .op(&right_path, runtime_canvas_path_boolean_op(op))
                    .expect("Skia path operation failed while rendering Android Path.op");
                builder.add_path(&result, None);
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::Rect {
                left,
                top,
                right,
                bottom,
                clockwise,
            } => {
                let rect = Rect::new(*left, *top, *right, *bottom);
                let direction = if *clockwise {
                    skia_safe::PathDirection::CW
                } else {
                    skia_safe::PathDirection::CCW
                };
                builder.add_rect(rect, direction, None);
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::RoundRect {
                left,
                top,
                right,
                bottom,
                radii,
                clockwise,
            } => {
                let rect = Rect::new(*left, *top, *right, *bottom);
                let rrect =
                    skia_safe::RRect::new_rect_radii(rect, &runtime_canvas_path_radii(radii));
                let direction = if *clockwise {
                    skia_safe::PathDirection::CW
                } else {
                    skia_safe::PathDirection::CCW
                };
                builder.add_rrect(rrect, direction, None);
            }
            crate::host_window::ActivityRuntimeCanvasPathOp::Close => {
                builder.close();
            }
        }
    }

    builder.detach()
}

fn runtime_canvas_path_boolean_op(
    op: &crate::host_window::ActivityRuntimeCanvasPathBooleanOp,
) -> skia_safe::PathOp {
    match op {
        crate::host_window::ActivityRuntimeCanvasPathBooleanOp::Difference => {
            skia_safe::PathOp::Difference
        }
        crate::host_window::ActivityRuntimeCanvasPathBooleanOp::Intersect => {
            skia_safe::PathOp::Intersect
        }
        crate::host_window::ActivityRuntimeCanvasPathBooleanOp::Union => skia_safe::PathOp::Union,
        crate::host_window::ActivityRuntimeCanvasPathBooleanOp::Xor => skia_safe::PathOp::XOR,
        crate::host_window::ActivityRuntimeCanvasPathBooleanOp::ReverseDifference => {
            skia_safe::PathOp::ReverseDifference
        }
    }
}

fn runtime_canvas_path_radii(radii: &[f32]) -> [Point; 4] {
    [
        Point::new(path_radius(radii, 0), path_radius(radii, 1)),
        Point::new(path_radius(radii, 2), path_radius(radii, 3)),
        Point::new(path_radius(radii, 4), path_radius(radii, 5)),
        Point::new(path_radius(radii, 6), path_radius(radii, 7)),
    ]
}

fn path_radius(radii: &[f32], index: usize) -> f32 {
    radii.get(index).copied().unwrap_or(0.0)
}

struct RuntimeCanvasBitmap<'a> {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    source_left: f32,
    source_top: f32,
    source_right: f32,
    source_bottom: f32,
    bitmap_width: u32,
    bitmap_height: u32,
    color: u32,
    blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode,
    color_filter: Option<crate::host_window::ActivityRuntimeCanvasColorFilter>,
    alpha_mask: bool,
    encoded_image: Option<&'a [u8]>,
    ops: &'a [ActivityRuntimeCanvasOp],
}

fn draw_runtime_canvas_bitmap(canvas: &skia_safe::Canvas, bitmap: RuntimeCanvasBitmap<'_>) {
    let destination_width = bitmap.right - bitmap.left;
    let destination_height = bitmap.bottom - bitmap.top;
    let source_width = bitmap.source_right - bitmap.source_left;
    let source_height = bitmap.source_bottom - bitmap.source_top;
    if bitmap.bitmap_width == 0
        || bitmap.bitmap_height == 0
        || destination_width <= 0.0
        || destination_height <= 0.0
        || source_width <= 0.0
        || source_height <= 0.0
    {
        return;
    }

    let destination = Rect::new(bitmap.left, bitmap.top, bitmap.right, bitmap.bottom);
    let layer_paint = runtime_canvas_bitmap_layer_paint(&bitmap);
    let save_count = canvas.save();
    canvas.clip_rect(destination, None, Some(false));
    // cite: frameworks-base Canvas.java#Canvas
    canvas.save_layer(
        &canvas::SaveLayerRec::default()
            .bounds(&destination)
            .paint(&layer_paint),
    );
    if let Some(encoded_image) = bitmap.encoded_image {
        if let Some(image) = Image::from_encoded(Data::new_copy(encoded_image)) {
            draw_runtime_canvas_encoded_bitmap(canvas, &bitmap, image, destination);
        }
    }
    canvas.translate((bitmap.left, bitmap.top));
    canvas.scale((
        destination_width / source_width,
        destination_height / source_height,
    ));
    canvas.translate((-bitmap.source_left, -bitmap.source_top));
    draw_runtime_canvas_ops(canvas, bitmap.ops);
    canvas.restore_to_count(save_count);
}

fn draw_runtime_canvas_encoded_bitmap(
    canvas: &skia_safe::Canvas,
    bitmap: &RuntimeCanvasBitmap<'_>,
    image: Image,
    destination: Rect,
) {
    let source = Rect::new(
        bitmap.source_left,
        bitmap.source_top,
        bitmap.source_right,
        bitmap.source_bottom,
    );
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    canvas.draw_image_rect(
        image,
        Some((&source, canvas::SrcRectConstraint::Strict)),
        destination,
        &paint,
    );
}

fn runtime_canvas_bitmap_layer_paint(bitmap: &RuntimeCanvasBitmap<'_>) -> Paint {
    let color = android_argb_color(bitmap.color);
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    // cite: frameworks-base BaseCanvas.java#drawBitmap
    paint.set_alpha_f(color.a() as f32 / 255.0);
    apply_runtime_canvas_blend_mode(&mut paint, &bitmap.blend_mode);

    // cite: frameworks-base Bitmap.java#ALPHA_8
    let mask_filter = bitmap
        .alpha_mask
        .then(|| skia_safe::color_filters::blend(color, skia_safe::BlendMode::SrcIn))
        .flatten();
    // cite: frameworks-base BlendModeColorFilter.java#BlendModeColorFilter
    let paint_filter = bitmap.color_filter.and_then(|filter| {
        skia_safe::color_filters::blend(
            android_argb_color(filter.color),
            runtime_canvas_skia_blend_mode(&filter.blend_mode),
        )
    });
    let color_filter = match (paint_filter, mask_filter) {
        (Some(outer), Some(inner)) => skia_safe::color_filters::compose(outer, inner),
        (Some(filter), None) | (None, Some(filter)) => Some(filter),
        (None, None) => None,
    };
    paint.set_color_filter(color_filter);

    paint
}

fn android_argb_color(color: u32) -> Color {
    Color::from_argb(
        ((color >> 24) & 0xff) as u8,
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    )
}

fn draw_label(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    element: &ActivityTextElement,
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    if element.width == 0 || element.height == 0 {
        return Ok(());
    }

    let frame = label_rect(element, surface_width, surface_height);
    // cite: frameworks-base TextView.java#getCompoundPaddingLeft, getCompoundPaddingRight
    let text_frame = Rect::new(
        frame.left() + element.text_padding_left as f32,
        frame.top(),
        (frame.right() - element.text_padding_right as f32).max(frame.left()),
        frame.bottom(),
    );
    let cleaned = clean_render_text(&element.text);

    // cite: frameworks-base GradientDrawable.java#draw
    if let Some(background_color) = element.background_color {
        let paint = fill_paint(android_argb_color(background_color));
        let radius = element.background_corner_radius as f32;
        if radius > 0.0 {
            canvas.draw_round_rect(frame, radius, radius, &paint);
        } else {
            canvas.draw_rect(frame, &paint);
        }
    }

    if let (Some(border_color), true) = (
        element.background_border_color,
        element.background_border_width > 0,
    ) {
        let stroke_width = element.background_border_width as f32;
        let mut paint = fill_paint(android_argb_color(border_color));
        paint.set_style(paint::Style::Stroke);
        paint.set_stroke_width(stroke_width);
        let inset_frame = frame.with_inset((stroke_width / 2.0, stroke_width / 2.0));
        let radius = (element.background_corner_radius as f32 - stroke_width / 2.0).max(0.0);
        if radius > 0.0 {
            canvas.draw_round_rect(inset_frame, radius, radius, &paint);
        } else {
            canvas.draw_rect(inset_frame, &paint);
        }
    }

    if let Some(image) = &element.image {
        let image_tint = element.text_color.or(theme_colors.foreground);
        if cleaned.is_empty() && element.cursor.is_none() {
            draw_button_image_with_fallback_tint(canvas, &frame, image, image_tint)?;
            return Ok(());
        }
        draw_compound_label_image(canvas, &frame, image, image_tint)?;
    }

    if cleaned.is_empty() && element.cursor.is_none() {
        return Ok(());
    }

    let font_size = label_font_size(element);
    let font = ui_font(font_size, false);
    let paint = fill_paint(
        element
            .text_color
            .map(android_argb_color)
            .unwrap_or_else(|| theme_colors.foreground_color_or(LABEL_TEXT)),
    );

    if element.cursor.is_none() && element.image.is_none() {
        let source_lines = clean_render_text_lines(&element.text);
        // cite: frameworks-base TextView.java#onMeasure
        let wrap_width = text_frame.width();
        let visual_lines = wrap_render_lines(&source_lines, &font, &paint, wrap_width);
        if visual_lines.len() > 1 {
            draw_wrapped_label_lines(canvas, &text_frame, element, &visual_lines, &font, &paint);
            return Ok(());
        }
    }

    let text_width = font.measure_str(&cleaned, Some(&paint)).0;
    let text_x = compound_label_text_x(&text_frame, element, text_width);
    // cite: frameworks-base TextView.java#getBaseline, getVerticalOffset, setIncludeFontPadding
    // cite: frameworks-base Layout.java#getLineBaseline
    let (_, metrics) = font.metrics();
    let line_height = metrics.bottom - metrics.top;
    let baseline = frame.top() + (frame.height() - line_height).max(0.0) / 2.0 - metrics.top;

    if !cleaned.is_empty() {
        canvas.draw_str(&cleaned, Point::new(text_x, baseline), &font, &paint);
    }
    draw_text_cursor(canvas, element, &cleaned, &font, &paint, text_x, baseline);

    Ok(())
}

const LABEL_TEXT_INSET: f32 = 4.0;

fn draw_wrapped_label_lines(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    element: &ActivityTextElement,
    lines: &[String],
    font: &skia_safe::Font,
    paint: &Paint,
) {
    let (_, metrics) = font.metrics();
    let line_height = (metrics.descent - metrics.ascent + metrics.leading).max(font.size() * 1.3);
    let mut baseline = frame.top() - metrics.ascent;

    for line in lines {
        if !line.is_empty() {
            let line_width = font.measure_str(line, Some(paint)).0;
            let text_x = aligned_label_text_x(frame, element.alignment, line_width);
            canvas.draw_str(line, Point::new(text_x, baseline), font, paint);
        }
        baseline += line_height;
    }
}

fn draw_compound_label_image(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    image: &ActivityImage,
    fallback_tint: Option<u32>,
) -> Result<()> {
    let image_size = compound_image_size(frame);
    let image_frame = Rect::from_xywh(
        frame.left() + 8.0,
        frame.top() + (frame.height() - image_size) / 2.0,
        image_size,
        image_size,
    );

    draw_button_image_with_fallback_tint(canvas, &image_frame, image, fallback_tint)
}

fn compound_label_text_x(frame: &Rect, element: &ActivityTextElement, text_width: f32) -> f32 {
    let Some(image) = &element.image else {
        return aligned_label_text_x(frame, element.alignment, text_width);
    };

    let image_size = compound_image_size(frame);
    let padding = if image.padding_after == 0 {
        8.0
    } else {
        image.padding_after as f32
    };

    frame.left() + 8.0 + image_size + padding
}

fn compound_image_size(frame: &Rect) -> f32 {
    frame.height().min(30.0).max(1.0)
}

pub const SCROLLABLE_TEXT_FONT_SIZE: f32 = 16.0;

fn label_font_size(element: &ActivityTextElement) -> f32 {
    // cite: frameworks-base TextView.java#setRawTextSize
    if let Some(text_size) = element.text_size {
        if text_size > 0 {
            return text_size as f32;
        }
    }

    if element.kind == ActivityTextKind::Label && element.clip.is_some() {
        return SCROLLABLE_TEXT_FONT_SIZE;
    }

    if element.height > 72 {
        return 42.0;
    }

    if element.height >= 48 {
        return 20.0;
    }

    if element.height >= 32 {
        return 18.0;
    }

    15.0
}

// cite: frameworks-base Layout.java#getLineLeft, getLineRight, Alignment
fn aligned_label_text_x(frame: &Rect, alignment: ActivityTextAlignment, text_width: f32) -> f32 {
    match alignment {
        ActivityTextAlignment::Start => frame.left(),
        ActivityTextAlignment::Center => frame.left() + (frame.width() - text_width) / 2.0,
        ActivityTextAlignment::End => frame.right() - text_width,
    }
}

fn draw_text_cursor(
    canvas: &skia_safe::Canvas,
    element: &ActivityTextElement,
    text: &str,
    font: &Font,
    text_paint: &Paint,
    text_x: f32,
    baseline: f32,
) {
    let Some(cursor) = &element.cursor else {
        return;
    };
    if !cursor.visible {
        return;
    }

    let selection = cursor.selection_end.max(cursor.selection_start).max(0) as usize;
    let prefix = text.chars().take(selection).collect::<String>();
    let prefix_width = font.measure_str(&prefix, Some(text_paint)).0;
    let cursor_x = text_x + prefix_width + 2.0;
    let cursor_top = baseline - font.size() * 0.78;
    let cursor_bottom = baseline + 4.0;
    let mut paint = fill_paint(TEXT_CURSOR);
    paint.set_stroke_width(2.0);

    canvas.draw_line(
        Point::new(cursor_x, cursor_top),
        Point::new(cursor_x, cursor_bottom),
        &paint,
    );
}

fn draw_button(
    canvas: &skia_safe::Canvas,
    surface_height: u32,
    element: &ActivityTextElement,
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    if element.width == 0 || element.height == 0 {
        return Ok(());
    }

    let rect = element_rect(element, surface_height);
    let text = clean_render_text(&element.text);
    let text_color = element
        .text_color
        .map(android_argb_color)
        .unwrap_or(FALLBACK_CONTROL_TEXT);

    if let Some(background_color) = element.background_color {
        let mut paint = fill_paint(android_argb_color(background_color));
        canvas.draw_round_rect(rect, 8.0, 8.0, &paint);

        paint.set_style(paint::Style::Stroke);
        paint.set_stroke_width(1.0);
        paint.set_color(FALLBACK_CONTROL_STROKE);
        canvas.draw_round_rect(rect, 8.0, 8.0, &paint);
    } else if should_draw_button_fallback_fill(element, &text) {
        let mut paint = fill_paint(FALLBACK_CONTROL_FILL);
        canvas.draw_round_rect(rect, 8.0, 8.0, &paint);

        paint.set_style(paint::Style::Stroke);
        paint.set_stroke_width(1.0);
        paint.set_color(FALLBACK_CONTROL_STROKE);
        canvas.draw_round_rect(rect, 8.0, 8.0, &paint);
    }

    let font = ui_font(button_font_size(&text), true);
    let text_paint = fill_paint(text_color);
    if let Some(image) = &element.image {
        let image_tint = element.text_color.or(theme_colors.foreground);
        if text.is_empty() {
            draw_button_image_with_fallback_tint(canvas, &rect, image, image_tint)?;
            return Ok(());
        }
        draw_compound_label_image(canvas, &rect, image, image_tint)?;
    }

    let text_width = font.measure_str(&text, Some(&text_paint)).0;
    let text_x = compound_button_text_x(&rect, element, text_width);
    let text_y = rect.top() + rect.height() / 2.0 + 8.0;

    canvas.draw_str(&text, Point::new(text_x, text_y), &font, &text_paint);

    Ok(())
}

fn should_draw_button_fallback_fill(element: &ActivityTextElement, text: &str) -> bool {
    element.text_color.is_none() && !(text.is_empty() && element.image.is_some())
}

fn compound_button_text_x(frame: &Rect, element: &ActivityTextElement, text_width: f32) -> f32 {
    if element.image.is_some() {
        return compound_label_text_x(frame, element, text_width);
    }

    frame.left() + ((frame.width() - text_width) / 2.0).max(6.0)
}

fn draw_list_item(
    canvas: &skia_safe::Canvas,
    surface_height: u32,
    element: &ActivityTextElement,
    theme_colors: SurfaceThemeColors,
) -> Result<()> {
    if element.width == 0 || element.height == 0 {
        return Ok(());
    }

    let rect = element_rect(element, surface_height);
    let (title, summary) = list_item_text(&element.text);
    if title.is_empty() {
        return Ok(());
    }

    let fill = fill_paint(themed_list_item_fill_color(element, theme_colors));
    canvas.draw_rect(rect, &fill);

    let divider = fill_paint(themed_list_item_divider_color(element, theme_colors));
    canvas.draw_line(
        Point::new(rect.left(), rect.bottom()),
        Point::new(rect.right(), rect.bottom()),
        &divider,
    );

    let title_font = ui_font(15.0, true);
    let title_paint = fill_paint(themed_list_item_primary_text_color(element, theme_colors));
    let text_x = list_item_text_left(&rect, element.image.as_ref());
    let text_right = list_item_text_right(&rect, element.accessory);
    draw_list_item_image(
        canvas,
        &rect,
        element.image.as_ref(),
        element.text_color.or(theme_colors.foreground),
    )?;

    if let Some(summary) = summary {
        let summary_font = ui_font(13.0, false);
        let summary_paint =
            fill_paint(themed_list_item_secondary_text_color(element, theme_colors));

        canvas.save();
        canvas.clip_rect(
            Rect::new(rect.left(), rect.top(), text_right, rect.bottom()),
            None,
            Some(false),
        );
        canvas.draw_str(
            &title,
            Point::new(text_x, rect.top() + 22.0),
            &title_font,
            &title_paint,
        );
        canvas.draw_str(
            &summary,
            Point::new(text_x, rect.top() + 42.0),
            &summary_font,
            &summary_paint,
        );
        canvas.restore();
    } else {
        let baseline = rect.top() + rect.height() / 2.0 + 5.0;
        canvas.save();
        canvas.clip_rect(
            Rect::new(rect.left(), rect.top(), text_right, rect.bottom()),
            None,
            Some(false),
        );
        canvas.draw_str(
            &title,
            Point::new(text_x, baseline),
            &title_font,
            &title_paint,
        );
        canvas.restore();
    }

    draw_list_item_accessory(canvas, &rect, element.accessory);
    Ok(())
}

#[cfg(test)]
fn list_item_fill_color(element: &ActivityTextElement) -> Color {
    element
        .background_color
        .map(android_argb_color)
        .unwrap_or(LIST_ITEM_FILL)
}

fn themed_list_item_fill_color(
    element: &ActivityTextElement,
    theme_colors: SurfaceThemeColors,
) -> Color {
    element
        .background_color
        .map(android_argb_color)
        .or_else(|| theme_colors.background_color())
        .unwrap_or(LIST_ITEM_FILL)
}

#[cfg(test)]
fn list_item_primary_text_color(element: &ActivityTextElement) -> Color {
    element
        .text_color
        .map(android_argb_color)
        .unwrap_or(LIST_ITEM_PRIMARY_TEXT)
}

fn themed_list_item_primary_text_color(
    element: &ActivityTextElement,
    theme_colors: SurfaceThemeColors,
) -> Color {
    element
        .text_color
        .map(android_argb_color)
        .unwrap_or_else(|| theme_colors.foreground_color_or(LIST_ITEM_PRIMARY_TEXT))
}

#[cfg(test)]
fn list_item_secondary_text_color(element: &ActivityTextElement) -> Color {
    element
        .text_color
        .map(secondary_android_argb_color)
        .unwrap_or(LIST_ITEM_SECONDARY_TEXT)
}

fn themed_list_item_secondary_text_color(
    element: &ActivityTextElement,
    theme_colors: SurfaceThemeColors,
) -> Color {
    element
        .text_color
        .map(secondary_android_argb_color)
        .unwrap_or_else(|| theme_colors.secondary_foreground_color_or(LIST_ITEM_SECONDARY_TEXT))
}

#[cfg(test)]
fn list_item_divider_color(element: &ActivityTextElement) -> Color {
    element
        .background_color
        .map(divider_android_argb_color)
        .unwrap_or(LIST_ITEM_DIVIDER)
}

fn themed_list_item_divider_color(
    element: &ActivityTextElement,
    theme_colors: SurfaceThemeColors,
) -> Color {
    element
        .background_color
        .map(divider_android_argb_color)
        .or_else(|| theme_colors.background.map(divider_android_argb_color))
        .unwrap_or(LIST_ITEM_DIVIDER)
}

fn secondary_android_argb_color(color: u32) -> Color {
    alpha_scaled_android_argb_color(color, 70)
}

fn disabled_android_argb_color(color: u32) -> Color {
    alpha_scaled_android_argb_color(color, 38)
}

fn alpha_scaled_android_argb_color(color: u32, percent: u16) -> Color {
    let alpha = ((color >> 24) & 0xff) as u16;
    let scaled_alpha = (alpha * percent / 100).max(1) as u8;

    Color::from_argb(
        scaled_alpha,
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    )
}

fn divider_android_argb_color(color: u32) -> Color {
    let red = ((color >> 16) & 0xff) as u8;
    let green = ((color >> 8) & 0xff) as u8;
    let blue = (color & 0xff) as u8;
    let luminance = (u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114) / 1000;
    if luminance < 128 {
        return Color::from_argb(72, 255, 255, 255);
    }

    Color::from_argb(42, 0, 0, 0)
}

fn list_item_text_left(rect: &Rect, image: Option<&ActivityImage>) -> f32 {
    if image.is_some() {
        return rect.left() + 54.0;
    }

    rect.left() + 14.0
}

fn list_item_text_right(rect: &Rect, accessory: Option<ActivityTextAccessory>) -> f32 {
    match accessory {
        Some(ActivityTextAccessory::Switch { .. }) => rect.right() - 76.0,
        Some(ActivityTextAccessory::ListValue) => rect.right() - 42.0,
        None => rect.right() - 14.0,
    }
    .max(rect.left() + 24.0)
}

fn draw_list_item_image(
    canvas: &skia_safe::Canvas,
    rect: &Rect,
    image: Option<&ActivityImage>,
    fallback_tint: Option<u32>,
) -> Result<()> {
    let Some(image) = image else {
        return Ok(());
    };

    let icon_size = 28.0_f32.min(rect.height().max(0.0));
    let icon_frame = Rect::from_xywh(
        rect.left() + 14.0,
        rect.top() + (rect.height() - icon_size) / 2.0,
        icon_size,
        icon_size,
    );

    draw_button_image_with_fallback_tint(canvas, &icon_frame, image, fallback_tint)
}

fn draw_list_item_accessory(
    canvas: &skia_safe::Canvas,
    rect: &Rect,
    accessory: Option<ActivityTextAccessory>,
) {
    match accessory {
        Some(ActivityTextAccessory::Switch { checked }) => {
            draw_switch_accessory(canvas, rect, checked)
        }
        Some(ActivityTextAccessory::ListValue) => draw_chevron_accessory(canvas, rect),
        None => {}
    }
}

fn draw_switch_accessory(canvas: &skia_safe::Canvas, rect: &Rect, checked: bool) {
    let track_width = 42.0;
    let track_height = 24.0;
    let track = Rect::from_xywh(
        rect.right() - track_width - 16.0,
        rect.top() + (rect.height() - track_height) / 2.0,
        track_width,
        track_height,
    );
    let track_paint = fill_paint(if checked {
        SWITCH_TRACK_ON
    } else {
        SWITCH_TRACK_OFF
    });

    canvas.draw_round_rect(track, track_height / 2.0, track_height / 2.0, &track_paint);

    let thumb_size = 20.0;
    let thumb_x = if checked {
        track.right() - thumb_size - 2.0
    } else {
        track.left() + 2.0
    };
    let thumb = Rect::from_xywh(
        thumb_x,
        track.top() + (track.height() - thumb_size) / 2.0,
        thumb_size,
        thumb_size,
    );
    let thumb_paint = fill_paint(SWITCH_THUMB);

    canvas.draw_round_rect(thumb, thumb_size / 2.0, thumb_size / 2.0, &thumb_paint);
}

fn draw_chevron_accessory(canvas: &skia_safe::Canvas, rect: &Rect) {
    let center_x = rect.right() - 24.0;
    let center_y = rect.top() + rect.height() / 2.0;
    let paint = stroke_paint(LIST_VALUE_CHEVRON, 1.8);

    canvas.draw_line(
        Point::new(center_x - 4.0, center_y - 7.0),
        Point::new(center_x + 3.0, center_y),
        &paint,
    );
    canvas.draw_line(
        Point::new(center_x + 3.0, center_y),
        Point::new(center_x - 4.0, center_y + 7.0),
        &paint,
    );
}

fn draw_button_image(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    image: &ActivityImage,
) -> Result<()> {
    if image.source == MISSING_FRAMEWORK_DRAWABLE_SOURCE {
        draw_missing_framework_drawable_image(canvas, frame, image.tint_color);
        return Ok(());
    }

    if let Some(skia_image) = Image::from_encoded(Data::new_copy(&image.data)) {
        draw_raster_button_image(canvas, frame, skia_image);
        return Ok(());
    }

    let drawable = parse_android_xml_drawable(image).map_err(|error| {
        WinedroidError::with_source(
            format!(
                "failed to decode drawable image `{}` as a raster image or Android XML drawable",
                image.source
            ),
            error,
        )
    })?;

    match drawable {
        AndroidXmlDrawable::Vector(vector) => draw_android_vector_drawable(canvas, frame, &vector),
        AndroidXmlDrawable::Shape(shape) => {
            draw_android_shape_drawable(canvas, frame, &shape);
            Ok(())
        }
    }
}

fn draw_button_image_with_fallback_tint(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    image: &ActivityImage,
    fallback_tint: Option<u32>,
) -> Result<()> {
    let tint_color = activity_image_tint_color(image, fallback_tint);
    if tint_color == image.tint_color {
        return draw_button_image(canvas, frame, image);
    }

    let mut image = image.clone();
    image.tint_color = tint_color;
    draw_button_image(canvas, frame, &image)
}

fn activity_image_tint_color(image: &ActivityImage, fallback_tint: Option<u32>) -> Option<u32> {
    image.tint_color.or(fallback_tint)
}

fn draw_raster_button_image(canvas: &skia_safe::Canvas, frame: &Rect, skia_image: Image) {
    let src = Rect::from_xywh(
        0.0,
        0.0,
        skia_image.width() as f32,
        skia_image.height() as f32,
    );
    let dst = centered_image_rect(frame, skia_image.width() as f32, skia_image.height() as f32);
    let paint = fill_paint(Color::WHITE);

    canvas.draw_image_rect(
        skia_image,
        Some((&src, canvas::SrcRectConstraint::Strict)),
        dst,
        &paint,
    );
}

const MISSING_FRAMEWORK_DRAWABLE_PATH: &str = "M10.3818 1.47367C11.4397 1.17531 12.5603 1.17534 13.6182 1.47367C14.4165 1.69888 15.1182 2.14438 15.8799 2.78226C16.6255 3.40671 17.4804 4.26163 18.5713 5.35257L18.6475 5.42874C19.7384 6.5197 20.5933 7.37457 21.2178 8.12015C21.8557 8.88181 22.3011 9.5835 22.5264 10.3819C22.8247 11.4398 22.8247 12.5603 22.5264 13.6182C22.3011 14.4166 21.8557 15.1182 21.2178 15.8799C20.5933 16.6255 19.7385 17.4803 18.6475 18.5713L18.5713 18.6475C17.4803 19.7385 16.6255 20.5933 15.8799 21.2178C15.1182 21.8558 14.4166 22.3012 13.6182 22.5264C12.5602 22.8247 11.4398 22.8248 10.3818 22.5264C9.58347 22.3012 8.88178 21.8557 8.12012 21.2178C7.37454 20.5933 6.51967 19.7385 5.42871 18.6475L5.35254 18.5713C4.26159 17.4804 3.40668 16.6255 2.78223 15.8799C2.14435 15.1183 1.69885 14.4166 1.47363 13.6182C1.17531 12.5603 1.17528 11.4398 1.47363 10.3819C1.69887 9.58347 2.1443 8.88184 2.78223 8.12015C3.40671 7.37455 4.26153 6.51974 5.35254 5.42874L5.42871 5.35257C6.51971 4.26156 7.37452 3.40674 8.12012 2.78226C8.88181 2.14433 9.58344 1.6989 10.3818 1.47367ZM12 14.75C11.4477 14.75 11 15.1978 11 15.75C11 16.3023 11.4477 16.75 12 16.75C12.5523 16.75 13 16.3023 13 15.75C13 15.1977 12.5523 14.75 12 14.75ZM12 7.00003C11.4477 7.00003 11 7.44775 11 8.00003V12C11 12.5523 11.4477 13 12 13C12.5523 13 13 12.5523 13 12V8.00003C13 7.44775 12.5523 7.00003 12 7.00003Z";
const MISSING_FRAMEWORK_DRAWABLE_FALLBACK_COLOR: Color = Color::new(0xff9e_9e9e);

fn draw_missing_framework_drawable_image(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    tint_color: Option<u32>,
) {
    let Some(mut path) = Path::from_svg(MISSING_FRAMEWORK_DRAWABLE_PATH) else {
        return;
    };
    path.set_fill_type(skia_safe::PathFillType::EvenOdd);

    let dst = centered_image_rect(frame, 24.0, 24.0);
    let save_count = canvas.save();
    canvas.translate((dst.left(), dst.top()));
    canvas.scale((dst.width() / 24.0, dst.height() / 24.0));

    let color = tint_color
        .map(Color::new)
        .unwrap_or(MISSING_FRAMEWORK_DRAWABLE_FALLBACK_COLOR);
    canvas.draw_path(&path, &fill_paint(color));

    canvas.restore_to_count(save_count);
}

fn draw_android_vector_drawable(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    vector: &AndroidVectorDrawable,
) -> Result<()> {
    let dst = centered_image_rect(frame, vector.viewport_width, vector.viewport_height);
    let save_count = canvas.save();

    canvas.translate((dst.left(), dst.top()));
    canvas.scale((
        dst.width() / vector.viewport_width,
        dst.height() / vector.viewport_height,
    ));

    for path in &vector.paths {
        draw_android_vector_path(canvas, path)?;
    }

    canvas.restore_to_count(save_count);

    Ok(())
}

fn draw_android_vector_path(
    canvas: &skia_safe::Canvas,
    vector_path: &AndroidVectorPath,
) -> Result<()> {
    let path = Path::from_svg(&vector_path.data).ok_or_else(|| {
        WinedroidError::new(format!(
            "failed to parse Android vector pathData `{}`",
            vector_path.data
        ))
    })?;
    let save_count = canvas.save();

    for group in &vector_path.groups {
        canvas.translate((group.translate_x, group.translate_y));
        canvas.scale((group.scale_x, group.scale_y));
    }

    let paint = fill_paint(vector_path.fill_color);
    canvas.draw_path(&path, &paint);
    canvas.restore_to_count(save_count);

    Ok(())
}

fn draw_android_shape_drawable(
    canvas: &skia_safe::Canvas,
    frame: &Rect,
    shape: &AndroidShapeDrawable,
) {
    let paint = fill_paint(shape.solid_color);

    canvas.draw_rect(*frame, &paint);
}

fn centered_image_rect(frame: &Rect, image_width: f32, image_height: f32) -> Rect {
    let padding = frame.width().min(frame.height()) * 0.12;
    let max_width = (frame.width() - padding * 2.0).max(1.0);
    let max_height = (frame.height() - padding * 2.0).max(1.0);
    let scale = (max_width / image_width).min(max_height / image_height);
    let width = image_width * scale;
    let height = image_height * scale;
    let x = frame.left() + (frame.width() - width) / 2.0;
    let y = frame.top() + (frame.height() - height) / 2.0;

    Rect::from_xywh(x, y, width, height)
}

#[derive(Debug, Clone, PartialEq)]
struct AndroidVectorDrawable {
    viewport_width: f32,
    viewport_height: f32,
    paths: Vec<AndroidVectorPath>,
}

#[derive(Debug, Clone, PartialEq)]
enum AndroidXmlDrawable {
    Vector(AndroidVectorDrawable),
    Shape(AndroidShapeDrawable),
}

#[derive(Debug, Clone, PartialEq)]
struct AndroidVectorPath {
    data: String,
    fill_color: Color,
    groups: Vec<AndroidVectorGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AndroidVectorGroup {
    scale_x: f32,
    scale_y: f32,
    translate_x: f32,
    translate_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AndroidShapeDrawable {
    solid_color: Color,
}

fn parse_android_xml_drawable(image: &ActivityImage) -> Result<AndroidXmlDrawable> {
    let events = binary_xml::parse_binary_xml(&image.data).map_err(|error| {
        WinedroidError::with_source(
            format!("failed to parse `{}` as Android binary XML", image.source),
            error,
        )
    })?;
    let root = android_xml_drawable_root(&events)?;

    match root {
        "vector" => parse_android_vector_drawable(image, &events).map(AndroidXmlDrawable::Vector),
        "shape" => parse_android_shape_drawable(image, &events).map(AndroidXmlDrawable::Shape),
        other => Err(WinedroidError::new(format!(
            "unsupported Android XML drawable root `{other}`"
        ))),
    }
}

fn android_xml_drawable_root(events: &[XmlEvent]) -> Result<&str> {
    events
        .iter()
        .find_map(|event| match event {
            XmlEvent::Start(element) => Some(element.name.as_str()),
            _ => None,
        })
        .ok_or_else(|| WinedroidError::new("Android XML drawable has no root element"))
}

fn parse_android_vector_drawable(
    image: &ActivityImage,
    events: &[XmlEvent],
) -> Result<AndroidVectorDrawable> {
    let mut viewport_width = None;
    let mut viewport_height = None;
    let mut tint = None;
    let layout_tint = image.tint_color.map(Color::new);
    let mut groups = Vec::new();
    let mut paths = Vec::new();
    let mut root_seen = false;

    for event in events {
        match event {
            XmlEvent::Start(element) if element.name == "vector" => {
                root_seen = true;
                viewport_width = Some(required_vector_float(&element, "viewportWidth", image)?);
                viewport_height = Some(required_vector_float(&element, "viewportHeight", image)?);
                tint = layout_tint
                    .map(Some)
                    .unwrap_or(optional_vector_color(&element, "tint", image)?);
            }
            XmlEvent::Start(element) if element.name == "group" => {
                groups.push(parse_android_vector_group(&element, image)?);
            }
            XmlEvent::End(element) if element.name == "group" => {
                groups.pop().ok_or_else(|| {
                    WinedroidError::new("Android vector drawable closed an unopened group")
                })?;
            }
            XmlEvent::Start(element) if element.name == "path" => {
                paths.push(parse_android_vector_path(
                    &element,
                    &groups,
                    image,
                    layout_tint.or(tint),
                )?);
            }
            _ => {}
        }
    }

    if !root_seen {
        return Err(WinedroidError::new(
            "Android XML drawable root is not a vector element",
        ));
    }

    let viewport_width = viewport_width.unwrap_or(0.0);
    let viewport_height = viewport_height.unwrap_or(0.0);
    if viewport_width <= 0.0 || viewport_height <= 0.0 {
        return Err(WinedroidError::new(format!(
            "Android vector drawable has invalid viewport {viewport_width}x{viewport_height}"
        )));
    }

    Ok(AndroidVectorDrawable {
        viewport_width,
        viewport_height,
        paths,
    })
}

fn parse_android_shape_drawable(
    image: &ActivityImage,
    events: &[XmlEvent],
) -> Result<AndroidShapeDrawable> {
    let mut root_seen = false;
    let mut solid_color = None;

    for event in events {
        match event {
            XmlEvent::Start(element) if element.name == "shape" => {
                root_seen = true;
            }
            XmlEvent::Start(element) if element.name == "solid" => {
                solid_color = optional_vector_color(element, "color", image)?;
            }
            _ => {}
        }
    }

    if !root_seen {
        return Err(WinedroidError::new(
            "Android XML drawable root is not a shape element",
        ));
    }

    let Some(solid_color) = solid_color else {
        return Err(WinedroidError::new(
            "Android shape drawable without solid color is not implemented",
        ));
    };

    Ok(AndroidShapeDrawable { solid_color })
}

fn parse_android_vector_group(
    element: &XmlStartElement,
    image: &ActivityImage,
) -> Result<AndroidVectorGroup> {
    Ok(AndroidVectorGroup {
        scale_x: optional_vector_float(element, "scaleX", image)?.unwrap_or(1.0),
        scale_y: optional_vector_float(element, "scaleY", image)?.unwrap_or(1.0),
        translate_x: optional_vector_float(element, "translateX", image)?.unwrap_or(0.0),
        translate_y: optional_vector_float(element, "translateY", image)?.unwrap_or(0.0),
    })
}

fn parse_android_vector_path(
    element: &XmlStartElement,
    groups: &[AndroidVectorGroup],
    image: &ActivityImage,
    tint: Option<Color>,
) -> Result<AndroidVectorPath> {
    let data = required_vector_string(element, "pathData")?;
    let fill_color = optional_vector_color(element, "fillColor", image)?.unwrap_or(Color::BLACK);
    let fill_alpha = optional_vector_float(element, "fillAlpha", image)?.unwrap_or(1.0);

    Ok(AndroidVectorPath {
        data,
        fill_color: color_with_alpha(tint.unwrap_or(fill_color), fill_alpha),
        groups: groups.to_vec(),
    })
}

fn required_vector_float(
    element: &XmlStartElement,
    name: &str,
    image: &ActivityImage,
) -> Result<f32> {
    optional_vector_float(element, name, image)?.ok_or_else(|| {
        WinedroidError::new(format!(
            "Android vector `{}` element is missing required `{name}` attribute",
            element.name
        ))
    })
}

fn optional_vector_float(
    element: &XmlStartElement,
    name: &str,
    image: &ActivityImage,
) -> Result<Option<f32>> {
    let Some(attribute) = element.attribute(name) else {
        return Ok(None);
    };

    match &attribute.value {
        XmlValue::Float(value) => Ok(Some(f32::from_bits(*value))),
        XmlValue::Integer(value) => Ok(Some(*value as f32)),
        XmlValue::Hex(value) => Ok(Some(*value as f32)),
        XmlValue::Dimension(value) | XmlValue::Fraction(value) => {
            Ok(Some(complex_to_float(*value)))
        }
        XmlValue::Reference(resource_id) => {
            resolve_vector_float_reference(image, *resource_id).map(Some)
        }
        XmlValue::String(value) => value.parse::<f32>().map(Some).map_err(|error| {
            WinedroidError::with_source(
                format!("Android vector attribute `{name}` is not a float"),
                error,
            )
        }),
        value => Err(WinedroidError::new(format!(
            "Android vector attribute `{name}` has unsupported float value `{value}`"
        ))),
    }
}

fn required_vector_string(element: &XmlStartElement, name: &str) -> Result<String> {
    let Some(attribute) = element.attribute(name) else {
        return Err(WinedroidError::new(format!(
            "Android vector `{}` element is missing required `{name}` attribute",
            element.name
        )));
    };

    match &attribute.value {
        XmlValue::String(value) => Ok(value.clone()),
        value => Err(WinedroidError::new(format!(
            "Android vector attribute `{name}` has unsupported string value `{value}`"
        ))),
    }
}

fn optional_vector_color(
    element: &XmlStartElement,
    name: &str,
    image: &ActivityImage,
) -> Result<Option<Color>> {
    let Some(attribute) = element.attribute(name) else {
        return Ok(None);
    };

    match &attribute.value {
        XmlValue::Color(value) | XmlValue::Hex(value) => Ok(Some(Color::new(*value))),
        XmlValue::Integer(value) => Ok(Some(Color::new(*value as u32))),
        XmlValue::Reference(resource_id) => {
            resolve_vector_color_reference(image, *resource_id).map(Some)
        }
        XmlValue::AttributeReference(attribute_id) => {
            resolve_vector_theme_color_attribute(image, *attribute_id).map(Some)
        }
        XmlValue::String(value) => parse_vector_color_string(value).map(Some),
        value => Err(WinedroidError::new(format!(
            "Android vector attribute `{name}` has unsupported color value `{value}`"
        ))),
    }
}

fn resolve_vector_float_reference(image: &ActivityImage, resource_id: u32) -> Result<f32> {
    let value = resolve_vector_resource_reference(image, resource_id, "float")?;

    match value {
        ResourcePrimitive::Float(value) => Ok(f32::from_bits(value)),
        ResourcePrimitive::Dimension(value) | ResourcePrimitive::Fraction(value) => {
            Ok(complex_to_float(value))
        }
        ResourcePrimitive::Integer(value) => Ok(value as f32),
        ResourcePrimitive::Hex(value) => Ok(value as f32),
        ResourcePrimitive::String(value) => value.parse::<f32>().map_err(|error| {
            WinedroidError::with_source(
                format!("Android vector float resource 0x{resource_id:08x} is not a float"),
                error,
            )
        }),
        value => Err(WinedroidError::new(format!(
            "Android vector float resource 0x{resource_id:08x} has unsupported value {value:?}"
        ))),
    }
}

fn resolve_vector_color_reference(image: &ActivityImage, resource_id: u32) -> Result<Color> {
    let value = resolve_vector_resource_reference(image, resource_id, "color")?;

    match value {
        ResourcePrimitive::Color(value) | ResourcePrimitive::Hex(value) => Ok(Color::new(value)),
        ResourcePrimitive::Integer(value) => Ok(Color::new(value as u32)),
        ResourcePrimitive::String(value) => parse_vector_color_string(&value),
        value => Err(WinedroidError::new(format!(
            "Android vector color resource 0x{resource_id:08x} has unsupported value {value:?}"
        ))),
    }
}

fn resolve_vector_resource_reference(
    image: &ActivityImage,
    resource_id: u32,
    value_kind: &str,
) -> Result<ResourcePrimitive> {
    const MAX_REFERENCE_COUNT: usize = 20;
    let mut current_id = resource_id;
    let mut reference_count = 0;
    let mut visited = BTreeSet::new();

    loop {
        if !visited.insert(current_id) {
            return Err(WinedroidError::new(format!(
                "Android vector {value_kind} resource reference cycle reached 0x{current_id:08x}"
            )));
        }

        if let Some(color) = android_framework::color_resource(current_id) {
            return Ok(ResourcePrimitive::Color(color));
        }

        // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource, ResTable::resolveReference
        let Some(resource) = resource_table::resolve_resource_value_for_config(
            &image.resources,
            current_id,
            image.theme.config,
        )?
        else {
            return Err(WinedroidError::new(format!(
                "Android vector {value_kind} resource 0x{current_id:08x} was not found"
            )));
        };

        let next_id = match &resource.value {
            ResourcePrimitive::Reference(next_id) => *next_id,
            _ => return Ok(resource.value),
        };
        if next_id == 0 {
            return Ok(resource.value);
        }
        if reference_count == MAX_REFERENCE_COUNT {
            return Err(WinedroidError::new(format!(
                "Android vector {value_kind} resource 0x{resource_id:08x} exceeded {MAX_REFERENCE_COUNT} references"
            )));
        }

        reference_count += 1;
        current_id = next_id;
    }
}

fn resolve_vector_theme_color_attribute(image: &ActivityImage, attribute_id: u32) -> Result<Color> {
    // cite: frameworks-base Resources.java#resolveAttribute
    // cite: frameworks-base VectorDrawable.java#applyTheme, canApplyTheme
    crate::layout_render::resolve_layout_theme_color_attribute(
        &image.resources,
        Some(&image.theme),
        attribute_id,
    )
    .map(Color::new)
    .map_err(|error| {
        WinedroidError::with_source(
            format!("failed to resolve Android vector theme color attribute 0x{attribute_id:08x}"),
            error,
        )
    })
}

fn parse_vector_color_string(value: &str) -> Result<Color> {
    let Some(hex) = value.strip_prefix('#') else {
        return Err(WinedroidError::new(format!(
            "Android vector color `{value}` is not a hex color"
        )));
    };
    let parsed = u32::from_str_radix(hex, 16).map_err(|error| {
        WinedroidError::with_source(format!("Android vector color `{value}` is invalid"), error)
    })?;

    match hex.len() {
        6 => Ok(Color::new(0xff00_0000 | parsed)),
        8 => Ok(Color::new(parsed)),
        _ => Err(WinedroidError::new(format!(
            "Android vector color `{value}` must be #RRGGBB or #AARRGGBB"
        ))),
    }
}

fn color_with_alpha(color: Color, alpha: f32) -> Color {
    let alpha = alpha.clamp(0.0, 1.0);
    let channel = ((color.a() as f32) * alpha).round() as u8;

    color.with_a(channel)
}

fn complex_to_float(value: u32) -> f32 {
    const MANTISSA_MASK: u32 = 0x00ff_ffff;
    const MANTISSA_SIGN_BIT: u32 = 0x0080_0000;
    const RADIX_MULTIPLIERS: [f32; 4] = [
        1.0 / (1u32 << 23) as f32,
        1.0 / (1u32 << 16) as f32,
        1.0 / (1u32 << 8) as f32,
        1.0,
    ];

    let mut mantissa = ((value >> 8) & MANTISSA_MASK) as i32;
    if mantissa & MANTISSA_SIGN_BIT as i32 != 0 {
        mantissa |= !MANTISSA_MASK as i32;
    }
    let radix = ((value >> 4) & 0x3) as usize;

    mantissa as f32 * RADIX_MULTIPLIERS[radix]
}

fn label_rect(element: &ActivityTextElement, surface_width: u32, surface_height: u32) -> Rect {
    let frame = element_rect(element, surface_height);

    if element.width <= 16 {
        return Rect::from_xywh(
            24.0,
            frame.top(),
            surface_width.saturating_sub(48) as f32,
            frame.height(),
        );
    }

    frame
}

fn element_rect(element: &ActivityTextElement, surface_height: u32) -> Rect {
    Rect::from_xywh(
        element.x as f32,
        element_top_y(element, surface_height) as f32,
        element.width as f32,
        element.height as f32,
    )
}

fn activity_rect_to_skia_rect(rect: ActivityRect, surface_height: u32) -> Rect {
    Rect::from_xywh(
        rect.x as f32,
        surface_height.saturating_sub(rect.y.saturating_add(rect.height)) as f32,
        rect.width as f32,
        rect.height as f32,
    )
}

fn element_top_y(element: &ActivityTextElement, surface_height: u32) -> i32 {
    i32::try_from(surface_height)
        .unwrap_or(i32::MAX)
        .saturating_sub(element.y)
        .saturating_sub(i32::try_from(element.height).unwrap_or(i32::MAX))
}

// cite: frameworks-base View.java#setElevation, getElevation
// cite: frameworks-base ViewOutlineProvider.java#BACKGROUND
// cite: frameworks-base ThreadedRenderer.java#setLightCenter
fn draw_elevation_shadow(
    canvas: &skia_safe::Canvas,
    surface_width: u32,
    surface_height: u32,
    frame: Rect,
    corner_radius: f32,
    elevation_px: f32,
) {
    if elevation_px <= 0.0 || frame.width() <= 0.0 || frame.height() <= 0.0 {
        return;
    }

    let mut builder = skia_safe::PathBuilder::new();
    if corner_radius > 0.0 {
        builder.add_rrect(
            RRect::new_rect_xy(frame, corner_radius, corner_radius),
            None,
            None,
        );
    } else {
        builder.add_rect(frame, None, None);
    }
    let path = builder.detach();

    // cite: frameworks-base dimens.xml#light_y, light_z, light_radius
    // cite: frameworks-base ThreadedRenderer.java#setLightCenter
    let z_ratio = surface_width.min(surface_height) as f32 / 450.0;
    let z_weighted_adjustment = (z_ratio + 2.0) / 3.0;
    let device_scale = canvas.local_to_device_as_3x3().scale_x();
    let light_pos = Point3::new(
        surface_width as f32 / 2.0 * device_scale,
        SHADOW_LIGHT_Y * device_scale,
        SHADOW_LIGHT_Z * z_weighted_adjustment * device_scale,
    );
    let z_plane_params = Point3::new(0.0, 0.0, elevation_px * device_scale);
    // cite: frameworks-base dimens.xml#ambient_shadow_alpha, spot_shadow_alpha
    let ambient_color = Color::from_argb((255.0 * SHADOW_AMBIENT_ALPHA).round() as u8, 0, 0, 0);
    let spot_color = Color::from_argb((255.0 * SHADOW_SPOT_ALPHA).round() as u8, 0, 0, 0);

    canvas.draw_shadow(
        &path,
        z_plane_params,
        light_pos,
        SHADOW_LIGHT_RADIUS * device_scale,
        ambient_color,
        spot_color,
        None,
    );
}

fn fill_paint(color: Color) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(color);
    paint.set_style(paint::Style::Fill);
    paint
}

fn stroke_paint(color: Color, width: f32) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(color);
    paint.set_style(paint::Style::Stroke);
    paint.set_stroke_width(width);
    paint
}

// cite: frameworks-base TextView.java#onMeasure
// cite: frameworks-base Layout.java#getDesiredWidthWithLimit
// cite: frameworks-base BoringLayout.java#isBoring
pub fn measured_label_text_width(text: &str, text_size: f32) -> u32 {
    let font = ui_font(text_size, false);
    clean_render_text_lines(text)
        .iter()
        .map(|line| font.measure_str(line, None).0)
        .fold(0.0_f32, f32::max)
        .ceil() as u32
}

fn ui_font(size: f32, strong: bool) -> Font {
    let typeface = if strong {
        matched_bold_typeface()
    } else {
        matched_regular_typeface()
    };
    let mut font = typeface
        .map(|typeface| Font::new(typeface, size))
        .unwrap_or_else(Font::default);
    font.set_size(size);
    font.set_subpixel(true);
    font.set_embolden(strong && matched_bold_typeface().is_none());
    font
}

fn matched_regular_typeface() -> Option<Typeface> {
    static TYPEFACE: OnceLock<Option<Typeface>> = OnceLock::new();
    #[cfg(target_os = "emscripten")]
    return TYPEFACE
        .get_or_init(|| {
            // cite: frameworks-base Typeface.java#preloadFontFile
            FontMgr::new().new_from_data(include_bytes!("../assets/fonts/Roboto-Regular.ttf"), None)
        })
        .clone();
    #[cfg(not(target_os = "emscripten"))]
    TYPEFACE
        .get_or_init(|| match_system_typeface(FontStyle::normal()))
        .clone()
}

fn matched_bold_typeface() -> Option<Typeface> {
    static TYPEFACE: OnceLock<Option<Typeface>> = OnceLock::new();
    #[cfg(target_os = "emscripten")]
    return TYPEFACE
        .get_or_init(|| {
            // cite: frameworks-base Typeface.java#DEFAULT_BOLD
            FontMgr::new().new_from_data(include_bytes!("../assets/fonts/Roboto-Bold.ttf"), None)
        })
        .clone();
    #[cfg(not(target_os = "emscripten"))]
    TYPEFACE
        .get_or_init(|| match_system_typeface(FontStyle::bold()))
        .clone()
}

#[cfg(not(target_os = "emscripten"))]
fn match_system_typeface(style: FontStyle) -> Option<Typeface> {
    let font_mgr = FontMgr::default();

    for family in [
        ".AppleSystemUIFont",
        "SF Pro Text",
        "Helvetica Neue",
        "Helvetica",
        "Arial",
        "DejaVu Sans",
        "Noto Sans",
    ] {
        if let Some(typeface) = font_mgr.match_family_style(family, style) {
            return Some(typeface);
        }
    }

    None
}

fn button_font_size(text: &str) -> f32 {
    match text.chars().count() {
        0..=1 => 24.0,
        2 => 21.0,
        3..=5 => 18.0,
        _ => 15.0,
    }
}

pub fn measure_label_text_height(text: &str, width: u32, font_size: f32) -> u32 {
    if text.trim().is_empty() || width == 0 {
        return 0;
    }

    let font = ui_font(font_size, false);
    let paint = fill_paint(android_argb_color(0xff00_0000));
    let wrap_width = width as f32 - LABEL_TEXT_INSET;
    let lines = wrap_render_lines(&clean_render_text_lines(text), &font, &paint, wrap_width);
    let (_, metrics) = font.metrics();
    let line_height = (metrics.descent - metrics.ascent + metrics.leading).max(font_size * 1.3);

    (lines.len() as f32 * line_height).ceil() as u32
}

fn clean_render_text_lines(text: &str) -> Vec<String> {
    text.split('\n').map(clean_render_text).collect::<Vec<_>>()
}

fn wrap_render_lines(
    lines: &[String],
    font: &skia_safe::Font,
    paint: &Paint,
    max_width: f32,
) -> Vec<String> {
    if max_width <= 0.0 {
        return lines.to_vec();
    }

    let mut wrapped = Vec::new();
    for line in lines {
        if line.is_empty() {
            wrapped.push(String::new());
            continue;
        }

        let mut current = String::new();
        for word in line.split(' ') {
            let candidate = if current.is_empty() {
                word.to_owned()
            } else {
                format!("{current} {word}")
            };
            if font.measure_str(&candidate, Some(paint)).0 <= max_width || current.is_empty() {
                current = candidate;
            } else {
                wrapped.push(std::mem::take(&mut current));
                current = word.to_owned();
            }
        }
        wrapped.push(current);
    }

    wrapped
}

fn clean_render_text(text: &str) -> String {
    text.chars()
        .filter(|character| !matches!(character, '\u{200e}' | '\u{200f}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn list_item_text(text: &str) -> (String, Option<String>) {
    let lines = text
        .lines()
        .map(clean_render_text)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let title = lines.first().cloned().unwrap_or_default();
    let summary = lines.get(1).cloned();

    (title, summary)
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_display_mask_blacks_out_corners_and_keeps_center() {
        let mut image = super::SkiaSurfaceImage {
            width: 20,
            height: 20,
            png: Vec::new(),
            pixels: vec![0x00ff_ffff; 400],
        };
        super::mask_round_display(&mut image).expect("pixel mask should apply");

        assert_eq!(image.pixels[0], 0);
        assert_eq!(image.pixels[19], 0);
        assert_eq!(image.pixels[399], 0);
        assert_eq!(image.pixels[10 * 20 + 10], 0x00ff_ffff);
        assert_eq!(image.pixels[3 * 20 + 10], 0x00ff_ffff);
    }

    #[test]
    fn round_display_mask_reencodes_png_frames() {
        let mut image = super::render_activity_surface(
            40,
            40,
            "",
            &[],
            &[],
            &[],
            &[],
            None,
            None,
            Some(0xffff_ffff),
        )
        .expect("surface should render");
        super::mask_round_display(&mut image).expect("png mask should apply");
        let decoded = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&image.png))
            .expect("masked png should decode");

        assert_eq!((decoded.width(), decoded.height()), (40, 40));
        let info = skia_safe::ImageInfo::new(
            (40, 40),
            skia_safe::ColorType::BGRA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut bytes = vec![0u8; 40 * 40 * 4];
        assert!(decoded.read_pixels(
            &info,
            &mut bytes,
            40 * 4,
            (0, 0),
            skia_safe::image::CachingHint::Allow
        ));
        assert_eq!(&bytes[0..3], &[0, 0, 0]);
        let center = (20 * 40 + 20) * 4;
        assert_eq!(&bytes[center..center + 3], &[255, 255, 255]);
    }

    use super::*;
    use crate::binary_xml::XmlAttribute;

    fn configured_value_resource_table(
        type_name: &str,
        key_name: &str,
        global_values: &[&str],
        default_value_type: u8,
        default_value: u32,
        configured_value_type: u8,
        configured_value: u32,
    ) -> Vec<u8> {
        configured_entries_resource_table(
            type_name,
            &[key_name],
            global_values,
            &[(default_value_type, default_value)],
            &[(configured_value_type, configured_value)],
        )
    }

    fn configured_entries_resource_table(
        type_name: &str,
        key_names: &[&str],
        global_values: &[&str],
        default_entries: &[(u8, u32)],
        configured_entries: &[(u8, u32)],
    ) -> Vec<u8> {
        assert_eq!(key_names.len(), default_entries.len());
        assert_eq!(key_names.len(), configured_entries.len());
        let global_strings = resource_string_pool(global_values);
        let type_strings = resource_string_pool(&[type_name]);
        let key_strings = resource_string_pool(key_names);
        let type_chunks = [
            resource_type_chunk(default_entries, Vec::new()),
            resource_type_chunk(configured_entries, landscape_resource_config()),
        ];
        let type_strings_offset = 288u32;
        let key_strings_offset = type_strings_offset + type_strings.len() as u32;
        let type_chunks_size = type_chunks.iter().map(Vec::len).sum::<usize>();
        let package_size = 288 + type_strings.len() + key_strings.len() + type_chunks_size;
        let mut package = Vec::new();

        push_resource_u16(&mut package, 0x0200);
        push_resource_u16(&mut package, 288);
        push_resource_u32(&mut package, package_size as u32);
        push_resource_u32(&mut package, 0x7f);
        package.extend(vec![0; 256]);
        push_resource_u32(&mut package, type_strings_offset);
        push_resource_u32(&mut package, 0);
        push_resource_u32(&mut package, key_strings_offset);
        push_resource_u32(&mut package, 0);
        push_resource_u32(&mut package, 0);
        package.extend(type_strings);
        package.extend(key_strings);
        for type_chunk in type_chunks {
            package.extend(type_chunk);
        }

        let table_size = 12 + global_strings.len() + package.len();
        let mut table = Vec::new();
        push_resource_u16(&mut table, 0x0002);
        push_resource_u16(&mut table, 12);
        push_resource_u32(&mut table, table_size as u32);
        push_resource_u32(&mut table, 1);
        table.extend(global_strings);
        table.extend(package);
        table
    }

    fn resource_type_chunk(entries: &[(u8, u32)], config: Vec<u8>) -> Vec<u8> {
        let header_size = 20 + config.len();
        let entries_start = header_size + entries.len() * 4;
        let chunk_size = entries_start + entries.len() * 16;
        let mut chunk = Vec::new();
        push_resource_u16(&mut chunk, 0x0201);
        push_resource_u16(&mut chunk, header_size as u16);
        push_resource_u32(&mut chunk, chunk_size as u32);
        chunk.push(0x01);
        chunk.push(0);
        push_resource_u16(&mut chunk, 0);
        push_resource_u32(&mut chunk, entries.len() as u32);
        push_resource_u32(&mut chunk, entries_start as u32);
        chunk.extend(config);
        for index in 0..entries.len() {
            push_resource_u32(&mut chunk, (index * 16) as u32);
        }
        for (key_index, (value_type, value)) in entries.iter().enumerate() {
            push_resource_u16(&mut chunk, 8);
            push_resource_u16(&mut chunk, 0);
            push_resource_u32(&mut chunk, key_index as u32);
            push_resource_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(*value_type);
            push_resource_u32(&mut chunk, *value);
        }
        chunk
    }

    fn landscape_resource_config() -> Vec<u8> {
        let mut config = vec![0; 36];
        write_resource_u32(&mut config, 0, 36);
        config[12] = 2;
        write_resource_u16(&mut config, 32, 1000);
        write_resource_u16(&mut config, 34, 600);
        config
    }

    fn resource_string_pool(values: &[&str]) -> Vec<u8> {
        let mut string_data = Vec::new();
        let mut offsets = Vec::new();
        for value in values {
            offsets.push(string_data.len() as u32);
            string_data.push(value.len() as u8);
            string_data.push(value.len() as u8);
            string_data.extend(value.as_bytes());
            string_data.push(0);
        }

        let header_size = 28usize;
        let strings_start = header_size + values.len() * 4;
        let chunk_size = strings_start + string_data.len();
        let mut pool = Vec::new();
        push_resource_u16(&mut pool, 0x0001);
        push_resource_u16(&mut pool, header_size as u16);
        push_resource_u32(&mut pool, chunk_size as u32);
        push_resource_u32(&mut pool, values.len() as u32);
        push_resource_u32(&mut pool, 0);
        push_resource_u32(&mut pool, 0x0000_0100);
        push_resource_u32(&mut pool, strings_start as u32);
        push_resource_u32(&mut pool, 0);
        for offset in offsets {
            push_resource_u32(&mut pool, offset);
        }
        pool.extend(string_data);
        pool
    }

    fn push_resource_u16(data: &mut Vec<u8>, value: u16) {
        data.extend(value.to_le_bytes());
    }

    fn push_resource_u32(data: &mut Vec<u8>, value: u32) {
        data.extend(value.to_le_bytes());
    }

    fn write_resource_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_resource_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn surface_pixel(surface: &mut skia_safe::Surface, x: i32, y: i32) -> [u8; 4] {
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut pixel = [0u8; 4];
        assert!(surface.read_pixels(&info, &mut pixel, 4, (x, y)));
        pixel
    }

    fn encoded_single_pixel_png(color: Color) -> Vec<u8> {
        let mut surface = surfaces::raster_n32_premul((1, 1)).expect("surface");
        surface.canvas().clear(color);
        surface
            .image_snapshot()
            .encode(None, EncodedImageFormat::PNG, 100)
            .expect("PNG should encode")
            .as_bytes()
            .to_vec()
    }

    fn runtime_bitmap_op_for_alpha_mask_glyph() -> ActivityRuntimeCanvasOp {
        // cite: androidx DrawCache.kt#clear, drawCachedImage
        // cite: androidx Vector.kt#draw
        ActivityRuntimeCanvasOp::Bitmap {
            left: 2.0,
            top: 2.0,
            right: 8.0,
            bottom: 8.0,
            source_left: 0.0,
            source_top: 0.0,
            source_right: 6.0,
            source_bottom: 6.0,
            bitmap_width: 6,
            bitmap_height: 6,
            color: 0xffff_ffff,
            blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
            color_filter: Some(crate::host_window::ActivityRuntimeCanvasColorFilter {
                color: 0xffff_0000,
                blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcIn,
            }),
            alpha_mask: true,
            encoded_image: None,
            ops: vec![
                ActivityRuntimeCanvasOp::Rect {
                    left: 0.0,
                    top: 0.0,
                    right: 6.0,
                    bottom: 6.0,
                    color: 0xff00_0000,
                    blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::Clear,
                    paint_style: crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill,
                },
                ActivityRuntimeCanvasOp::Rect {
                    left: 2.0,
                    top: 2.0,
                    right: 4.0,
                    bottom: 4.0,
                    color: 0xffff_ffff,
                    blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
                    paint_style: crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill,
                },
            ],
        }
    }

    #[test]
    fn bitmap_replay_clear_blend_does_not_erase_destination_pixels() {
        let mut surface = surfaces::raster_n32_premul((10, 10)).expect("surface");
        surface.canvas().clear(Color::from_argb(255, 0, 255, 0));

        let op = runtime_bitmap_op_for_alpha_mask_glyph();
        draw_runtime_canvas_ops(surface.canvas(), std::slice::from_ref(&op));

        // cite: frameworks-base Canvas.java#Canvas
        assert_eq!(surface_pixel(&mut surface, 0, 0), [0, 255, 0, 255]);
        assert_eq!(surface_pixel(&mut surface, 2, 2), [0, 255, 0, 255]);
    }

    #[test]
    fn bitmap_replay_alpha_mask_applies_src_in_color_filter_tint() {
        let mut surface = surfaces::raster_n32_premul((10, 10)).expect("surface");
        surface.canvas().clear(Color::from_argb(255, 0, 255, 0));

        let op = runtime_bitmap_op_for_alpha_mask_glyph();
        draw_runtime_canvas_ops(surface.canvas(), std::slice::from_ref(&op));

        // cite: frameworks-base Bitmap.java#ALPHA_8
        // cite: frameworks-base BlendModeColorFilter.java#BlendModeColorFilter
        assert_eq!(surface_pixel(&mut surface, 5, 5), [255, 0, 0, 255]);
    }

    #[test]
    fn bitmap_replay_draws_bitmapfactory_encoded_resource_pixels() {
        let mut surface = surfaces::raster_n32_premul((8, 8)).expect("surface");
        surface.canvas().clear(Color::TRANSPARENT);
        let encoded = encoded_single_pixel_png(Color::from_argb(255, 255, 0, 0));
        let op = ActivityRuntimeCanvasOp::Bitmap {
            left: 2.0,
            top: 2.0,
            right: 6.0,
            bottom: 6.0,
            source_left: 0.0,
            source_top: 0.0,
            source_right: 1.0,
            source_bottom: 1.0,
            bitmap_width: 1,
            bitmap_height: 1,
            color: 0xffff_ffff,
            blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
            color_filter: None,
            alpha_mask: false,
            encoded_image: Some(encoded),
            ops: Vec::new(),
        };

        draw_runtime_canvas_ops(surface.canvas(), &[op]);

        // cite: frameworks-base BitmapFactory.java#decodeResource, decodeResourceStream
        // cite: frameworks-base BaseCanvas.java#drawBitmap
        assert_eq!(surface_pixel(&mut surface, 4, 4), [255, 0, 0, 255]);
    }

    #[test]
    fn clean_render_text_lines_preserves_explicit_breaks() {
        let lines = clean_render_text_lines("one\n  two   words \nthree");

        assert_eq!(lines, vec!["one", "two words", "three"]);
    }

    #[test]
    fn wrap_render_lines_wraps_long_lines_and_keeps_breaks() {
        let font = ui_font(16.0, false);
        let paint = fill_paint(android_argb_color(0xff00_0000));
        let source = vec![
            "the quick brown fox jumps over the lazy dog".to_owned(),
            String::new(),
            "short".to_owned(),
        ];

        let wrapped = wrap_render_lines(&source, &font, &paint, 80.0);

        assert!(wrapped.len() > 3);
        assert!(wrapped.iter().any(|line| line.is_empty()));
        assert_eq!(wrapped.last().map(String::as_str), Some("short"));
        for line in &wrapped {
            if line.contains(' ') {
                assert!(font.measure_str(line, Some(&paint)).0 <= 80.0);
            }
        }
    }

    #[test]
    fn identifies_background_only_elements() {
        let mut element = ActivityTextElement {
            text: String::new(),
            kind: ActivityTextKind::Label,
            view_id: None,
            preference_key: None,
            accessory: None,
            image: None,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: Some(0xff12_3456),
            alignment: ActivityTextAlignment::Start,
            editable: false,
            cursor: None,
            x: 0,
            y: 0,
            width: 100,
            height: 40,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert!(is_background_only_element(&element));

        element.text = "Title".to_owned();
        assert!(!is_background_only_element(&element));
    }

    #[test]
    fn surface_background_uses_largest_projected_background_color() {
        let elements = vec![
            ActivityTextElement {
                text: String::new(),
                kind: ActivityTextKind::Label,
                view_id: None,
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: Some(0xff22_2222),
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 0,
                y: 0,
                width: 40,
                height: 40,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
            ActivityTextElement {
                text: String::new(),
                kind: ActivityTextKind::Label,
                view_id: None,
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: Some(0xff15_1b2c),
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 0,
                y: 0,
                width: 200,
                height: 200,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
        ];

        assert_eq!(
            surface_background_color(&elements),
            Some(Color::from_argb(0xff, 0x15, 0x1b, 0x2c))
        );
    }

    #[test]
    fn surface_background_does_not_use_control_background_color() {
        let elements = vec![ActivityTextElement {
            text: "OK".to_owned(),
            kind: ActivityTextKind::Button,
            view_id: Some(1),
            preference_key: None,
            accessory: None,
            image: None,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: Some(0xff12_3456),
            alignment: ActivityTextAlignment::Center,
            editable: false,
            cursor: None,
            x: 20,
            y: 20,
            width: 80,
            height: 44,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        }];

        assert_eq!(surface_background_color(&elements), None);
    }

    #[test]
    fn list_item_colors_use_projected_element_colors() {
        let element = ActivityTextElement {
            text: "Theme\nSystem".to_owned(),
            kind: ActivityTextKind::ListItem,
            view_id: None,
            preference_key: Some("object:1".to_owned()),
            accessory: Some(ActivityTextAccessory::ListValue),
            image: None,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: Some(0xffef_efef),
            background_color: Some(0xff15_1b2c),
            alignment: ActivityTextAlignment::Start,
            editable: false,
            cursor: None,
            x: 0,
            y: 0,
            width: 200,
            height: 58,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert_eq!(
            list_item_fill_color(&element),
            Color::from_argb(0xff, 0x15, 0x1b, 0x2c)
        );
        assert_eq!(
            list_item_primary_text_color(&element),
            Color::from_argb(0xff, 0xef, 0xef, 0xef)
        );
        assert_eq!(
            list_item_secondary_text_color(&element),
            Color::from_argb(0xb2, 0xef, 0xef, 0xef)
        );
        assert_eq!(
            list_item_divider_color(&element),
            Color::from_argb(0x48, 0xff, 0xff, 0xff)
        );
    }

    #[test]
    fn list_item_colors_fall_back_to_projected_surface_theme() {
        let element = ActivityTextElement {
            text: "Donate".to_owned(),
            kind: ActivityTextKind::ListItem,
            view_id: Some(2131296277),
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
            x: 24,
            y: 132,
            width: 372,
            height: 48,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };
        let theme_colors = SurfaceThemeColors {
            background: Some(0xff15_1b2c),
            foreground: Some(0xffef_efef),
        };

        assert_eq!(
            themed_list_item_fill_color(&element, theme_colors),
            Color::from_argb(0xff, 0x15, 0x1b, 0x2c)
        );
        assert_eq!(
            themed_list_item_primary_text_color(&element, theme_colors),
            Color::from_argb(0xff, 0xef, 0xef, 0xef)
        );
        assert_eq!(
            themed_list_item_secondary_text_color(&element, theme_colors),
            Color::from_argb(0xb2, 0xef, 0xef, 0xef)
        );
        assert_eq!(
            themed_list_item_divider_color(&element, theme_colors),
            Color::from_argb(0x48, 0xff, 0xff, 0xff)
        );
    }

    #[test]
    fn image_only_button_does_not_draw_fallback_control_fill() {
        let element = ActivityTextElement {
            text: String::new(),
            kind: ActivityTextKind::Button,
            view_id: Some(2131296591),
            preference_key: None,
            accessory: None,
            image: Some(test_activity_image(None)),
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: None,
            alignment: ActivityTextAlignment::Center,
            editable: false,
            cursor: None,
            x: 368,
            y: 708,
            width: 44,
            height: 44,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert!(!should_draw_button_fallback_fill(&element, ""));
    }

    #[test]
    fn untinted_images_use_fallback_tint_without_overriding_explicit_tint() {
        let untinted = test_activity_image(None);
        let tinted = test_activity_image(Some(0xff12_3456));

        assert_eq!(
            activity_image_tint_color(&untinted, Some(0xffef_efef)),
            Some(0xffef_efef)
        );
        assert_eq!(
            activity_image_tint_color(&tinted, Some(0xffef_efef)),
            Some(0xff12_3456)
        );
    }

    #[test]
    fn popup_and_dialog_colors_use_projected_surface_theme() {
        let elements = vec![
            ActivityTextElement {
                text: String::new(),
                kind: ActivityTextKind::Label,
                view_id: None,
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: Some(0xff15_1b2c),
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 0,
                y: 0,
                width: 220,
                height: 220,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
            ActivityTextElement {
                text: "Settings".to_owned(),
                kind: ActivityTextKind::Label,
                view_id: Some(2),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: Some(0xffef_efef),
                background_color: None,
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 24,
                y: 160,
                width: 120,
                height: 42,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
        ];
        let colors = surface_theme_colors(&elements);

        assert_eq!(
            popup_fill_color(colors),
            Color::from_argb(0xff, 0x15, 0x1b, 0x2c)
        );
        assert_eq!(
            dialog_fill_color(colors),
            Color::from_argb(0xff, 0x15, 0x1b, 0x2c)
        );
        assert_eq!(
            popup_text_color(colors),
            Color::from_argb(0xff, 0xef, 0xef, 0xef)
        );
        assert_eq!(
            dialog_text_color(colors),
            Color::from_argb(0xff, 0xef, 0xef, 0xef)
        );
        assert_eq!(
            popup_divider_color(colors),
            Color::from_argb(0x48, 0xff, 0xff, 0xff)
        );
    }

    #[test]
    fn renders_png_without_softbuffer_pixels_for_activity_surface() {
        let image = render_activity_surface(
            180,
            120,
            "bootstrap",
            &[ActivityTextElement {
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
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 8,
                y: 40,
                width: 80,
                height: 44,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            &[],
            &[],
            &[],
            None,
            None,
            None,
        )
        .expect("Skia surface should render");

        assert_eq!(image.width, 180);
        assert_eq!(image.height, 120);
        assert!(image.pixels.is_empty());
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "renderer output should be a PNG"
        );
    }

    #[test]
    fn renders_display_pixels_without_png_encoding() {
        let image = render_activity_surface_pixels_scaled(
            32,
            24,
            "x",
            &[],
            &[],
            &[],
            &[],
            None,
            None,
            None,
            2.0,
        )
        .expect("display pixels should render");

        assert_eq!(image.width, 64);
        assert_eq!(image.height, 48);
        assert_eq!(image.pixels.len(), 64 * 48);
        assert!(image.png.is_empty());
    }

    #[test]
    fn renders_display_pixels_with_png_encoding_when_requested() {
        let image = render_activity_surface_pixels_and_png_scaled(
            32,
            24,
            "x",
            &[],
            &[],
            &[],
            &[],
            None,
            None,
            None,
            2.0,
        )
        .expect("display pixels and PNG should render");

        assert_eq!(image.width, 64);
        assert_eq!(image.height, 48);
        assert_eq!(image.pixels.len(), 64 * 48);
        assert!(image.png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn softbuffer_00rrggbb_channel_order_converts_opaque_skia_bgra() {
        let pixels = opaque_softbuffer_pixels(&[0x33, 0x22, 0x11, 0xff], Color::BLACK);

        assert_eq!(pixels, vec![0x0011_2233]);
    }

    // cite: frameworks-base ColorUtils.java#compositeColors, compositeComponent
    #[test]
    fn color_utils_src_over_flattens_premultiplied_skia_bgra_onto_host_background() {
        let pixels = opaque_softbuffer_pixels(&[0x00, 0x00, 0x80, 0x80], BACKGROUND);

        assert_eq!(pixels, vec![0x00f9_7a7b]);
    }

    #[test]
    fn renders_activity_surface_at_device_pixel_scale() {
        let image = render_activity_surface_scaled(
            180,
            120,
            "x",
            &[],
            &[],
            &[],
            &[],
            None,
            None,
            None,
            2.0,
        )
        .expect("hi-dpi surface should render");

        assert_eq!(image.width, 360);
        assert_eq!(image.height, 240);
    }

    #[test]
    fn screenshot_flash_shrinks_then_holds_scale() {
        assert_eq!(screenshot_flash_scale(0), 1.0);
        assert_eq!(
            screenshot_flash_scale(SCREENSHOT_FLASH_SHRINK_MS / 2),
            1.0 - (1.0 - SCREENSHOT_FLASH_MIN_SCALE) / 2.0
        );
        assert_eq!(
            screenshot_flash_scale(SCREENSHOT_FLASH_SHRINK_MS),
            SCREENSHOT_FLASH_MIN_SCALE
        );
        assert_eq!(
            screenshot_flash_scale(SCREENSHOT_FLASH_DURATION_MS),
            SCREENSHOT_FLASH_MIN_SCALE
        );
    }

    #[test]
    fn screenshot_flash_holds_then_rises_off_screen() {
        let rise_start = SCREENSHOT_FLASH_SHRINK_MS + SCREENSHOT_FLASH_HOLD_MS;
        let rise_distance = 760.0 + SCREENSHOT_FLASH_RISE_ABOVE;

        assert_eq!(screenshot_flash_rise(0, 760), 0.0);
        assert_eq!(screenshot_flash_rise(rise_start, 760), 0.0);
        let mid_rise = rise_start + SCREENSHOT_FLASH_RISE_MS / 2;
        assert!((screenshot_flash_rise(mid_rise, 760) - rise_distance / 2.0).abs() < 0.001);
        assert_eq!(
            screenshot_flash_rise(SCREENSHOT_FLASH_DURATION_MS, 760),
            rise_distance
        );
    }

    #[test]
    fn renders_screenshot_flash_over_live_softbuffer_pixels() {
        let snapshot = render_activity_surface(64, 64, "", &[], &[], &[], &[], None, None, None)
            .expect("snapshot should render");
        let background = render_activity_surface_pixels_scaled(
            64,
            64,
            "live",
            &[],
            &[],
            &[],
            &[],
            None,
            None,
            None,
            2.0,
        )
        .expect("snapshot pixels should render");

        let flash = render_screenshot_flash_pixels_scaled(
            64,
            64,
            &background.pixels,
            &snapshot.png,
            SCREENSHOT_FLASH_DURATION_MS,
            2.0,
        )
        .expect("softbuffer flash should render");

        assert_eq!(flash.width, 128);
        assert_eq!(flash.height, 128);
        assert_eq!(flash.pixels.len(), 128 * 128);
        assert_eq!(flash.pixels, background.pixels);
        assert!(flash.png.is_empty());
    }

    #[test]
    fn screenshot_flash_rejects_a_mismatched_live_background() {
        let snapshot = render_activity_surface(64, 64, "", &[], &[], &[], &[], None, None, None)
            .expect("snapshot should render");

        let error = render_screenshot_flash_pixels_scaled(64, 64, &[], &snapshot.png, 0, 1.0)
            .expect_err("mismatched live pixels should fail");

        assert!(error
            .to_string()
            .contains("screenshot flash background pixel count mismatch"));
    }

    #[test]
    fn renders_runtime_view_root_surface() {
        let image = render_activity_surface(
            180,
            120,
            "bootstrap",
            &[ActivityTextElement {
                text: "Runtime View root\nLandroidx/compose/ui/platform/ComposeView;".to_owned(),
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
                width: 180,
                height: 120,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            &[],
            &[],
            &[],
            None,
            None,
            None,
        )
        .expect("Skia runtime root surface should render");

        assert_eq!(image.width, 180);
        assert_eq!(image.height, 120);
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "runtime root renderer output should be a PNG"
        );
    }

    #[test]
    fn renders_runtime_view_root_canvas_frame() {
        let image = render_activity_surface(
            180,
            120,
            "bootstrap",
            &[ActivityTextElement {
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
                width: 180,
                height: 120,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            &[ActivityRuntimeCanvasFrame {
                x: 0,
                y: 0,
                width: 180,
                height: 120,
                ops: vec![
                    ActivityRuntimeCanvasOp::Rect {
                        left: 0.0,
                        top: 0.0,
                        right: 180.0,
                        bottom: 120.0,
                        color: 0xffff_ffff,
                        blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
                        paint_style: crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill,
                    },
                    ActivityRuntimeCanvasOp::Save,
                    ActivityRuntimeCanvasOp::Translate { dx: 8.0, dy: 4.0 },
                    ActivityRuntimeCanvasOp::RoundRect {
                        left: 24.0,
                        top: 24.0,
                        right: 156.0,
                        bottom: 96.0,
                        rx: 12.0,
                        ry: 12.0,
                        color: 0xff22_66dd,
                        blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
                        paint_style: crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill,
                    },
                    ActivityRuntimeCanvasOp::Text {
                        text: "Compose".to_owned(),
                        x: 42.0,
                        y: 66.0,
                        color: 0xffff_ffff,
                        text_size: 18.0,
                    },
                    ActivityRuntimeCanvasOp::Restore,
                ],
            }],
            &[],
            &[],
            None,
            None,
            None,
        )
        .expect("Skia runtime canvas surface should render");

        assert_eq!(image.width, 180);
        assert_eq!(image.height, 120);
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "runtime canvas renderer output should be a PNG"
        );
    }

    #[test]
    fn composites_every_runtime_view_root_frame_not_just_the_last() {
        fn rect_frame(left: f32, right: f32, color: u32) -> ActivityRuntimeCanvasFrame {
            ActivityRuntimeCanvasFrame {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
                ops: vec![ActivityRuntimeCanvasOp::Rect {
                    left,
                    top: 0.0,
                    right,
                    bottom: 20.0,
                    color,
                    blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
                    paint_style: crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill,
                }],
            }
        }
        fn root_element() -> ActivityTextElement {
            ActivityTextElement {
                text: "root".to_owned(),
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
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 0,
                y: 0,
                width: 40,
                height: 20,
                clip: None,
            }
        }

        let content = rect_frame(0.0, 20.0, 0xffff_0000);
        let popup = rect_frame(20.0, 40.0, 0xff00_00ff);
        let render = |frames: &[ActivityRuntimeCanvasFrame]| {
            render_activity_surface(
                40,
                20,
                "x",
                &[root_element()],
                frames,
                &[],
                &[],
                None,
                None,
                None,
            )
            .expect("render should succeed")
            .png
        };

        let both = render(&[content.clone(), popup.clone()]);
        assert_ne!(both, render(std::slice::from_ref(&content)));
        assert_ne!(both, render(std::slice::from_ref(&popup)));
    }

    #[test]
    fn renders_action_bar_band() {
        let action_bar = crate::host_window::ActivityActionBar {
            title: "Currency".to_owned(),
            items: vec![crate::host_window::ActivityActionItem {
                item_id: 1,
                title: "More".to_owned(),
                enabled: true,
                image: None,
                overflow: true,
            }],
            background_color: Some(0xff30_3030),
            navigation_icon: None,
            navigation_is_up: false,
            is_decor: true,
        };
        let render = |bar: Option<&crate::host_window::ActivityActionBar>| {
            render_activity_surface(200, 200, "x", &[], &[], &[], &[], bar, None, None)
                .expect("render should succeed")
                .png
        };

        assert_ne!(render(Some(&action_bar)), render(None));
    }

    fn test_activity_image(tint_color: Option<u32>) -> ActivityImage {
        ActivityImage {
            source: "res/icon.xml".to_owned(),
            data: Vec::new(),
            resources: Vec::new(),
            theme: crate::layout_render::LayoutTheme {
                styles: Vec::new(),
                config: resource_table::ResourceResolutionConfig::default(),
            },
            tint_color,
            padding_after: 0,
        }
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource
    #[test]
    fn shape_background_resource_falls_back_to_known_framework_color() {
        let resources = configured_entries_resource_table(
            "drawable",
            &["icon"],
            &["res/drawable/icon.xml"],
            &[(0x03, 0)],
            &[(0x03, 0)],
        );
        let config =
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let theme = crate::layout_render::LayoutTheme {
            styles: Vec::new(),
            config,
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let apk_path = std::env::temp_dir().join(format!(
            "winedroid-framework-color-background-{}-{unique}.apk",
            std::process::id()
        ));
        let apk_file = std::fs::File::create(&apk_path).expect("test APK should be created");
        zip::ZipWriter::new(apk_file)
            .finish()
            .expect("empty test APK should finish");
        let mut apk = crate::apk::ApkArchive::open(&apk_path).expect("test APK should open");

        let style = crate::layout_render::test_shape_drawable_style(
            &mut apk,
            &resources,
            0x0106_000d,
            &theme,
        )
        .expect("framework color background should resolve without error");
        drop(apk);
        std::fs::remove_file(&apk_path).expect("test APK should be removed");

        assert_eq!(style, None);
    }

    // cite: frameworks-base ResourcesImpl.java#loadDrawable, loadDrawableForCookie
    // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource
    #[test]
    fn resources_impl_selected_display_drawable_retains_config_and_malformed_xml_errors() {
        use std::io::Write;

        let resources = configured_value_resource_table(
            "drawable",
            "icon",
            &["res/drawable/icon.xml", "res/drawable-land/icon.xml"],
            0x03,
            0,
            0x03,
            1,
        );
        let config =
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let theme = crate::layout_render::LayoutTheme {
            styles: Vec::new(),
            config,
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let apk_path = std::env::temp_dir().join(format!(
            "winedroid-qualified-drawable-{}-{unique}.apk",
            std::process::id()
        ));
        let apk_file = std::fs::File::create(&apk_path).expect("test APK should be created");
        let mut zip = zip::ZipWriter::new(apk_file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("res/drawable/icon.xml", options)
            .expect("default drawable entry should start");
        zip.write_all(b"default")
            .expect("default drawable should write");
        zip.start_file("res/drawable-land/icon.xml", options)
            .expect("configured drawable entry should start");
        zip.write_all(b"configured")
            .expect("configured drawable should write");
        zip.finish().expect("test APK should finish");

        let mut apk = crate::apk::ApkArchive::open(&apk_path).expect("test APK should open");
        let drawable = crate::layout_render::load_drawable_resource_from_apk(
            &mut apk,
            &resources,
            0x7f01_0000,
            &theme,
        )
        .expect("configured drawable should load");
        drop(apk);

        assert_eq!(drawable.path, "res/drawable-land/icon.xml");
        assert_eq!(drawable.data, b"configured");
        assert_eq!(drawable.theme, theme);

        let mut apk = crate::apk::ApkArchive::open(&apk_path).expect("test APK should reopen");
        let error = crate::layout_render::test_shape_drawable_style(
            &mut apk,
            &resources,
            0x7f01_0000,
            &theme,
        )
        .expect_err("malformed configured shape drawable should propagate");
        drop(apk);
        std::fs::remove_file(&apk_path).expect("test APK should be removed");

        assert!(error
            .to_string()
            .contains("failed to parse shape background resource"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource, ResTable::resolveReference
    // cite: frameworks-base ResourcesImpl.java#loadDrawableForCookie
    #[test]
    fn configured_drawable_file_alias_reaches_terminal_file_and_shape_classification() {
        use std::io::Write;

        let resources = configured_entries_resource_table(
            "drawable",
            &["icon_alias", "icon"],
            &["res/drawable/icon.xml", "res/drawable-land/icon.xml"],
            &[(0x01, 0x7f01_0001), (0x03, 0)],
            &[(0x01, 0x7f01_0001), (0x03, 1)],
        );
        let config =
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let file =
            resource_table::resolve_resource_file_for_config(&resources, 0x7f01_0000, config)
                .expect("configured drawable alias should resolve")
                .expect("configured drawable alias should exist");
        let classified =
            crate::layout_render::is_drawable_file_resource(&resources, 0x7f01_0000, config)
                .expect("configured drawable alias classification should resolve");

        assert_eq!(file.resource_id, 0x7f01_0001);
        assert_eq!(file.path, "res/drawable-land/icon.xml");
        assert!(classified);

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let apk_path = std::env::temp_dir().join(format!(
            "winedroid-aliased-shape-{}-{unique}.apk",
            std::process::id()
        ));
        let apk_file = std::fs::File::create(&apk_path).expect("test APK should be created");
        let mut zip = zip::ZipWriter::new(apk_file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("res/drawable/icon.xml", options)
            .expect("default drawable entry should start");
        zip.write_all(b"default")
            .expect("default drawable should write");
        zip.start_file("res/drawable-land/icon.xml", options)
            .expect("configured drawable entry should start");
        zip.write_all(b"configured")
            .expect("configured drawable should write");
        zip.finish().expect("test APK should finish");

        let theme = crate::layout_render::LayoutTheme {
            styles: Vec::new(),
            config,
        };
        let mut apk = crate::apk::ApkArchive::open(&apk_path).expect("test APK should open");
        let error = crate::layout_render::test_shape_drawable_style(
            &mut apk,
            &resources,
            0x7f01_0000,
            &theme,
        )
        .expect_err("aliased malformed configured shape should propagate");
        drop(apk);
        std::fs::remove_file(&apk_path).expect("test APK should be removed");

        assert!(error.to_string().contains("res/drawable-land/icon.xml"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    #[test]
    fn drawable_file_alias_cycle_and_excessive_depth_fail_explicitly() {
        let config =
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let cycle_entries = [(0x01, 0x7f01_0001), (0x01, 0x7f01_0000)];
        let cycle_resources = configured_entries_resource_table(
            "drawable",
            &["cycle_a", "cycle_b"],
            &[],
            &cycle_entries,
            &cycle_entries,
        );
        let cycle_error =
            resource_table::resolve_resource_file_for_config(&cycle_resources, 0x7f01_0000, config)
                .expect_err("drawable alias cycle should fail");

        let key_names = [
            "reference_00",
            "reference_01",
            "reference_02",
            "reference_03",
            "reference_04",
            "reference_05",
            "reference_06",
            "reference_07",
            "reference_08",
            "reference_09",
            "reference_10",
            "reference_11",
            "reference_12",
            "reference_13",
            "reference_14",
            "reference_15",
            "reference_16",
            "reference_17",
            "reference_18",
            "reference_19",
            "reference_20",
            "terminal",
        ];
        let mut depth_entries = Vec::new();
        for index in 0..21 {
            depth_entries.push((0x01, 0x7f01_0000 + index + 1));
        }
        depth_entries.push((0x03, 0));
        let depth_resources = configured_entries_resource_table(
            "drawable",
            &key_names,
            &["res/drawable/icon.xml"],
            &depth_entries,
            &depth_entries,
        );
        let boundary =
            resource_table::resolve_resource_file_for_config(&depth_resources, 0x7f01_0001, config)
                .expect("twenty drawable aliases should resolve")
                .expect("terminal drawable should exist");
        let depth_error =
            resource_table::resolve_resource_file_for_config(&depth_resources, 0x7f01_0000, config)
                .expect_err("drawable alias depth should be bounded");

        assert!(cycle_error.to_string().contains("reference cycle"));
        assert_eq!(boundary.path, "res/drawable/icon.xml");
        assert!(depth_error.to_string().contains("exceeded 20 references"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource, ResTable::resolveReference
    // cite: frameworks-base VectorDrawable.java#updateStateFromTypedArray
    // cite: frameworks-base GradientDrawable.java#updateGradientDrawableSolid
    #[test]
    fn resource_references_inside_vector_and_shape_use_drawable_display_config() {
        let resources = configured_value_resource_table(
            "color",
            "icon_color",
            &[],
            0x1c,
            0xffff_0000,
            0x1c,
            0xff00_ff00,
        );
        let mut image = test_activity_image(None);
        image.resources = resources;
        image.theme.config =
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let path = XmlStartElement {
            name: "path".to_owned(),
            namespace: None,
            attributes: vec![
                XmlAttribute {
                    name: "pathData".to_owned(),
                    namespace: None,
                    value: XmlValue::String("M0,0 L1,1".to_owned()),
                },
                XmlAttribute {
                    name: "fillColor".to_owned(),
                    namespace: None,
                    value: XmlValue::Reference(0x7f01_0000),
                },
            ],
        };
        let vector_path = parse_android_vector_path(&path, &[], &image, None)
            .expect("vector resource color should resolve");
        let shape_events = vec![
            XmlEvent::Start(XmlStartElement {
                name: "shape".to_owned(),
                namespace: None,
                attributes: Vec::new(),
            }),
            XmlEvent::Start(XmlStartElement {
                name: "solid".to_owned(),
                namespace: None,
                attributes: vec![XmlAttribute {
                    name: "color".to_owned(),
                    namespace: None,
                    value: XmlValue::Reference(0x7f01_0000),
                }],
            }),
        ];
        let shape = parse_android_shape_drawable(&image, &shape_events)
            .expect("shape resource color should resolve");

        let dimension_resources = configured_value_resource_table(
            "dimen",
            "icon_viewport",
            &[],
            0x05,
            0x0000_0830,
            0x05,
            0x0000_1830,
        );
        let mut dimension_image = test_activity_image(None);
        dimension_image.resources = dimension_resources;
        dimension_image.theme.config = image.theme.config;
        let vector = XmlStartElement {
            name: "vector".to_owned(),
            namespace: None,
            attributes: vec![XmlAttribute {
                name: "viewportWidth".to_owned(),
                namespace: None,
                value: XmlValue::Reference(0x7f01_0000),
            }],
        };
        let viewport_width = required_vector_float(&vector, "viewportWidth", &dimension_image)
            .expect("vector dimension resource should resolve");

        assert_eq!(vector_path.fill_color, Color::new(0xff00_ff00));
        assert_eq!(shape.solid_color, Color::new(0xff00_ff00));
        assert_eq!(viewport_width, 24.0);
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    #[test]
    fn resource_aliases_inside_vector_color_and_dimension_follow_configured_chain() {
        let config =
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let color_resources = configured_entries_resource_table(
            "color",
            &["icon_color_alias", "icon_color"],
            &[],
            &[(0x01, 0x7f01_0001), (0x1c, 0xffff_0000)],
            &[(0x01, 0x7f01_0001), (0x1c, 0xff00_ff00)],
        );
        let mut color_image = test_activity_image(None);
        color_image.resources = color_resources;
        color_image.theme.config = config;
        let color = resolve_vector_color_reference(&color_image, 0x7f01_0000)
            .expect("configured color alias should resolve");

        let dimension_resources = configured_entries_resource_table(
            "dimen",
            &["icon_viewport_alias", "icon_viewport"],
            &[],
            &[(0x01, 0x7f01_0001), (0x05, 0x0000_0830)],
            &[(0x01, 0x7f01_0001), (0x05, 0x0000_1830)],
        );
        let mut dimension_image = test_activity_image(None);
        dimension_image.resources = dimension_resources;
        dimension_image.theme.config = config;
        let dimension = resolve_vector_float_reference(&dimension_image, 0x7f01_0000)
            .expect("configured dimension alias should resolve");

        assert_eq!(color, Color::new(0xff00_ff00));
        assert_eq!(dimension, 24.0);
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    #[test]
    fn vector_resource_reference_resolution_treats_zero_as_terminal_null() {
        let resources = configured_entries_resource_table(
            "color",
            &["optional_icon_color"],
            &[],
            &[(0x01, 0)],
            &[(0x01, 0)],
        );
        let mut image = test_activity_image(None);
        image.resources = resources;

        assert_eq!(
            resolve_vector_resource_reference(&image, 0x7f01_0000, "color")
                .expect("null vector resource reference should remain a typed value"),
            ResourcePrimitive::Reference(0),
        );
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    #[test]
    fn resource_alias_cycles_and_excessive_depth_fail_explicitly() {
        let config =
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600);
        let cycle_entries = [(0x01, 0x7f01_0001), (0x01, 0x7f01_0000)];
        let cycle_resources = configured_entries_resource_table(
            "color",
            &["cycle_a", "cycle_b"],
            &[],
            &cycle_entries,
            &cycle_entries,
        );
        let mut cycle_image = test_activity_image(None);
        cycle_image.resources = cycle_resources;
        cycle_image.theme.config = config;
        let cycle_error = resolve_vector_color_reference(&cycle_image, 0x7f01_0000)
            .expect_err("resource alias cycle should fail");

        let key_names = [
            "reference_00",
            "reference_01",
            "reference_02",
            "reference_03",
            "reference_04",
            "reference_05",
            "reference_06",
            "reference_07",
            "reference_08",
            "reference_09",
            "reference_10",
            "reference_11",
            "reference_12",
            "reference_13",
            "reference_14",
            "reference_15",
            "reference_16",
            "reference_17",
            "reference_18",
            "reference_19",
            "reference_20",
            "terminal",
        ];
        let mut depth_entries = Vec::new();
        for index in 0..21 {
            depth_entries.push((0x01, 0x7f01_0000 + index + 1));
        }
        depth_entries.push((0x1c, 0xff00_ff00));
        let depth_resources = configured_entries_resource_table(
            "color",
            &key_names,
            &[],
            &depth_entries,
            &depth_entries,
        );
        let mut depth_image = test_activity_image(None);
        depth_image.resources = depth_resources;
        depth_image.theme.config = config;
        let depth_error = resolve_vector_color_reference(&depth_image, 0x7f01_0000)
            .expect_err("resource alias depth should be bounded");

        assert!(cycle_error.to_string().contains("reference cycle"));
        assert!(depth_error.to_string().contains("exceeded 20 references"));
    }

    // cite: frameworks-base Resources.java#resolveAttribute
    // cite: frameworks-base VectorDrawable.java#applyTheme, canApplyTheme
    // cite: frameworks-base GradientDrawable.java#applyTheme, canApplyTheme
    #[test]
    fn vector_and_shape_theme_color_attributes_use_activity_image_theme() {
        let attribute_id = 0x7f04_0000;
        let mut image = test_activity_image(None);
        image.resources = crate::layout_render::tests::vector_theme_color_resource_table(
            attribute_id,
            0xff12_3456,
        );
        image.theme.styles = vec![crate::layout_render::LayoutThemeStyle {
            resource_id: 0x7f01_0000,
            force: true,
        }];
        let path = XmlStartElement {
            name: "path".to_owned(),
            namespace: None,
            attributes: vec![
                XmlAttribute {
                    name: "pathData".to_owned(),
                    namespace: None,
                    value: XmlValue::String("M0,0 L1,1".to_owned()),
                },
                XmlAttribute {
                    name: "fillColor".to_owned(),
                    namespace: None,
                    value: XmlValue::AttributeReference(attribute_id),
                },
            ],
        };
        let vector_path = parse_android_vector_path(&path, &[], &image, None)
            .expect("vector theme color should resolve");
        let shape = parse_android_shape_drawable(
            &image,
            &[
                XmlEvent::Start(XmlStartElement {
                    name: "shape".to_owned(),
                    namespace: None,
                    attributes: Vec::new(),
                }),
                XmlEvent::Start(XmlStartElement {
                    name: "solid".to_owned(),
                    namespace: None,
                    attributes: vec![XmlAttribute {
                        name: "color".to_owned(),
                        namespace: None,
                        value: XmlValue::AttributeReference(attribute_id),
                    }],
                }),
            ],
        )
        .expect("shape theme color should resolve");

        assert_eq!(vector_path.fill_color, Color::new(0xff12_3456));
        assert_eq!(shape.solid_color, Color::new(0xff12_3456));
    }

    // cite: frameworks-base Resources.java#resolveAttribute
    // cite: frameworks-base VectorDrawable.java#applyTheme, canApplyTheme
    #[test]
    fn vector_theme_color_attribute_missing_from_activity_theme_errors() {
        let image = test_activity_image(None);

        let error = resolve_vector_theme_color_attribute(&image, 0x7f04_0000)
            .expect_err("missing vector theme attribute should fail");

        assert!(error.to_string().contains("was not found in active theme"));
    }

    #[test]
    fn renders_runtime_canvas_frame_without_text_elements() {
        let image = render_activity_surface(
            180,
            120,
            "bootstrap",
            &[],
            &[ActivityRuntimeCanvasFrame {
                x: 0,
                y: 0,
                width: 180,
                height: 120,
                ops: vec![ActivityRuntimeCanvasOp::Rect {
                    left: 0.0,
                    top: 0.0,
                    right: 180.0,
                    bottom: 120.0,
                    color: 0xff22_66dd,
                    blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
                    paint_style: crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill,
                }],
            }],
            &[],
            &[],
            None,
            None,
            None,
        )
        .expect("Skia runtime canvas surface should render");

        assert_eq!(image.width, 180);
        assert_eq!(image.height, 120);
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "runtime canvas renderer output should be a PNG"
        );
    }

    #[test]
    fn renders_runtime_canvas_bitmap_ops() {
        let image = render_activity_surface(
            180,
            120,
            "bootstrap",
            &[],
            &[ActivityRuntimeCanvasFrame {
                x: 0,
                y: 0,
                width: 180,
                height: 120,
                ops: vec![ActivityRuntimeCanvasOp::Bitmap {
                    left: 20.0,
                    top: 24.0,
                    right: 160.0,
                    bottom: 96.0,
                    source_left: 0.0,
                    source_top: 0.0,
                    source_right: 100.0,
                    source_bottom: 50.0,
                    bitmap_width: 100,
                    bitmap_height: 50,
                    color: 0xff00_0000,
                    blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
                    color_filter: None,
                    alpha_mask: false,
                    encoded_image: None,
                    ops: vec![ActivityRuntimeCanvasOp::Rect {
                        left: 0.0,
                        top: 0.0,
                        right: 100.0,
                        bottom: 50.0,
                        color: 0xff22_66dd,
                        blend_mode: crate::host_window::ActivityRuntimeCanvasBlendMode::SrcOver,
                        paint_style: crate::host_window::ActivityRuntimeCanvasPaintStyle::Fill,
                    }],
                }],
            }],
            &[],
            &[],
            None,
            None,
            None,
        )
        .expect("Skia runtime bitmap surface should render");

        assert_eq!(image.width, 180);
        assert_eq!(image.height, 120);
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "runtime bitmap renderer output should be a PNG"
        );
    }

    #[test]
    fn parses_shape_drawable_solid_color() {
        let events = vec![
            XmlEvent::Start(XmlStartElement {
                name: "shape".to_owned(),
                namespace: None,
                attributes: Vec::new(),
            }),
            XmlEvent::Start(XmlStartElement {
                name: "solid".to_owned(),
                namespace: None,
                attributes: vec![XmlAttribute {
                    name: "color".to_owned(),
                    namespace: None,
                    value: XmlValue::Color(0x1f00_0000),
                }],
            }),
        ];

        let image = test_activity_image(None);
        let shape = parse_android_shape_drawable(&image, &events)
            .expect("shape drawable with solid color should parse");

        assert_eq!(
            shape,
            AndroidShapeDrawable {
                solid_color: Color::new(0x1f00_0000)
            }
        );
    }

    #[test]
    fn renders_popup_overlay_on_activity_surface() {
        let image = render_activity_surface(
            220,
            180,
            "bootstrap",
            &[ActivityTextElement {
                text: String::new(),
                kind: ActivityTextKind::Button,
                view_id: Some(2131296591),
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
                x: 168,
                y: 8,
                width: 44,
                height: 44,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            &[],
            &[ActivityOverlay {
                anchor_view_id: Some(2131296591),
                items: vec![crate::host_window::ActivityOverlayItem {
                    item_id: 2131296592,
                    text: "Settings".to_owned(),
                    enabled: true,
                }],
            }],
            &[],
            None,
            None,
            None,
        )
        .expect("Skia surface should render popup overlay");

        assert_eq!(image.width, 220);
        assert_eq!(image.height, 180);
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "renderer output should be a PNG"
        );
    }

    #[test]
    fn renders_dialog_overlay_on_activity_surface() {
        let image = render_activity_surface(
            240,
            240,
            "bootstrap",
            &[ActivityTextElement {
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
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 8,
                y: 40,
                width: 80,
                height: 44,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            &[],
            &[],
            &[ActivityDialogOverlay {
                title: Some("Support Us".to_owned()),
                content_layouts: vec![2131492910],
                text_elements: vec![ActivityTextElement {
                    text: "Open source calculator".to_owned(),
                    kind: ActivityTextKind::Label,
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
                    y: 40,
                    width: 160,
                    height: 32,
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                }],
                items: Vec::new(),
                item_scroll_offset: 0,
                buttons: vec![crate::host_window::ActivityDialogButton {
                    text: "Close".to_owned(),
                    enabled: true,
                }],
            }],
            None,
            None,
            None,
        )
        .expect("Skia surface should render dialog overlay");

        assert_eq!(image.width, 240);
        assert_eq!(image.height, 240);
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "renderer output should be a PNG"
        );
    }

    #[test]
    fn renders_list_item_switch_accessory() {
        let image = render_activity_surface(
            260,
            120,
            "bootstrap",
            &[ActivityTextElement {
                text: "Vibrate\nUse haptic feedback".to_owned(),
                kind: ActivityTextKind::ListItem,
                view_id: None,
                preference_key: Some("object:1".to_owned()),
                accessory: Some(ActivityTextAccessory::Switch { checked: true }),
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 12,
                y: 32,
                width: 236,
                height: 58,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            &[],
            &[],
            &[],
            None,
            None,
            None,
        )
        .expect("Skia surface should render switch accessory");

        assert_eq!(image.width, 260);
        assert_eq!(image.height, 120);
        assert!(
            image.png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "renderer output should be a PNG"
        );
    }

    #[test]
    fn label_text_x_honors_alignment() {
        let frame = Rect::from_xywh(10.0, 20.0, 100.0, 30.0);

        assert_eq!(
            aligned_label_text_x(&frame, ActivityTextAlignment::Start, 40.0),
            10.0
        );
        assert_eq!(
            aligned_label_text_x(&frame, ActivityTextAlignment::Center, 40.0),
            40.0
        );
        assert_eq!(
            aligned_label_text_x(&frame, ActivityTextAlignment::End, 40.0),
            70.0
        );
    }
}
