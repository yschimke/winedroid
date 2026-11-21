use std::ffi::c_void;

use crate::error::Result;
use crate::input::InputKey;
use crate::trace;

mod geometry;
mod headless;
mod session;

pub use geometry::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityWindowRequest {
    pub package_name: String,
    pub activity_name: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub display_round: bool,
    pub detail_lines: Vec<String>,
    pub text_elements: Vec<ActivityTextElement>,
    pub runtime_canvas_frames: Vec<ActivityRuntimeCanvasFrame>,
    pub overlays: Vec<ActivityOverlay>,
    pub dialogs: Vec<ActivityDialogOverlay>,
    pub action_bar: Option<ActivityActionBar>,
    pub frame_interval_nanos: Option<i64>,
    pub screenshots_dir: Option<crate::external_storage::ScreenshotDirectory>,
    pub window_background: Option<u32>,
    pub resource_id_names: std::collections::BTreeMap<i32, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivitySurfaceState {
    pub message: String,
    pub text_elements: Vec<ActivityTextElement>,
    pub runtime_canvas_frames: Vec<ActivityRuntimeCanvasFrame>,
    pub overlays: Vec<ActivityOverlay>,
    pub dialogs: Vec<ActivityDialogOverlay>,
    pub action_bar: Option<ActivityActionBar>,
    pub floating_panel: Option<ActivityRect>,
    pub window_background: Option<u32>,
    pub resource_id_names: std::collections::BTreeMap<i32, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityActionBar {
    pub title: String,
    pub items: Vec<ActivityActionItem>,
    pub background_color: Option<u32>,
    pub navigation_icon: Option<ActivityImage>,
    pub navigation_is_up: bool,
    pub is_decor: bool,
}

pub const ANDROID_R_ID_HOME: i32 = 0x0102_002c;

pub const ACTION_BAR_OVERFLOW_TOGGLE_ID: i32 = 0x6fff_fffe;

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityActionItem {
    pub item_id: i32,
    pub title: String,
    pub enabled: bool,
    pub image: Option<ActivityImage>,
    pub overflow: bool,
}

pub const ACTION_BAR_HEIGHT: u32 = 56;

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityRuntimeCanvasFrame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub ops: Vec<ActivityRuntimeCanvasOp>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActivityRuntimeCanvasPathOp {
    MoveTo {
        x: f32,
        y: f32,
    },
    LineTo {
        x: f32,
        y: f32,
    },
    RLineTo {
        dx: f32,
        dy: f32,
    },
    QuadTo {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
    CubicTo {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    },
    RCubicTo {
        dx1: f32,
        dy1: f32,
        dx2: f32,
        dy2: f32,
        dx3: f32,
        dy3: f32,
    },
    BooleanOp {
        left: Vec<ActivityRuntimeCanvasPathOp>,
        right: Vec<ActivityRuntimeCanvasPathOp>,
        op: ActivityRuntimeCanvasPathBooleanOp,
    },
    Rect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        clockwise: bool,
    },
    RoundRect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        radii: Vec<f32>,
        clockwise: bool,
    },
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityRuntimeCanvasPathBooleanOp {
    Difference,
    Intersect,
    Union,
    Xor,
    ReverseDifference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityRuntimeCanvasClipOp {
    Intersect,
    Difference,
}

// cite: frameworks-base BlendMode.java#CLEAR, SRC, DST, SRC_OVER, DST_OVER, SRC_IN, DST_IN, SRC_OUT, DST_OUT, SRC_ATOP, DST_ATOP, XOR, PLUS, MODULATE, SCREEN, OVERLAY, DARKEN, LIGHTEN, COLOR_DODGE, COLOR_BURN, HARD_LIGHT, SOFT_LIGHT, DIFFERENCE, EXCLUSION, MULTIPLY, HUE, SATURATION, COLOR, LUMINOSITY
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityRuntimeCanvasBlendMode {
    Clear,
    Src,
    Dst,
    SrcOver,
    DstOver,
    SrcIn,
    DstIn,
    SrcOut,
    DstOut,
    SrcAtop,
    DstAtop,
    Xor,
    Plus,
    Modulate,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Multiply,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

// cite: frameworks-base BlendModeColorFilter.java#BlendModeColorFilter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityRuntimeCanvasColorFilter {
    pub color: u32,
    pub blend_mode: ActivityRuntimeCanvasBlendMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityRuntimeCanvasPaintStyle {
    Fill,
    Stroke,
    FillAndStroke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityRuntimeCanvasStrokeCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActivityRuntimeCanvasOp {
    Save,
    Restore,
    Translate {
        dx: f32,
        dy: f32,
    },
    Scale {
        sx: f32,
        sy: f32,
    },
    Rotate {
        degrees: f32,
    },
    ConcatMatrix {
        values: [f32; 9],
    },
    ClipRect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        op: ActivityRuntimeCanvasClipOp,
    },
    ClipPath {
        ops: Vec<ActivityRuntimeCanvasPathOp>,
        op: ActivityRuntimeCanvasClipOp,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: u32,
        blend_mode: ActivityRuntimeCanvasBlendMode,
        stroke_width: f32,
        stroke_cap: ActivityRuntimeCanvasStrokeCap,
    },
    Rect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        color: u32,
        blend_mode: ActivityRuntimeCanvasBlendMode,
        paint_style: ActivityRuntimeCanvasPaintStyle,
    },
    Circle {
        cx: f32,
        cy: f32,
        radius: f32,
        color: u32,
        blend_mode: ActivityRuntimeCanvasBlendMode,
        paint_style: ActivityRuntimeCanvasPaintStyle,
        stroke_width: f32,
    },
    RoundRect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        rx: f32,
        ry: f32,
        color: u32,
        blend_mode: ActivityRuntimeCanvasBlendMode,
        paint_style: ActivityRuntimeCanvasPaintStyle,
    },
    Path {
        ops: Vec<ActivityRuntimeCanvasPathOp>,
        color: u32,
        blend_mode: ActivityRuntimeCanvasBlendMode,
        paint_style: ActivityRuntimeCanvasPaintStyle,
    },
    Text {
        text: String,
        x: f32,
        y: f32,
        color: u32,
        text_size: f32,
    },
    Bitmap {
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
        blend_mode: ActivityRuntimeCanvasBlendMode,
        color_filter: Option<ActivityRuntimeCanvasColorFilter>,
        alpha_mask: bool,
        encoded_image: Option<Vec<u8>>,
        ops: Vec<ActivityRuntimeCanvasOp>,
    },
    RenderNode {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        alpha: f32,
        scale_x: f32,
        scale_y: f32,
        clip_to_bounds: bool,
        ops: Vec<ActivityRuntimeCanvasOp>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityTextElement {
    pub text: String,
    pub kind: ActivityTextKind,
    pub view_id: Option<i32>,
    pub preference_key: Option<String>,
    pub accessory: Option<ActivityTextAccessory>,
    pub image: Option<ActivityImage>,
    pub text_size: Option<u32>,
    pub text_padding_left: u32,
    pub text_padding_right: u32,
    pub text_color: Option<u32>,
    pub background_color: Option<u32>,
    pub background_corner_radius: u32,
    pub background_border_color: Option<u32>,
    pub background_border_width: u32,
    pub elevation_px: u32,
    pub alignment: ActivityTextAlignment,
    pub editable: bool,
    pub cursor: Option<ActivityTextCursor>,
    pub x: u32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub clip: Option<ActivityRect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityTextCursor {
    pub selection_start: i32,
    pub selection_end: i32,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityImage {
    pub source: String,
    pub data: Vec<u8>,
    pub resources: Vec<u8>,
    pub theme: crate::layout_render::LayoutTheme,
    pub tint_color: Option<u32>,
    pub padding_after: u32,
}

pub const MISSING_FRAMEWORK_DRAWABLE_SOURCE: &str = "winedroid:missing-framework-drawable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityOverlay {
    pub anchor_view_id: Option<i32>,
    pub items: Vec<ActivityOverlayItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityOverlayItem {
    pub item_id: i32,
    pub text: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityOverlayItemFrame {
    pub item: ActivityOverlayItem,
    pub frame: ActivityRect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDialogOverlay {
    pub title: Option<String>,
    pub content_layouts: Vec<i32>,
    pub text_elements: Vec<ActivityTextElement>,
    pub items: Vec<ActivityDialogItem>,
    pub item_scroll_offset: usize,
    pub buttons: Vec<ActivityDialogButton>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDialogItem {
    pub text: String,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDialogItemFrame {
    pub dialog_index: usize,
    pub item_index: usize,
    pub item: ActivityDialogItem,
    pub frame: ActivityRect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDialogButton {
    pub text: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDialogButtonFrame {
    pub dialog_index: usize,
    pub button_index: usize,
    pub button: ActivityDialogButton,
    pub frame: ActivityRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityTextKind {
    Label,
    Button,
    ListItem,
    RuntimeViewRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityTextAccessory {
    Switch { checked: bool },
    ListValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityTextAlignment {
    Start,
    Center,
    End,
}

impl ActivityTextElement {
    fn contains_host_point(&self, x: u32, y: u32) -> bool {
        let element_left = i64::from(self.x);
        let element_top = i64::from(self.y).saturating_add(i64::from(self.height));
        let point_x = i64::from(x);
        let point_y = i64::from(y);

        if point_x < element_left
            || point_x >= element_left.saturating_add(i64::from(self.width))
            || point_y < i64::from(self.y)
            || point_y >= element_top
        {
            return false;
        }

        if let Some(clip) = self.clip {
            return clip.contains_host_point(x, y);
        }

        true
    }
}

impl ActivityRect {
    pub fn contains_host_point(&self, x: u32, y: u32) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width)
            && y >= self.y
            && y < self.y.saturating_add(self.height)
    }
}

fn text_element_view_click_target(element: &ActivityTextElement) -> Option<i32> {
    if element.kind == ActivityTextKind::ListItem && element.preference_key.is_some() {
        return None;
    }

    if is_background_only_text_element(element) {
        return None;
    }

    element.view_id
}

fn is_background_only_text_element(element: &ActivityTextElement) -> bool {
    element.kind == ActivityTextKind::Label
        && element.background_color.is_some()
        && element.text.is_empty()
        && element.image.is_none()
        && element.accessory.is_none()
        && element.cursor.is_none()
}

pub struct FrameDriveOutcome {
    pub surface: Option<ActivitySurfaceState>,
    pub wants_more_frames: bool,
    pub main_thread_idle: bool,
    pub next_wake_delay_millis: Option<i64>,
}

#[derive(Clone, Copy)]
pub struct ActivityClickHandler {
    context: *const c_void,
    click_view_id: unsafe fn(*const c_void, i32) -> Result<i32>,
    click_preference: unsafe fn(*const c_void, &str) -> Result<i32>,
    click_touch: unsafe fn(*const c_void, u32, u32) -> Result<i32>,
    touch_hold: unsafe fn(*const c_void, u32, u32, u64) -> Result<i32>,
    touch_swipe: unsafe fn(*const c_void, (u32, u32), (u32, u32), u64, Option<u32>) -> Result<i32>,
    click_dialog_button: unsafe fn(*const c_void, usize, usize) -> Result<i32>,
    click_dialog_item: unsafe fn(*const c_void, usize, usize) -> Result<i32>,
    focus_view: unsafe fn(*const c_void, i32) -> Result<i32>,
    key_input: unsafe fn(*const c_void, &str, InputKey) -> Result<i32>,
    read_view_text: unsafe fn(*const c_void, i32) -> Result<Option<String>>,
    read_surface: unsafe fn(*const c_void) -> Result<ActivitySurfaceState>,
    set_frame_interval: unsafe fn(*const c_void, i64) -> Result<()>,
    drive_frame: unsafe fn(*const c_void, i64) -> Result<FrameDriveOutcome>,
}

impl ActivityClickHandler {
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new(
        context: *const c_void,
        click_view_id: unsafe fn(*const c_void, i32) -> Result<i32>,
        click_preference: unsafe fn(*const c_void, &str) -> Result<i32>,
        click_touch: unsafe fn(*const c_void, u32, u32) -> Result<i32>,
        touch_hold: unsafe fn(*const c_void, u32, u32, u64) -> Result<i32>,
        touch_swipe: unsafe fn(
            *const c_void,
            (u32, u32),
            (u32, u32),
            u64,
            Option<u32>,
        ) -> Result<i32>,
        click_dialog_button: unsafe fn(*const c_void, usize, usize) -> Result<i32>,
        click_dialog_item: unsafe fn(*const c_void, usize, usize) -> Result<i32>,
        focus_view: unsafe fn(*const c_void, i32) -> Result<i32>,
        key_input: unsafe fn(*const c_void, &str, InputKey) -> Result<i32>,
        read_view_text: unsafe fn(*const c_void, i32) -> Result<Option<String>>,
        read_surface: unsafe fn(*const c_void) -> Result<ActivitySurfaceState>,
        set_frame_interval: unsafe fn(*const c_void, i64) -> Result<()>,
        drive_frame: unsafe fn(*const c_void, i64) -> Result<FrameDriveOutcome>,
    ) -> Self {
        Self {
            context,
            click_view_id,
            click_preference,
            click_touch,
            touch_hold,
            touch_swipe,
            click_dialog_button,
            click_dialog_item,
            focus_view,
            key_input,
            read_view_text,
            read_surface,
            set_frame_interval,
            drive_frame,
        }
    }

    fn click_view_id(&self, view_id: i32) -> Result<i32> {
        unsafe { (self.click_view_id)(self.context, view_id) }
    }

    fn click_preference(&self, preference_key: &str) -> Result<i32> {
        unsafe { (self.click_preference)(self.context, preference_key) }
    }

    fn click_touch(&self, x: u32, y: u32) -> Result<i32> {
        unsafe { (self.click_touch)(self.context, x, y) }
    }

    fn touch_hold(&self, x: u32, y: u32, hold_ms: u64) -> Result<i32> {
        unsafe { (self.touch_hold)(self.context, x, y, hold_ms) }
    }

    fn touch_swipe(
        &self,
        start: (u32, u32),
        end: (u32, u32),
        duration_ms: u64,
        steps: Option<u32>,
    ) -> Result<i32> {
        unsafe { (self.touch_swipe)(self.context, start, end, duration_ms, steps) }
    }

    fn click_dialog_button(&self, dialog_index: usize, button_index: usize) -> Result<i32> {
        unsafe { (self.click_dialog_button)(self.context, dialog_index, button_index) }
    }

    fn click_dialog_item(&self, dialog_index: usize, item_index: usize) -> Result<i32> {
        unsafe { (self.click_dialog_item)(self.context, dialog_index, item_index) }
    }

    fn focus_view(&self, view_id: i32) -> Result<i32> {
        unsafe { (self.focus_view)(self.context, view_id) }
    }

    fn key_input(&self, characters: &str, key: InputKey) -> Result<i32> {
        unsafe { (self.key_input)(self.context, characters, key) }
    }

    fn read_view_text(&self, view_id: i32) -> Result<Option<String>> {
        unsafe { (self.read_view_text)(self.context, view_id) }
    }

    fn read_surface(&self) -> Result<ActivitySurfaceState> {
        unsafe { (self.read_surface)(self.context) }
    }

    fn set_frame_interval(&self, frame_interval_nanos: i64) -> Result<()> {
        unsafe { (self.set_frame_interval)(self.context, frame_interval_nanos) }
    }

    fn drive_frame(&self, skipped_intervals: i64) -> Result<FrameDriveOutcome> {
        unsafe { (self.drive_frame)(self.context, skipped_intervals) }
    }
}

impl ActivityWindowRequest {
    pub(crate) fn new(
        package_name: &str,
        activity_name: &str,
        display_size: crate::config::DisplaySize,
    ) -> Self {
        Self {
            package_name: package_name.to_owned(),
            activity_name: activity_name.to_owned(),
            title: format!("{package_name} - {activity_name}"),
            width: display_size.width(),
            height: display_size.height(),
            display_round: false,
            detail_lines: Vec::new(),
            text_elements: Vec::new(),
            runtime_canvas_frames: Vec::new(),
            overlays: Vec::new(),
            dialogs: Vec::new(),
            action_bar: None,
            frame_interval_nanos: None,
            screenshots_dir: None,
            window_background: None,
            resource_id_names: std::collections::BTreeMap::new(),
        }
    }

    pub fn with_resource_id_names(
        mut self,
        resource_id_names: std::collections::BTreeMap<i32, String>,
    ) -> Self {
        self.resource_id_names = resource_id_names;
        self
    }

    pub fn with_display_round(mut self, display_round: bool) -> Self {
        self.display_round = display_round;
        self
    }

    pub fn with_window_background(mut self, window_background: Option<u32>) -> Self {
        self.window_background = window_background;
        self
    }

    pub fn with_frame_interval_nanos(mut self, frame_interval_nanos: Option<i64>) -> Self {
        self.frame_interval_nanos = frame_interval_nanos;
        self
    }

    pub fn with_screenshots_dir(
        mut self,
        screenshots_dir: Option<crate::external_storage::ScreenshotDirectory>,
    ) -> Self {
        self.screenshots_dir = screenshots_dir;
        self
    }

    pub fn with_action_bar(mut self, action_bar: Option<ActivityActionBar>) -> Self {
        self.action_bar = action_bar;
        self
    }

    pub fn with_detail_lines(mut self, detail_lines: Vec<String>) -> Self {
        self.detail_lines = detail_lines;
        self
    }

    pub fn with_text_elements(mut self, text_elements: Vec<ActivityTextElement>) -> Self {
        self.text_elements = text_elements;
        self
    }

    pub fn with_runtime_canvas_frames(
        mut self,
        runtime_canvas_frames: Vec<ActivityRuntimeCanvasFrame>,
    ) -> Self {
        self.runtime_canvas_frames = runtime_canvas_frames;
        self
    }
}

const ACTIVITY_OVERLAY_MARGIN: u32 = 8;
const ACTIVITY_OVERLAY_WIDTH: u32 = 220;
pub const ACTIVITY_OVERLAY_ITEM_HEIGHT: u32 = 48;
const ACTIVITY_DIALOG_HORIZONTAL_MARGIN: u32 = 28;
const ACTIVITY_DIALOG_VERTICAL_MARGIN: u32 = 64;
const ACTIVITY_DIALOG_MAX_WIDTH: u32 = 360;
const ACTIVITY_DIALOG_MIN_HEIGHT: u32 = 180;
pub const ACTIVITY_DIALOG_ITEM_HEIGHT: u32 = 48;
const ACTIVITY_DIALOG_BUTTON_HEIGHT: u32 = 40;
const ACTIVITY_DIALOG_BUTTON_BOTTOM_MARGIN: u32 = 12;
const ACTIVITY_DIALOG_BUTTON_RIGHT_MARGIN: u32 = 22;

pub fn open_activity_window(
    request: &ActivityWindowRequest,
    click_handler: Option<ActivityClickHandler>,
) -> Result<()> {
    if headless::headless_requested()? {
        return headless::run_headless_activity_session(request, click_handler);
    }

    #[cfg(target_os = "emscripten")]
    return emscripten::open_activity_window(request, click_handler);

    #[cfg(not(target_os = "emscripten"))]
    return desktop::open_activity_window(request, click_handler);
}

pub fn trace_activity_surface_bounds(
    label: &str,
    width: u32,
    height: u32,
    surface: &ActivitySurfaceState,
) {
    if !trace::enabled("bounds") {
        return;
    }

    for line in activity_surface_bound_lines(label, width, height, surface) {
        trace::event("bounds", line);
    }
}

pub fn trace_activity_surface_runtime_canvas_ops(label: &str, surface: &ActivitySurfaceState) {
    if !trace::enabled("render.ops") {
        return;
    }

    for (frame_index, frame) in surface.runtime_canvas_frames.iter().enumerate() {
        let prefix = format!(
            "{label} frame #{frame_index} {}x{} ops={}",
            frame.width,
            frame.height,
            frame.ops.len()
        );
        trace::event("render.ops", prefix);
        trace_activity_runtime_canvas_ops(label, frame_index, &frame.ops, 0);
    }
}

fn trace_activity_runtime_canvas_ops(
    label: &str,
    frame_index: usize,
    ops: &[ActivityRuntimeCanvasOp],
    depth: usize,
) {
    for (op_index, op) in ops.iter().enumerate() {
        trace::event(
            "render.ops",
            format!(
                "{label} frame #{frame_index} op #{op_index}: {}{}",
                "  ".repeat(depth),
                activity_runtime_canvas_op_line(op),
            ),
        );

        match op {
            ActivityRuntimeCanvasOp::Bitmap { ops, .. }
            | ActivityRuntimeCanvasOp::RenderNode { ops, .. } => {
                trace_activity_runtime_canvas_ops(label, frame_index, ops, depth + 1);
            }
            ActivityRuntimeCanvasOp::Save
            | ActivityRuntimeCanvasOp::Restore
            | ActivityRuntimeCanvasOp::Translate { .. }
            | ActivityRuntimeCanvasOp::Scale { .. }
            | ActivityRuntimeCanvasOp::Rotate { .. }
            | ActivityRuntimeCanvasOp::ConcatMatrix { .. }
            | ActivityRuntimeCanvasOp::ClipRect { .. }
            | ActivityRuntimeCanvasOp::ClipPath { .. }
            | ActivityRuntimeCanvasOp::Line { .. }
            | ActivityRuntimeCanvasOp::Rect { .. }
            | ActivityRuntimeCanvasOp::Circle { .. }
            | ActivityRuntimeCanvasOp::RoundRect { .. }
            | ActivityRuntimeCanvasOp::Path { .. }
            | ActivityRuntimeCanvasOp::Text { .. } => {}
        }
    }
}

fn activity_runtime_canvas_op_line(op: &ActivityRuntimeCanvasOp) -> String {
    match op {
        ActivityRuntimeCanvasOp::Save => "save".to_owned(),
        ActivityRuntimeCanvasOp::Restore => "restore".to_owned(),
        ActivityRuntimeCanvasOp::Translate { dx, dy } => {
            format!("translate dx={dx:.2} dy={dy:.2}")
        }
        ActivityRuntimeCanvasOp::Scale { sx, sy } => format!("scale sx={sx:.2} sy={sy:.2}"),
        ActivityRuntimeCanvasOp::Rotate { degrees } => format!("rotate degrees={degrees:.2}"),
        ActivityRuntimeCanvasOp::ConcatMatrix { values } => format!("concat matrix={values:?}"),
        ActivityRuntimeCanvasOp::ClipRect {
            left,
            top,
            right,
            bottom,
            op,
        } => format!(
            "clip_rect left={left:.2} top={top:.2} right={right:.2} bottom={bottom:.2} op={op:?}"
        ),
        ActivityRuntimeCanvasOp::ClipPath { ops, op } => {
            format!("clip_path op={op:?} path_ops={}", ops.len())
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
        } => format!(
            "line {x1:.2},{y1:.2}->{x2:.2},{y2:.2} color=0x{color:08x} blend={blend_mode:?} stroke={stroke_width:.2} cap={stroke_cap:?}"
        ),
        ActivityRuntimeCanvasOp::Rect {
            left,
            top,
            right,
            bottom,
            color,
            blend_mode,
            paint_style,
        } => format!(
            "rect left={left:.2} top={top:.2} right={right:.2} bottom={bottom:.2} color=0x{color:08x} blend={blend_mode:?} style={paint_style:?}"
        ),
        ActivityRuntimeCanvasOp::Circle {
            cx,
            cy,
            radius,
            color,
            blend_mode,
            paint_style,
            stroke_width,
        } => format!(
            "circle cx={cx:.2} cy={cy:.2} r={radius:.2} color=0x{color:08x} blend={blend_mode:?} style={paint_style:?} stroke={stroke_width:.2}"
        ),
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
        } => format!(
            "round_rect left={left:.2} top={top:.2} right={right:.2} bottom={bottom:.2} rx={rx:.2} ry={ry:.2} color=0x{color:08x} blend={blend_mode:?} style={paint_style:?}"
        ),
        ActivityRuntimeCanvasOp::Path {
            ops,
            color,
            blend_mode,
            paint_style,
        } => format!(
            "path color=0x{color:08x} blend={blend_mode:?} style={paint_style:?} path_ops={}",
            ops.len()
        ),
        ActivityRuntimeCanvasOp::Text {
            text,
            x,
            y,
            color,
            text_size,
        } => {
            format!("text {text:?} x={x:.2} y={y:.2} color=0x{color:08x} size={text_size:.2}")
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
        } => format!(
            "bitmap dst={left:.2},{top:.2},{right:.2},{bottom:.2} src={source_left:.2},{source_top:.2},{source_right:.2},{source_bottom:.2} size={}x{} color=0x{color:08x} blend={blend_mode:?} color_filter={} alpha_mask={alpha_mask} encoded_bytes={} nested_ops={}",
            bitmap_width,
            bitmap_height,
            color_filter
                .map(|filter| format!("0x{:08x}/{:?}", filter.color, filter.blend_mode))
                .unwrap_or_else(|| "none".to_owned()),
            encoded_image.as_ref().map(Vec::len).unwrap_or(0),
            ops.len()
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
        } => format!(
            "render_node bounds={left:.2},{top:.2},{right:.2},{bottom:.2} alpha={alpha:.2} scale={scale_x:.2},{scale_y:.2} clip_to_bounds={clip_to_bounds} nested_ops={}",
            ops.len()
        ),
    }
}

fn activity_surface_bound_lines(
    label: &str,
    width: u32,
    height: u32,
    surface: &ActivitySurfaceState,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "{label} surface frame={}x{}+0+0 elements={} canvasFrames={} overlays={} dialogs={}",
        width,
        height,
        surface.text_elements.len(),
        surface.runtime_canvas_frames.len(),
        surface.overlays.len(),
        surface.dialogs.len()
    ));

    for (index, element) in surface.text_elements.iter().enumerate() {
        lines.push(format!(
            "{label} element #{index} kind={:?}{}{} text={} frame={}{}{}{}{}{}",
            element.kind,
            display_optional_i32(" id", element.view_id),
            display_optional_string(" preference", element.preference_key.as_deref()),
            display_quoted_text(&element.text),
            display_text_element_screen_frame(element, height),
            display_optional_rect(" clip", element.clip, height),
            display_optional_u32_hex(" textColor", element.text_color),
            display_optional_u32_hex(" backgroundColor", element.background_color),
            display_activity_image(element.image.as_ref()),
            display_text_cursor(element.cursor.as_ref())
        ));
    }

    for (overlay_index, overlay) in surface.overlays.iter().enumerate() {
        let frame = activity_overlay_frame(width, height, &surface.text_elements, overlay);
        lines.push(format!(
            "{label} overlay #{overlay_index}{} frame={} items={}",
            display_optional_i32(" anchor", overlay.anchor_view_id),
            display_activity_rect_screen_frame(frame, height),
            overlay.items.len()
        ));

        for item_frame in
            activity_overlay_item_frames(width, height, &surface.text_elements, overlay)
        {
            lines.push(format!(
                "{label} overlay #{overlay_index} item id={} enabled={} text={} frame={}",
                item_frame.item.item_id,
                item_frame.item.enabled,
                display_quoted_text(&item_frame.item.text),
                display_activity_rect_screen_frame(item_frame.frame, height)
            ));
        }
    }

    for (dialog_index, dialog) in surface.dialogs.iter().enumerate() {
        let frame = activity_dialog_frame(width, height, dialog);
        let content = activity_dialog_content_rect(width, height, dialog);
        lines.push(format!(
            "{label} dialog #{dialog_index} title={} frame={} content={} elements={} items={} buttons={}",
            dialog
                .title
                .as_deref()
                .map(display_quoted_text)
                .unwrap_or_else(|| "none".to_owned()),
            display_activity_rect_screen_frame(frame, height),
            display_activity_rect_screen_frame(content, height),
            dialog.text_elements.len(),
            dialog.items.len(),
            dialog.buttons.len()
        ));

        for (element_index, element) in dialog.text_elements.iter().enumerate() {
            let absolute = offset_dialog_trace_element(element, content.x, content.y);
            lines.push(format!(
                "{label} dialog #{dialog_index} element #{element_index} kind={:?}{} text={} frame={} local={}{}{}{}",
                element.kind,
                display_optional_i32(" id", element.view_id),
                display_quoted_text(&element.text),
                display_text_element_screen_frame(&absolute, height),
                display_text_element_local_frame(element),
                display_optional_u32_hex(" textColor", element.text_color),
                display_optional_u32_hex(" backgroundColor", element.background_color),
                display_activity_image(element.image.as_ref())
            ));
        }

        for item_frame in activity_dialog_item_frames(width, height, dialog_index, dialog) {
            lines.push(format!(
                "{label} dialog #{dialog_index} item #{} enabled={} selected={} text={} frame={}",
                item_frame.item_index,
                item_frame.item.enabled,
                item_frame.item.selected,
                display_quoted_text(&item_frame.item.text),
                display_activity_rect_screen_frame(item_frame.frame, height)
            ));
        }

        for button_frame in activity_dialog_button_frames(width, height, dialog_index, dialog) {
            lines.push(format!(
                "{label} dialog #{dialog_index} button #{} enabled={} text={} frame={}",
                button_frame.button_index,
                button_frame.button.enabled,
                display_quoted_text(&button_frame.button.text),
                display_activity_rect_screen_frame(button_frame.frame, height)
            ));
        }
    }

    lines
}

pub fn activity_surface_ui_dump(
    width: u32,
    height: u32,
    package: &str,
    surface: &ActivitySurfaceState,
) -> String {
    let root = build_activity_surface_dump_tree(width, height, surface);
    let mut output = String::from(
        "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>\n<hierarchy rotation=\"0\">\n",
    );
    write_ui_dump_node(&mut output, &root, 0, 0, package, 1);
    output.push_str("</hierarchy>\n");
    output
}

pub(crate) fn build_activity_surface_dump_tree(
    width: u32,
    height: u32,
    surface: &ActivitySurfaceState,
) -> UiDumpNode {
    let names = &surface.resource_id_names;
    let mut root = UiDumpNode::new("android.widget.FrameLayout", 0, 0, width, height);

    if let Some((dialog_index, dialog)) = surface.dialogs.iter().enumerate().last() {
        root.children
            .push(dialog_dump_node(width, height, dialog_index, dialog, names));
        return root;
    }

    if let Some(overlay) = surface.overlays.last() {
        root.children.push(overlay_dump_node(
            width,
            height,
            &surface.text_elements,
            overlay,
            names,
        ));
        return root;
    }

    if let Some(action_bar) = &surface.action_bar {
        root.children
            .push(action_bar_dump_node(width, action_bar, names));
    }

    for element in &surface.text_elements {
        root.children
            .push(text_element_dump_node(element, height, names));
    }

    root
}

pub(crate) struct UiDumpNode {
    text: String,
    resource_id: String,
    class_name: &'static str,
    content_desc: String,
    checkable: bool,
    checked: bool,
    clickable: bool,
    enabled: bool,
    focusable: bool,
    focused: bool,
    scrollable: bool,
    long_clickable: bool,
    password: bool,
    selected: bool,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    children: Vec<UiDumpNode>,
}

impl UiDumpNode {
    fn new(class_name: &'static str, left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            text: String::new(),
            resource_id: String::new(),
            class_name,
            content_desc: String::new(),
            checkable: false,
            checked: false,
            clickable: false,
            enabled: true,
            focusable: false,
            focused: false,
            scrollable: false,
            long_clickable: false,
            password: false,
            selected: false,
            left,
            top,
            right,
            bottom,
            children: Vec::new(),
        }
    }
}

fn write_ui_dump_node(
    output: &mut String,
    node: &UiDumpNode,
    index: usize,
    drawing_order: usize,
    package: &str,
    depth: usize,
) {
    let indent = "  ".repeat(depth);
    output.push_str(&format!(
        "{indent}<node index=\"{index}\" text=\"{}\" resource-id=\"{}\" class=\"{}\" package=\"{}\" content-desc=\"{}\" checkable=\"{}\" checked=\"{}\" clickable=\"{}\" enabled=\"{}\" focusable=\"{}\" focused=\"{}\" scrollable=\"{}\" long-clickable=\"{}\" password=\"{}\" selected=\"{}\" bounds=\"[{},{}][{},{}]\" drawing-order=\"{drawing_order}\" hint=\"\"",
        xml_escape(&node.text),
        xml_escape(&node.resource_id),
        node.class_name,
        xml_escape(package),
        xml_escape(&node.content_desc),
        node.checkable,
        node.checked,
        node.clickable,
        node.enabled,
        node.focusable,
        node.focused,
        node.scrollable,
        node.long_clickable,
        node.password,
        node.selected,
        node.left,
        node.top,
        node.right,
        node.bottom,
    ));

    if node.children.is_empty() {
        output.push_str("/>\n");
        return;
    }

    output.push_str(">\n");
    for (child_index, child) in node.children.iter().enumerate() {
        write_ui_dump_node(
            output,
            child,
            child_index,
            child_index + 1,
            package,
            depth + 1,
        );
    }
    output.push_str(&format!("{indent}</node>\n"));
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn dump_resource_id_name(
    names: &std::collections::BTreeMap<i32, String>,
    view_id: Option<i32>,
) -> String {
    let Some(view_id) = view_id else {
        return String::new();
    };
    if view_id == ANDROID_R_ID_HOME {
        return "android:id/home".to_owned();
    }
    names
        .get(&view_id)
        .cloned()
        .unwrap_or_else(|| format!("0x{:08x}", view_id as u32))
}

fn text_element_dump_node(
    element: &ActivityTextElement,
    surface_height: u32,
    names: &std::collections::BTreeMap<i32, String>,
) -> UiDumpNode {
    let top = text_element_screen_top(element, surface_height);
    let class_name = if element.editable {
        "android.widget.EditText"
    } else {
        match element.kind {
            ActivityTextKind::Button => {
                if element.text.is_empty() && element.image.is_some() {
                    "android.widget.ImageButton"
                } else {
                    "android.widget.Button"
                }
            }
            ActivityTextKind::ListItem => "android.widget.TextView",
            ActivityTextKind::RuntimeViewRoot => "android.view.View",
            ActivityTextKind::Label => {
                if element.text.is_empty() && element.image.is_some() {
                    "android.widget.ImageView"
                } else if element.text.is_empty() {
                    "android.view.View"
                } else {
                    "android.widget.TextView"
                }
            }
        }
    };

    let mut node = UiDumpNode::new(
        class_name,
        element.x,
        top,
        element.x.saturating_add(element.width),
        top.saturating_add(element.height),
    );
    node.text = element.text.clone();
    node.resource_id = dump_resource_id_name(names, element.view_id);
    node.clickable = element.editable
        || element.preference_key.is_some()
        || text_element_view_click_target(element).is_some();
    node.focusable = node.clickable;
    node.focused = element.cursor.is_some();
    node.scrollable = element.clip.is_some();
    if let Some(ActivityTextAccessory::Switch { checked }) = element.accessory {
        node.checkable = true;
        node.checked = checked;
    }
    node
}

fn action_bar_dump_node(
    width: u32,
    action_bar: &ActivityActionBar,
    names: &std::collections::BTreeMap<i32, String>,
) -> UiDumpNode {
    let mut bar = UiDumpNode::new("android.view.ViewGroup", 0, 0, width, ACTION_BAR_HEIGHT);
    let title_left = if action_bar.navigation_icon.is_some() || action_bar.navigation_is_up {
        let mut home = UiDumpNode::new("android.widget.ImageButton", 0, 0, 56, ACTION_BAR_HEIGHT);
        home.resource_id = dump_resource_id_name(names, Some(ANDROID_R_ID_HOME));
        home.clickable = true;
        home.focusable = true;
        bar.children.push(home);
        56
    } else {
        0
    };

    let slot = 48_u32;
    let mut right = width;
    if action_bar.items.iter().any(|item| item.overflow) {
        right = right.saturating_sub(slot);
        let mut overflow = UiDumpNode::new(
            "android.widget.ImageButton",
            right,
            0,
            right + slot,
            ACTION_BAR_HEIGHT,
        );
        overflow.resource_id = dump_resource_id_name(names, Some(ACTION_BAR_OVERFLOW_TOGGLE_ID));
        overflow.clickable = true;
        overflow.focusable = true;
        bar.children.push(overflow);
    }
    for item in action_bar.items.iter().filter(|item| item.image.is_some()) {
        right = right.saturating_sub(slot);
        let mut action = UiDumpNode::new(
            "android.widget.ImageButton",
            right,
            0,
            right + slot,
            ACTION_BAR_HEIGHT,
        );
        action.resource_id = dump_resource_id_name(names, Some(item.item_id));
        action.content_desc = item.title.clone();
        action.clickable = true;
        action.focusable = true;
        action.enabled = item.enabled;
        bar.children.push(action);
    }

    let mut title = UiDumpNode::new(
        "android.widget.TextView",
        title_left,
        0,
        right,
        ACTION_BAR_HEIGHT,
    );
    title.text = action_bar.title.clone();
    bar.children.insert(0, title);
    bar
}

fn overlay_dump_node(
    width: u32,
    height: u32,
    text_elements: &[ActivityTextElement],
    overlay: &ActivityOverlay,
    names: &std::collections::BTreeMap<i32, String>,
) -> UiDumpNode {
    let frames = activity_overlay_item_frames(width, height, text_elements, overlay);
    let mut list = UiDumpNode::new("android.widget.ListView", 0, 0, 0, 0);

    for item_frame in &frames {
        let top = activity_rect_screen_top(item_frame.frame, height);
        let mut item = UiDumpNode::new(
            "android.widget.TextView",
            item_frame.frame.x,
            top,
            item_frame.frame.x.saturating_add(item_frame.frame.width),
            top.saturating_add(item_frame.frame.height),
        );
        item.text = item_frame.item.text.clone();
        item.resource_id = dump_resource_id_name(names, Some(item_frame.item.item_id));
        item.enabled = item_frame.item.enabled;
        item.clickable = item_frame.item.enabled;
        item.focusable = item_frame.item.enabled;
        list.children.push(item);
    }

    if let (Some(first), Some(last)) = (list.children.first(), list.children.last()) {
        list.left = first.left;
        list.top = first.top;
        list.right = first.right.max(last.right);
        list.bottom = last.bottom;
    }
    list
}

fn dialog_dump_node(
    width: u32,
    height: u32,
    dialog_index: usize,
    dialog: &ActivityDialogOverlay,
    names: &std::collections::BTreeMap<i32, String>,
) -> UiDumpNode {
    let frame = activity_dialog_frame(width, height, dialog);
    let top = activity_rect_screen_top(frame, height);
    let mut node = UiDumpNode::new(
        "android.widget.FrameLayout",
        frame.x,
        top,
        frame.x.saturating_add(frame.width),
        top.saturating_add(frame.height),
    );

    for element in &dialog.text_elements {
        let offset = offset_dialog_trace_element(element, frame.x, frame.y);
        node.children
            .push(text_element_dump_node(&offset, height, names));
    }

    for item_frame in activity_dialog_item_frames(width, height, dialog_index, dialog) {
        let item_top = activity_rect_screen_top(item_frame.frame, height);
        let mut item = UiDumpNode::new(
            "android.widget.TextView",
            item_frame.frame.x,
            item_top,
            item_frame.frame.x.saturating_add(item_frame.frame.width),
            item_top.saturating_add(item_frame.frame.height),
        );
        item.text = item_frame.item.text.clone();
        item.enabled = item_frame.item.enabled;
        item.clickable = item_frame.item.enabled;
        item.focusable = item_frame.item.enabled;
        item.selected = item_frame.item.selected;
        node.children.push(item);
    }

    for button_frame in activity_dialog_button_frames(width, height, dialog_index, dialog) {
        let button_top = activity_rect_screen_top(button_frame.frame, height);
        let mut button = UiDumpNode::new(
            "android.widget.Button",
            button_frame.frame.x,
            button_top,
            button_frame
                .frame
                .x
                .saturating_add(button_frame.frame.width),
            button_top.saturating_add(button_frame.frame.height),
        );
        button.text = button_frame.button.text.clone();
        button.enabled = button_frame.button.enabled;
        button.clickable = button_frame.button.enabled;
        button.focusable = button_frame.button.enabled;
        node.children.push(button);
    }

    node
}

fn display_text_element_screen_frame(element: &ActivityTextElement, surface_height: u32) -> String {
    let top = text_element_screen_top(element, surface_height);
    format!("{}x{}+{}+{}", element.width, element.height, element.x, top)
}

fn display_text_element_local_frame(element: &ActivityTextElement) -> String {
    format!(
        "{}x{}+{}+{}",
        element.width, element.height, element.x, element.y
    )
}

fn display_activity_rect_screen_frame(rect: ActivityRect, surface_height: u32) -> String {
    let top = activity_rect_screen_top(rect, surface_height);
    format!("{}x{}+{}+{}", rect.width, rect.height, rect.x, top)
}

fn display_optional_rect(prefix: &str, rect: Option<ActivityRect>, surface_height: u32) -> String {
    rect.map(|rect| {
        format!(
            "{prefix}={}",
            display_activity_rect_screen_frame(rect, surface_height)
        )
    })
    .unwrap_or_default()
}

fn text_element_screen_top(element: &ActivityTextElement, surface_height: u32) -> u32 {
    let top = i64::from(surface_height)
        .saturating_sub(i64::from(element.y))
        .saturating_sub(i64::from(element.height));

    if top <= 0 {
        return 0;
    }

    u32::try_from(top).unwrap_or(u32::MAX)
}

fn activity_rect_screen_top(rect: ActivityRect, surface_height: u32) -> u32 {
    surface_height.saturating_sub(rect.y.saturating_add(rect.height))
}

fn offset_dialog_trace_element(
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

fn display_optional_i32(prefix: &str, value: Option<i32>) -> String {
    value
        .map(|value| format!("{prefix}={value}"))
        .unwrap_or_default()
}

fn display_optional_string(prefix: &str, value: Option<&str>) -> String {
    value
        .map(|value| format!("{prefix}={value:?}"))
        .unwrap_or_default()
}

fn display_optional_u32_hex(prefix: &str, value: Option<u32>) -> String {
    value
        .map(|value| format!("{prefix}=0x{value:08x}"))
        .unwrap_or_default()
}

fn display_activity_image(image: Option<&ActivityImage>) -> String {
    let Some(image) = image else {
        return String::new();
    };

    let tint = display_optional_u32_hex(" tint", image.tint_color);
    format!(
        " image={:?} bytes={}{} paddingAfter={}",
        image.source,
        image.data.len(),
        tint,
        image.padding_after
    )
}

fn display_text_cursor(cursor: Option<&ActivityTextCursor>) -> String {
    cursor
        .map(|cursor| {
            format!(
                " cursor={}..{} visible={}",
                cursor.selection_start, cursor.selection_end, cursor.visible
            )
        })
        .unwrap_or_default()
}

fn display_quoted_text(text: &str) -> String {
    let mut value = text.replace('\n', "\\n");
    if value.chars().count() > 80 {
        value = value.chars().take(77).collect::<String>();
        value.push_str("...");
    }

    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_dump_matches_the_api36_uiautomator_hierarchy_recorded_for_open_calculator() {
        let element = ActivityTextElement {
            text: "7".to_owned(),
            kind: ActivityTextKind::Button,
            view_id: Some(0x7f0901e9),
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
            y: 303,
            width: 101,
            height: 101,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };
        let surface = ActivitySurfaceState {
            message: String::new(),
            text_elements: vec![element],
            runtime_canvas_frames: Vec::new(),
            overlays: Vec::new(),
            dialogs: Vec::new(),
            action_bar: None,
            floating_panel: None,
            window_background: None,
            resource_id_names: std::collections::BTreeMap::from([(
                0x7f0901e9,
                "com.darkempire78.opencalculator:id/sevenButton".to_owned(),
            )]),
        };

        let dump = activity_surface_ui_dump(420, 760, "com.darkempire78.opencalculator", &surface);

        assert!(dump.starts_with(
            "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>\n<hierarchy rotation=\"0\">"
        ));
        assert!(dump.contains(
            "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.FrameLayout\" \
             package=\"com.darkempire78.opencalculator\" content-desc=\"\" checkable=\"false\" \
             checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"false\" \
             focused=\"false\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" \
             selected=\"false\" bounds=\"[0,0][420,760]\" drawing-order=\"0\" hint=\"\">"
        ));
        assert!(dump.contains(
            "<node index=\"0\" text=\"7\" \
             resource-id=\"com.darkempire78.opencalculator:id/sevenButton\" \
             class=\"android.widget.Button\" package=\"com.darkempire78.opencalculator\" \
             content-desc=\"\" checkable=\"false\" checked=\"false\" clickable=\"true\" \
             enabled=\"true\" focusable=\"true\" focused=\"false\" scrollable=\"false\" \
             long-clickable=\"false\" password=\"false\" selected=\"false\" \
             bounds=\"[8,356][109,457]\" drawing-order=\"1\" hint=\"\"/>"
        ));
        assert!(dump.ends_with("</hierarchy>\n"));
        assert!(surface.text_elements[0].contains_host_point(58, 760 - 406));
    }

    #[test]
    fn action_bar_overflow_dropdown_is_pinned_below_the_bar_at_the_right() {
        let overlay = ActivityOverlay {
            anchor_view_id: Some(ACTION_BAR_OVERFLOW_TOGGLE_ID),
            items: vec![ActivityOverlayItem {
                item_id: 7,
                text: "Update".to_owned(),
                enabled: true,
            }],
        };
        let frame = activity_overlay_frame(420, 760, &[], &overlay);

        assert_eq!(frame.x + frame.width, 420 - ACTIVITY_OVERLAY_MARGIN);
        assert_eq!(frame.y + frame.height, 760 - ACTION_BAR_HEIGHT);
    }

    #[test]
    fn nav_anchored_menu_drops_down_below_the_bar_at_the_left() {
        let overlay = ActivityOverlay {
            anchor_view_id: Some(ANDROID_R_ID_HOME),
            items: vec![
                ActivityOverlayItem {
                    item_id: 1,
                    text: "Help".to_owned(),
                    enabled: true,
                },
                ActivityOverlayItem {
                    item_id: 2,
                    text: "Settings".to_owned(),
                    enabled: true,
                },
            ],
        };
        let frame = activity_overlay_frame(420, 760, &[], &overlay);

        assert_eq!(frame.x, ACTIVITY_OVERLAY_MARGIN);
        assert_eq!(frame.y + frame.height, 760 - ACTION_BAR_HEIGHT);
    }

    #[test]
    fn builds_activity_window_request() {
        let request = ActivityWindowRequest::new(
            "com.example",
            "com.example.MainActivity",
            crate::config::DisplaySize::new(360, 800).unwrap(),
        );

        assert_eq!(request.package_name, "com.example");
        assert_eq!(request.activity_name, "com.example.MainActivity");
        assert_eq!(request.title, "com.example - com.example.MainActivity");
        assert_eq!(request.width, 360);
        assert_eq!(request.height, 800);
        assert!(request.detail_lines.is_empty());
        assert!(request.text_elements.is_empty());
        assert!(request.overlays.is_empty());
        assert!(request.dialogs.is_empty());
    }

    #[test]
    fn stores_activity_text_elements() {
        let request = ActivityWindowRequest::new(
            "com.example",
            "com.example.MainActivity",
            crate::config::DisplaySize::default(),
        )
        .with_text_elements(vec![ActivityTextElement {
            text: "Accept".to_owned(),
            kind: ActivityTextKind::Label,
            view_id: Some(2131296342),
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
            y: 32,
            width: 100,
            height: 24,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        }]);

        assert_eq!(
            request.text_elements,
            vec![ActivityTextElement {
                text: "Accept".to_owned(),
                kind: ActivityTextKind::Label,
                view_id: Some(2131296342),
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
                y: 32,
                width: 100,
                height: 24,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn stores_activity_window_detail_lines() {
        let request = ActivityWindowRequest::new(
            "com.example",
            "com.example.MainActivity",
            crate::config::DisplaySize::default(),
        )
        .with_detail_lines(vec!["line one".to_owned(), "line two".to_owned()]);

        assert_eq!(
            request.detail_lines,
            vec!["line one".to_owned(), "line two".to_owned()]
        );
    }

    #[test]
    fn stores_activity_overlays() {
        let mut request = ActivityWindowRequest::new(
            "com.example",
            "com.example.MainActivity",
            crate::config::DisplaySize::default(),
        );
        request.overlays = vec![ActivityOverlay {
            anchor_view_id: Some(2131296591),
            items: vec![ActivityOverlayItem {
                item_id: 2131296592,
                text: "Settings".to_owned(),
                enabled: true,
            }],
        }];

        assert_eq!(
            request.overlays,
            vec![ActivityOverlay {
                anchor_view_id: Some(2131296591),
                items: vec![ActivityOverlayItem {
                    item_id: 2131296592,
                    text: "Settings".to_owned(),
                    enabled: true,
                }],
            }]
        );
    }

    #[test]
    fn stores_activity_surface_state() {
        let surface = ActivitySurfaceState {
            message: "active Activity".to_owned(),
            text_elements: vec![ActivityTextElement {
                text: "Settings".to_owned(),
                kind: ActivityTextKind::Label,
                view_id: Some(2131296357),
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
                y: 16,
                width: 120,
                height: 32,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            runtime_canvas_frames: Vec::new(),
            overlays: Vec::new(),
            dialogs: Vec::new(),
            action_bar: None,
            floating_panel: None,
            window_background: None,
            resource_id_names: std::collections::BTreeMap::new(),
        };

        assert_eq!(surface.message, "active Activity");
        assert_eq!(surface.text_elements.len(), 1);
        assert!(surface.overlays.is_empty());
        assert!(surface.dialogs.is_empty());
    }

    #[test]
    fn overlay_item_frames_follow_anchor() {
        let elements = vec![ActivityTextElement {
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
            x: 368,
            y: 8,
            width: 44,
            height: 44,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        }];
        let overlay = ActivityOverlay {
            anchor_view_id: Some(2131296591),
            items: vec![
                ActivityOverlayItem {
                    item_id: 1,
                    text: "Settings".to_owned(),
                    enabled: true,
                },
                ActivityOverlayItem {
                    item_id: 2,
                    text: "About".to_owned(),
                    enabled: true,
                },
            ],
        };
        let frames = activity_overlay_item_frames(420, 760, &elements, &overlay);

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].frame.x, 192);
        assert_eq!(frames[0].frame.y, 56);
        assert_eq!(frames[0].frame.width, 220);
        assert_eq!(frames[0].frame.height, ACTIVITY_OVERLAY_ITEM_HEIGHT);
        assert_eq!(frames[1].frame.y, 104);
    }

    #[test]
    fn dialog_button_frames_use_dialog_bottom_edge() {
        let dialog = ActivityDialogOverlay {
            title: Some("Support Us".to_owned()),
            content_layouts: vec![2131492910],
            text_elements: Vec::new(),
            items: Vec::new(),
            item_scroll_offset: 0,
            buttons: vec![ActivityDialogButton {
                text: "Close".to_owned(),
                enabled: true,
            }],
        };
        let dialog_frame = activity_dialog_frame(420, 760, &dialog);
        let frames = activity_dialog_button_frames(420, 760, 0, &dialog);

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].dialog_index, 0);
        assert_eq!(frames[0].button_index, 0);
        assert!(frames[0].frame.x >= dialog_frame.x);
        assert!(frames[0].frame.y >= dialog_frame.y);
        assert!(
            frames[0].frame.x.saturating_add(frames[0].frame.width)
                <= dialog_frame.x.saturating_add(dialog_frame.width)
        );
    }

    #[test]
    fn dialog_item_frames_fill_dialog_content_from_top() {
        let dialog = ActivityDialogOverlay {
            title: Some("Number of decimal places".to_owned()),
            content_layouts: Vec::new(),
            text_elements: Vec::new(),
            items: vec![
                ActivityDialogItem {
                    text: "10".to_owned(),
                    enabled: true,
                    selected: true,
                },
                ActivityDialogItem {
                    text: "50".to_owned(),
                    enabled: true,
                    selected: false,
                },
            ],
            item_scroll_offset: 0,
            buttons: Vec::new(),
        };
        let content = activity_dialog_content_rect(420, 760, &dialog);
        let frames = activity_dialog_item_frames(420, 760, 0, &dialog);

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].dialog_index, 0);
        assert_eq!(frames[0].item_index, 0);
        assert_eq!(frames[0].item.text, "10");
        assert_eq!(frames[0].frame.x, content.x);
        assert_eq!(frames[0].frame.width, content.width);
        assert_eq!(frames[0].frame.height, ACTIVITY_DIALOG_ITEM_HEIGHT);
        assert_eq!(
            frames[0].frame.y.saturating_add(frames[0].frame.height),
            content.y.saturating_add(content.height)
        );
        assert_eq!(
            frames[1].frame.y.saturating_add(frames[1].frame.height),
            frames[0].frame.y
        );
    }

    #[test]
    fn text_element_hit_test_uses_stable_frame() {
        let element = ActivityTextElement {
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
            y: 101,
            width: 101,
            height: 44,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert!(element.contains_host_point(8, 101));
        assert!(element.contains_host_point(108, 144));
        assert!(!element.contains_host_point(109, 144));
        assert!(!element.contains_host_point(108, 145));
    }

    #[test]
    fn text_element_hit_test_respects_clip_rect() {
        let element = ActivityTextElement {
            text: "Language".to_owned(),
            kind: ActivityTextKind::ListItem,
            view_id: None,
            preference_key: Some("language".to_owned()),
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
            y: 620,
            width: 320,
            height: 58,
            clip: Some(ActivityRect {
                x: 0,
                y: 28,
                width: 420,
                height: 600,
            }),
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert!(element.contains_host_point(40, 624));
        assert!(!element.contains_host_point(40, 650));
    }

    #[test]
    fn text_element_view_click_target_accepts_clickable_labels() {
        let element = ActivityTextElement {
            text: String::new(),
            kind: ActivityTextKind::Label,
            view_id: Some(2131296272),
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
            x: 30,
            y: 660,
            width: 44,
            height: 44,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert_eq!(text_element_view_click_target(&element), Some(2131296272));
    }

    #[test]
    fn text_element_view_click_target_skips_background_only_elements() {
        let element = ActivityTextElement {
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
            background_color: Some(0xff15_1b2c),
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
        };

        assert_eq!(text_element_view_click_target(&element), None);
    }

    #[test]
    fn text_element_view_click_target_skips_preference_rows() {
        let element = ActivityTextElement {
            text: "Language".to_owned(),
            kind: ActivityTextKind::ListItem,
            view_id: Some(2131296272),
            preference_key: Some("language".to_owned()),
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
            x: 30,
            y: 660,
            width: 300,
            height: 44,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert_eq!(text_element_view_click_target(&element), None);
    }

    #[test]
    fn text_element_view_click_target_accepts_view_backed_list_rows() {
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
            y: 560,
            width: 372,
            height: 48,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        };

        assert_eq!(text_element_view_click_target(&element), Some(2131296277));
    }

    #[test]
    fn activity_surface_bound_lines_include_elements_overlays_and_dialogs() {
        let surface = ActivitySurfaceState {
            message: "activity".to_owned(),
            text_elements: vec![ActivityTextElement {
                text: "Menu".to_owned(),
                kind: ActivityTextKind::Button,
                view_id: Some(10),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: Some(0xffef_efef),
                background_color: Some(0xff15_1b2c),
                alignment: ActivityTextAlignment::Center,
                editable: false,
                cursor: None,
                x: 8,
                y: 156,
                width: 80,
                height: 44,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
            runtime_canvas_frames: Vec::new(),
            overlays: vec![ActivityOverlay {
                anchor_view_id: Some(10),
                items: vec![ActivityOverlayItem {
                    item_id: 20,
                    text: "Settings".to_owned(),
                    enabled: true,
                }],
            }],
            dialogs: vec![ActivityDialogOverlay {
                title: Some("About".to_owned()),
                content_layouts: vec![30],
                text_elements: vec![ActivityTextElement {
                    text: "Open source".to_owned(),
                    kind: ActivityTextKind::Label,
                    view_id: Some(31),
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
                    width: 120,
                    height: 32,
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                }],
                items: Vec::new(),
                item_scroll_offset: 0,
                buttons: vec![ActivityDialogButton {
                    text: "Close".to_owned(),
                    enabled: true,
                }],
            }],
            action_bar: None,
            floating_panel: None,
            window_background: None,
            resource_id_names: std::collections::BTreeMap::new(),
        };

        let lines = activity_surface_bound_lines("after-click", 200, 240, &surface);

        assert!(lines
            .iter()
            .any(|line| { line.contains("after-click surface frame=200x240+0+0 elements=1") }));
        assert!(lines.iter().any(|line| {
            line.contains("element #0")
                && line.contains("id=10")
                && line.contains("frame=80x44+8+40")
                && line.contains("backgroundColor=0xff151b2c")
        }));
        assert!(lines
            .iter()
            .any(|line| { line.contains("overlay #0 anchor=10") && line.contains("items=1") }));
        assert!(lines.iter().any(|line| {
            line.contains("overlay #0 item id=20") && line.contains("\"Settings\"")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("dialog #0 title=\"About\"")
                && line.contains("elements=1")
                && line.contains("buttons=1")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("dialog #0 element #0")
                && line.contains("local=120x32+0+40")
                && line.contains("\"Open source\"")
        }));
        assert!(lines
            .iter()
            .any(|line| { line.contains("dialog #0 button #0") && line.contains("\"Close\"") }));
    }
}

#[cfg(not(target_os = "emscripten"))]
mod desktop;

#[cfg(target_os = "emscripten")]
mod emscripten;
