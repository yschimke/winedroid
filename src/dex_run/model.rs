use super::*;

pub struct DexActivityStart {
    pub class_descriptor: String,
    pub method_signature: String,
    pub result: String,
    pub view_snapshot: DexViewSnapshot,
    pub resources: Option<Vec<u8>>,
}

pub struct DexActivityRuntime {
    pub(in crate::dex_run) program: DexProgram,
    pub(in crate::dex_run) class_descriptor: String,
    pub(in crate::dex_run) method_signature: String,
    pub(in crate::dex_run) result: String,
    pub(in crate::dex_run) last_view_snapshot: std::cell::RefCell<Option<DexViewSnapshot>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DexUiPumpResult {
    pub dispatched: usize,
    pub view_snapshot_changed: bool,
    pub pending_non_frame_work: bool,
    pub pending_main_work: bool,
    pub work_budget_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DexHostAction {
    OpenBrowser { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::dex_run) struct RuntimeActivityStart {
    pub(in crate::dex_run) class_descriptor: String,
    pub(in crate::dex_run) method_signature: String,
    pub(in crate::dex_run) result: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DexViewSnapshot {
    pub content_layouts: Vec<i32>,
    pub inflated_layouts: Vec<i32>,
    pub theme_styles: Vec<DexThemeStyle>,
    pub resource_sdk_version: u16,
    pub resource_ui_mode: i32,
    pub resource_display_width: u16,
    pub resource_display_height: u16,
    pub resource_screen_round: bool,
    pub runtime_view_roots: Vec<DexRuntimeViewRoot>,
    pub runtime_canvas_frames: Vec<DexRuntimeCanvasFrame>,
    pub view_lookup_ids: Vec<i32>,
    pub view_texts: Vec<DexViewText>,
    pub view_text_cursors: Vec<DexViewTextCursor>,
    pub view_images: Vec<DexViewImage>,
    pub view_visibilities: Vec<DexViewVisibility>,
    pub click_listeners: Vec<DexViewClickListener>,
    pub declared_on_click_handlers: Vec<DexDeclaredOnClickHandler>,
    pub adapter_rows: Vec<DexAdapterRows>,
    pub dynamic_children: Vec<DexDynamicViewGroup>,
    pub activity_overlays: Vec<DexActivityOverlay>,
    pub dialog_overlays: Vec<DexDialogOverlay>,
    pub action_bar_title: Option<String>,
    pub action_bar_nav_icon: Option<u32>,
    pub action_bar_overflow_open: bool,
    pub action_bar_present: bool,
    pub action_bar_home_as_up: bool,
    pub options_menu: Vec<DexMenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexMenuItem {
    pub item_id: i32,
    pub title: String,
    pub icon_resource: Option<u32>,
    pub show_as_action: i32,
    pub enabled: bool,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexAdapterRows {
    pub adapter_view_id: i32,
    pub item_layout_id: i32,
    pub rows: Vec<DexAdapterRow>,
}

// cite: frameworks-base ViewGroup.java#addView
#[derive(Debug, Clone, PartialEq)]
pub struct DexDynamicViewGroup {
    pub container_view_id: u32,
    pub children: Vec<DexDynamicView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DexDynamicView {
    pub tag_name: String,
    pub text: Option<String>,
    pub background_color: Option<u32>,
    pub drawable_resource_id: Option<u32>,
    pub visibility: i32,
    // cite: frameworks-base View.java#setMinimumWidth, setMinimumHeight
    pub explicit_width: Option<u32>,
    pub explicit_height: Option<u32>,
    // cite: frameworks-base View.java#setOnClickListener
    pub click_id: Option<i32>,
    pub children: Vec<DexDynamicView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DexAdapterRow {
    pub click_id: i32,
    pub texts: Vec<DexViewText>,
    pub images: Vec<DexViewImage>,
    pub visibilities: Vec<DexViewVisibility>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DexThemeStyle {
    pub resource_id: u32,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexRuntimeViewRoot {
    pub view_key: String,
    pub class_descriptor: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DexRuntimeCanvasFrame {
    pub root_view_key: String,
    pub class_descriptor: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub ops: Vec<DexRuntimeCanvasOp>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DexRuntimeCanvasPathOp {
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
        left: Vec<DexRuntimeCanvasPathOp>,
        right: Vec<DexRuntimeCanvasPathOp>,
        op: DexRuntimeCanvasPathBooleanOp,
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
pub enum DexRuntimeCanvasPathBooleanOp {
    Difference,
    Intersect,
    Union,
    Xor,
    ReverseDifference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexRuntimeCanvasClipOp {
    Intersect,
    Difference,
}

// cite: frameworks-base BlendMode.java#CLEAR, SRC, DST, SRC_OVER, DST_OVER, SRC_IN, DST_IN, SRC_OUT, DST_OUT, SRC_ATOP, DST_ATOP, XOR, PLUS, MODULATE, SCREEN, OVERLAY, DARKEN, LIGHTEN, COLOR_DODGE, COLOR_BURN, HARD_LIGHT, SOFT_LIGHT, DIFFERENCE, EXCLUSION, MULTIPLY, HUE, SATURATION, COLOR, LUMINOSITY
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexRuntimeCanvasBlendMode {
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
pub struct DexRuntimeCanvasColorFilter {
    pub color: u32,
    pub blend_mode: DexRuntimeCanvasBlendMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexRuntimeCanvasPaintStyle {
    Fill,
    Stroke,
    FillAndStroke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexRuntimeCanvasStrokeCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DexRuntimeCanvasOp {
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
        op: DexRuntimeCanvasClipOp,
    },
    ClipPath {
        ops: Vec<DexRuntimeCanvasPathOp>,
        op: DexRuntimeCanvasClipOp,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: u32,
        blend_mode: DexRuntimeCanvasBlendMode,
        stroke_width: f32,
        stroke_cap: DexRuntimeCanvasStrokeCap,
    },
    Rect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        color: u32,
        blend_mode: DexRuntimeCanvasBlendMode,
        paint_style: DexRuntimeCanvasPaintStyle,
    },
    Circle {
        cx: f32,
        cy: f32,
        radius: f32,
        color: u32,
        blend_mode: DexRuntimeCanvasBlendMode,
        paint_style: DexRuntimeCanvasPaintStyle,
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
        blend_mode: DexRuntimeCanvasBlendMode,
        paint_style: DexRuntimeCanvasPaintStyle,
    },
    Path {
        ops: Vec<DexRuntimeCanvasPathOp>,
        color: u32,
        blend_mode: DexRuntimeCanvasBlendMode,
        paint_style: DexRuntimeCanvasPaintStyle,
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
        blend_mode: DexRuntimeCanvasBlendMode,
        color_filter: Option<DexRuntimeCanvasColorFilter>,
        alpha_mask: bool,
        encoded_image: Option<Vec<u8>>,
        ops: Vec<DexRuntimeCanvasOp>,
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
        ops: Vec<DexRuntimeCanvasOp>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexViewText {
    pub view_id: i32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexViewTextCursor {
    pub view_id: i32,
    pub selection_start: i32,
    pub selection_end: i32,
    pub cursor_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexViewImage {
    pub view_id: i32,
    pub drawable_resource_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexViewVisibility {
    pub view_id: i32,
    pub visibility: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexViewClickListener {
    pub view_id: i32,
    pub listener: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexDeclaredOnClickHandler {
    pub view_id: i32,
    pub method_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexPreferenceSnapshot {
    pub runtime_key: String,
    pub key: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub icon_resource_id: Option<u32>,
    pub class_descriptor: String,
    pub kind: DexPreferenceKind,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DexPreferenceKind {
    Category,
    Switch {
        checked: bool,
    },
    List {
        value: Option<String>,
        entries: Vec<String>,
        entry_values: Vec<String>,
    },
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexActivityOverlay {
    pub anchor_view_id: Option<i32>,
    pub items: Vec<DexActivityOverlayItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexActivityOverlayItem {
    pub item_id: i32,
    pub text: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexDialogOverlay {
    pub title: Option<String>,
    pub content_layouts: Vec<i32>,
    pub items: Vec<DexDialogItem>,
    pub buttons: Vec<DexDialogButton>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexDialogItem {
    pub text: String,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexDialogButton {
    pub text: String,
    pub enabled: bool,
}
