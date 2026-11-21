use super::super::*;

fn render_node_property_name_from_getter(name: &str) -> Option<&'static str> {
    let property = match name {
        "getAlpha" => "alpha",
        "getScaleX" => "scaleX",
        "getScaleY" => "scaleY",
        "getElevation" => "elevation",
        "getPivotX" => "pivotX",
        "getPivotY" => "pivotY",
        "getTranslationX" => "translationX",
        "getTranslationY" => "translationY",
        "getRotationZ" => "rotationZ",
        "getRotationX" => "rotationX",
        "getRotationY" => "rotationY",
        "getCameraDistance" => "cameraDistance",
        "getAmbientShadowColor" => "ambientShadowColor",
        "getSpotShadowColor" => "spotShadowColor",
        "getClipToBounds" => "clipToBounds",
        "getClipToOutline" => "clipToOutline",
        "getUseCompositingLayer" => "useCompositingLayer",
        "hasOverlappingRendering" => "hasOverlappingRendering",
        _ => return None,
    };

    Some(property)
}

fn render_node_property_name_from_setter(name: &str) -> Option<&'static str> {
    let property = match name {
        "setTranslationX" => "translationX",
        "setTranslationY" => "translationY",
        "setRotationZ" => "rotationZ",
        "setRotationX" => "rotationX",
        "setRotationY" => "rotationY",
        "setCameraDistance" => "cameraDistance",
        "setAmbientShadowColor" => "ambientShadowColor",
        "setSpotShadowColor" => "spotShadowColor",
        _ => return None,
    };

    Some(property)
}

impl DexProgram {
    pub(super) fn invoke_android_view_core_builtin(
        &self,
        method: &DexMethodId,
        invocation: Option<&DexInvocation>,
    ) -> Option<DexExecutionResult> {
        if method.class_descriptor == "Landroid/graphics/Rect;"
            && method.name == "set"
            && method.proto.descriptor_is("(IIII)V")
        {
            let invocation = invocation?;
            self.rect_set(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Rect;"
            && method.name == "set"
            && method.proto.descriptor_is("(Landroid/graphics/Rect;)V")
        {
            let invocation = invocation?;
            self.rect_set_from_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Rect;"
            && method.name == "setEmpty"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.rect_set_empty(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Rect;"
            && matches!(method.name.as_str(), "width" | "height")
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.rect_dimension(invocation, &method.name)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/Rect;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.rect_set_empty(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Rect;"
            && method.name == "<init>"
            && method.proto.descriptor_is("(IIII)V")
        {
            let invocation = invocation?;
            self.rect_set(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base Rect.java#Rect
        if method.class_descriptor == "Landroid/graphics/Rect;"
            && method.name == "<init>"
            && method.proto.descriptor_is("(Landroid/graphics/Rect;)V")
        {
            let invocation = invocation?;
            if invocation
                .arguments
                .get(1)
                .map(|value| value == &DexValue::Null)
                == Some(true)
            {
                self.rect_set_empty(&DexInvocation {
                    kind: invocation.kind,
                    range: invocation.range,
                    method_index: invocation.method_index,
                    argument_registers: invocation.argument_registers.clone(),
                    arguments: smallvec::smallvec![invocation.arguments[0].clone()],
                })?;
            } else {
                self.rect_set_from_rect(invocation)?;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/RectF;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.initialize_rectf(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/RectF;"
            && matches!(method.name.as_str(), "width" | "height")
            && method.proto.descriptor_is("()F")
        {
            let invocation = invocation?;
            let value = self.rectf_dimension(invocation, &method.name)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/graphics/RectF;"
            && method.name == "set"
            && method.proto.descriptor_is("(FFFF)V")
        {
            let invocation = invocation?;
            self.rectf_set(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base RectF.java#RectF
        if method.class_descriptor == "Landroid/graphics/RectF;"
            && method.name == "<init>"
            && method.proto.descriptor_is("(FFFF)V")
        {
            let invocation = invocation?;
            self.rectf_set(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/drawable/ColorDrawable;"
            && method.name == "<init>"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 2 {
                return None;
            }
            if !dex_value_is_object_like(&invocation.arguments[0]) {
                return None;
            }
            let DexValue::Int(color) = invocation.arguments[1] else {
                return None;
            };
            if !self.write_stored_instance_field(
                &invocation.arguments[0],
                &color_drawable_color_field(),
                DexValue::Int(color),
            ) {
                return None;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/drawable/Drawable;"
            && method.name == "setBounds"
            && method.proto.descriptor_is("(Landroid/graphics/Rect;)V")
        {
            let invocation = invocation?;
            self.drawable_set_bounds_from_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/drawable/Drawable;"
            && method.name == "setHotspot"
            && method.proto.descriptor_is("(FF)V")
        {
            let invocation = invocation?;
            self.drawable_set_hotspot(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/drawable/Drawable;"
            && method.name == "setState"
            && method.proto.descriptor_is("([I)Z")
        {
            let invocation = invocation?;
            let value = self.drawable_set_state(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/drawable/Drawable;"
            && method.name == "setVisible"
            && method.proto.descriptor_is("(ZZ)Z")
        {
            let invocation = invocation?;
            let value = self.drawable_set_visible(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/drawable/RippleDrawable;"
            && method.name == "<init>"
            && method.proto.descriptor()
                == "(Landroid/content/res/ColorStateList;Landroid/graphics/drawable/Drawable;Landroid/graphics/drawable/Drawable;)V"
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 4 {
                return None;
            }
            if !dex_value_is_object_like(&invocation.arguments[0])
                || !dex_value_is_nullable_object_like(&invocation.arguments[1])
                || !dex_value_is_nullable_object_like(&invocation.arguments[2])
                || !dex_value_is_nullable_object_like(&invocation.arguments[3])
            {
                return None;
            }
            if !self.write_stored_instance_field(
                &invocation.arguments[0],
                &ripple_drawable_color_field(),
                invocation.arguments[1].clone(),
            ) {
                return None;
            }
            if !self.write_stored_instance_field(
                &invocation.arguments[0],
                &ripple_drawable_content_field(),
                invocation.arguments[2].clone(),
            ) {
                return None;
            }
            if !self.write_stored_instance_field(
                &invocation.arguments[0],
                &ripple_drawable_mask_field(),
                invocation.arguments[3].clone(),
            ) {
                return None;
            }
            if !self.write_stored_instance_field(
                &invocation.arguments[0],
                &ripple_drawable_radius_field(),
                DexValue::Int(-1),
            ) {
                return None;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/drawable/RippleDrawable;"
            && method.name == "getRadius"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1 {
                return None;
            }
            if !dex_value_is_object_like(&invocation.arguments[0]) {
                return None;
            }
            let value = match self.read_stored_instance_field(
                &invocation.arguments[0],
                &ripple_drawable_radius_field(),
            ) {
                Some(DexValue::Int(radius)) => radius,
                Some(_) => return None,
                None => -1,
            };

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/drawable/RippleDrawable;"
            && method.name == "setRadius"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 2 {
                return None;
            }
            if !dex_value_is_object_like(&invocation.arguments[0]) {
                return None;
            }
            let DexValue::Int(radius) = invocation.arguments[1] else {
                return None;
            };
            if !self.write_stored_instance_field(
                &invocation.arguments[0],
                &ripple_drawable_radius_field(),
                DexValue::Int(radius),
            ) {
                return None;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/drawable/RippleDrawable;"
            && method.name == "setColor"
            && method
                .proto
                .descriptor_is("(Landroid/content/res/ColorStateList;)V")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 2 {
                return None;
            }
            if !dex_value_is_object_like(&invocation.arguments[0])
                || !dex_value_is_nullable_object_like(&invocation.arguments[1])
            {
                return None;
            }
            if !self.write_stored_instance_field(
                &invocation.arguments[0],
                &ripple_drawable_color_field(),
                invocation.arguments[1].clone(),
            ) {
                return None;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Matrix;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.initialize_matrix(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Matrix;", "reset", "()V") {
            let invocation = invocation?;
            self.matrix_reset(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Matrix;"
            && matches!(method.name.as_str(), "preTranslate" | "postTranslate")
            && method.proto.descriptor_is("(FF)Z")
        {
            let invocation = invocation?;
            let value = self.matrix_translate(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Path;",
            "transform",
            "(Landroid/graphics/Matrix;)V",
        ) {
            let invocation = invocation?;
            self.path_transform(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Path;",
            "computeBounds",
            "(Landroid/graphics/RectF;Z)V",
        ) {
            let invocation = invocation?;
            self.path_compute_bounds(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Matrix;", "setTranslate", "(FF)V") {
            let invocation = invocation?;
            self.matrix_set_translate(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Matrix;", "setValues", "([F)V") {
            let invocation = invocation?;
            self.matrix_set_values(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Matrix;", "getValues", "([F)V") {
            let invocation = invocation?;
            self.matrix_get_values(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(
                method.name.as_str(),
                "transformMatrixToGlobal" | "transformMatrixToLocal"
            )
            && method.proto.descriptor_is("(Landroid/graphics/Matrix;)V")
        {
            let invocation = invocation?;
            self.view_transform_matrix(invocation, &method.name)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Canvas;", "<init>", "()V") {
            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "<init>",
            "(Landroid/graphics/Bitmap;)V",
        ) {
            let invocation = invocation?;
            self.canvas_init_with_bitmap(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "<init>",
            "(Ljava/lang/String;)V",
        ) {
            let invocation = invocation?;
            self.render_node_init(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "setClipToBounds",
            "(Z)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_set_clip_to_bounds(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "beginRecording",
            "()Landroid/graphics/RecordingCanvas;",
        ) {
            let invocation = invocation?;
            let canvas = self.render_node_begin_recording(invocation)?;

            return Some(DexExecutionResult::Value(canvas));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "beginRecording",
            "(II)Landroid/graphics/RecordingCanvas;",
        ) {
            let invocation = invocation?;
            let canvas = self.render_node_begin_recording(invocation)?;

            return Some(DexExecutionResult::Value(canvas));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "endRecording", "()V")
        {
            let invocation = invocation?;
            self.render_node_end_recording(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "discardDisplayList",
            "()V",
        ) {
            let invocation = invocation?;
            self.render_node_discard_display_list(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "hasDisplayList",
            "()Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_has_display_list(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/RenderNode;"
            && matches!(
                method.name.as_str(),
                "getLeft" | "getTop" | "getRight" | "getBottom" | "getWidth" | "getHeight"
            )
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.render_node_get_position_dimension(invocation, &method.name)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "getUniqueId", "()J") {
            let invocation = invocation?;
            let value = self.render_node_get_unique_id(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Long(value)));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "offsetLeftAndRight",
            "(I)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_offset_position(invocation, true)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "offsetTopAndBottom",
            "(I)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_offset_position(invocation, false)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "getMatrix",
            "(Landroid/graphics/Matrix;)V",
        ) {
            let invocation = invocation?;
            self.render_node_get_matrix(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "getInverseMatrix",
            "(Landroid/graphics/Matrix;)V",
        ) {
            let invocation = invocation?;
            self.render_node_get_matrix(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/RenderNode;"
            && matches!(
                method.name.as_str(),
                "getAlpha"
                    | "getScaleX"
                    | "getScaleY"
                    | "getElevation"
                    | "getPivotX"
                    | "getPivotY"
                    | "getTranslationX"
                    | "getTranslationY"
                    | "getRotationZ"
                    | "getRotationX"
                    | "getRotationY"
                    | "getCameraDistance"
            )
            && method.proto.descriptor_is("()F")
        {
            let invocation = invocation?;
            let property = render_node_property_name_from_getter(&method.name)?;
            let value = self.render_node_get_float_property(invocation, property)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/graphics/RenderNode;"
            && matches!(
                method.name.as_str(),
                "getAmbientShadowColor" | "getSpotShadowColor"
            )
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let property = render_node_property_name_from_getter(&method.name)?;
            let value = self.render_node_get_int_property(invocation, property)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/RenderNode;"
            && matches!(
                method.name.as_str(),
                "getClipToBounds"
                    | "getClipToOutline"
                    | "getUseCompositingLayer"
                    | "hasOverlappingRendering"
            )
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let property = render_node_property_name_from_getter(&method.name)?;
            let value = self.render_node_get_bool_property(invocation, property)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "setUseCompositingLayer",
            "(ZLandroid/graphics/Paint;)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_set_compositing_layer(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/RenderNode;"
            && matches!(
                method.name.as_str(),
                "setTranslationX"
                    | "setTranslationY"
                    | "setRotationZ"
                    | "setRotationX"
                    | "setRotationY"
                    | "setCameraDistance"
            )
            && method.proto.descriptor_is("(F)Z")
        {
            let invocation = invocation?;
            let property = render_node_property_name_from_setter(&method.name)?;
            let value = self.render_node_set_float_property(invocation, property)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/RenderNode;"
            && matches!(
                method.name.as_str(),
                "setAmbientShadowColor" | "setSpotShadowColor"
            )
            && method.proto.descriptor_is("(I)Z")
        {
            let invocation = invocation?;
            let property = render_node_property_name_from_setter(&method.name)?;
            let value = self.render_node_set_int_property(invocation, property)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "setClipToOutline",
            "(Z)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_set_clip_to_outline(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "setElevation", "(F)Z")
        {
            let invocation = invocation?;
            let value = self.render_node_set_elevation(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "setAlpha", "(F)Z") {
            let invocation = invocation?;
            let value = self.render_node_set_alpha(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "setScaleX", "(F)Z") {
            let invocation = invocation?;
            let value = self.render_node_set_scale_x(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "setScaleY", "(F)Z") {
            let invocation = invocation?;
            let value = self.render_node_set_scale_y(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "setPosition",
            "(IIII)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_set_position(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "setPivotX", "(F)Z") {
            let invocation = invocation?;
            let value = self.render_node_set_pivot_x(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/RenderNode;", "setPivotY", "(F)Z") {
            let invocation = invocation?;
            let value = self.render_node_set_pivot_y(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "setHasOverlappingRendering",
            "(Z)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_set_has_overlapping_rendering(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/RenderNode;",
            "setOutline",
            "(Landroid/graphics/Outline;)Z",
        ) {
            let invocation = invocation?;
            let value = self.render_node_set_outline(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/Path;", "<init>", "()V") {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "setFillType"
            && method
                .proto
                .descriptor_is("(Landroid/graphics/Path$FillType;)V")
        {
            let invocation = invocation?;
            self.path_set_fill_type(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "getFillType"
            && method
                .proto
                .descriptor_is("()Landroid/graphics/Path$FillType;")
        {
            let invocation = invocation?;
            let value = self.path_get_fill_type(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "computeBounds"
            && method.proto.descriptor_is("(Landroid/graphics/RectF;Z)V")
        {
            let invocation = invocation?;
            self.path_compute_bounds(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "isEmpty"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.path_is_empty(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        // cite: frameworks-base Path.java#reset
        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "reset"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.path_rewind(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "rewind"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.path_rewind(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "moveTo"
            && method.proto.descriptor_is("(FF)V")
        {
            let invocation = invocation?;
            self.path_move_to(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "lineTo"
            && method.proto.descriptor_is("(FF)V")
        {
            let invocation = invocation?;
            self.path_line_to(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "rLineTo"
            && method.proto.descriptor_is("(FF)V")
        {
            let invocation = invocation?;
            self.path_r_line_to(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "quadTo"
            && method.proto.descriptor_is("(FFFF)V")
        {
            let invocation = invocation?;
            self.path_quad_to(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && matches!(method.name.as_str(), "cubicTo" | "rCubicTo")
            && method.proto.descriptor_is("(FFFFFF)V")
        {
            let invocation = invocation?;
            self.path_cubic_to(invocation, method.name == "rCubicTo")?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Path;",
            "addRect",
            "(Landroid/graphics/RectF;Landroid/graphics/Path$Direction;)V",
        ) {
            let invocation = invocation?;
            self.path_add_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Path;",
            "op",
            "(Landroid/graphics/Path;Landroid/graphics/Path;Landroid/graphics/Path$Op;)Z",
        ) {
            let invocation = invocation?;
            let value = self.path_op(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Path;",
            "addRoundRect",
            "(Landroid/graphics/RectF;[FLandroid/graphics/Path$Direction;)V",
        ) {
            let invocation = invocation?;
            self.path_add_round_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Path;"
            && method.name == "close"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.path_close(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/view/ViewOutlineProvider;", "<init>", "()V") {
            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Outline;", "<init>", "()V") {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Outline;"
            && method.name == "setRoundRect"
            && method.proto.descriptor_is("(IIIIF)V")
        {
            let invocation = invocation?;
            self.outline_set_round_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Outline;"
            && method.name == "setPath"
            && method.proto.descriptor_is("(Landroid/graphics/Path;)V")
        {
            let invocation = invocation?;
            self.outline_set_path(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base Outline.java#canClip
        if method.class_descriptor == "Landroid/graphics/Outline;"
            && method.name == "canClip"
            && method.proto.descriptor_is("()Z")
        {
            return Some(DexExecutionResult::Int(1));
        }

        if method.class_descriptor == "Landroid/graphics/Outline;"
            && method.name == "setAlpha"
            && method.proto.descriptor_is("(F)V")
        {
            let invocation = invocation?;
            self.outline_set_alpha(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Canvas;"
            && method.name == "translate"
            && method.proto.descriptor_is("(FF)V")
        {
            let invocation = invocation?;
            self.canvas_translate(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Canvas;", "scale", "(FF)V") {
            let invocation = invocation?;
            self.canvas_scale(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Canvas;", "rotate", "(F)V") {
            let invocation = invocation?;
            self.canvas_rotate(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Canvas;", "save", "()I") {
            let invocation = invocation?;
            let value = self.canvas_save(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "saveLayer",
            "(FFFFLandroid/graphics/Paint;)I",
        ) {
            let invocation = invocation?;
            let value = self.canvas_save_layer(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/Canvas;", "restore", "()V") {
            let invocation = invocation?;
            self.canvas_restore(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Canvas;", "enableZ", "()V") {
            let invocation = invocation?;
            self.canvas_set_z_enabled(invocation, true)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Canvas;", "disableZ", "()V") {
            let invocation = invocation?;
            self.canvas_set_z_enabled(invocation, false)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "concat",
            "(Landroid/graphics/Matrix;)V",
        ) {
            let invocation = invocation?;
            self.canvas_concat_matrix(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "clipRect",
            "(FFFFLandroid/graphics/Region$Op;)Z",
        ) {
            let invocation = invocation?;
            let value = self.canvas_clip_rect_region_op(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "clipPath",
            "(Landroid/graphics/Path;Landroid/graphics/Region$Op;)Z",
        ) {
            let invocation = invocation?;
            let value = self.canvas_clip_path_region_op(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "isHardwareAccelerated",
            "()Z",
        ) {
            let invocation = invocation?;
            let value = self.canvas_is_hardware_accelerated(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "getClipBounds",
            "(Landroid/graphics/Rect;)Z",
        ) {
            let invocation = invocation?;
            let value = self.canvas_get_clip_bounds(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawLine",
            "(FFFFLandroid/graphics/Paint;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_line(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawRect",
            "(FFFFLandroid/graphics/Paint;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawCircle",
            "(FFFLandroid/graphics/Paint;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_circle(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawRoundRect",
            "(FFFFFFLandroid/graphics/Paint;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_round_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawPath",
            "(Landroid/graphics/Path;Landroid/graphics/Paint;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_path(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawBitmap",
            "(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/Rect;Landroid/graphics/Paint;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_bitmap_rect(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawBitmap",
            "(Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_bitmap_at(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Canvas;",
            "drawRenderNode",
            "(Landroid/graphics/RenderNode;)V",
        ) {
            let invocation = invocation?;
            self.canvas_draw_render_node(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint$Style;",
            "values",
            "()[Landroid/graphics/Paint$Style;",
        ) {
            let value = self.paint_style_values();

            return Some(DexExecutionResult::Value(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint$Cap;",
            "values",
            "()[Landroid/graphics/Paint$Cap;",
        ) {
            let value = self.paint_cap_values();

            return Some(DexExecutionResult::Value(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint$Join;",
            "values",
            "()[Landroid/graphics/Paint$Join;",
        ) {
            let value = self.paint_join_values();

            return Some(DexExecutionResult::Value(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "<init>", "(I)V") {
            let invocation = invocation?;
            self.paint_init_with_flags(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "<init>", "()V") {
            let invocation = invocation?;
            self.paint_init_default(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "getTextBounds"
            && matches!(
                method.proto.descriptor().as_str(),
                "(Ljava/lang/String;IILandroid/graphics/Rect;)V"
                    | "(Ljava/lang/CharSequence;IILandroid/graphics/Rect;)V"
            )
        {
            let invocation = invocation?;
            self.paint_get_text_bounds(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "getFlags"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.paint_get_flags(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "getColor", "()I") {
            let invocation = invocation?;
            let value = self.paint_get_color(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "getColorLong", "()J") {
            let invocation = invocation?;
            let value = self.paint_get_color_long(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Long(value)));
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "getAlpha", "()I") {
            let invocation = invocation?;
            let value = self.paint_get_alpha(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "isFilterBitmap", "()Z") {
            let invocation = invocation?;
            let value = self.paint_is_filter_bitmap(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "getTextSize"
            && method.proto.descriptor_is("()F")
        {
            let invocation = invocation?;
            let value = self.paint_get_text_size(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "getTextScaleX"
            && method.proto.descriptor_is("()F")
        {
            let invocation = invocation?;
            let value = self.paint_get_text_scale_x(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "getLetterSpacing"
            && method.proto.descriptor_is("()F")
        {
            let invocation = invocation?;
            let value = self.paint_get_letter_spacing(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint;",
            "getTextLocale",
            "()Ljava/util/Locale;",
        ) {
            let invocation = invocation?;
            let value = self.paint_get_text_locale(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "getStrokeWidth", "()F") {
            let invocation = invocation?;
            let value = self.paint_get_stroke_width(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "getStrokeMiter", "()F") {
            let invocation = invocation?;
            let value = self.paint_get_stroke_miter(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint;",
            "getStrokeCap",
            "()Landroid/graphics/Paint$Cap;",
        ) {
            let invocation = invocation?;
            let value = self.paint_get_stroke_cap(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint;",
            "getStrokeJoin",
            "()Landroid/graphics/Paint$Join;",
        ) {
            let invocation = invocation?;
            let value = self.paint_get_stroke_join(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setFlags"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.paint_set_flags(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setHinting"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.paint_set_hinting(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setColor"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.paint_set_color(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setAlpha"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.paint_set_alpha(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "setColor", "(J)V") {
            let invocation = invocation?;
            self.paint_set_color_long(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setTextSize"
            && method.proto.descriptor_is("(F)V")
        {
            let invocation = invocation?;
            self.paint_set_text_size(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setTextScaleX"
            && method.proto.descriptor_is("(F)V")
        {
            let invocation = invocation?;
            self.paint_set_text_scale_x(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setLetterSpacing"
            && method.proto.descriptor_is("(F)V")
        {
            let invocation = invocation?;
            self.paint_set_letter_spacing(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "setStrokeWidth", "(F)V") {
            let invocation = invocation?;
            self.paint_set_stroke_width(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/graphics/Paint;", "setStrokeMiter", "(F)V") {
            let invocation = invocation?;
            self.paint_set_stroke_miter(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint;",
            "setStrokeCap",
            "(Landroid/graphics/Paint$Cap;)V",
        ) {
            let invocation = invocation?;
            self.paint_set_stroke_cap(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint;",
            "setStrokeJoin",
            "(Landroid/graphics/Paint$Join;)V",
        ) {
            let invocation = invocation?;
            self.paint_set_stroke_join(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setShader"
            && method
                .proto
                .descriptor_is("(Landroid/graphics/Shader;)Landroid/graphics/Shader;")
        {
            let invocation = invocation?;
            let value = self.paint_set_shader(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setStyle"
            && method
                .proto
                .descriptor_is("(Landroid/graphics/Paint$Style;)V")
        {
            let invocation = invocation?;
            self.paint_set_style(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setBlendMode"
            && method
                .proto
                .descriptor_is("(Landroid/graphics/BlendMode;)V")
        {
            let invocation = invocation?;
            self.paint_set_blend_mode(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature(
            "Landroid/graphics/Paint;",
            "setColorFilter",
            "(Landroid/graphics/ColorFilter;)Landroid/graphics/ColorFilter;",
        ) {
            let invocation = invocation?;
            let value = self.paint_set_color_filter(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/graphics/Paint;"
            && method.name == "setTypeface"
            && method.proto.descriptor()
                == "(Landroid/graphics/Typeface;)Landroid/graphics/Typeface;"
        {
            let invocation = invocation?;
            let value = self.paint_set_typeface(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/graphics/BlendModeColorFilter;"
            && method.name == "<init>"
            && method
                .proto
                .descriptor_is("(ILandroid/graphics/BlendMode;)V")
        {
            let invocation = invocation?;
            self.blend_mode_color_filter_init(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setTag"
            && method.proto.descriptor_is("(ILjava/lang/Object;)V")
        {
            let invocation = invocation?;
            self.write_view_tag(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setTag"
            && method.proto.descriptor_is("(Ljava/lang/Object;)V")
        {
            let invocation = invocation?;
            self.write_default_view_tag(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getTag"
            && method.proto.descriptor_is("(I)Ljava/lang/Object;")
        {
            let invocation = invocation?;
            let value = self.read_view_tag(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getTag"
            && method.proto.descriptor_is("()Ljava/lang/Object;")
        {
            let invocation = invocation?;
            let value = self.read_default_view_tag(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base ViewParent.java#getParent
        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/View;" | "Landroid/view/ViewParent;"
        ) && method.name == "getParent"
            && method.proto.descriptor_is("()Landroid/view/ViewParent;")
        {
            let value = invocation
                .and_then(|invocation| self.read_view_parent(invocation))
                .unwrap_or(DexValue::Null);

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getDisplay"
            && method.proto.descriptor_is("()Landroid/view/Display;")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }

            return Some(DexExecutionResult::Value(DexValue::Reference(
                std::rc::Rc::from("Landroid/view/Display;.bootstrap"),
            )));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isInEditMode"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }

            return Some(DexExecutionResult::Int(0));
        }

        // cite: frameworks-base ViewGroup.java#requestDisallowInterceptTouchEvent, FLAG_DISALLOW_INTERCEPT
        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/ViewParent;" | "Landroid/view/ViewGroup;"
        ) && method.name == "requestDisallowInterceptTouchEvent"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 2
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }
            let DexValue::Int(disallow) = invocation.arguments[1] else {
                return None;
            };
            self.write_stored_instance_field(
                &invocation.arguments[0],
                &DexFieldId {
                    class_descriptor: "Landroid/view/ViewGroup;".to_owned(),
                    type_descriptor: "Z".to_owned(),
                    name: "disallowIntercept".to_owned(),
                },
                DexValue::Int(i32::from(disallow != 0)),
            );

            return Some(DexExecutionResult::Void);
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/ViewParent;" | "Landroid/view/ViewGroup;"
        ) && method.name == "onDescendantInvalidated"
            && method
                .proto
                .descriptor_is("(Landroid/view/View;Landroid/view/View;)V")
        {
            let invocation = invocation?;
            self.view_parent_on_descendant_invalidated(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getRootView"
            && method.proto.descriptor_is("()Landroid/view/View;")
        {
            let invocation = invocation?;
            let value = self.read_view_root(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getRootWindowInsets"
            && method.proto.descriptor_is("()Landroid/view/WindowInsets;")
        {
            let invocation = invocation?;
            let value = self.view_root_window_insets(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getWindowInsetsController"
            && method
                .proto
                .descriptor_is("()Landroid/view/WindowInsetsController;")
        {
            let invocation = invocation?;
            let value = view_window_insets_controller(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base WindowOnBackInvokedDispatcher.java#registerOnBackInvokedCallback, registerOnBackInvokedCallbackUnchecked
        if method.class_descriptor == "Landroid/window/OnBackInvokedDispatcher;"
            && method.name == "registerOnBackInvokedCallback"
            && method
                .proto
                .descriptor_is("(ILandroid/window/OnBackInvokedCallback;)V")
        {
            let invocation = invocation?;
            self.register_on_back_invoked_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base WindowOnBackInvokedDispatcher.java#unregisterOnBackInvokedCallback
        if method.class_descriptor == "Landroid/window/OnBackInvokedDispatcher;"
            && method.name == "unregisterOnBackInvokedCallback"
            && method
                .proto
                .descriptor_is("(Landroid/window/OnBackInvokedCallback;)V")
        {
            let invocation = invocation?;
            self.unregister_on_back_invoked_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "findOnBackInvokedDispatcher"
            && method
                .proto
                .descriptor_is("()Landroid/window/OnBackInvokedDispatcher;")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }

            return Some(DexExecutionResult::Value(DexValue::Reference(
                std::rc::Rc::from("Landroid/window/OnBackInvokedDispatcher;.bootstrap"),
            )));
        }

        if is_bootstrap_view_constructor_class(&method.class_descriptor)
            && matches!(
                method.name.as_str(),
                "onAttachedToWindow" | "onDetachedFromWindow"
            )
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getHandler"
            && method.proto.descriptor_is("()Landroid/os/Handler;")
        {
            let invocation = invocation?;
            let value = self.view_handler(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getViewTreeObserver"
            && method
                .proto
                .descriptor_is("()Landroid/view/ViewTreeObserver;")
        {
            let invocation = invocation?;
            let value = view_tree_observer_reference(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "isAlive"
            && method.proto.descriptor_is("()Z")
        {
            return Some(DexExecutionResult::Int(1));
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "addOnGlobalLayoutListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnGlobalLayoutListener;)V"
        {
            let invocation = invocation?;
            self.add_view_tree_observer_global_layout_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "removeOnGlobalLayoutListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnGlobalLayoutListener;)V"
        {
            let invocation = invocation?;
            self.remove_view_tree_observer_global_layout_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "addOnScrollChangedListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnScrollChangedListener;)V"
        {
            let invocation = invocation?;
            self.add_view_tree_observer_scroll_changed_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "removeOnScrollChangedListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnScrollChangedListener;)V"
        {
            let invocation = invocation?;
            self.remove_view_tree_observer_scroll_changed_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "addOnPreDrawListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewTreeObserver$OnPreDrawListener;)V")
        {
            let invocation = invocation?;
            self.add_view_tree_observer_pre_draw_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "removeOnPreDrawListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewTreeObserver$OnPreDrawListener;)V")
        {
            let invocation = invocation?;
            self.remove_view_tree_observer_pre_draw_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "addOnDrawListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewTreeObserver$OnDrawListener;)V")
        {
            let invocation = invocation?;
            self.add_view_tree_observer_draw_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "removeOnDrawListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewTreeObserver$OnDrawListener;)V")
        {
            let invocation = invocation?;
            self.remove_view_tree_observer_draw_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "addOnWindowFocusChangeListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnWindowFocusChangeListener;)V"
        {
            let invocation = invocation?;
            self.add_view_tree_observer_window_focus_change_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "removeOnWindowFocusChangeListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnWindowFocusChangeListener;)V"
        {
            let invocation = invocation?;
            self.remove_view_tree_observer_window_focus_change_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "addOnTouchModeChangeListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnTouchModeChangeListener;)V"
        {
            let invocation = invocation?;
            self.add_view_tree_observer_touch_mode_change_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewTreeObserver;"
            && method.name == "removeOnTouchModeChangeListener"
            && method.proto.descriptor()
                == "(Landroid/view/ViewTreeObserver$OnTouchModeChangeListener;)V"
        {
            let invocation = invocation?;
            self.remove_view_tree_observer_touch_mode_change_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "addOnAttachStateChangeListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnAttachStateChangeListener;)V")
        {
            let invocation = invocation?;
            self.add_view_attach_state_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "removeOnAttachStateChangeListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnAttachStateChangeListener;)V")
        {
            let invocation = invocation?;
            self.remove_view_attach_state_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isAttachedToWindow"
            && method.proto.descriptor_is("()Z")
        {
            let value = invocation
                .and_then(|invocation| self.view_is_attached_to_window(invocation))
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isHardwareAccelerated"
            && method.proto.descriptor_is("()Z")
        {
            let value = invocation
                .and_then(|invocation| self.view_is_hardware_accelerated(invocation))
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "hasWindowFocus"
            && method.proto.descriptor_is("()Z")
        {
            let value = invocation
                .and_then(|invocation| self.view_has_window_focus(invocation))
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View$MeasureSpec;"
            && method.name == "makeMeasureSpec"
            && method.proto.descriptor_is("(II)I")
        {
            let invocation = invocation?;
            let value = measure_spec_make(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View$MeasureSpec;"
            && matches!(method.name.as_str(), "getMode" | "getSize")
            && method.proto.descriptor_is("(I)I")
        {
            let invocation = invocation?;
            let value = measure_spec_part(invocation, &method.name)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "layout"
            && method.proto.descriptor_is("(IIII)V")
        {
            let invocation = invocation?;
            self.write_view_layout(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(
                method.name.as_str(),
                "setLeft" | "setTop" | "setRight" | "setBottom"
            )
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_bound(invocation, &method.name)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setMeasuredDimension"
            && method.proto.descriptor_is("(II)V")
        {
            let invocation = invocation?;
            self.write_view_set_measured_dimension(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "onMeasure"
            && method.proto.descriptor_is("(II)V")
        {
            let invocation = invocation?;
            self.write_view_measure(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/View;" | "Landroid/view/ViewGroup;"
        ) && method.name == "onLayout"
            && method.proto.descriptor_is("(ZIIII)V")
        {
            let invocation = invocation?;
            self.write_view_on_layout(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/View;" | "Landroid/view/ViewGroup;"
        ) && matches!(
            (method.name.as_str(), method.proto.descriptor().as_str()),
            ("draw", "(Landroid/graphics/Canvas;)V")
                | ("onDraw", "(Landroid/graphics/Canvas;)V")
                | ("dispatchDraw", "(Landroid/graphics/Canvas;)V")
        ) {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "invalidate"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.write_view_invalidated(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(
                method.name.as_str(),
                "getMeasuredWidth" | "getMeasuredHeight"
            )
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_measured_dimension(invocation, &method.name)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "unscheduleDrawable"
            && method
                .proto
                .descriptor_is("(Landroid/graphics/drawable/Drawable;)V")
        {
            let invocation = invocation?;
            self.view_unschedule_drawable(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getResources"
            && method
                .proto
                .descriptor_is("()Landroid/content/res/Resources;")
        {
            return Some(DexExecutionResult::Value(DexValue::Reference(
                std::rc::Rc::from("Landroid/content/res/Resources;.bootstrap"),
            )));
        }

        // cite: frameworks-base View.java#setForeground, mForegroundInfo
        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setForeground"
            && method
                .proto
                .descriptor_is("(Landroid/graphics/drawable/Drawable;)V")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 2
                || !dex_value_is_object_like(&invocation.arguments[0])
                || !matches!(invocation.arguments[1], DexValue::Null | DexValue::Int(0))
            {
                return None;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(
                method.name.as_str(),
                "setBackground" | "setBackgroundDrawable"
            )
            && method
                .proto
                .descriptor_is("(Landroid/graphics/drawable/Drawable;)V")
        {
            let invocation = invocation?;
            self.write_view_background(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setBackgroundColor"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_background_color(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base View.java#setMinimumWidth
        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setMinimumWidth"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field_and_request_layout(invocation, &view_minimum_width_field())?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base View.java#setMinimumHeight
        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setMinimumHeight"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field_and_request_layout(invocation, &view_minimum_height_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setBackgroundResource"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            let DexValue::Int(resource_id) = *invocation.arguments.get(1)? else {
                return None;
            };
            let background = if resource_id == 0 {
                DexValue::Null
            } else {
                self.drawable_resource_value(u32::from_ne_bytes(resource_id.to_ne_bytes()))
            };
            self.write_stored_instance_field(
                invocation.arguments.first()?,
                &view_background_field(),
                background,
            );

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getBackground"
            && method
                .proto
                .descriptor_is("()Landroid/graphics/drawable/Drawable;")
        {
            let invocation = invocation?;
            let value = self.read_view_background(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setBackgroundTintList"
            && method
                .proto
                .descriptor_is("(Landroid/content/res/ColorStateList;)V")
        {
            let invocation = invocation?;
            self.write_view_background_tint_list(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getBackgroundTintList"
            && method
                .proto
                .descriptor_is("()Landroid/content/res/ColorStateList;")
        {
            let invocation = invocation?;
            let value = self.read_view_background_tint_list(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setBackgroundTintMode"
            && method
                .proto
                .descriptor_is("(Landroid/graphics/PorterDuff$Mode;)V")
        {
            let invocation = invocation?;
            self.write_view_background_tint_mode(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getBackgroundTintMode"
            && method
                .proto
                .descriptor_is("()Landroid/graphics/PorterDuff$Mode;")
        {
            let invocation = invocation?;
            let value = self.read_view_background_tint_mode(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOutlineProvider"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewOutlineProvider;)V")
        {
            let invocation = invocation?;
            self.write_view_outline_provider(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getOutlineProvider"
            && method
                .proto
                .descriptor_is("()Landroid/view/ViewOutlineProvider;")
        {
            let invocation = invocation?;
            let value = self.read_view_outline_provider(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setContentDescription"
            && method.proto.descriptor_is("(Ljava/lang/CharSequence;)V")
        {
            let invocation = invocation?;
            self.write_view_content_description(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getContentDescription"
            && method.proto.descriptor_is("()Ljava/lang/CharSequence;")
        {
            let invocation = invocation?;
            let value = self.read_view_content_description(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setTooltipText"
            && method.proto.descriptor_is("(Ljava/lang/CharSequence;)V")
        {
            let invocation = invocation?;
            self.write_view_tooltip_text(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getTooltipText"
            && method.proto.descriptor_is("()Ljava/lang/CharSequence;")
        {
            let invocation = invocation?;
            let value = self.read_view_tooltip_text(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getContext"
            && method.proto.descriptor_is("()Landroid/content/Context;")
        {
            return Some(DexExecutionResult::Value(self.active_context_value()));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(
                method.name.as_str(),
                "getWindowToken" | "getApplicationWindowToken"
            )
            && method.proto.descriptor_is("()Landroid/os/IBinder;")
        {
            let invocation = invocation?;
            let value = view_window_token(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getAutofillId"
            && method
                .proto
                .descriptor_is("()Landroid/view/autofill/AutofillId;")
        {
            let invocation = invocation?;
            let value = self.view_autofill_id(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isLaidOut"
            && method.proto.descriptor_is("()Z")
        {
            let value = invocation
                .and_then(|invocation| self.read_view_is_laid_out(invocation))
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isShowingLayoutBounds"
            && method.proto.descriptor_is("()Z")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/View;" | "Landroid/widget/AbsListView;" | "Landroid/widget/ListView;"
        ) && method.name == "isInTouchMode"
            && method.proto.descriptor_is("()Z")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && is_view_dimension_getter(&method.name)
            && method.proto.descriptor_is("()I")
        {
            let value = invocation
                .and_then(|invocation| self.read_view_dimension(invocation, &method.name))
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(
                method.name.as_str(),
                "getLocationOnScreen" | "getLocationInWindow"
            )
            && method.proto.descriptor_is("([I)V")
        {
            let invocation = invocation?;
            self.write_view_location_into_array(invocation, method.name == "getLocationOnScreen")?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getWindowVisibleDisplayFrame"
            && method.proto.descriptor_is("(Landroid/graphics/Rect;)V")
        {
            let invocation = invocation?;
            self.write_view_window_visible_display_frame(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setPadding"
            && method.proto.descriptor_is("(IIII)V")
        {
            let invocation = invocation?;
            self.write_view_padding(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && is_view_padding_getter(&method.name)
            && method.proto.descriptor_is("()I")
        {
            let padding_name = match method.name.as_str() {
                "getPaddingLeft" => "paddingLeft",
                "getPaddingTop" => "paddingTop",
                "getPaddingRight" => "paddingRight",
                "getPaddingBottom" => "paddingBottom",
                _ => return None,
            };
            let value = invocation
                .and_then(|invocation| invocation.arguments.first())
                .and_then(|receiver| {
                    self.read_stored_instance_field(receiver, &view_padding_field(padding_name))
                })
                .and_then(|value| match value {
                    DexValue::Int(value) => Some(value),
                    _ => None,
                })
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOnApplyWindowInsetsListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnApplyWindowInsetsListener;)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setWindowInsetsAnimationCallback"
            && method
                .proto
                .descriptor_is("(Landroid/view/WindowInsetsAnimation$Callback;)V")
        {
            let invocation = invocation?;
            self.write_view_window_insets_animation_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setViewTranslationCallback"
            && method
                .proto
                .descriptor_is("(Landroid/view/translation/ViewTranslationCallback;)V")
        {
            let invocation = invocation?;
            self.write_view_translation_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/WindowInsetsAnimation$Callback;"
            && method.name == "<init>"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field(
                invocation,
                &window_insets_animation_callback_dispatch_mode_field(),
            )?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/WindowInsetsAnimation$Callback;"
            && method.name == "getDispatchMode"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_int_field(
                invocation,
                &window_insets_animation_callback_dispatch_mode_field(),
                0,
            )?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOnClickListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnClickListener;)V")
        {
            let invocation = invocation?;
            self.write_view_click_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOnLongClickListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnLongClickListener;)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOnKeyListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnKeyListener;)V")
        {
            let invocation = invocation?;
            self.write_view_key_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base View.java#setOnGenericMotionListener
        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOnGenericMotionListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnGenericMotionListener;)V")
        {
            let invocation = invocation?;
            self.write_view_listener_field(
                invocation,
                "Landroid/view/View;",
                "Landroid/view/View$OnGenericMotionListener;",
                "onGenericMotionListener",
            )?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base View.java#onGenericMotionEvent
        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/View;" | "Landroid/view/ViewGroup;"
        ) && method.name == "onGenericMotionEvent"
            && method.proto.descriptor_is("(Landroid/view/MotionEvent;)Z")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOnTouchListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnTouchListener;)V")
        {
            let invocation = invocation?;
            self.write_view_touch_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOnDragListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnDragListener;)V")
        {
            let invocation = invocation?;
            self.write_view_drag_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "playSoundEffect"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 2 {
                return None;
            }
            if !dex_value_is_object_like(&invocation.arguments[0]) {
                return None;
            }
            if !matches!(invocation.arguments[1], DexValue::Int(_)) {
                return None;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View$AccessibilityDelegate;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/accessibility/AccessibilityNodeProvider;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setAccessibilityDelegate"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$AccessibilityDelegate;)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "postOnAnimation"
            && method.proto.descriptor_is("(Ljava/lang/Runnable;)V")
        {
            let invocation = invocation?;
            self.post_view_runnable(invocation, Some(0))?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "post"
            && method.proto.descriptor_is("(Ljava/lang/Runnable;)Z")
        {
            let invocation = invocation?;
            let value = self.post_view_runnable(invocation, Some(0))?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "postDelayed"
            && method.proto.descriptor_is("(Ljava/lang/Runnable;J)Z")
        {
            let invocation = invocation?;
            let value = self.post_view_runnable(invocation, None)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "removeCallbacks"
            && method.proto.descriptor_is("(Ljava/lang/Runnable;)Z")
        {
            let invocation = invocation?;
            let value = self.remove_view_callbacks(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setVisibility"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_visibility(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getVisibility"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_visibility(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isShown"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.view_is_shown(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setSystemUiVisibility"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_system_ui_visibility(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getSystemUiVisibility"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_system_ui_visibility(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setLayoutParams"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewGroup$LayoutParams;)V")
        {
            let invocation = invocation?;
            self.write_view_layout_params(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getLayoutParams"
            && method
                .proto
                .descriptor_is("()Landroid/view/ViewGroup$LayoutParams;")
        {
            let invocation = invocation?;
            let value = self.read_view_layout_params(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "generateDefaultLayoutParams"
            && method
                .proto
                .descriptor_is("()Landroid/view/ViewGroup$LayoutParams;")
        {
            let value = self.generate_default_layout_params();

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "checkLayoutParams"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewGroup$LayoutParams;)Z")
        {
            let invocation = invocation?;
            let value = self.view_group_check_layout_params(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        // cite: frameworks-base RelativeLayout.java#LayoutParams
        // cite: frameworks-base LinearLayout.java#LayoutParams
        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/ViewGroup$LayoutParams;"
                | "Landroid/view/ViewGroup$MarginLayoutParams;"
                | "Landroid/widget/RelativeLayout$LayoutParams;"
                | "Landroid/widget/LinearLayout$LayoutParams;"
        ) && method.name == "<init>"
            && method.proto.descriptor_is("(II)V")
        {
            let invocation = invocation?;
            self.write_view_group_layout_params_size(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base LinearLayout.java#LayoutParams
        if method.class_descriptor == "Landroid/widget/LinearLayout$LayoutParams;"
            && method.name == "<init>"
            && method.proto.descriptor_is("(IIF)V")
        {
            let invocation = invocation?;
            self.write_linear_layout_params_size_and_weight(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup$MarginLayoutParams;"
            && method.name == "setMargins"
            && method.proto.descriptor_is("(IIII)V")
        {
            let invocation = invocation?;
            self.write_view_group_margin_layout_params_margins(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/RelativeLayout$LayoutParams;"
            && method.name == "addRule"
            && method.proto.descriptor_is("(II)V")
        {
            let invocation = invocation?;
            self.relative_layout_params_add_rule(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/LinearLayout;"
            && method.name == "setOrientation"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field_and_request_layout(
                invocation,
                &linear_layout_orientation_field(),
            )?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/LinearLayout;"
            && method.name == "setGravity"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field_and_request_layout(
                invocation,
                &linear_layout_gravity_field(),
            )?;

            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base FrameLayout.java#setForegroundGravity
        if method.class_descriptor == "Landroid/widget/FrameLayout;"
            && method.name == "setForegroundGravity"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field_and_request_layout(
                invocation,
                &frame_layout_foreground_gravity_field(),
            )?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setFocusable"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_focusable_bool(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setFocusable"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_focusable_mode(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getFocusable"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_int_field(invocation, &view_focusable_mode_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setFocusableInTouchMode"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            let field = view_focus_field(&method.name)?;
            self.write_view_bool_field(invocation, &field)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(
                method.name.as_str(),
                "isFocusable" | "isFocusableInTouchMode"
            )
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let field = view_focus_field(&method.name)?;
            let value = self.read_view_bool_field(invocation, &field, 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setDefaultFocusHighlightEnabled"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_bool_field(invocation, &view_default_focus_highlight_enabled_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getDefaultFocusHighlightEnabled"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.read_view_bool_field(
                invocation,
                &view_default_focus_highlight_enabled_field(),
                1,
            )?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && matches!(method.name.as_str(), "isFocused" | "hasFocus")
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.read_view_bool_field(invocation, &view_focused_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setForceDarkAllowed"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_bool_field(invocation, &view_force_dark_allowed_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isForceDarkAllowed"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value =
                self.read_view_bool_field(invocation, &view_force_dark_allowed_field(), 1)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setOverScrollMode"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field(invocation, &view_over_scroll_mode_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/view/View;", "setLayoutDirection", "(I)V") {
            let invocation = invocation?;
            self.write_view_layout_direction(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getOverScrollMode"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_int_field(invocation, &view_over_scroll_mode_field(), 1)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setKeepScreenOn"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_bool_field(invocation, &view_keep_screen_on_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getKeepScreenOn"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.read_view_bool_field(invocation, &view_keep_screen_on_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.matches_builtin_signature("Landroid/view/View;", "setRequestedFrameRate", "(F)V")
        {
            let invocation = invocation?;
            self.write_view_requested_frame_rate(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/view/View;", "setAlpha", "(F)V") {
            let invocation = invocation?;
            self.write_view_render_node_float_property(
                invocation,
                &view_float_field("alpha"),
                &render_node_alpha_field(),
                1.0,
            )?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/view/View;", "getAlpha", "()F") {
            let invocation = invocation?;
            let value = self.read_view_float_field(invocation, &view_float_field("alpha"), 1.0)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.matches_builtin_signature("Landroid/view/View;", "setElevation", "(F)V") {
            let invocation = invocation?;
            self.write_view_render_node_float_property(
                invocation,
                &view_float_field("elevation"),
                &render_node_elevation_field(),
                0.0,
            )?;

            return Some(DexExecutionResult::Void);
        }

        if method.matches_builtin_signature("Landroid/view/View;", "getElevation", "()F") {
            let invocation = invocation?;
            let value =
                self.read_view_float_field(invocation, &view_float_field("elevation"), 0.0)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setImportantForAccessibility"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field(invocation, &view_important_for_accessibility_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getImportantForAccessibility"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value =
                self.read_view_int_field(invocation, &view_important_for_accessibility_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setImportantForAutofill"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field(invocation, &view_important_for_autofill_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getImportantForAutofill"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value =
                self.read_view_int_field(invocation, &view_important_for_autofill_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setImportantForContentCapture"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field(invocation, &view_important_for_content_capture_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getImportantForContentCapture"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_int_field(
                invocation,
                &view_important_for_content_capture_field(),
                0,
            )?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getContentCaptureSession"
            && method
                .proto
                .descriptor_is("()Landroid/view/contentcapture/ContentCaptureSession;")
        {
            return Some(DexExecutionResult::Value(DexValue::Null));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setWillNotDraw"
            && method.proto.descriptor_is("(Z)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setEnabled"
            && method.proto.descriptor_is("(Z)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setClickable"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_bool_field(invocation, &view_clickable_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isClickable"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.read_view_bool_field(invocation, &view_clickable_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setLongClickable"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_bool_field(invocation, &view_long_clickable_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isLongClickable"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.read_view_bool_field(invocation, &view_long_clickable_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setScrollContainer"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_bool_field(invocation, &view_scroll_container_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isScrollContainer"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.read_view_bool_field(invocation, &view_scroll_container_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setSaveFromParentEnabled"
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_bool_field(invocation, &view_save_from_parent_enabled_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setSystemGestureExclusionRects"
            && method.proto.descriptor_is("(Ljava/util/List;)V")
        {
            let invocation = invocation?;
            self.write_view_system_gesture_exclusion_rects(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isSaveFromParentEnabled"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value =
                self.read_view_bool_field(invocation, &view_save_from_parent_enabled_field(), 1)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "requestLayout"
            && method.proto.descriptor_is("()V")
        {
            if let Some(invocation) = invocation {
                self.write_view_layout_requested(invocation)?;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "requestApplyInsets"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "forceLayout"
            && method.proto.descriptor_is("()V")
        {
            if let Some(invocation) = invocation {
                self.write_view_layout_requested(invocation)?;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "isLayoutRequested"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.read_view_bool_field(invocation, &view_layout_requested_field(), 0)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "awakenScrollBars"
            && method.proto.descriptor_is("()Z")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/View;" | "Landroid/view/ViewGroup;"
        ) && method.name == "requestFocus"
            && matches!(
                method.proto.descriptor().as_str(),
                "()Z" | "(I)Z" | "(ILandroid/graphics/Rect;)Z"
            )
        {
            let invocation = invocation?;
            let value = self.request_view_focus(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/View;" | "Landroid/view/ViewGroup;"
        ) && method.name == "clearFocus"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.clear_view_focus(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "requestRectangleOnScreen"
            && method.proto.descriptor_is("(Landroid/graphics/Rect;Z)Z")
        {
            let invocation = invocation?;
            view_request_rectangle_on_screen(invocation)?;

            return Some(DexExecutionResult::Int(0));
        }

        // cite: frameworks-base View.java#requestRectangleOnScreen
        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "requestRectangleOnScreen"
            && method.proto.descriptor_is("(Landroid/graphics/Rect;)Z")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 2
                || !dex_value_is_object_like(&invocation.arguments[0])
                || !dex_value_is_object_like(&invocation.arguments[1])
            {
                return None;
            }

            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "findFocus"
            && method.proto.descriptor_is("()Landroid/view/View;")
        {
            return Some(DexExecutionResult::Value(DexValue::Null));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "focusableViewAvailable"
            && method.proto.descriptor_is("(Landroid/view/View;)V")
        {
            return Some(DexExecutionResult::Void);
        }

        // cite: frameworks-base ViewGroup.java#shouldDelayChildPressedState
        if method.matches_builtin_signature(
            "Landroid/view/ViewGroup;",
            "shouldDelayChildPressedState",
            "()Z",
        ) {
            let invocation = invocation?;
            if invocation.arguments.len() != 1
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }

            return Some(DexExecutionResult::Int(1));
        }

        // cite: frameworks-base FrameLayout.java#shouldDelayChildPressedState
        // cite: frameworks-base LinearLayout.java#shouldDelayChildPressedState
        // cite: frameworks-base RelativeLayout.java#shouldDelayChildPressedState
        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/widget/FrameLayout;"
                | "Landroid/widget/LinearLayout;"
                | "Landroid/widget/RelativeLayout;"
        ) && method.name == "shouldDelayChildPressedState"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }

            return Some(DexExecutionResult::Int(0));
        }

        // cite: frameworks-base ScrollView.java#shouldDelayChildPressedState
        // cite: frameworks-base HorizontalScrollView.java#shouldDelayChildPressedState
        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/widget/ScrollView;" | "Landroid/widget/HorizontalScrollView;"
        ) && method.name == "shouldDelayChildPressedState"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }

            return Some(DexExecutionResult::Int(1));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "performHapticFeedback"
            && method.proto.descriptor_is("(I)Z")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "saveAttributeDataForStyleable"
            && method.proto.descriptor()
                == "(Landroid/content/Context;[ILandroid/util/AttributeSet;Landroid/content/res/TypedArray;II)V"
        {
            let invocation = invocation?;
            view_save_attribute_data_for_styleable(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "setId"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_id(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getId"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_id(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getUniqueDrawingId"
            && method.proto.descriptor_is("()J")
        {
            let invocation = invocation?;
            let value = self.view_unique_drawing_id(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Long(value)));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "getDrawingTime"
            && method.proto.descriptor_is("()J")
        {
            let invocation = invocation?;
            let value = self.read_view_drawing_time(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Long(value)));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "findViewById"
            && method.proto.descriptor_is("(I)Landroid/view/View;")
        {
            let invocation = invocation?;
            let value = self.view_find_view_by_id(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/widget/Toolbar;"
            && method.name == "setNavigationIcon"
            && matches!(
                method.proto.descriptor().as_str(),
                "(I)V" | "(Landroid/graphics/drawable/Drawable;)V"
            )
        {
            let invocation = invocation?;
            self.write_toolbar_navigation_icon(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/Toolbar;"
            && method.name == "setNavigationOnClickListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/View$OnClickListener;)V")
        {
            let invocation = invocation?;
            self.write_view_click_listener(invocation)?;
            if let Some(listener) = invocation.arguments.get(1) {
                self.runtime.borrow_mut().action_bar_nav_click = Some(listener.clone());
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/TextView;"
            && method.name == "setOnEditorActionListener"
            && method
                .proto
                .descriptor_is("(Landroid/widget/TextView$OnEditorActionListener;)V")
        {
            let invocation = invocation?;
            self.write_view_listener_field(
                invocation,
                "Landroid/widget/TextView;",
                "Landroid/widget/TextView$OnEditorActionListener;",
                "onEditorActionListener",
            )?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/AdapterView;"
            && method.name == "setOnItemLongClickListener"
            && method.proto.descriptor()
                == "(Landroid/widget/AdapterView$OnItemLongClickListener;)V"
        {
            let invocation = invocation?;
            self.write_view_listener_field(
                invocation,
                "Landroid/widget/AdapterView;",
                "Landroid/widget/AdapterView$OnItemLongClickListener;",
                "onItemLongClickListener",
            )?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "setOnHierarchyChangeListener"
            && method
                .proto
                .descriptor_is("(Landroid/view/ViewGroup$OnHierarchyChangeListener;)V")
        {
            let invocation = invocation?;
            self.write_view_hierarchy_change_listener(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "removeAllViews"
            && method.proto.descriptor_is("()V")
        {
            if let Some(invocation) = invocation {
                self.view_group_remove_all_views(invocation)?;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "removeView"
            && method.proto.descriptor_is("(Landroid/view/View;)V")
        {
            let invocation = invocation?;
            self.view_group_remove_view(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "removeViewAt"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.view_group_remove_view_at(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "addView"
            && method.proto.descriptor_is("(Landroid/view/View;)V")
        {
            if let Some(invocation) = invocation {
                self.view_group_add_view(invocation)?;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "addView"
            && method.proto.descriptor()
                == "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V"
        {
            let invocation = invocation?;
            self.view_group_add_view_with_layout_params(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewManager;"
            && method.name == "addView"
            && method.proto.descriptor()
                == "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V"
        {
            let invocation = invocation?;
            self.view_manager_add_view(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/view/ViewManager;" | "Landroid/view/WindowManager;"
        ) && matches!(method.name.as_str(), "removeView" | "removeViewImmediate")
            && method.proto.descriptor_is("(Landroid/view/View;)V")
        {
            let invocation = invocation?;
            self.view_manager_remove_view(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewManager;"
            && method.name == "updateViewLayout"
            && method.proto.descriptor()
                == "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V"
        {
            let invocation = invocation?;
            self.view_manager_update_view_layout(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "addView"
            && method.proto.descriptor_is("(Landroid/view/View;I)V")
        {
            let invocation = invocation?;
            self.view_group_add_view_at_index(invocation, false)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "addView"
            && method.proto.descriptor()
                == "(Landroid/view/View;ILandroid/view/ViewGroup$LayoutParams;)V"
        {
            let invocation = invocation?;
            self.view_group_add_view_at_index(invocation, true)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "addViewInLayout"
            && method.proto.descriptor()
                == "(Landroid/view/View;ILandroid/view/ViewGroup$LayoutParams;Z)Z"
        {
            let invocation = invocation?;
            let value = self.view_group_add_view_in_layout(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "getChildCount"
            && method.proto.descriptor_is("()I")
        {
            let value = invocation
                .and_then(|invocation| self.view_group_child_count(invocation))
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "getChildAt"
            && method.proto.descriptor_is("(I)Landroid/view/View;")
        {
            let invocation = invocation?;
            let value = self.view_group_child_at(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "setDescendantFocusability"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            self.write_view_int_field(invocation, &view_group_descendant_focusability_field())?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "getDescendantFocusability"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.read_view_int_field(
                invocation,
                &view_group_descendant_focusability_field(),
                0,
            )?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && matches!(method.name.as_str(), "setClipChildren" | "setClipToPadding")
            && method.proto.descriptor_is("(Z)V")
        {
            let invocation = invocation?;
            self.write_view_int_field(invocation, &view_group_clip_field(&method.name))?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && matches!(method.name.as_str(), "getClipChildren" | "getClipToPadding")
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value =
                self.read_view_int_field(invocation, &view_group_clip_field(&method.name), 1)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "requestLayout"
            && method.proto.descriptor_is("()V")
        {
            if let Some(invocation) = invocation {
                self.write_view_layout_requested(invocation)?;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewGroup;"
            && method.name == "setLayoutTransition"
            && method
                .proto
                .descriptor_is("(Landroid/animation/LayoutTransition;)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewConfiguration;"
            && method.name == "get"
            && method.proto.descriptor()
                == "(Landroid/content/Context;)Landroid/view/ViewConfiguration;"
        {
            return Some(DexExecutionResult::Value(DexValue::Reference(
                std::rc::Rc::from("Landroid/view/ViewConfiguration;.bootstrap"),
            )));
        }

        if method.class_descriptor == "Landroid/view/KeyCharacterMap;"
            && method.name == "load"
            && method
                .proto
                .descriptor_is("(I)Landroid/view/KeyCharacterMap;")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1 {
                return None;
            }
            let DexValue::Int(device_id) = invocation.arguments[0] else {
                return None;
            };

            return Some(DexExecutionResult::Value(DexValue::Reference(
                std::rc::Rc::from(format!("Landroid/view/KeyCharacterMap;:device:{device_id}")),
            )));
        }

        if method.class_descriptor == "Landroid/view/KeyCharacterMap;"
            && method.name == "getKeyboardType"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1
                || !dex_value_is_object_like(&invocation.arguments[0])
            {
                return None;
            }

            return Some(DexExecutionResult::Int(3));
        }

        if method.class_descriptor == "Landroid/view/Choreographer;"
            && method.name == "getInstance"
            && method.proto.descriptor_is("()Landroid/view/Choreographer;")
        {
            return Some(DexExecutionResult::Value(DexValue::Reference(
                std::rc::Rc::from("Landroid/view/Choreographer;:main"),
            )));
        }

        if method.class_descriptor == "Landroid/view/Choreographer;"
            && method.name == "postFrameCallback"
            && method
                .proto
                .descriptor_is("(Landroid/view/Choreographer$FrameCallback;)V")
        {
            let invocation = invocation?;
            self.post_choreographer_frame_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/Choreographer;"
            && method.name == "removeFrameCallback"
            && method
                .proto
                .descriptor_is("(Landroid/view/Choreographer$FrameCallback;)V")
        {
            let invocation = invocation?;
            self.remove_choreographer_frame_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/Choreographer;"
            && method.name == "postFrameCallbackDelayed"
            && method
                .proto
                .descriptor_is("(Landroid/view/Choreographer$FrameCallback;J)V")
        {
            let invocation = invocation?;
            self.post_delayed_choreographer_frame_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/Choreographer;"
            && method.name == "postCallback"
            && method
                .proto
                .descriptor_is("(ILjava/lang/Runnable;Ljava/lang/Object;)V")
        {
            let invocation = invocation?;
            self.post_choreographer_callback(invocation, false)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/Choreographer;"
            && method.name == "postCallbackDelayed"
            && method
                .proto
                .descriptor_is("(ILjava/lang/Runnable;Ljava/lang/Object;J)V")
        {
            let invocation = invocation?;
            self.post_choreographer_callback(invocation, true)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/Choreographer;"
            && method.name == "removeCallbacks"
            && method
                .proto
                .descriptor_is("(ILjava/lang/Runnable;Ljava/lang/Object;)V")
        {
            let invocation = invocation?;
            self.remove_choreographer_callbacks(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/ViewConfiguration;"
            && matches!(
                method.name.as_str(),
                "getScaledTouchSlop"
                    | "getScaledMinimumFlingVelocity"
                    | "getScaledMaximumFlingVelocity"
                    | "getTapTimeout"
                    | "getLongPressTimeout"
            )
            && method.proto.descriptor_is("()I")
        {
            return Some(DexExecutionResult::Int(view_configuration_int(
                &method.name,
            )));
        }

        if method.class_descriptor == "Landroid/view/ViewConfiguration;"
            && method.name == "shouldShowMenuShortcutsWhenKeyboardPresent"
            && method.proto.descriptor_is("()Z")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.matches_builtin_signature(
            "Landroid/view/ViewConfiguration;",
            "getScrollFriction",
            "()F",
        ) {
            return Some(DexExecutionResult::Value(DexValue::Float(0.015)));
        }

        if method.class_descriptor == "Landroid/view/ViewConfiguration;"
            && matches!(
                method.name.as_str(),
                "getScaledHorizontalScrollFactor" | "getScaledVerticalScrollFactor"
            )
            && method.proto.descriptor_is("()F")
        {
            return Some(DexExecutionResult::Value(DexValue::Float(64.0)));
        }

        if method.class_descriptor == "Landroid/view/WindowInsets$Builder;"
            && method.name == "<init>"
            && matches!(
                method.proto.descriptor().as_str(),
                "()V" | "(Landroid/view/WindowInsets;)V"
            )
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/WindowInsets$Builder;"
            && method.name == "build"
            && method.proto.descriptor_is("()Landroid/view/WindowInsets;")
        {
            let invocation = invocation?;
            let value = window_insets_builder_build(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/WindowInsets$Builder;"
            && method.name == "setSystemWindowInsets"
            && method.proto.descriptor()
                == "(Landroid/graphics/Insets;)Landroid/view/WindowInsets$Builder;"
        {
            let invocation = invocation?;
            let value = self.window_insets_builder_set_insets(invocation, "systemWindowInsets")?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/WindowInsets;"
            && method.name == "isVisible"
            && method.proto.descriptor_is("(I)Z")
        {
            let invocation = invocation?;
            let value = window_insets_is_visible(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/WindowInsets$Type;"
            && method.proto.descriptor_is("()I")
        {
            let value = window_insets_type_mask(&method.name)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/GestureDetector$SimpleOnGestureListener;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/GestureDetector;"
            && method.name == "<init>"
            && matches!(
                method.proto.descriptor().as_str(),
                "(Landroid/content/Context;Landroid/view/GestureDetector$OnGestureListener;)V"
                    | "(Landroid/content/Context;Landroid/view/GestureDetector$OnGestureListener;Landroid/os/Handler;)V"
            )
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/GestureDetector;"
            && method.name == "setIsLongpressEnabled"
            && method.proto.descriptor_is("(Z)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "onTouchEvent"
            && method.proto.descriptor_is("(Landroid/view/MotionEvent;)Z")
        {
            let invocation = invocation?;
            let value = self.view_on_touch_event(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/GestureDetector;"
            && method.name == "onTouchEvent"
            && method.proto.descriptor_is("(Landroid/view/MotionEvent;)Z")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && matches!(method.name.as_str(), "getAction" | "getActionMasked")
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = if method.name == "getAction" {
                self.motion_event_action(invocation)?
            } else {
                self.motion_event_action_masked(invocation)?
            };

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getMetaState"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.motion_event_meta_state(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && matches!(method.name.as_str(), "getDownTime" | "getEventTime")
            && method.proto.descriptor_is("()J")
        {
            let invocation = invocation?;
            let field = if method.name == "getDownTime" {
                motion_event_down_time_field()
            } else {
                motion_event_event_time_field()
            };
            let value = self.motion_event_time(invocation, &field)?;

            return Some(DexExecutionResult::Value(DexValue::Long(value)));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "obtainNoHistory"
            && method
                .proto
                .descriptor_is("(Landroid/view/MotionEvent;)Landroid/view/MotionEvent;")
        {
            let invocation = invocation?;
            let value = self.motion_event_obtain_no_history(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "recycle"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            if invocation.arguments.len() != 1 {
                return None;
            }
            if !dex_value_is_object_like(&invocation.arguments[0]) {
                return None;
            }

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && matches!(method.name.as_str(), "getX" | "getRawX")
            && matches!(method.proto.descriptor().as_str(), "()F" | "(I)F")
        {
            let invocation = invocation?;
            let value = self.motion_event_axis_value(invocation, &motion_event_x_field())?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && matches!(method.name.as_str(), "getY" | "getRawY")
            && matches!(method.proto.descriptor().as_str(), "()F" | "(I)F")
        {
            let invocation = invocation?;
            let value = self.motion_event_axis_value(invocation, &motion_event_y_field())?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        // cite: frameworks-base MotionEvent.java#getAxisValue, AXIS_SCROLL
        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getAxisValue"
            && matches!(method.proto.descriptor().as_str(), "(I)F" | "(II)F")
        {
            let invocation = invocation?;
            let value = self.motion_event_get_axis_value(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getPressure"
            && matches!(method.proto.descriptor().as_str(), "()F" | "(I)F")
        {
            let invocation = invocation?;
            let value = self.motion_event_pressure(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Float(value)));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && matches!(method.name.as_str(), "getPointerCount" | "getActionIndex")
            && method.proto.descriptor_is("()I")
        {
            let value = if method.name == "getPointerCount" {
                1
            } else {
                0
            };

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getHistorySize"
            && method.proto.descriptor_is("()I")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getButtonState"
            && method.proto.descriptor_is("()I")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getSource"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.motion_event_source(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/InputEvent;"
            && method.name == "isFromSource"
            && method.proto.descriptor_is("(I)Z")
        {
            let invocation = invocation?;
            let value = self.input_event_is_from_source(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getClassification"
            && method.proto.descriptor_is("()I")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getPointerId"
            && method.proto.descriptor_is("(I)I")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/view/MotionEvent;"
            && method.name == "getToolType"
            && method.proto.descriptor_is("(I)I")
        {
            return Some(DexExecutionResult::Int(1));
        }

        if method.class_descriptor == "Landroid/animation/LayoutTransition;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/animation/AnimatorListenerAdapter;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/animation/LayoutTransition;"
            && method.name == "disableTransitionType"
            && method.proto.descriptor_is("(I)V")
        {
            let invocation = invocation?;
            layout_transition_type(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/OverScroller;"
            && method.name == "<init>"
            && matches!(
                method.proto.descriptor().as_str(),
                "(Landroid/content/Context;)V"
                    | "(Landroid/content/Context;Landroid/view/animation/Interpolator;)V"
            )
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/EdgeEffect;"
            && method.name == "<init>"
            && matches!(
                method.proto.descriptor().as_str(),
                "(Landroid/content/Context;)V"
                    | "(Landroid/content/Context;Landroid/util/AttributeSet;)V"
            )
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/OverScroller;"
            && method.name == "abortAnimation"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        None
    }
}

fn window_insets_type_mask(name: &str) -> Option<i32> {
    match name {
        "statusBars" => Some(1),
        "navigationBars" => Some(2),
        "captionBar" => Some(4),
        "ime" => Some(8),
        "systemGestures" => Some(16),
        "mandatorySystemGestures" => Some(32),
        "tappableElement" => Some(64),
        "displayCutout" => Some(128),
        "systemOverlays" => Some(512),
        "systemBars" => Some(1 | 2 | 4),
        "all" => Some(-1),
        _ => None,
    }
}

fn window_insets_is_visible(invocation: &DexInvocation) -> Option<i32> {
    if invocation.arguments.len() != 2 || !dex_value_is_object_like(&invocation.arguments[0]) {
        return None;
    }

    let DexValue::Int(type_mask) = invocation.arguments[1] else {
        return None;
    };

    const IME: i32 = 8;

    if type_mask == IME {
        return Some(0);
    }

    Some(1)
}

fn view_window_insets_controller(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 1 || !dex_value_is_object_like(&invocation.arguments[0]) {
        return None;
    }

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/view/WindowInsetsController;:view:{}",
        map_identity_key(&invocation.arguments[0])
    ))))
}

fn view_request_rectangle_on_screen(invocation: &DexInvocation) -> Option<()> {
    if invocation.arguments.len() != 3
        || !dex_value_is_object_like(&invocation.arguments[0])
        || !dex_value_is_object_like(&invocation.arguments[1])
    {
        return None;
    }

    let DexValue::Int(_) = invocation.arguments[2] else {
        return None;
    };

    Some(())
}
