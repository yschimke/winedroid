use super::*;

impl DexRunInvocationHandler<'_> {
    pub(super) fn invoke_supported(
        &mut self,
        invocation: DexInvocation,
    ) -> Result<DexInvocationResult> {
        let method = self
            .program
            .method_id(self.current_dex_file_index, invocation.method_index)?;

        trace_invocation(self.depth, &invocation, &method);
        let records_menu_inflate = is_menu_inflater_inflate_method(&method);
        if records_menu_inflate {
            self.program.record_menu_inflate(&invocation)?;
            if self.program.resolve_exact_method(&method)?.is_none() {
                self.program.sync_inflated_menu_items(&invocation)?;
                return Ok(DexInvocationResult::Void);
            }
        }
        if records_content_view_method(&method) {
            self.program.record_content_view_argument(&invocation, 1);
        }

        let direct_kind = matches!(
            invocation.kind,
            DexInvokeKind::Static | DexInvokeKind::Direct
        );
        let method_key = method_cache_key(self.current_dex_file_index, invocation.method_index);
        if direct_kind {
            let cached = self
                .program
                .caches
                .borrow()
                .direct_resolved_methods
                .get(&method_key)
                .cloned();
            if let Some(resolved) = cached {
                if let Some(Some(arg_index)) = self
                    .program
                    .caches
                    .borrow()
                    .null_assert_args
                    .get(&method_key)
                    .copied()
                {
                    if null_assert_shortcut_applies(&invocation, &resolved, arg_index)? {
                        return Ok(DexInvocationResult::Void);
                    }
                }
                if let Some(result) = self.invoke_resolved_intrinsic(&resolved, &invocation)? {
                    if records_menu_inflate {
                        self.program.sync_inflated_menu_items(&invocation)?;
                    }
                    return Ok(result);
                }
                let result = self.program.execute_method(
                    None,
                    &resolved,
                    Some(&invocation.arguments),
                    self.depth + 1,
                )?;
                if records_menu_inflate {
                    self.program.sync_inflated_menu_items(&invocation)?;
                }
                return Ok(invocation_result_from_execution(result));
            }
        }
        if !direct_kind {
            let cached =
                object_receiver_dispatch_descriptor(&invocation).and_then(|receiver_descriptor| {
                    self.program
                        .caches
                        .borrow()
                        .virtual_resolved_methods
                        .get(&method_key)
                        .and_then(|by_receiver| by_receiver.get(receiver_descriptor))
                        .cloned()
                });
            if let Some(resolved) = cached {
                if let Some(result) = self.invoke_resolved_intrinsic(&resolved, &invocation)? {
                    if records_menu_inflate {
                        self.program.sync_inflated_menu_items(&invocation)?;
                    }
                    return Ok(result);
                }
                let result = self.program.execute_method(
                    None,
                    &resolved,
                    Some(&invocation.arguments),
                    self.depth + 1,
                )?;
                if records_menu_inflate {
                    self.program.sync_inflated_menu_items(&invocation)?;
                }
                return Ok(invocation_result_from_execution(result));
            }
        }

        let proto_descriptor = self
            .program
            .method_proto_descriptor(self.current_dex_file_index, invocation.method_index)?;
        let proto_descriptor: &str = &proto_descriptor;

        if method.class_descriptor == "Landroid/app/Activity;"
            && method.name == "runOnUiThread"
            && proto_descriptor == "(Ljava/lang/Runnable;)V"
        {
            self.run_on_ui_thread(&invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.class_descriptor == "Landroid/os/Handler;"
            && method.name == "post"
            && proto_descriptor == "(Ljava/lang/Runnable;)Z"
        {
            self.post_handler_runnable(&invocation)?;
            return Ok(DexInvocationResult::Value(DexValue::Int(1)));
        }

        // cite: frameworks-base HandlerExecutor.java#execute
        if method.class_descriptor == "Ljava/util/concurrent/Executor;"
            && method.name == "execute"
            && proto_descriptor == "(Ljava/lang/Runnable;)V"
        {
            self.post_handler_runnable(&invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.class_descriptor == "Ljava/lang/Thread;"
            && method.name == "start"
            && proto_descriptor == "()V"
        {
            self.start_thread(&invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.class_descriptor == "Landroid/util/Xml;"
            && method.name == "parse"
            && proto_descriptor
                == "(Ljava/io/InputStream;Landroid/util/Xml$Encoding;Lorg/xml/sax/ContentHandler;)V"
        {
            self.program.sax_parse(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if adapter_view_set_adapter_method(&method) {
            if let (Some(adapter_view), Some(adapter)) =
                (invocation.arguments.first(), invocation.arguments.get(1))
            {
                if dex_value_is_object_like(adapter_view) {
                    let adapter = adapter.clone();
                    let adapter_view = adapter_view.clone();
                    self.program.write_adapter_view_adapter(&invocation);
                    if dex_value_is_object_like(&adapter) {
                        self.program
                            .runtime
                            .borrow_mut()
                            .adapter_bound_view
                            .insert(map_identity_key(&adapter), adapter_view.clone());
                        self.materialize_adapter_view(&adapter_view, &adapter)?;
                    } else {
                        self.program.clear_view_group_children(&adapter_view);
                    }
                    return Ok(DexInvocationResult::Void);
                }
            }
        }

        if method.name == "notifyDataSetChanged"
            && proto_descriptor == "()V"
            && invocation
                .arguments
                .first()
                .is_some_and(dex_value_is_object_like)
        {
            if let Some(adapter) = invocation.arguments.first() {
                let adapter = adapter.clone();
                let bound_view = self
                    .program
                    .runtime
                    .borrow()
                    .adapter_bound_view
                    .get(&map_identity_key(&adapter))
                    .cloned();
                if let Some(adapter_view) = bound_view {
                    self.materialize_adapter_view(&adapter_view, &adapter)?;
                    return Ok(DexInvocationResult::Void);
                }
            }
        }

        if activity_set_content_view_runtime_view_method(&method) {
            self.activity_set_content_view_runtime_view(&invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if dialog_show_runtime_view_method(&method) {
            self.dialog_show_runtime_view(&invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if dialog_dismiss_runtime_view_method(&method) {
            self.dialog_dismiss_runtime_view(&invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if view_group_add_view_runtime_child_method(&method) {
            self.view_group_add_view_runtime_child(&method, &invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if view_group_remove_all_views_method(&method) {
            self.view_group_remove_all_views(&invocation)?;
            return Ok(DexInvocationResult::Void);
        }

        if view_group_draw_child_method(&method) {
            let handled = self.view_group_draw_child(&invocation)?;
            return Ok(DexInvocationResult::Value(DexValue::Int(handled)));
        }

        if arrays_sort_object_with_comparator_method(&method) {
            self.program
                .arrays_sort_object_with_comparator(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if arrays_sort_object_range_method(&method) {
            self.program
                .arrays_sort_object_range_with_comparator(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if collections_sort_list_with_comparator_method(&method) {
            self.program
                .collections_sort_list_with_comparator(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if tree_set_add_value_method(self.program, &method, &invocation) {
            let added = self.program.tree_set_add_value(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Value(DexValue::Int(added)));
        }

        if method.name == "dispatchGenericMotionEvent"
            && proto_descriptor == "(Landroid/view/MotionEvent;)Z"
            && is_bootstrap_class_descriptor(&method.class_descriptor)
        {
            let super_call = invocation.kind == DexInvokeKind::Super;
            if let Some(handled) = self.program.view_dispatch_generic_motion_event_call(
                &invocation,
                super_call,
                self.depth,
            ) {
                return Ok(DexInvocationResult::Value(DexValue::Int(handled?)));
            }
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "performClick"
            && proto_descriptor == "()Z"
        {
            let handled = self.program.perform_view_click(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Value(DexValue::Int(handled)));
        }

        if method.class_descriptor == "Landroid/view/MenuItem$OnMenuItemClickListener;"
            && method.name == "onMenuItemClick"
            && proto_descriptor == "(Landroid/view/MenuItem;)Z"
        {
            let handled = self
                .program
                .dispatch_menu_item_declared_on_click_handler(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Value(DexValue::Int(handled)));
        }

        if method.name == "finish"
            && proto_descriptor == "()V"
            && self.program.is_activity_class(&method.class_descriptor)
        {
            self.program.finish_activity(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.name == "startActivity"
            && matches!(
                proto_descriptor,
                "(Landroid/content/Intent;)V" | "(Landroid/content/Intent;Landroid/os/Bundle;)V"
            )
        {
            self.program
                .start_activity_from_intent(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.name == "startActivityForResult"
            && matches!(
                proto_descriptor,
                "(Landroid/content/Intent;I)V" | "(Landroid/content/Intent;ILandroid/os/Bundle;)V"
            )
        {
            let request_code = match invocation.arguments.get(2) {
                Some(DexValue::Int(code)) => *code,
                _ => 0,
            };
            self.program
                .start_activity_for_result(&invocation, request_code, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.name == "setResult"
            && matches!(proto_descriptor, "(I)V" | "(ILandroid/content/Intent;)V")
            && self.program.is_activity_class(&method.class_descriptor)
        {
            let result_code = match invocation.arguments.get(1) {
                Some(DexValue::Int(code)) => *code,
                _ => 0,
            };
            let result_intent = invocation.arguments.get(2).cloned();
            self.program.set_activity_result(result_code, result_intent);
            return Ok(DexInvocationResult::Void);
        }

        if matches!(
            method.class_descriptor.as_str(),
            "Landroid/app/FragmentTransaction;" | "Landroidx/fragment/app/FragmentTransaction;"
        ) && matches!(
            method.name.as_str(),
            "commit" | "commitAllowingStateLoss" | "commitNow" | "commitNowAllowingStateLoss"
        ) {
            self.program.run_pending_fragment(self.depth)?;
            return Ok(DexInvocationResult::Value(DexValue::Int(0)));
        }

        if method.class_descriptor == "Landroid/widget/TextView;"
            && method.name == "setText"
            && proto_descriptor == "(Ljava/lang/CharSequence;)V"
        {
            self.program
                .set_text_view_text_and_dispatch(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.class_descriptor == "Landroid/widget/TextView;"
            && method.name == "setText"
            && proto_descriptor == "(I)V"
        {
            self.program
                .set_text_view_text_resource_and_dispatch(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Void);
        }

        if method.class_descriptor == "Ljava/lang/Class;"
            && method.name == "forName"
            && proto_descriptor == "(Ljava/lang/String;)Ljava/lang/Class;"
        {
            let result = self.program.class_for_name(&invocation, self.depth)?;
            return Ok(invocation_result_from_execution(result));
        }

        if method.class_descriptor == "Ljava/lang/Class;"
            && method.name == "forName"
            && proto_descriptor == "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;"
        {
            let result = self
                .program
                .class_for_name_with_loader(&invocation, self.depth)?;
            return Ok(invocation_result_from_execution(result));
        }

        if method.class_descriptor == "Ljava/lang/Class;"
            && method.name == "newInstance"
            && proto_descriptor == "()Ljava/lang/Object;"
        {
            let result = self.program.class_new_instance(
                &invocation,
                self.current_class_descriptor.as_deref(),
                self.depth,
            )?;
            return Ok(invocation_result_from_execution(result));
        }

        if method.class_descriptor == "Ljava/lang/reflect/Constructor;"
            && method.name == "newInstance"
            && proto_descriptor == "([Ljava/lang/Object;)Ljava/lang/Object;"
        {
            let value = self
                .program
                .reflect_constructor_new_instance(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Value(value));
        }

        if method.class_descriptor == "Ljava/util/concurrent/FutureTask;"
            && method.name == "get"
            && proto_descriptor == "()Ljava/lang/Object;"
        {
            let result = self.program.future_task_get(&invocation, self.depth)?;
            return Ok(invocation_result_from_execution(result));
        }

        if method.class_descriptor == "Ljava/io/File;"
            && method.name == "listFiles"
            && proto_descriptor == "(Ljava/io/FileFilter;)[Ljava/io/File;"
        {
            let value = self
                .program
                .file_list_files_with_filter(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/LayoutInflater;"
            && method.name == "inflate"
            && proto_descriptor == "(ILandroid/view/ViewGroup;Z)Landroid/view/View;"
        {
            let value =
                self.program
                    .layout_inflater_inflate_and_finish(&invocation, 4, self.depth)?;
            return Ok(DexInvocationResult::Value(value));
        }

        if method.class_descriptor == "Landroid/view/LayoutInflater;"
            && method.name == "inflate"
            && proto_descriptor == "(ILandroid/view/ViewGroup;)Landroid/view/View;"
        {
            let value =
                self.program
                    .layout_inflater_inflate_and_finish(&invocation, 3, self.depth)?;
            return Ok(DexInvocationResult::Value(value));
        }

        if java_lang_object_equals_method(&method) {
            let value = self
                .program
                .runtime_object_equals(&invocation, self.depth)?;
            return Ok(DexInvocationResult::Value(DexValue::Int(value)));
        }

        if java_lang_object_virtual_method(&method) {
            if let Some(result) =
                self.invoke_receiver_dispatched_method(&method, &invocation, records_menu_inflate)?
            {
                return Ok(result);
            }
        }

        if java_collection_interface_method(&method) {
            if let Some(result) =
                self.invoke_receiver_dispatched_method(&method, &invocation, records_menu_inflate)?
            {
                return Ok(result);
            }
        }

        if view_group_should_delay_child_pressed_state_method(&method)
            && matches!(
                invocation.kind,
                DexInvokeKind::Virtual | DexInvokeKind::Interface
            )
        {
            // cite: art interpreter_common.h#DoInvoke, FindMethodToCall
            if let Some(result) =
                self.invoke_receiver_dispatched_method(&method, &invocation, records_menu_inflate)?
            {
                return Ok(result);
            }

            let receiver_descriptor =
                receiver_dispatch_descriptor(&invocation).ok_or_else(|| {
                    WinedroidError::new(
                        "ViewGroup.shouldDelayChildPressedState requires an object-backed receiver",
                    )
                })?;
            if let Some(result) = self.program.invoke_builtin_method_from_class_hierarchy(
                receiver_descriptor,
                &method,
                &invocation,
            )? {
                return Ok(invocation_result_from_execution(result));
            }

            return Err(WinedroidError::new(format!(
                "ViewGroup.shouldDelayChildPressedState is not implemented for the receiver hierarchy\nreceiver: {receiver_descriptor}\nnext missing primitive: ViewGroup.shouldDelayChildPressedState receiver dispatch"
            )));
        }

        if method.class_descriptor == "Ljava/lang/StringBuilder;"
            && method.name == "append"
            && proto_descriptor == "(Ljava/lang/Object;)Ljava/lang/StringBuilder;"
        {
            let value = self.append_string_builder_object(&invocation)?;
            return Ok(DexInvocationResult::Value(value));
        }

        if method.class_descriptor == "Ljava/lang/String;"
            && method.name == "valueOf"
            && proto_descriptor == "(Ljava/lang/Object;)Ljava/lang/String;"
        {
            if invocation.arguments.len() != 1 {
                return Err(WinedroidError::new(format!(
                    "String.valueOf(Object) expected 1 argument, got {}",
                    invocation.arguments.len()
                )));
            }
            let value = self.string_value_of_object(&invocation.arguments[0])?;
            return Ok(DexInvocationResult::Value(DexValue::String(
                std::rc::Rc::from(value),
            )));
        }

        if method.class_descriptor == "Ljava/util/ArrayList;"
            && method.name == "<init>"
            && proto_descriptor == "(Ljava/util/Collection;)V"
        {
            self.program
                .initialize_array_list_from_collection_iterating(&invocation, self.depth + 1)?;
            return Ok(DexInvocationResult::Void);
        }

        if let Some(result) = self.program.invoke_java_collection_equality_method(
            &method,
            &invocation,
            self.depth + 1,
        )? {
            return Ok(invocation_result_from_execution(result));
        }

        if method.class_descriptor == "Landroid/view/View;"
            && method.name == "measure"
            && proto_descriptor == "(II)V"
        {
            if let Some(()) = self
                .program
                .measure_view_with_dispatch(&invocation, self.depth + 1)?
            {
                return Ok(DexInvocationResult::Void);
            }
        }

        if method.class_descriptor == "Landroid/view/inputmethod/InputConnectionWrapper;" {
            if method.name == "<init>"
                && proto_descriptor == "(Landroid/view/inputmethod/InputConnection;Z)V"
            {
                self.program.input_connection_wrapper_init(&invocation);
                return Ok(DexInvocationResult::Void);
            }
            if method.name == "setTarget"
                && proto_descriptor == "(Landroid/view/inputmethod/InputConnection;)V"
            {
                self.program
                    .input_connection_wrapper_set_target(&invocation);
                return Ok(DexInvocationResult::Void);
            }
            if let Some(result) = self.program.input_connection_wrapper_delegate(
                &method,
                &invocation,
                self.depth + 1,
            )? {
                return Ok(invocation_result_from_execution(result));
            }
        }

        let known_direct_code = direct_kind
            && self
                .program
                .caches
                .borrow()
                .direct_code_methods
                .contains(&method_key);
        let known_virtual_code = !direct_kind
            && object_receiver_dispatch_descriptor(&invocation).is_some_and(|receiver_class| {
                self.program
                    .caches
                    .borrow()
                    .virtual_code_methods
                    .get(&method_key)
                    .is_some_and(|classes| classes.contains(receiver_class))
            });
        if !known_direct_code && !known_virtual_code {
            if let Some(result) = self.program.invoke_builtin_method_hinted(
                Some(Rc::as_ptr(&method) as usize),
                &method,
                Some(&invocation),
            ) {
                return Ok(invocation_result_from_execution(result));
            }
        }

        // cite: libcore AnnotationFactory.java#invoke
        if let Some(value) = self.program.annotation_element_value(&method, &invocation) {
            return Ok(DexInvocationResult::Value(value));
        }

        if let Some(result) =
            self.invoke_receiver_dispatched_method(&method, &invocation, records_menu_inflate)?
        {
            if let Some(receiver_class) = object_receiver_dispatch_descriptor(&invocation) {
                self.program
                    .caches
                    .borrow_mut()
                    .virtual_code_methods
                    .entry(method_key)
                    .or_default()
                    .insert(receiver_class.to_owned());
            }
            return Ok(result);
        }

        if let Some(resolved) = self.program.resolve_exact_method(&method)? {
            if direct_kind && resolved.encoded_method.code_offset != 0 {
                let null_assert = self
                    .program
                    .code_item(&resolved)
                    .ok()
                    .and_then(|code| detect_null_assert_arg(self.program, &resolved, &code));
                let mut caches = self.program.caches.borrow_mut();
                caches.direct_code_methods.insert(method_key);
                caches
                    .direct_resolved_methods
                    .insert(method_key, resolved.clone());
                caches.null_assert_args.insert(method_key, null_assert);
            }
            if let Some(result) = self.invoke_resolved_intrinsic(&resolved, &invocation)? {
                if records_menu_inflate {
                    self.program.sync_inflated_menu_items(&invocation)?;
                }
                return Ok(result);
            }
            let result = self.program.execute_method(
                None,
                &resolved,
                Some(&invocation.arguments),
                self.depth + 1,
            )?;
            if records_menu_inflate {
                self.program.sync_inflated_menu_items(&invocation)?;
            }
            return Ok(invocation_result_from_execution(result));
        }

        if invocation.kind == DexInvokeKind::Super {
            // cite: art class.cc#FindClassMethodWithSignature
            if let Some(resolved) = self
                .program
                .resolve_exact_method_in_class_hierarchy(&method.class_descriptor, &method)?
            {
                if let Some(result) = self.invoke_resolved_intrinsic(&resolved, &invocation)? {
                    if records_menu_inflate {
                        self.program.sync_inflated_menu_items(&invocation)?;
                    }
                    return Ok(result);
                }
                let result = self.program.execute_method(
                    None,
                    &resolved,
                    Some(&invocation.arguments),
                    self.depth + 1,
                )?;
                if records_menu_inflate {
                    self.program.sync_inflated_menu_items(&invocation)?;
                }
                return Ok(invocation_result_from_execution(result));
            }
        }

        if let Some(result) = self
            .program
            .invoke_builtin_method_in_hierarchy(&method, &invocation)
        {
            return Ok(invocation_result_from_execution(result));
        }

        // cite: libcore AbstractList.java#listIterator
        if matches!(
            method.class_descriptor.as_str(),
            "Ljava/util/List;" | "Ljava/util/AbstractList;"
        ) && method.name == "listIterator"
            && method.proto.descriptor_is("(I)Ljava/util/ListIterator;")
        {
            if let Some(value) = self
                .program
                .dex_backed_list_iterator_at(&invocation, self.depth + 1)?
            {
                return Ok(DexInvocationResult::Value(value));
            }
        }

        // cite: libcore AbstractList.java#iterator
        if matches!(
            method.class_descriptor.as_str(),
            "Ljava/util/List;"
                | "Ljava/util/AbstractList;"
                | "Ljava/util/Collection;"
                | "Ljava/util/AbstractCollection;"
                | "Ljava/lang/Iterable;"
        ) && method.name == "iterator"
            && method.proto.descriptor_is("()Ljava/util/Iterator;")
        {
            if let Some(value) = self
                .program
                .dex_backed_list_iterator(&invocation, self.depth + 1)?
            {
                return Ok(DexInvocationResult::Value(value));
            }
        }

        Err(WinedroidError::new(format!(
            "DEX invocation not implemented\nkind: {}\nmethod: {}.{}\nargument registers: {}\nnext missing primitive: method {}.{}",
            invocation.instruction_name(),
            method.class_descriptor,
            method.display_signature(),
            argument_registers_display(&invocation.argument_registers),
            method.class_descriptor,
            method.display_signature()
        )))
    }
    pub(super) fn invoke_supported_from_frame(
        &mut self,
        invocation: DexInvocationFrame<'_>,
    ) -> Result<DexInvocationResult> {
        #[cfg(all(feature = "jit", not(target_family = "wasm")))]
        if jit_dispatch_enabled() {
            return self.invoke(invocation.materialize()?);
        }

        if cached_env_flag(&DEX_TRACE, "WINEDROID_DEX_TRACE") || method_trace_prefixes().is_some() {
            return self.invoke(invocation.materialize()?);
        }

        let method_key = method_cache_key(self.current_dex_file_index, invocation.method_index());
        if matches!(
            invocation.kind(),
            DexInvokeKind::Static | DexInvokeKind::Direct
        ) {
            let cached = self
                .program
                .caches
                .borrow()
                .direct_resolved_methods
                .get(&method_key)
                .cloned();
            if let Some(resolved) = cached {
                if resolved.encoded_method.code_offset == 0 {
                    return self.invoke(invocation.materialize()?);
                }
                if is_menu_inflater_inflate_method(&resolved.method)
                    || records_content_view_method(&resolved.method)
                {
                    return self.invoke(invocation.materialize()?);
                }
                if let Some(Some(arg_index)) = self
                    .program
                    .caches
                    .borrow()
                    .null_assert_args
                    .get(&method_key)
                    .copied()
                {
                    if null_assert_shortcut_applies(&invocation, &resolved, arg_index)? {
                        return Ok(DexInvocationResult::Void);
                    }
                }
                if let Some(intrinsic) = self.program.method_intrinsic(&resolved)? {
                    if let Some(result) = self.invoke_resolved_intrinsic_from_frame(
                        &resolved,
                        intrinsic,
                        &invocation,
                    )? {
                        return Ok(result);
                    }
                    return self.invoke(invocation.materialize()?);
                }
                let result = self.program.execute_method_from_invocation_frame(
                    &resolved,
                    &invocation,
                    self.depth + 1,
                )?;
                return Ok(invocation_result_from_execution(result));
            }
        }

        if matches!(
            invocation.kind(),
            DexInvokeKind::Virtual | DexInvokeKind::Interface
        ) {
            let cached =
                invocation
                    .object_argument_class_descriptor(0)?
                    .and_then(|receiver_descriptor| {
                        self.program
                            .caches
                            .borrow()
                            .virtual_resolved_methods
                            .get(&method_key)
                            .and_then(|by_receiver| by_receiver.get(receiver_descriptor))
                            .cloned()
                    });
            if let Some(resolved) = cached {
                if resolved.encoded_method.code_offset == 0 {
                    return self.invoke(invocation.materialize()?);
                }
                if is_menu_inflater_inflate_method(&resolved.method)
                    || records_content_view_method(&resolved.method)
                {
                    return self.invoke(invocation.materialize()?);
                }
                if let Some(intrinsic) = self.program.method_intrinsic(&resolved)? {
                    if let Some(result) = self.invoke_resolved_intrinsic_from_frame(
                        &resolved,
                        intrinsic,
                        &invocation,
                    )? {
                        return Ok(result);
                    }
                    return self.invoke(invocation.materialize()?);
                }
                let result = self.program.execute_method_from_invocation_frame(
                    &resolved,
                    &invocation,
                    self.depth + 1,
                )?;
                return Ok(invocation_result_from_execution(result));
            }
        }

        if let Some(result) = invoke_uncached_code_from_frame(self, &invocation, method_key)? {
            return Ok(result);
        }

        self.invoke(invocation.materialize()?)
    }
}
