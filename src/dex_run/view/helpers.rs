use super::*;

pub(in crate::dex_run) fn text_view_get_paint(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {
            Some(DexValue::Reference(std::rc::Rc::from(format!(
                "Landroid/text/TextPaint;:{}",
                map_identity_key(&invocation.arguments[0])
            ))))
        }
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run) fn view_window_token(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {
            Some(DexValue::Reference(std::rc::Rc::from(format!(
                "Landroid/os/IBinder;:window-token:{}",
                map_identity_key(&invocation.arguments[0])
            ))))
        }
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run) fn view_reference(view_id: i32) -> DexValue {
    // cite: frameworks-base PhoneWindow.java#generateLayout, ID_ANDROID_CONTENT
    // cite: frameworks-base screen.xml#@android:id/content
    // cite: frameworks-base screen_action_bar.xml#@android:id/content
    // cite: frameworks-base screen_simple.xml#@android:id/content
    // cite: frameworks-base screen_toolbar.xml#@android:id/content
    let descriptor = if view_id == ANDROID_CONTENT_VIEW_ID {
        "Landroid/widget/FrameLayout;"
    } else {
        "Landroid/view/View;"
    };
    DexValue::Reference(std::rc::Rc::from(format!("{descriptor}:id/{view_id}")))
}
pub(in crate::dex_run::view) fn is_decor_view(value: &DexValue) -> bool {
    matches!(value, DexValue::Reference(reference) if reference.as_ref() == "Lcom/android/internal/policy/DecorView;.decor")
}

// cite: frameworks-base PhoneWindow.java#getDecorView, installDecor, generateDecor
// cite: frameworks-base DecorView.java#DecorView
pub(in crate::dex_run) fn decor_view_reference() -> DexValue {
    DexValue::Reference(std::rc::Rc::from(
        "Lcom/android/internal/policy/DecorView;.decor",
    ))
}

pub(in crate::dex_run) fn decor_action_bar_reference() -> DexValue {
    DexValue::Reference(std::rc::Rc::from(
        "Landroid/widget/Toolbar;:window/action-bar",
    ))
}

pub(in crate::dex_run) fn dex_value_is_object_like(value: &DexValue) -> bool {
    matches!(value, DexValue::Object(_) | DexValue::Reference(_))
}

pub(in crate::dex_run) fn dex_value_is_nullable_object_like(value: &DexValue) -> bool {
    matches!(
        value,
        DexValue::Null | DexValue::Int(0) | DexValue::Object(_) | DexValue::Reference(_)
    )
}

pub(in crate::dex_run) fn view_save_attribute_data_for_styleable(
    invocation: &DexInvocation,
) -> Option<()> {
    if invocation.arguments.len() != 7 {
        return None;
    }

    if !dex_value_is_object_like(&invocation.arguments[0])
        || !dex_value_is_object_like(&invocation.arguments[1])
        || !dex_value_is_object_like(&invocation.arguments[2])
        || !dex_value_is_nullable_object_like(&invocation.arguments[3])
        || !dex_value_is_object_like(&invocation.arguments[4])
    {
        return None;
    }

    match (&invocation.arguments[5], &invocation.arguments[6]) {
        (DexValue::Int(_), DexValue::Int(_)) => Some(()),
        _ => None,
    }
}

pub(in crate::dex_run) fn view_id_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "id".to_owned(),
    }
}

pub(in crate::dex_run) fn view_visibility_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "visibility".to_owned(),
    }
}

// cite: frameworks-base View.java#mSystemUiVisibility
pub(in crate::dex_run) fn view_system_ui_visibility_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "systemUiVisibility".to_owned(),
    }
}

pub(in crate::dex_run) fn view_focus_field(method_name: &str) -> Option<DexFieldId> {
    let name = match method_name {
        "setFocusable" | "isFocusable" => "focusable",
        "setFocusableInTouchMode" | "isFocusableInTouchMode" => "focusableInTouchMode",
        _ => return None,
    };

    Some(DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: name.to_owned(),
    })
}

pub(in crate::dex_run) fn view_focusable_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "focusableMode".to_owned(),
    }
}

pub(in crate::dex_run) fn view_default_focus_highlight_enabled_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "defaultFocusHighlightEnabled".to_owned(),
    }
}

pub(in crate::dex_run) fn view_focused_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "focused".to_owned(),
    }
}

pub(in crate::dex_run) fn view_force_dark_allowed_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "forceDarkAllowed".to_owned(),
    }
}

pub(in crate::dex_run) fn view_over_scroll_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "overScrollMode".to_owned(),
    }
}

pub(in crate::dex_run) fn view_layout_direction_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "layoutDirection".to_owned(),
    }
}

pub(in crate::dex_run) fn view_keep_screen_on_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "keepScreenOn".to_owned(),
    }
}

pub(in crate::dex_run) fn view_clickable_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "clickable".to_owned(),
    }
}

pub(in crate::dex_run) fn view_long_clickable_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "longClickable".to_owned(),
    }
}

pub(in crate::dex_run) fn view_scroll_container_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "scrollContainer".to_owned(),
    }
}

pub(in crate::dex_run) fn view_save_from_parent_enabled_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "saveFromParentEnabled".to_owned(),
    }
}

pub(in crate::dex_run) fn view_layout_requested_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "layoutRequested".to_owned(),
    }
}

// cite: frameworks-base View.java#mOldWidthMeasureSpec, mOldHeightMeasureSpec
pub(in crate::dex_run) fn view_old_measure_spec_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

// cite: frameworks-base View.java#PFLAG_LAYOUT_REQUIRED
pub(in crate::dex_run) fn view_layout_required_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "layoutRequired".to_owned(),
    }
}

pub(in crate::dex_run) fn view_system_gesture_exclusion_rects_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Ljava/util/List;".to_owned(),
        name: "systemGestureExclusionRects".to_owned(),
    }
}

pub(in crate::dex_run) fn view_invalidated_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "invalidated".to_owned(),
    }
}

pub(in crate::dex_run) fn view_window_focus_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "windowFocus".to_owned(),
    }
}

pub(in crate::dex_run) fn view_important_for_accessibility_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "importantForAccessibility".to_owned(),
    }
}

pub(in crate::dex_run) fn view_important_for_autofill_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "importantForAutofill".to_owned(),
    }
}

pub(in crate::dex_run) fn view_important_for_content_capture_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "importantForContentCapture".to_owned(),
    }
}

pub(in crate::dex_run) fn view_layout_params_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/ViewGroup$LayoutParams;".to_owned(),
        name: "layoutParams".to_owned(),
    }
}

pub(in crate::dex_run) fn view_group_layout_params_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/ViewGroup$LayoutParams;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn view_group_margin_layout_params_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/ViewGroup$MarginLayoutParams;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn relative_layout_params_rule_field(verb: i32) -> DexFieldId {
    // cite: frameworks-base RelativeLayout.java#addRule, mRules
    DexFieldId {
        class_descriptor: "Landroid/widget/RelativeLayout$LayoutParams;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: format!("rule:{verb}"),
    }
}

pub(in crate::dex_run) fn linear_layout_params_weight_field() -> DexFieldId {
    // cite: frameworks-base LinearLayout.java#LayoutParams, weight
    DexFieldId {
        class_descriptor: "Landroid/widget/LinearLayout$LayoutParams;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "weight".to_owned(),
    }
}

pub(in crate::dex_run) fn frame_layout_foreground_gravity_field() -> DexFieldId {
    // cite: frameworks-base FrameLayout.java#setForegroundGravity
    DexFieldId {
        class_descriptor: "Landroid/widget/FrameLayout;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "foregroundGravity".to_owned(),
    }
}

pub(in crate::dex_run) fn linear_layout_gravity_field() -> DexFieldId {
    // cite: frameworks-base LinearLayout.java#setGravity, mGravity
    DexFieldId {
        class_descriptor: "Landroid/widget/LinearLayout;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "gravity".to_owned(),
    }
}

pub(in crate::dex_run) fn linear_layout_orientation_field() -> DexFieldId {
    // cite: frameworks-base LinearLayout.java#setOrientation, mOrientation
    DexFieldId {
        class_descriptor: "Landroid/widget/LinearLayout;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "orientation".to_owned(),
    }
}

pub(in crate::dex_run) fn view_group_descendant_focusability_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/ViewGroup;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "descendantFocusability".to_owned(),
    }
}

pub(in crate::dex_run) fn view_group_clip_field(method_name: &str) -> DexFieldId {
    let name = match method_name {
        "setClipChildren" | "getClipChildren" => "clipChildren",
        "setClipToPadding" | "getClipToPadding" => "clipToPadding",
        _ => method_name,
    };

    DexFieldId {
        class_descriptor: "Landroid/view/ViewGroup;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn view_measured_width_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "measuredWidth".to_owned(),
    }
}

pub(in crate::dex_run) fn view_measured_height_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "measuredHeight".to_owned(),
    }
}

pub(in crate::dex_run) fn view_bounds_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn view_float_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn view_long_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "J".to_owned(),
        name: name.to_owned(),
    }
}

// cite: frameworks-base ViewGroup.java#mOnHierarchyChangeListener
pub(in crate::dex_run) fn view_hierarchy_change_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/ViewGroup;".to_owned(),
        type_descriptor: "Landroid/view/ViewGroup$OnHierarchyChangeListener;".to_owned(),
        name: "mOnHierarchyChangeListener".to_owned(),
    }
}

pub(in crate::dex_run) fn image_view_adjust_view_bounds_field() -> DexFieldId {
    // cite: frameworks-base ImageView.java#setAdjustViewBounds, mAdjustViewBounds
    DexFieldId {
        class_descriptor: "Landroid/widget/ImageView;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "adjustViewBounds".to_owned(),
    }
}

pub(in crate::dex_run) fn image_view_max_width_field() -> DexFieldId {
    // cite: frameworks-base ImageView.java#setMaxWidth, mMaxWidth
    DexFieldId {
        class_descriptor: "Landroid/widget/ImageView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "maxWidth".to_owned(),
    }
}

pub(in crate::dex_run) fn image_view_max_height_field() -> DexFieldId {
    // cite: frameworks-base ImageView.java#setMaxHeight, mMaxHeight
    DexFieldId {
        class_descriptor: "Landroid/widget/ImageView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "maxHeight".to_owned(),
    }
}

pub(in crate::dex_run) fn image_view_scale_type_field() -> DexFieldId {
    // cite: frameworks-base ImageView.java#setScaleType, mScaleType
    DexFieldId {
        class_descriptor: "Landroid/widget/ImageView;".to_owned(),
        type_descriptor: "Landroid/widget/ImageView$ScaleType;".to_owned(),
        name: "scaleType".to_owned(),
    }
}

pub(in crate::dex_run) fn view_minimum_width_field() -> DexFieldId {
    // cite: frameworks-base View.java#setMinimumWidth, mMinWidth
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "minWidth".to_owned(),
    }
}

pub(in crate::dex_run) fn view_minimum_height_field() -> DexFieldId {
    // cite: frameworks-base View.java#setMinimumHeight, mMinHeight
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "minHeight".to_owned(),
    }
}

pub(in crate::dex_run) fn view_background_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        name: "background".to_owned(),
    }
}

pub(in crate::dex_run) fn view_background_tint_list_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/content/res/ColorStateList;".to_owned(),
        name: "backgroundTintList".to_owned(),
    }
}

pub(in crate::dex_run) fn view_background_tint_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/graphics/PorterDuff$Mode;".to_owned(),
        name: "backgroundTintMode".to_owned(),
    }
}

pub(in crate::dex_run) fn view_outline_provider_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/ViewOutlineProvider;".to_owned(),
        name: "outlineProvider".to_owned(),
    }
}

pub(in crate::dex_run) fn color_drawable_color_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/ColorDrawable;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "color".to_owned(),
    }
}

pub(in crate::dex_run) fn ripple_drawable_color_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/RippleDrawable;".to_owned(),
        type_descriptor: "Landroid/content/res/ColorStateList;".to_owned(),
        name: "color".to_owned(),
    }
}

pub(in crate::dex_run) fn ripple_drawable_content_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/RippleDrawable;".to_owned(),
        type_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        name: "content".to_owned(),
    }
}

pub(in crate::dex_run) fn ripple_drawable_mask_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/RippleDrawable;".to_owned(),
        type_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        name: "mask".to_owned(),
    }
}

pub(in crate::dex_run) fn ripple_drawable_radius_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/RippleDrawable;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "radius".to_owned(),
    }
}

pub(in crate::dex_run) fn drawable_bounds_int_field(name: &str) -> DexFieldId {
    let field_name = match name {
        "left" => "boundsLeft",
        "top" => "boundsTop",
        "right" => "boundsRight",
        "bottom" => "boundsBottom",
        _ => name,
    };

    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: field_name.to_owned(),
    }
}

pub(in crate::dex_run) fn drawable_hotspot_float_field(name: &str) -> DexFieldId {
    let field_name = match name {
        "x" => "hotspotX",
        "y" => "hotspotY",
        _ => name,
    };

    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: field_name.to_owned(),
    }
}

pub(in crate::dex_run) fn drawable_state_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        type_descriptor: "[I".to_owned(),
        name: "state".to_owned(),
    }
}

pub(in crate::dex_run) fn drawable_visible_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "visible".to_owned(),
    }
}

pub(in crate::dex_run) fn window_bar_color_field(method_name: &str) -> DexFieldId {
    let name = match method_name {
        "setStatusBarColor" | "getStatusBarColor" => "statusBarColor",
        "setNavigationBarColor" | "getNavigationBarColor" => "navigationBarColor",
        _ => method_name,
    };

    DexFieldId {
        class_descriptor: "Landroid/view/Window;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn window_callback_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/Window;".to_owned(),
        type_descriptor: "Landroid/view/Window$Callback;".to_owned(),
        name: "callback".to_owned(),
    }
}

pub(in crate::dex_run) fn window_layout_params_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/Window;".to_owned(),
        type_descriptor: "Landroid/view/WindowManager$LayoutParams;".to_owned(),
        name: "attributes".to_owned(),
    }
}

pub(in crate::dex_run) fn window_manager_layout_params_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/WindowManager$LayoutParams;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn window_manager_layout_params_title_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/WindowManager$LayoutParams;".to_owned(),
        type_descriptor: "Ljava/lang/CharSequence;".to_owned(),
        name: "mTitle".to_owned(),
    }
}

pub(in crate::dex_run) fn window_decor_fits_system_windows_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/Window;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "decorFitsSystemWindows".to_owned(),
    }
}

pub(in crate::dex_run) fn window_contrast_enforced_field(method_name: &str) -> DexFieldId {
    let name = match method_name {
        "setStatusBarContrastEnforced" | "isStatusBarContrastEnforced" => {
            "statusBarContrastEnforced"
        }
        "setNavigationBarContrastEnforced" | "isNavigationBarContrastEnforced" => {
            "navigationBarContrastEnforced"
        }
        _ => method_name,
    };

    DexFieldId {
        class_descriptor: "Landroid/view/Window;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn window_insets_controller_system_bars_appearance_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/WindowInsetsController;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "systemBarsAppearance".to_owned(),
    }
}

pub(in crate::dex_run) fn window_metrics_bounds_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/WindowMetrics;".to_owned(),
        type_descriptor: "Landroid/graphics/Rect;".to_owned(),
        name: "bounds".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_background_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        name: "background".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_overlap_anchor_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "overlapAnchor".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_input_method_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "inputMethodMode".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_window_layout_type_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "windowLayoutType".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_is_clipped_to_screen_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "isClippedToScreen".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_epicenter_bounds_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "Landroid/graphics/Rect;".to_owned(),
        name: "epicenterBounds".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_content_view_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "Landroid/view/View;".to_owned(),
        name: "contentView".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_window_showing_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupWindow;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "isShowing".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_menu_anchor_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupMenu;".to_owned(),
        type_descriptor: "Landroid/view/View;".to_owned(),
        name: "anchor".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_menu_click_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/PopupMenu;".to_owned(),
        type_descriptor: "Landroid/widget/PopupMenu$OnMenuItemClickListener;".to_owned(),
        name: "menuItemClickListener".to_owned(),
    }
}

pub(in crate::dex_run) fn popup_menu_menu_reference(popup_key: &str) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/view/Menu;:popup/{popup_key}"
    )))
}

pub(in crate::dex_run) fn abs_list_view_cache_color_hint_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/AbsListView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "cacheColorHint".to_owned(),
    }
}

pub(in crate::dex_run) fn abs_list_view_scroll_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/AbsListView;".to_owned(),
        type_descriptor: "Landroid/widget/AbsListView$OnScrollListener;".to_owned(),
        name: "onScrollListener".to_owned(),
    }
}

pub(in crate::dex_run) fn horizontal_scroll_view_scroll_x_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "scrollX".to_owned(),
    }
}

pub(in crate::dex_run) fn view_scroll_y_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "scrollY".to_owned(),
    }
}

pub(in crate::dex_run) fn list_view_divider_height_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/ListView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "dividerHeight".to_owned(),
    }
}

pub(in crate::dex_run) fn list_view_divider_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/ListView;".to_owned(),
        type_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        name: "divider".to_owned(),
    }
}

pub(in crate::dex_run) fn adapter_view_adapter_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/AdapterView;".to_owned(),
        type_descriptor: "Landroid/widget/Adapter;".to_owned(),
        name: "adapter".to_owned(),
    }
}

pub(in crate::dex_run) fn adapter_data_set_observer_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/Adapter;".to_owned(),
        type_descriptor: "Landroid/database/DataSetObserver;".to_owned(),
        name: "registeredDataSetObserver".to_owned(),
    }
}

pub(in crate::dex_run) fn adapter_view_item_click_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/AdapterView;".to_owned(),
        type_descriptor: "Landroid/widget/AdapterView$OnItemClickListener;".to_owned(),
        name: "onItemClickListener".to_owned(),
    }
}

pub(in crate::dex_run) fn adapter_view_item_selected_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/AdapterView;".to_owned(),
        type_descriptor: "Landroid/widget/AdapterView$OnItemSelectedListener;".to_owned(),
        name: "onItemSelectedListener".to_owned(),
    }
}

pub(in crate::dex_run) fn adapter_view_selected_position_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/AdapterView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "selectedPosition".to_owned(),
    }
}

pub(in crate::dex_run) fn intent_target_class_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/Intent;".to_owned(),
        type_descriptor: "Ljava/lang/Class;".to_owned(),
        name: "winedroidTargetClass".to_owned(),
    }
}

pub(in crate::dex_run) fn intent_action_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/Intent;".to_owned(),
        type_descriptor: "Ljava/lang/String;".to_owned(),
        name: "winedroidAction".to_owned(),
    }
}

// cite: frameworks-base Intent.java#mType
pub(in crate::dex_run) fn intent_type_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/Intent;".to_owned(),
        type_descriptor: "Ljava/lang/String;".to_owned(),
        name: "winedroidType".to_owned(),
    }
}

pub(in crate::dex_run) fn intent_data_uri_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/Intent;".to_owned(),
        type_descriptor: "Landroid/net/Uri;".to_owned(),
        name: "winedroidDataUri".to_owned(),
    }
}

pub(in crate::dex_run) fn uri_string_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/net/Uri;".to_owned(),
        type_descriptor: "Ljava/lang/String;".to_owned(),
        name: "winedroidUriString".to_owned(),
    }
}

pub(in crate::dex_run) fn image_view_drawable_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/ImageView;".to_owned(),
        type_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        name: "drawable".to_owned(),
    }
}

pub(in crate::dex_run) fn toolbar_navigation_icon_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/Toolbar;".to_owned(),
        type_descriptor: "Landroid/graphics/drawable/Drawable;".to_owned(),
        name: "navigationIcon".to_owned(),
    }
}

pub(in crate::dex_run) fn image_view_tint_list_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/ImageView;".to_owned(),
        type_descriptor: "Landroid/content/res/ColorStateList;".to_owned(),
        name: "imageTintList".to_owned(),
    }
}

pub(in crate::dex_run) fn image_view_tint_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/ImageView;".to_owned(),
        type_descriptor: "Landroid/graphics/PorterDuff$Mode;".to_owned(),
        name: "imageTintMode".to_owned(),
    }
}

pub(in crate::dex_run) fn view_content_description_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Ljava/lang/CharSequence;".to_owned(),
        name: "contentDescription".to_owned(),
    }
}

pub(in crate::dex_run) fn view_tooltip_text_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Ljava/lang/CharSequence;".to_owned(),
        name: "tooltipText".to_owned(),
    }
}

pub(in crate::dex_run) fn view_key_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/View$OnKeyListener;".to_owned(),
        name: "onKeyListener".to_owned(),
    }
}

pub(in crate::dex_run) fn view_touch_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/View$OnTouchListener;".to_owned(),
        name: "onTouchListener".to_owned(),
    }
}

pub(in crate::dex_run) fn view_generic_motion_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/View$OnGenericMotionListener;".to_owned(),
        name: "onGenericMotionListener".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_scroll_axis_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "axisScroll".to_owned(),
    }
}

pub(in crate::dex_run) fn view_drag_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/View$OnDragListener;".to_owned(),
        name: "onDragListener".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_action_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "action".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_meta_state_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "metaState".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_down_time_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "J".to_owned(),
        name: "downTime".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_event_time_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "J".to_owned(),
        name: "eventTime".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_x_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "x".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_y_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "y".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_pressure_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "pressure".to_owned(),
    }
}

pub(in crate::dex_run) fn motion_event_source_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/MotionEvent;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "source".to_owned(),
    }
}

pub(in crate::dex_run) fn view_window_insets_animation_callback_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/WindowInsetsAnimation$Callback;".to_owned(),
        name: "windowInsetsAnimationCallback".to_owned(),
    }
}

pub(in crate::dex_run) fn view_translation_callback_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "Landroid/view/translation/ViewTranslationCallback;".to_owned(),
        name: "viewTranslationCallback".to_owned(),
    }
}

pub(in crate::dex_run) fn window_insets_animation_callback_dispatch_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/WindowInsetsAnimation$Callback;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "dispatchMode".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_text_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "Ljava/lang/CharSequence;".to_owned(),
        name: "text".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_cursor_visible_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "cursorVisible".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_editor_action_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "Landroid/widget/TextView$OnEditorActionListener;".to_owned(),
        name: "onEditorActionListener".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_width_field() -> DexFieldId {
    // cite: frameworks-base TextView.java#setWidth, mMinWidth, mMaxWidth
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "width".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_text_size_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "textSize".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_text_color_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "textColor".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_text_appearance_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "textAppearance".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_flags_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "flags".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_hinting_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "hinting".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_color_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "color".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_color_long_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "J".to_owned(),
        name: "colorLong".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_alpha_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "alpha".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_text_size_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "textSize".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_text_scale_x_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "textScaleX".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_letter_spacing_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "letterSpacing".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_text_locale_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Ljava/util/Locale;".to_owned(),
        name: "textLocale".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_stroke_width_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "strokeWidth".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_stroke_miter_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "strokeMiter".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_stroke_cap_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Landroid/graphics/Paint$Cap;".to_owned(),
        name: "strokeCap".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_stroke_join_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Landroid/graphics/Paint$Join;".to_owned(),
        name: "strokeJoin".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_shader_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Landroid/graphics/Shader;".to_owned(),
        name: "shader".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_style_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Landroid/graphics/Paint$Style;".to_owned(),
        name: "style".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_typeface_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Landroid/graphics/Typeface;".to_owned(),
        name: "typeface".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_blend_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Landroid/graphics/BlendMode;".to_owned(),
        name: "blendMode".to_owned(),
    }
}

pub(in crate::dex_run) fn paint_color_filter_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Paint;".to_owned(),
        type_descriptor: "Landroid/graphics/ColorFilter;".to_owned(),
        name: "colorFilter".to_owned(),
    }
}

// cite: frameworks-base Outline.java#mPath
pub(in crate::dex_run) fn outline_path_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Outline;".to_owned(),
        type_descriptor: "Landroid/graphics/Path;".to_owned(),
        name: "path".to_owned(),
    }
}

pub(in crate::dex_run) fn outline_field(name: &str, value: &DexValue) -> DexFieldId {
    let type_descriptor = match value {
        DexValue::Float(_) => "F",
        _ => "I",
    };

    DexFieldId {
        class_descriptor: "Landroid/graphics/Outline;".to_owned(),
        type_descriptor: type_descriptor.to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_selection_start_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "selectionStart".to_owned(),
    }
}

pub(in crate::dex_run) fn text_view_selection_end_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/widget/TextView;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "selectionEnd".to_owned(),
    }
}

pub(in crate::dex_run) fn is_text_view_class(class_descriptor: &str) -> bool {
    matches!(
        class_descriptor,
        "Landroid/widget/TextView;" | "Landroid/widget/EditText;"
    )
}

pub(in crate::dex_run) fn is_image_view_compatible_class(class_descriptor: &str) -> bool {
    matches!(
        class_descriptor,
        "Landroid/widget/ImageView;" | "Landroid/widget/ImageButton;"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::dex_run) struct TextChangeRange {
    pub(in crate::dex_run) start: i32,
    pub(in crate::dex_run) removed: i32,
    pub(in crate::dex_run) added: i32,
}

pub(in crate::dex_run) fn text_change_range(old_text: &str, new_text: &str) -> TextChangeRange {
    let old_units: Vec<u16> = old_text.encode_utf16().collect();
    let new_units: Vec<u16> = new_text.encode_utf16().collect();
    let mut prefix = 0usize;

    while prefix < old_units.len()
        && prefix < new_units.len()
        && old_units[prefix] == new_units[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix + prefix < old_units.len()
        && suffix + prefix < new_units.len()
        && old_units[old_units.len() - suffix - 1] == new_units[new_units.len() - suffix - 1]
    {
        suffix += 1;
    }

    TextChangeRange {
        start: prefix as i32,
        removed: (old_units.len() - prefix - suffix) as i32,
        added: (new_units.len() - prefix - suffix) as i32,
    }
}

pub(in crate::dex_run) fn text_view_text_argument_string(value: &DexValue) -> Result<String> {
    match value {
        DexValue::String(value) => Ok(value.clone().to_string()),
        DexValue::Null => Ok(String::new()),
        other => Err(WinedroidError::new(format!(
            "TextView.setText only supports string-backed CharSequence values\nvalue: {other}\nnext missing primitive: CharSequence object text extraction"
        ))),
    }
}

pub(in crate::dex_run) fn text_view_text_value(value: &DexValue) -> Option<DexValue> {
    match value {
        DexValue::String(value) => Some(DexValue::String(value.clone())),
        DexValue::Null => Some(DexValue::String(std::rc::Rc::from(String::new()))),
        DexValue::Object(_) | DexValue::Reference(_) => Some(value.clone()),
        DexValue::Uninitialized | DexValue::Int(_) | DexValue::Long(_) | DexValue::Float(_) => None,
    }
}

pub(in crate::dex_run) fn rect_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Rect;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn rectf_float_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RectF;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn bitmap_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Bitmap;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn bitmap_bool_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Bitmap;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn bitmap_object_field(name: &str, type_descriptor: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Bitmap;".to_owned(),
        type_descriptor: type_descriptor.to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn canvas_save_count_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Canvas;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "saveCount".to_owned(),
    }
}

pub(in crate::dex_run) fn canvas_bitmap_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Canvas;".to_owned(),
        type_descriptor: "Landroid/graphics/Bitmap;".to_owned(),
        name: "bitmap".to_owned(),
    }
}

pub(in crate::dex_run) fn canvas_float_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Canvas;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn canvas_bool_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Canvas;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn path_fill_type_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Path;".to_owned(),
        type_descriptor: "Landroid/graphics/Path$FillType;".to_owned(),
        name: "fillType".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_name_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Ljava/lang/String;".to_owned(),
        name: "name".to_owned(),
    }
}

pub(in crate::dex_run) fn recording_canvas_render_node_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RecordingCanvas;".to_owned(),
        type_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        name: "renderNode".to_owned(),
    }
}

pub(in crate::dex_run) fn recording_canvas_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RecordingCanvas;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_recording_canvas_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Landroid/graphics/RecordingCanvas;".to_owned(),
        name: "recordingCanvas".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_clip_to_bounds_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "clipToBounds".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_use_compositing_layer_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "useCompositingLayer".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_compositing_layer_paint_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Landroid/graphics/Paint;".to_owned(),
        name: "compositingLayerPaint".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_clip_to_outline_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "clipToOutline".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_elevation_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "elevation".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_alpha_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "alpha".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_scale_x_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "scaleX".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_scale_y_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "scaleY".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_float_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_position_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_pivot_x_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "pivotX".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_pivot_y_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "pivotY".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_has_overlapping_rendering_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "hasOverlappingRendering".to_owned(),
    }
}

pub(in crate::dex_run) fn render_node_outline_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/RenderNode;".to_owned(),
        type_descriptor: "Landroid/graphics/Outline;".to_owned(),
        name: "outline".to_owned(),
    }
}

pub(in crate::dex_run) fn blend_mode_color_filter_color_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/BlendModeColorFilter;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "color".to_owned(),
    }
}

pub(in crate::dex_run) fn blend_mode_color_filter_mode_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/BlendModeColorFilter;".to_owned(),
        type_descriptor: "Landroid/graphics/BlendMode;".to_owned(),
        name: "mode".to_owned(),
    }
}

pub(in crate::dex_run) fn point_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Point;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn insets_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/Insets;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn window_insets_builder_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/WindowInsets$Builder;".to_owned(),
        type_descriptor: "Landroid/graphics/Insets;".to_owned(),
        name: name.to_owned(),
    }
}

// cite: frameworks-base TypedValue.java#complexToDimensionPixelSize
pub(in crate::dex_run) fn android_dimension_float_to_pixel_size(value: f32) -> i32 {
    let pixels = if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    };

    if pixels != 0 || value == 0.0 {
        return pixels;
    }

    if value > 0.0 {
        1
    } else {
        -1
    }
}

pub(in crate::dex_run) fn is_bootstrap_view_constructor_class(class_descriptor: &str) -> bool {
    matches!(
        class_descriptor,
        "Landroid/view/View;"
            | "Landroid/view/ViewGroup;"
            | "Landroid/widget/AdapterView;"
            | "Landroid/widget/AbsListView;"
            | "Landroid/widget/ListView;"
            | "Landroid/widget/FrameLayout;"
            | "Landroid/widget/LinearLayout;"
            | "Landroid/widget/TextView;"
            | "Landroid/widget/EditText;"
            | "Landroid/widget/Button;"
            | "Landroid/widget/ImageButton;"
            | "Landroid/widget/ImageView;"
            | "Landroid/widget/RelativeLayout;"
            | "Landroid/widget/TableLayout;"
            | "Landroid/widget/TableRow;"
            | "Landroid/widget/Space;"
            | "Landroid/widget/HorizontalScrollView;"
    )
}

pub(in crate::dex_run) fn android_complex_dimension_to_pixel_size(value: u32) -> i32 {
    android_dimension_float_to_pixel_size(android_complex_dimension_to_px_float(value))
}

// cite: frameworks-base TypedValue.java#complexToDimensionPixelOffset
pub(in crate::dex_run) fn android_complex_dimension_to_pixel_offset(value: u32) -> i32 {
    android_complex_dimension_to_px_float(value) as i32
}

// cite: frameworks-base TypedValue.java#complexToDimension, applyDimension, complexToFloat
pub(in crate::dex_run) fn android_complex_dimension_to_px_float(value: u32) -> f32 {
    let float_value = android_complex_to_float(value);
    let unit = value & 0x0f;

    match unit {
        0 => float_value,
        1 | 2 => float_value,
        3 => float_value * 160.0 / 72.0,
        4 => float_value * 160.0,
        5 => float_value * 160.0 / 25.4,
        _ => float_value,
    }
}

pub(in crate::dex_run) fn android_complex_to_float(value: u32) -> f32 {
    const RADIX_MULTIPLIERS: [f32; 4] = [
        1.0 / 256.0,
        1.0 / 32_768.0,
        1.0 / 8_388_608.0,
        1.0 / 2_147_483_648.0,
    ];

    let mantissa = (value & 0xffff_ff00) as i32;
    let radix = ((value >> 4) & 0x03) as usize;

    mantissa as f32 * RADIX_MULTIPLIERS[radix]
}

pub(in crate::dex_run) fn thread_local_value_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Ljava/lang/ThreadLocal;".to_owned(),
        type_descriptor: "Ljava/lang/Object;".to_owned(),
        name: "value".to_owned(),
    }
}

pub(in crate::dex_run) fn view_padding_field(name: &str) -> DexFieldId {
    // cite: frameworks-base View.java#setPadding, mUserPaddingLeft, mPaddingTop, mUserPaddingRight, mUserPaddingBottom
    DexFieldId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

pub(in crate::dex_run) fn is_view_padding_getter(name: &str) -> bool {
    matches!(
        name,
        "getPaddingLeft" | "getPaddingTop" | "getPaddingRight" | "getPaddingBottom"
    )
}

pub(in crate::dex_run) fn is_view_dimension_getter(name: &str) -> bool {
    matches!(
        name,
        "getWidth" | "getHeight" | "getLeft" | "getTop" | "getRight" | "getBottom"
    )
}

pub(in crate::dex_run::view) fn runtime_view_root_value(
    root: &DexRuntimeViewRoot,
) -> Option<DexValue> {
    if let Some(object_id) = root.view_key.strip_prefix("object:") {
        let id = object_id.parse::<u64>().ok()?;
        return Some(DexValue::Object(DexObjectRef {
            id,
            class_descriptor: std::rc::Rc::from(root.class_descriptor.clone()),
        }));
    }

    root.view_key
        .strip_prefix("reference:")
        .map(|reference| DexValue::Reference(std::rc::Rc::from(reference.to_owned())))
}

pub(in crate::dex_run::view) fn exact_measure_spec(size: i32) -> i32 {
    const EXACTLY: i32 = 0x4000_0000;

    (size & 0x3fff_ffff) | EXACTLY
}

pub(in crate::dex_run::view) fn at_most_measure_spec(size: i32) -> i32 {
    const AT_MOST: i32 = 0x8000_0000_u32 as i32;

    (size & 0x3fff_ffff) | AT_MOST
}

// cite: frameworks-base ViewRootImpl.java#getRootMeasureSpec
pub(in crate::dex_run::view) fn root_measure_spec(window_size: i32, measurement: i32) -> i32 {
    match measurement {
        -1 => exact_measure_spec(window_size),
        -2 => at_most_measure_spec(window_size),
        size => exact_measure_spec(size.max(0)),
    }
}

pub(in crate::dex_run) fn view_configuration_int(name: &str) -> i32 {
    match name {
        "getScaledTouchSlop" => 8,
        "getScaledMinimumFlingVelocity" => 50,
        "getScaledMaximumFlingVelocity" => 8000,
        "getTapTimeout" => 100,
        "getLongPressTimeout" => 500,
        _ => 0,
    }
}

pub(in crate::dex_run) fn window_insets_builder_build(
    invocation: &DexInvocation,
) -> Option<DexValue> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {
            Some(DexValue::Reference(std::rc::Rc::from(format!(
                "Landroid/view/WindowInsets;:builder:{}",
                map_identity_key(&invocation.arguments[0])
            ))))
        }
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run) fn layout_reference(layout_id: i32) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/view/View;:layout/{layout_id}"
    )))
}

pub(in crate::dex_run::view) fn layout_id_from_view_value(value: &DexValue) -> Option<i32> {
    let DexValue::Reference(reference) = value else {
        return None;
    };

    layout_id_from_reference_str(reference)
}

pub(in crate::dex_run) fn layout_id_from_reference_str(reference: &str) -> Option<i32> {
    let marker = ":layout/";
    let start = reference.find(marker)? + marker.len();
    let digits = reference[start..]
        .chars()
        .take_while(|value| value.is_ascii_digit())
        .collect::<String>();

    if digits.is_empty() {
        return None;
    }

    digits.parse().ok()
}

pub(in crate::dex_run) fn inflated_view_constructor_candidates(
    class_descriptor: &str,
    root: &DexValue,
    context: &DexValue,
    attrs: &DexValue,
) -> Vec<(DexMethodId, Vec<DexValue>)> {
    // cite: frameworks-base LayoutInflater.java#mConstructorSignature, createView
    vec![(
        DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: "<init>".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "VLL".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec![
                    "Landroid/content/Context;".to_owned(),
                    "Landroid/util/AttributeSet;".to_owned(),
                ],
            },
        },
        vec![root.clone(), context.clone(), attrs.clone()],
    )]
}

pub(in crate::dex_run) fn layout_child_reference(
    layout_id: i32,
    view_id: i32,
    class_descriptor: &str,
) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "{class_descriptor}:layout/{layout_id}/id/{view_id}"
    )))
}

pub(in crate::dex_run) fn layout_node_reference(
    layout_id: i32,
    ordinal: usize,
    class_descriptor: &str,
) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "{class_descriptor}:layout/{layout_id}/node/{ordinal}"
    )))
}

// cite: frameworks-base LayoutInflater.java#parseInclude
pub(in crate::dex_run) fn xml_element_layout_reference(
    element: &binary_xml::XmlStartElement,
) -> Option<i32> {
    let attribute = element.attribute("layout")?;

    match &attribute.value {
        binary_xml::XmlValue::Reference(value)
        | binary_xml::XmlValue::Hex(value)
        | binary_xml::XmlValue::AttributeReference(value) => i32::try_from(*value).ok(),
        binary_xml::XmlValue::Integer(value) => Some(*value),
        _ => None,
    }
}

pub(in crate::dex_run) fn xml_element_view_id(
    element: &binary_xml::XmlStartElement,
) -> Option<i32> {
    let attribute = element.attribute("id")?;

    match &attribute.value {
        binary_xml::XmlValue::Reference(value)
        | binary_xml::XmlValue::Hex(value)
        | binary_xml::XmlValue::AttributeReference(value) => i32::try_from(*value).ok(),
        binary_xml::XmlValue::Integer(value) => Some(*value),
        binary_xml::XmlValue::Null
        | binary_xml::XmlValue::String(_)
        | binary_xml::XmlValue::Boolean(_)
        | binary_xml::XmlValue::Float(_)
        | binary_xml::XmlValue::Dimension(_)
        | binary_xml::XmlValue::Fraction(_)
        | binary_xml::XmlValue::Color(_)
        | binary_xml::XmlValue::Unknown { .. } => None,
    }
}

pub(in crate::dex_run::view) fn stable_view_drawing_id(identity_key: &str) -> i64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;

    for byte in identity_key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    (hash & 0x7fff_ffff_ffff_ffff) as i64
}

pub(in crate::dex_run::view) fn nullable_object_argument(value: &DexValue) -> Option<DexValue> {
    match value {
        DexValue::Null | DexValue::Int(0) => Some(DexValue::Null),
        DexValue::Object(_) | DexValue::Reference(_) => Some(value.clone()),
        DexValue::Uninitialized
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run::view) fn canvas_float_argument(value: &DexValue) -> Option<f32> {
    match value {
        DexValue::Float(value) => Some(*value),
        DexValue::Int(bits) => Some(f32::from_bits(*bits as u32)),
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Long(_)
        | DexValue::String(_)
        | DexValue::Object(_)
        | DexValue::Reference(_) => None,
    }
}

pub(in crate::dex_run) fn androidx_view_tree_owner_tag_value(
    type_name: &str,
    key_name: &str,
) -> Option<DexValue> {
    if type_name != "id" {
        return None;
    }

    let descriptor = match key_name {
        "view_tree_lifecycle_owner" => "Landroidx/lifecycle/LifecycleOwner;",
        "view_tree_navigation_event_dispatcher_owner" => {
            "Landroidx/navigationevent/NavigationEventDispatcherOwner;"
        }
        "view_tree_on_back_pressed_dispatcher_owner" => {
            "Landroidx/activity/OnBackPressedDispatcherOwner;"
        }
        "view_tree_saved_state_registry_owner" => "Landroidx/savedstate/SavedStateRegistryOwner;",
        "view_tree_view_model_store_owner" => "Landroidx/lifecycle/ViewModelStoreOwner;",
        _ => return None,
    };

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "{descriptor}.bootstrap"
    ))))
}

pub(in crate::dex_run) fn layout_element_class_descriptor(name: &str) -> Option<String> {
    if name == "merge" || name == "include" {
        return Some("Landroid/view/View;".to_owned());
    }

    if name.contains('.') {
        return Some(format!("L{};", name.replace('.', "/")));
    }

    let descriptor = match name {
        "View" => "Landroid/view/View;",
        "ViewGroup" => "Landroid/view/ViewGroup;",
        "TextView" => "Landroid/widget/TextView;",
        "EditText" => "Landroid/widget/EditText;",
        "Button" => "Landroid/widget/Button;",
        "ImageButton" => "Landroid/widget/ImageButton;",
        "ImageView" => "Landroid/widget/ImageView;",
        "LinearLayout" => "Landroid/widget/LinearLayout;",
        "FrameLayout" => "Landroid/widget/FrameLayout;",
        "RelativeLayout" => "Landroid/widget/RelativeLayout;",
        "TableLayout" => "Landroid/widget/TableLayout;",
        "TableRow" => "Landroid/widget/TableRow;",
        "ListView" => "Landroid/widget/ListView;",
        "Space" => "Landroid/widget/Space;",
        "HorizontalScrollView" => "Landroid/widget/HorizontalScrollView;",
        _ => return None,
    };

    Some(descriptor.to_owned())
}
