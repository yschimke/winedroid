use super::helpers::*;
use super::*;

// cite: frameworks-base MotionEvent.java#ACTION_DOWN, ACTION_UP, ACTION_MOVE
const MOTION_EVENT_ACTION_DOWN: i32 = 0;
const MOTION_EVENT_ACTION_UP: i32 = 1;
const MOTION_EVENT_ACTION_MOVE: i32 = 2;
const SYNTHETIC_SWIPE_STEP_MILLIS: u64 = 16;
const SYNTHETIC_SWIPE_MAX_STEPS: u32 = 64;
const MOTION_EVENT_ACTION_MASK: i32 = 0xff;
const MOTION_EVENT_META_STATE_NONE: i32 = 0;
const MOTION_EVENT_PRESSURE_TOUCH: f32 = 1.0;
const MOTION_EVENT_SOURCE_TOUCHSCREEN: i32 = 0x0000_1002;
const SYNTHETIC_TAP_DURATION_MILLIS: i64 = 16;
const ACTION_BAR_OVERFLOW_TOGGLE_ID: i32 = 0x6fff_fffe;

impl DexProgram {
    pub(in crate::dex_run) fn write_view_listener_field(
        &self,
        invocation: &DexInvocation,
        class_descriptor: &str,
        type_descriptor: &str,
        name: &str,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 || !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        let field = DexFieldId {
            class_descriptor: class_descriptor.to_owned(),
            type_descriptor: type_descriptor.to_owned(),
            name: name.to_owned(),
        };
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &field,
            invocation.arguments[1].clone(),
        );
        Some(())
    }

    pub(in crate::dex_run) fn write_view_click_listener(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        let view_key = map_identity_key(&invocation.arguments[0]);

        if invocation.arguments[1] == DexValue::Null {
            self.runtime
                .borrow_mut()
                .view_click_listeners
                .remove(&view_key);
            return Some(());
        }

        match &invocation.arguments[1] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        self.runtime
            .borrow_mut()
            .view_click_listeners
            .insert(view_key, invocation.arguments[1].clone());

        Some(())
    }

    pub(in crate::dex_run) fn write_view_key_listener(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        let listener = match &invocation.arguments[1] {
            DexValue::Null | DexValue::Int(0) => DexValue::Null,
            DexValue::Object(_) | DexValue::Reference(_) => invocation.arguments[1].clone(),
            DexValue::Uninitialized
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &view_key_listener_field(),
            listener,
        )
        .then_some(())
    }

    pub(in crate::dex_run) fn write_view_touch_listener(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        let listener = match &invocation.arguments[1] {
            DexValue::Null | DexValue::Int(0) => DexValue::Null,
            DexValue::Object(_) | DexValue::Reference(_) => invocation.arguments[1].clone(),
            DexValue::Uninitialized
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &view_touch_listener_field(),
            listener,
        )
        .then_some(())
    }

    pub(in crate::dex_run) fn write_view_drag_listener(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        let listener = match &invocation.arguments[1] {
            DexValue::Null | DexValue::Int(0) => DexValue::Null,
            DexValue::Object(_) | DexValue::Reference(_) => invocation.arguments[1].clone(),
            DexValue::Uninitialized
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &view_drag_listener_field(),
            listener,
        )
        .then_some(())
    }

    pub(in crate::dex_run) fn write_view_window_insets_animation_callback(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_nullable_object_like(&invocation.arguments[1])
        {
            return None;
        }

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &view_window_insets_animation_callback_field(),
            invocation.arguments[1].clone(),
        )
        .then_some(())
    }

    pub(in crate::dex_run) fn write_view_translation_callback(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_nullable_object_like(&invocation.arguments[1])
        {
            return None;
        }

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &view_translation_callback_field(),
            invocation.arguments[1].clone(),
        )
        .then_some(())
    }

    pub(in crate::dex_run) fn perform_view_click(
        &self,
        invocation: &DexInvocation,
        depth: usize,
    ) -> Result<i32> {
        if invocation.arguments.len() != 1 {
            return Err(WinedroidError::new(format!(
                "View.performClick expected one receiver argument, got {}",
                invocation.arguments.len()
            )));
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => {
                return Err(WinedroidError::new(format!(
                    "View.performClick requires an object-backed View receiver\nreceiver: {}",
                    invocation.arguments[0]
                )));
            }
        }

        let view = invocation.arguments[0].clone();
        if let Some(view_id) = self.view_id_from_value(&view) {
            if self.active_menu_overlay_item(view_id).is_some() {
                return self.dispatch_active_menu_overlay_item_click(view_id, depth);
            }
        }

        let view_key = map_identity_key(&view);
        let listener = self
            .runtime
            .borrow()
            .view_click_listeners
            .get(&view_key)
            .cloned();

        let Some(listener) = listener else {
            let declared_handler = self
                .runtime
                .borrow()
                .view_declared_on_click_handlers
                .get(&view_key)
                .cloned();

            let Some(method_name) = declared_handler else {
                return Ok(0);
            };

            return self.dispatch_declared_on_click_handler(method_name, view, depth);
        };

        self.dispatch_view_click_listener(listener, view, depth)
    }

    pub(in crate::dex_run) fn perform_view_click_by_id(
        &self,
        view_id: i32,
        depth: usize,
    ) -> Result<i32> {
        if let Some(view) = self.active_dialog_registered_view_by_id(view_id) {
            if self.view_has_click_binding(&view) {
                return self.perform_view_click(
                    &DexInvocation {
                        kind: DexInvokeKind::Virtual,
                        range: false,
                        method_index: 0,
                        argument_registers: Default::default(),
                        arguments: smallvec::smallvec![view],
                    },
                    depth,
                );
            }
        }

        if view_id == ACTION_BAR_OVERFLOW_TOGGLE_ID {
            let mut runtime = self.runtime.borrow_mut();
            runtime.action_bar_overflow_open = !runtime.action_bar_overflow_open;
            return Ok(1);
        }

        if view_id == 0x0102_002c {
            return self.perform_action_bar_navigation(depth);
        }

        // cite: androidx ListMenuPresenter.java#onItemClick
        // cite: androidx MenuBuilder.java#performItemAction
        // cite: androidx MenuItemImpl.java#invoke
        // cite: androidx AppCompatDelegateImpl.java#onMenuItemSelected
        if self
            .runtime
            .borrow()
            .options_menu
            .iter()
            .any(|item| item.item_id == view_id)
        {
            trace::event("click", format!("dispatch options item id={view_id}"));
            self.runtime.borrow_mut().action_bar_overflow_open = false;
            self.dismiss_menu_overlay_containing_item(view_id);
            return self.dispatch_options_item_selected(view_id, depth);
        }

        if self.active_menu_overlay_item(view_id).is_some() {
            trace::event(
                "click",
                format!("dispatch active menu overlay item id={view_id}"),
            );
            return self.dispatch_active_menu_overlay_item_click(view_id, depth);
        }

        // cite: frameworks-base PopupWindow.java#PopupDecorView
        if self.dismiss_top_menu_overlay() {
            trace::event(
                "click",
                format!("dismiss top menu overlay for id={view_id}"),
            );
            return Ok(1);
        }

        if let Some((list_view, position)) = self.adapter_item_click(view_id) {
            trace::event(
                "click",
                format!("dispatch adapter item click id={view_id} position={position}"),
            );
            return self.dispatch_adapter_item_click(&list_view, position, depth);
        }

        // cite: frameworks-base ViewGroup.java#addView
        if let Some(view) = self.dynamic_view_click_target(view_id) {
            if self.view_has_click_binding(&view) {
                trace::event(
                    "click",
                    format!("dispatch dynamic addView row click id={view_id}"),
                );
                return self.perform_view_click(
                    &DexInvocation {
                        kind: DexInvokeKind::Virtual,
                        range: false,
                        method_index: 0,
                        argument_registers: Default::default(),
                        arguments: smallvec::smallvec![view],
                    },
                    depth,
                );
            }
        }

        if let Some(view) = self.active_registered_view_by_id(view_id) {
            if self.view_has_click_binding(&view) {
                trace::event(
                    "click",
                    format!("dispatch registered view click id={view_id}"),
                );
                return self.perform_view_click(
                    &DexInvocation {
                        kind: DexInvokeKind::Virtual,
                        range: false,
                        method_index: 0,
                        argument_registers: Default::default(),
                        arguments: smallvec::smallvec![view],
                    },
                    depth,
                );
            }
        }

        self.perform_view_click(
            &DexInvocation {
                kind: DexInvokeKind::Virtual,
                range: false,
                method_index: 0,
                argument_registers: Default::default(),
                arguments: smallvec::smallvec![view_reference(view_id)],
            },
            depth,
        )
    }

    pub(in crate::dex_run) fn perform_runtime_view_touch(
        &self,
        x: f32,
        y: f32,
        depth: usize,
    ) -> Result<i32> {
        self.perform_runtime_view_touch_hold(x, y, SYNTHETIC_TAP_DURATION_MILLIS as u64, depth)
    }

    pub(in crate::dex_run) fn perform_runtime_view_touch_hold(
        &self,
        x: f32,
        y: f32,
        hold_ms: u64,
        depth: usize,
    ) -> Result<i32> {
        let (root, root_view_key, x, y) = self.resolve_runtime_touch_target(x, y)?;

        trace::event(
            "runtime",
            format!("runtime View touch x={x:.1} y={y:.1} hold={hold_ms}ms root={root}"),
        );
        let down_time = self.runtime.borrow().main_thread_uptime_millis;
        let handled_down = self.dispatch_runtime_view_motion_event(
            &root,
            MOTION_EVENT_ACTION_DOWN,
            x,
            y,
            down_time,
            down_time,
            depth,
        )?;

        if !self.runtime_view_root_attached(&root_view_key) {
            return Ok(i32::from(handled_down != 0));
        }

        // cite: frameworks-base MotionEvent.java#getDownTime, getEventTime
        // cite: androidx TapGestureDetector.kt#detectTapGestures
        // cite: androidx DragGestureDetector.kt#awaitLongPressOrCancellation
        let hold_millis = hold_ms.max(SYNTHETIC_TAP_DURATION_MILLIS as u64) as i64;
        let target_up_time = down_time.saturating_add(hold_millis);
        let display_size = self.display_size();
        let (width, height) = self
            .runtime
            .borrow()
            .window_visible_frame
            .map(|(width, height)| (width.max(0) as u32, height.max(0) as u32))
            .unwrap_or((display_size.width(), display_size.height()));
        let pump_step_millis = self.runtime.borrow().frame_interval_millis_ceil().max(1);
        loop {
            let current = self.runtime.borrow().main_thread_uptime_millis;
            if current >= target_up_time {
                break;
            }
            self.advance_runtime_uptime(
                current.saturating_add(pump_step_millis).min(target_up_time),
            );
            self.dispatch_pending_main_loop(width, height, depth + 1)?;
        }
        let up_time = self.advance_runtime_uptime(target_up_time);
        let handled_up = self.dispatch_runtime_view_motion_event(
            &root,
            MOTION_EVENT_ACTION_UP,
            x,
            y,
            down_time,
            up_time,
            depth,
        )?;

        Ok(i32::from(handled_down != 0 || handled_up != 0))
    }

    pub(in crate::dex_run) fn perform_runtime_view_swipe(
        &self,
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        duration_ms: u64,
        steps: Option<u32>,
        depth: usize,
    ) -> Result<i32> {
        let (root, root_view_key, x, y) = self.resolve_runtime_touch_target(start_x, start_y)?;
        let delta_x = end_x - start_x;
        let delta_y = end_y - start_y;
        let steps = steps
            .unwrap_or_else(|| {
                (duration_ms / SYNTHETIC_SWIPE_STEP_MILLIS)
                    .clamp(1, SYNTHETIC_SWIPE_MAX_STEPS as u64) as u32
            })
            .max(1);
        let duration_millis =
            duration_ms.max(SYNTHETIC_TAP_DURATION_MILLIS as u64 * steps as u64) as i64;

        trace::event(
            "runtime",
            format!(
                "runtime View swipe {start_x:.1}:{start_y:.1} -> {end_x:.1}:{end_y:.1} duration={duration_ms}ms steps={steps} root={root}"
            ),
        );
        let down_time = self.runtime.borrow().main_thread_uptime_millis;
        let mut handled = self.dispatch_runtime_view_motion_event(
            &root,
            MOTION_EVENT_ACTION_DOWN,
            x,
            y,
            down_time,
            down_time,
            depth,
        )?;

        // cite: frameworks-base MotionEvent.java#ACTION_MOVE, getDownTime, getEventTime
        for step in 1..=steps {
            if !self.runtime_view_root_attached(&root_view_key) {
                return Ok(i32::from(handled != 0));
            }

            let progress = step as f32 / steps as f32;
            let move_time = self.advance_runtime_uptime(
                down_time.saturating_add(duration_millis * step as i64 / steps as i64),
            );
            let handled_move = self.dispatch_runtime_view_motion_event(
                &root,
                MOTION_EVENT_ACTION_MOVE,
                x + delta_x * progress,
                y + delta_y * progress,
                down_time,
                move_time,
                depth,
            )?;
            handled |= handled_move;
        }

        if !self.runtime_view_root_attached(&root_view_key) {
            return Ok(i32::from(handled != 0));
        }

        let up_time = self.advance_runtime_uptime(down_time.saturating_add(duration_millis));
        let handled_up = self.dispatch_runtime_view_motion_event(
            &root,
            MOTION_EVENT_ACTION_UP,
            x + delta_x,
            y + delta_y,
            down_time,
            up_time,
            depth,
        )?;

        Ok(i32::from(handled != 0 || handled_up != 0))
    }

    fn resolve_runtime_touch_target(&self, x: f32, y: f32) -> Result<(DexValue, String, f32, f32)> {
        let runtime = self.runtime.borrow();
        let root = runtime.runtime_view_roots.last().ok_or_else(|| {
            WinedroidError::new(
                "runtime touch dispatch has no runtime View root\nnext missing primitive: runtime ViewRoot touch target",
            )
        })?;

        let view = runtime_view_root_value(root).ok_or_else(|| {
            WinedroidError::new(format!(
                "runtime View root key could not be converted to a View object\nroot key: {}",
                root.view_key
            ))
        })?;
        let (origin_x, origin_y) = runtime
            .runtime_canvas_frames
            .iter()
            .rev()
            .find(|frame| frame.root_view_key == root.view_key)
            .map(|frame| (frame.x, frame.y))
            .unwrap_or((0, 0));

        Ok((
            view,
            root.view_key.clone(),
            x - origin_x as f32,
            y - origin_y as f32,
        ))
    }

    fn runtime_view_root_attached(&self, root_view_key: &str) -> bool {
        self.runtime
            .borrow()
            .runtime_view_roots
            .iter()
            .any(|candidate| candidate.view_key == root_view_key)
    }

    fn advance_runtime_uptime(&self, at_least: i64) -> i64 {
        let mut runtime = self.runtime.borrow_mut();
        let next = runtime.main_thread_uptime_millis.max(at_least);
        runtime.main_thread_uptime_millis = next;
        next
    }

    pub(in crate::dex_run) fn allocate_motion_event_with_times(
        &self,
        action: i32,
        x: f32,
        y: f32,
        down_time: i64,
        event_time: i64,
    ) -> Result<DexValue> {
        self.allocate_motion_event_with_source_and_times(
            action,
            x,
            y,
            MOTION_EVENT_SOURCE_TOUCHSCREEN,
            down_time,
            event_time,
        )
    }

    pub(super) fn allocate_motion_event_with_source_and_times(
        &self,
        action: i32,
        x: f32,
        y: f32,
        source: i32,
        down_time: i64,
        event_time: i64,
    ) -> Result<DexValue> {
        let event = self.allocate_object("Landroid/view/MotionEvent;".to_owned());
        if !self.write_stored_instance_field(
            &event,
            &motion_event_action_field(),
            DexValue::Int(action),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent action field",
            ));
        }
        if !self.write_stored_instance_field(
            &event,
            &motion_event_meta_state_field(),
            DexValue::Int(MOTION_EVENT_META_STATE_NONE),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent metaState field",
            ));
        }
        if !self.write_stored_instance_field(
            &event,
            &motion_event_down_time_field(),
            DexValue::Long(down_time),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent downTime field",
            ));
        }
        if !self.write_stored_instance_field(
            &event,
            &motion_event_event_time_field(),
            DexValue::Long(event_time),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent eventTime field",
            ));
        }
        if !self.write_stored_instance_field(&event, &motion_event_x_field(), DexValue::Float(x)) {
            return Err(WinedroidError::new("failed to store MotionEvent x field"));
        }
        if !self.write_stored_instance_field(&event, &motion_event_y_field(), DexValue::Float(y)) {
            return Err(WinedroidError::new("failed to store MotionEvent y field"));
        }
        if !self.write_stored_instance_field(
            &event,
            &motion_event_pressure_field(),
            DexValue::Float(MOTION_EVENT_PRESSURE_TOUCH),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent pressure field",
            ));
        }
        if !self.write_stored_instance_field(
            &event,
            &motion_event_source_field(),
            DexValue::Int(source),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent source field",
            ));
        }

        Ok(event)
    }

    fn dispatch_runtime_view_motion_event(
        &self,
        view: &DexValue,
        action: i32,
        x: f32,
        y: f32,
        down_time: i64,
        event_time: i64,
        depth: usize,
    ) -> Result<i32> {
        let Some(class_descriptor) = value_class_descriptor(view) else {
            return Err(WinedroidError::new(format!(
                "runtime touch dispatch requires an object-backed View root\nroot: {view}"
            )));
        };
        let event = self.allocate_motion_event_with_times(action, x, y, down_time, event_time)?;

        if let Some(handled) = self.execute_optional_view_motion_method(
            view,
            class_descriptor,
            "dispatchTouchEvent",
            &event,
            depth,
        )? {
            return Ok(handled);
        }

        if self.dispatch_runtime_view_motion_event_to_children(
            view, action, x, y, down_time, event_time, depth,
        )? != 0
        {
            return Ok(1);
        }

        if let Some(handled) = self.execute_optional_view_motion_method(
            view,
            class_descriptor,
            "onTouchEvent",
            &event,
            depth,
        )? {
            return Ok(handled);
        }

        Ok(0)
    }

    fn dispatch_runtime_view_motion_event_to_children(
        &self,
        view: &DexValue,
        action: i32,
        x: f32,
        y: f32,
        down_time: i64,
        event_time: i64,
        depth: usize,
    ) -> Result<i32> {
        for child in self.view_group_child_snapshot(view).into_iter().rev() {
            let left = self.read_view_bound(&child, "left") as f32;
            let top = self.read_view_bound(&child, "top") as f32;
            let right = self.read_view_bound(&child, "right") as f32;
            let bottom = self.read_view_bound(&child, "bottom") as f32;

            if x < left || x >= right || y < top || y >= bottom {
                continue;
            }

            let handled = self.dispatch_runtime_view_motion_event(
                &child,
                action,
                x - left,
                y - top,
                down_time,
                event_time,
                depth + 1,
            )?;
            if handled != 0 {
                return Ok(1);
            }
        }

        Ok(0)
    }

    pub(super) fn execute_optional_view_motion_method(
        &self,
        view: &DexValue,
        class_descriptor: &str,
        method_name: &str,
        event: &DexValue,
        depth: usize,
    ) -> Result<Option<i32>> {
        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto: crate::dex::DexProto {
                shorty: "ZL".to_owned(),
                return_type: "Z".to_owned(),
                parameters: vec!["Landroid/view/MotionEvent;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            trace::event(
                "runtime",
                format!(
                    "runtime View touch method missing class={class_descriptor} method={method_name}"
                ),
            );
            return Ok(None);
        };
        let result = self.execute_method(
            None,
            &resolved,
            Some(&[view.clone(), event.clone()]),
            depth + 1,
        )?;

        match result {
            DexExecutionResult::Int(value) => {
                let handled = i32::from(value != 0);
                trace::event(
                    "runtime",
                    format!(
                        "runtime View touch method class={class_descriptor} resolved_class={} method={method_name} handled={}",
                        resolved.class_descriptor,
                        handled != 0
                    ),
                );
                Ok(Some(handled))
            }
            DexExecutionResult::Void
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(format!(
                "runtime View touch method did not return boolean\nclass: {class_descriptor}\nmethod: {}\nresult: {result}",
                method.display_signature()
            ))),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "runtime View touch method did not return boolean\nclass: {class_descriptor}\nmethod: {}\nresult: parked",
                    method.display_signature()
                )))
            }
        }
    }

    pub(in crate::dex_run) fn motion_event_action(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let event = invocation.arguments.first()?;
        let DexValue::Int(action) =
            self.read_stored_instance_field(event, &motion_event_action_field())?
        else {
            return None;
        };

        Some(action)
    }

    pub(in crate::dex_run) fn motion_event_meta_state(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let event = invocation.arguments.first()?;
        let DexValue::Int(meta_state) =
            self.read_stored_instance_field(event, &motion_event_meta_state_field())?
        else {
            return None;
        };

        Some(meta_state)
    }

    pub(in crate::dex_run) fn motion_event_time(
        &self,
        invocation: &DexInvocation,
        field: &DexFieldId,
    ) -> Option<i64> {
        let event = invocation.arguments.first()?;
        let DexValue::Long(time_millis) = self.read_stored_instance_field(event, field)? else {
            return None;
        };

        Some(time_millis)
    }

    pub(in crate::dex_run) fn motion_event_action_masked(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        Some(self.motion_event_action(invocation)? & MOTION_EVENT_ACTION_MASK)
    }

    pub(in crate::dex_run) fn view_on_touch_event(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        if invocation.arguments.len() != 2
            || !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_object_like(&invocation.arguments[1])
        {
            return None;
        }

        let DexValue::Int(_) = self
            .read_stored_instance_field(&invocation.arguments[1], &motion_event_action_field())?
        else {
            return None;
        };

        let clickable =
            self.view_bool_value(&invocation.arguments[0], &view_clickable_field(), 0)?;
        let long_clickable =
            self.view_bool_value(&invocation.arguments[0], &view_long_clickable_field(), 0)?;
        let handled = clickable != 0
            || long_clickable != 0
            || self.view_has_click_binding(&invocation.arguments[0]);

        Some(i32::from(handled))
    }

    fn view_bool_value(&self, view: &DexValue, field: &DexFieldId, default: i32) -> Option<i32> {
        let value = self
            .read_stored_instance_field(view, field)
            .unwrap_or(DexValue::Int(default));
        let DexValue::Int(value) = value else {
            return None;
        };

        Some(i32::from(value != 0))
    }

    pub(in crate::dex_run) fn motion_event_axis_value(
        &self,
        invocation: &DexInvocation,
        field: &DexFieldId,
    ) -> Option<f32> {
        let event = invocation.arguments.first()?;
        let DexValue::Float(value) = self.read_stored_instance_field(event, field)? else {
            return None;
        };

        Some(value)
    }

    pub(in crate::dex_run) fn motion_event_pressure(
        &self,
        invocation: &DexInvocation,
    ) -> Option<f32> {
        let event = invocation.arguments.first()?;
        let DexValue::Float(value) =
            self.read_stored_instance_field(event, &motion_event_pressure_field())?
        else {
            return None;
        };

        Some(value)
    }

    pub(in crate::dex_run) fn motion_event_source(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let event = invocation.arguments.first()?;
        let DexValue::Int(source) =
            self.read_stored_instance_field(event, &motion_event_source_field())?
        else {
            return None;
        };

        Some(source)
    }

    pub(in crate::dex_run) fn input_event_is_from_source(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let DexValue::Int(requested_source) = &invocation.arguments[1] else {
            return None;
        };
        let source = self.motion_event_source(invocation)?;

        Some(i32::from((source & *requested_source) == *requested_source))
    }

    pub(in crate::dex_run) fn motion_event_obtain_no_history(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        let action = self.motion_event_action(invocation)?;
        let x = self.motion_event_axis_value(invocation, &motion_event_x_field())?;
        let y = self.motion_event_axis_value(invocation, &motion_event_y_field())?;
        let source = self.motion_event_source(invocation)?;
        let down_time = self.motion_event_time(invocation, &motion_event_down_time_field())?;
        let event_time = self.motion_event_time(invocation, &motion_event_event_time_field())?;

        self.allocate_motion_event_with_source_and_times(
            action, x, y, source, down_time, event_time,
        )
        .ok()
    }

    pub(in crate::dex_run::view) fn view_has_click_binding(&self, view: &DexValue) -> bool {
        let view_key = map_identity_key(view);
        let runtime = self.runtime.borrow();

        runtime.view_click_listeners.contains_key(&view_key)
            || runtime
                .view_declared_on_click_handlers
                .contains_key(&view_key)
    }

    pub(in crate::dex_run) fn active_menu_overlay_item(
        &self,
        item_id: i32,
    ) -> Option<(DexValue, RuntimeMenuOverlayItem)> {
        let runtime = self.runtime.borrow();

        for overlay in runtime.active_menu_overlays.iter().rev() {
            if let Some(item) = overlay.items.iter().find(|item| item.item_id == item_id) {
                return Some((overlay.list_view.clone(), item.clone()));
            }
        }

        None
    }

    pub(in crate::dex_run) fn active_menu_overlay_item_listener(
        &self,
        item_id: i32,
    ) -> Option<DexValue> {
        let runtime = self.runtime.borrow();
        for overlay in runtime.active_menu_overlays.iter().rev() {
            if overlay.items.iter().any(|item| item.item_id == item_id) {
                return overlay.menu_item_click_listener.clone();
            }
        }
        None
    }

    fn dispatch_popup_menu_item_click(
        &self,
        listener: &DexValue,
        item_id: i32,
        depth: usize,
    ) -> Result<i32> {
        let Some(listener_descriptor) = value_class_descriptor(listener) else {
            return Ok(0);
        };
        let method = DexMethodId {
            class_descriptor: listener_descriptor.to_owned(),
            name: "onMenuItemClick".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "ZL".to_owned(),
                return_type: "Z".to_owned(),
                parameters: vec!["Landroid/view/MenuItem;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(listener_descriptor, &method)?
        else {
            return Err(WinedroidError::new(format!(
                "PopupMenu.OnMenuItemClickListener.onMenuItemClick was not found\nlistener class: {listener_descriptor}"
            )));
        };

        let item = DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/view/MenuItem;:option/{item_id}"
        )));
        let result =
            self.execute_method(None, &resolved, Some(&[listener.clone(), item]), depth + 1)?;
        match result {
            DexExecutionResult::Int(value) => Ok(i32::from(value != 0)),
            DexExecutionResult::Value(DexValue::Int(value)) => Ok(i32::from(value != 0)),
            DexExecutionResult::Void
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "PopupMenu.OnMenuItemClickListener.onMenuItemClick did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "PopupMenu.OnMenuItemClickListener.onMenuItemClick did not complete normally",
                ))
            }
        }
    }

    pub(in crate::dex_run) fn perform_action_bar_navigation(&self, depth: usize) -> Result<i32> {
        const ANDROID_R_ID_HOME: i32 = 0x0102_002c;

        let listener = self.runtime.borrow().action_bar_nav_click.clone();
        if let Some(listener) = listener {
            let nav_view = DexValue::Reference(std::rc::Rc::from(
                "Landroid/view/View;:action-bar-navigation",
            ));
            return self.dispatch_view_click_listener(listener, nav_view, depth);
        }

        self.dispatch_options_item_selected(ANDROID_R_ID_HOME, depth)
    }

    pub(in crate::dex_run) fn dispatch_options_item_selected(
        &self,
        item_id: i32,
        depth: usize,
    ) -> Result<i32> {
        let Some(activity) = self.active_activity_receiver() else {
            return Ok(0);
        };
        let Some(class_descriptor) = value_class_descriptor(&activity).map(str::to_owned) else {
            return Ok(0);
        };

        let method = DexMethodId {
            class_descriptor: class_descriptor.clone(),
            name: "onOptionsItemSelected".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "ZL".to_owned(),
                return_type: "Z".to_owned(),
                parameters: vec!["Landroid/view/MenuItem;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(&class_descriptor, &method)?
        else {
            return Ok(0);
        };

        let item = DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/view/MenuItem;:option/{item_id}"
        )));
        let result = self.execute_method(None, &resolved, Some(&[activity, item]), depth + 1)?;
        match result {
            DexExecutionResult::Void | DexExecutionResult::Int(_) => Ok(1),
            DexExecutionResult::Value(_) | DexExecutionResult::Throw(_) => Err(
                WinedroidError::new("Activity.onOptionsItemSelected did not complete normally"),
            ),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "Activity.onOptionsItemSelected did not complete normally",
                ))
            }
        }
    }

    fn active_adapter_item_click_listener(&self, list_view: &DexValue) -> Option<DexValue> {
        let view_id = self.view_id_from_value(list_view)?;
        let activity = self.active_activity_receiver()?;
        self.runtime
            .borrow()
            .adapter_item_click_listeners
            .get(&(map_identity_key(&activity), view_id))
            .cloned()
    }

    pub(in crate::dex_run) fn dispatch_adapter_item_click(
        &self,
        list_view: &DexValue,
        position: i32,
        depth: usize,
    ) -> Result<i32> {
        let listener = self
            .active_adapter_item_click_listener(list_view)
            .or_else(|| {
                self.read_stored_instance_field(
                    list_view,
                    &adapter_view_item_click_listener_field(),
                )
            })
            .unwrap_or(DexValue::Null);
        let Some(listener_descriptor) = value_class_descriptor(&listener) else {
            return Ok(0);
        };
        let on_item_click_method = DexMethodId {
            class_descriptor: listener_descriptor.to_owned(),
            name: "onItemClick".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "VLLIJ".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec![
                    "Landroid/widget/AdapterView;".to_owned(),
                    "Landroid/view/View;".to_owned(),
                    "I".to_owned(),
                    "J".to_owned(),
                ],
            },
        };
        let Some(resolved) = self
            .resolve_exact_method_in_class_hierarchy(listener_descriptor, &on_item_click_method)?
        else {
            return Err(WinedroidError::new(format!(
                "AdapterView item click listener method was not found\nlistener class: {listener_descriptor}\nnext missing primitive: AdapterView.OnItemClickListener dispatch"
            )));
        };
        let item_view = DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/view/View;:adapter-item/{position}"
        )));
        let row_id = DexValue::Long(i64::from(position));
        let result = self.execute_method(
            None,
            &resolved,
            Some(&[
                listener,
                list_view.clone(),
                item_view,
                DexValue::Int(position),
                row_id.clone(),
                row_id,
            ]),
            depth + 1,
        )?;

        match result {
            DexExecutionResult::Void => Ok(1),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "AdapterView item click listener did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "AdapterView item click listener did not complete normally",
                ))
            }
        }
    }

    pub(in crate::dex_run) fn dispatch_active_menu_overlay_item_click(
        &self,
        item_id: i32,
        depth: usize,
    ) -> Result<i32> {
        let Some((list_view, item)) = self.active_menu_overlay_item(item_id) else {
            return Ok(0);
        };

        if !item.enabled {
            return Ok(0);
        }

        // cite: androidx SupportMenuInflater.java#setItem, InflatedOnMenuItemClickListener
        // cite: androidx MenuItemImpl.java#invoke
        if let Some(method_name) = item.on_click.as_deref() {
            let menu_item = DexValue::Reference(std::rc::Rc::from(format!(
                "Landroid/view/MenuItem;:option/{item_id}"
            )));
            let handled = self.dispatch_menu_item_declared_on_click_method_name(
                method_name,
                menu_item,
                depth,
            )?;
            if handled != 0 {
                self.dismiss_menu_overlay_containing_item(item_id);
                return Ok(handled);
            }
        }

        if let Some(listener) = self.active_menu_overlay_item_listener(item_id) {
            let handled = self.dispatch_popup_menu_item_click(&listener, item_id, depth)?;
            let popup_key = map_identity_key(&list_view);
            self.runtime
                .borrow_mut()
                .active_menu_overlays
                .retain(|overlay| overlay.popup_key != popup_key);
            return Ok(handled);
        }

        let listener = self
            .read_stored_instance_field(&list_view, &adapter_view_item_click_listener_field())
            .unwrap_or(DexValue::Null);
        let Some(listener_descriptor) = value_class_descriptor(&listener) else {
            return Err(WinedroidError::new(format!(
                "popup menu item click has no object-backed AdapterView.OnItemClickListener\nitem id: {item_id}"
            )));
        };
        let on_item_click_method = DexMethodId {
            class_descriptor: listener_descriptor.to_owned(),
            name: "onItemClick".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "VLLIJ".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec![
                    "Landroid/widget/AdapterView;".to_owned(),
                    "Landroid/view/View;".to_owned(),
                    "I".to_owned(),
                    "J".to_owned(),
                ],
            },
        };
        let Some(resolved) = self
            .resolve_exact_method_in_class_hierarchy(listener_descriptor, &on_item_click_method)?
        else {
            return Err(WinedroidError::new(format!(
                "popup menu item click listener method was not found\nlistener class: {listener_descriptor}\nnext missing primitive: AdapterView.OnItemClickListener dispatch"
            )));
        };
        let item_view = DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/view/View;:popup-menu-item/{item_id}"
        )));
        let row_id = DexValue::Long(i64::from(item_id));
        let result = self.execute_method(
            None,
            &resolved,
            Some(&[
                listener,
                list_view,
                item_view,
                DexValue::Int(item.position),
                row_id.clone(),
                row_id,
            ]),
            depth + 1,
        )?;

        match result {
            DexExecutionResult::Void => Ok(1),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "popup menu item click listener did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "popup menu item click listener did not complete normally",
                ))
            }
        }
    }

    pub(in crate::dex_run) fn dispatch_view_click_listener(
        &self,
        listener: DexValue,
        view: DexValue,
        depth: usize,
    ) -> Result<i32> {
        let Some(listener_descriptor) = value_class_descriptor(&listener) else {
            return Err(WinedroidError::new(format!(
                "View.performClick requires an object-backed OnClickListener\nlistener: {listener}"
            )));
        };

        let on_click_method = DexMethodId {
            class_descriptor: listener_descriptor.to_owned(),
            name: "onClick".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "VL".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec!["Landroid/view/View;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(listener_descriptor, &on_click_method)?
        else {
            return Err(WinedroidError::new(format!(
                "View.performClick OnClickListener.onClick was not found\nlistener class: {listener_descriptor}\nnext missing primitive: Android view click listener dispatch"
            )));
        };

        let result =
            self.execute_method(None, &resolved, Some(&[listener.clone(), view]), depth + 1)?;

        match result {
            DexExecutionResult::Void => Ok(1),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "View.performClick OnClickListener.onClick did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "View.performClick OnClickListener.onClick did not complete normally",
                ))
            }
        }
    }

    pub(in crate::dex_run) fn dispatch_declared_on_click_handler(
        &self,
        method_name: String,
        view: DexValue,
        depth: usize,
    ) -> Result<i32> {
        let receiver = self.active_activity_receiver().ok_or_else(|| {
            WinedroidError::new("XML onClick handler dispatch has no retained Activity receiver")
        })?;
        let Some(activity_descriptor) = value_class_descriptor(&receiver) else {
            return Err(WinedroidError::new(format!(
                "XML onClick handler dispatch requires an object-backed Activity receiver\nreceiver: {receiver}"
            )));
        };

        let on_click_method = DexMethodId {
            class_descriptor: activity_descriptor.to_owned(),
            name: method_name.clone(),
            proto: crate::dex::DexProto {
                shorty: "VL".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec!["Landroid/view/View;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(activity_descriptor, &on_click_method)?
        else {
            return Err(WinedroidError::new(format!(
                "XML onClick handler method was not found\nactivity class: {activity_descriptor}\nmethod: {method_name}(Landroid/view/View;)V\nnext missing primitive: Android XML onClick handler dispatch"
            )));
        };

        let result = self.execute_method(None, &resolved, Some(&[receiver, view]), depth + 1)?;

        match result {
            DexExecutionResult::Void => Ok(1),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "XML onClick handler did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "XML onClick handler did not complete normally",
                ))
            }
        }
    }

    pub(in crate::dex_run) fn dispatch_menu_item_declared_on_click_handler(
        &self,
        invocation: &DexInvocation,
        depth: usize,
    ) -> Result<i32> {
        if invocation.arguments.len() != 2 {
            return Err(WinedroidError::new(format!(
                "MenuItem XML onClick expected listener receiver and MenuItem, got {} argument(s)",
                invocation.arguments.len()
            )));
        }

        let method_name =
            menu_item_on_click_listener_method_name(&invocation.arguments[0]).ok_or_else(|| {
                WinedroidError::new(format!(
                    "MenuItem XML onClick listener receiver is not a Winedroid listener\nreceiver: {}",
                    invocation.arguments[0]
                ))
            })?;
        let menu_item = invocation.arguments[1].clone();

        self.dispatch_menu_item_declared_on_click_method_name(method_name, menu_item, depth)
    }

    pub(in crate::dex_run) fn dispatch_menu_item_declared_on_click_method_name(
        &self,
        method_name: &str,
        menu_item: DexValue,
        depth: usize,
    ) -> Result<i32> {
        let receiver = self.active_activity_receiver().ok_or_else(|| {
            WinedroidError::new("MenuItem XML onClick dispatch has no retained Activity receiver")
        })?;
        let Some(activity_descriptor) = value_class_descriptor(&receiver) else {
            return Err(WinedroidError::new(format!(
                "MenuItem XML onClick dispatch requires an object-backed Activity receiver\nreceiver: {receiver}"
            )));
        };

        if let Some(resolved) =
            self.resolve_menu_item_on_click_method(activity_descriptor, method_name, "Z")?
        {
            let result =
                self.execute_method(None, &resolved, Some(&[receiver, menu_item]), depth + 1)?;
            return match result {
                DexExecutionResult::Int(value) => Ok(value),
                DexExecutionResult::Value(DexValue::Int(value)) => Ok(value),
                DexExecutionResult::Void
                | DexExecutionResult::Value(_)
                | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                    "MenuItem XML onClick boolean handler did not return a boolean",
                )),
                DexExecutionResult::Parked(execution) => {
                    self.discard_parked_execution(&execution);
                    Err(WinedroidError::new(
                        "MenuItem XML onClick boolean handler did not return a boolean",
                    ))
                }
            };
        }

        if let Some(resolved) =
            self.resolve_menu_item_on_click_method(activity_descriptor, method_name, "V")?
        {
            let result =
                self.execute_method(None, &resolved, Some(&[receiver, menu_item]), depth + 1)?;
            return match result {
                DexExecutionResult::Void => Ok(1),
                DexExecutionResult::Int(_)
                | DexExecutionResult::Value(_)
                | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                    "MenuItem XML onClick void handler did not complete normally",
                )),
                DexExecutionResult::Parked(execution) => {
                    self.discard_parked_execution(&execution);
                    Err(WinedroidError::new(
                        "MenuItem XML onClick void handler did not complete normally",
                    ))
                }
            };
        }

        Err(WinedroidError::new(format!(
            "MenuItem XML onClick handler method was not found\nactivity class: {activity_descriptor}\nmethod: {method_name}(Landroid/view/MenuItem;)Z or V\nnext missing primitive: Android menu XML onClick handler dispatch"
        )))
    }

    pub(in crate::dex_run) fn resolve_menu_item_on_click_method(
        &self,
        activity_descriptor: &str,
        method_name: &str,
        return_type: &str,
    ) -> Result<Option<Rc<ResolvedMethod>>> {
        let on_click_method = DexMethodId {
            class_descriptor: activity_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto: crate::dex::DexProto {
                shorty: format!("{return_type}L"),
                return_type: return_type.to_owned(),
                parameters: vec!["Landroid/view/MenuItem;".to_owned()],
            },
        };

        self.resolve_exact_method_in_class_hierarchy(activity_descriptor, &on_click_method)
    }
}
