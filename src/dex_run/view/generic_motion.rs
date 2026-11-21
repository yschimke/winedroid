use super::helpers::*;
use super::*;

// cite: frameworks-base MotionEvent.java#ACTION_SCROLL, AXIS_SCROLL
const MOTION_EVENT_ACTION_SCROLL: i32 = 8;
const MOTION_EVENT_AXIS_SCROLL: i32 = 26;
// cite: frameworks-base InputDevice.java#SOURCE_ROTARY_ENCODER, SOURCE_CLASS_POINTER
const INPUT_SOURCE_ROTARY_ENCODER: i32 = 0x0040_0000;
const INPUT_SOURCE_CLASS_POINTER: i32 = 0x0000_0002;

impl DexProgram {
    // cite: frameworks-base ViewRootImpl.java#processGenericMotionEvent
    // cite: frameworks-base ViewGroup.java#dispatchGenericFocusedEvent, mFocused
    pub(in crate::dex_run) fn perform_runtime_view_rotary_scroll(
        &self,
        axis_scroll: f32,
        depth: usize,
    ) -> Result<i32> {
        let Some(focused) = self.focused_view() else {
            trace::event(
                "runtime",
                format!("runtime View rotary scroll axis={axis_scroll} unhandled: no focused View"),
            );
            return Ok(0);
        };
        let top = self.focus_path_from_top(&focused).remove(0);
        let event_time = self.runtime.borrow().main_thread_uptime_millis;
        let event = self.allocate_rotary_motion_event(axis_scroll, event_time)?;

        trace::event(
            "runtime",
            format!("runtime View rotary scroll axis={axis_scroll} top={top} focused={focused}"),
        );

        self.dispatch_view_generic_motion_event(&top, &event, depth)
    }

    fn focus_path_from_top(&self, focused: &DexValue) -> Vec<DexValue> {
        let mut path = vec![focused.clone()];
        let mut visited = vec![map_identity_key(focused)];
        loop {
            let key = visited.last().cloned().unwrap_or_default();
            let Some(parent) = self.runtime.borrow().view_parents.get(&key).cloned() else {
                break;
            };
            let parent_key = map_identity_key(&parent);
            if visited.contains(&parent_key) {
                break;
            }
            visited.push(parent_key);
            path.push(parent);
        }
        path.reverse();
        path
    }

    fn allocate_rotary_motion_event(&self, axis_scroll: f32, event_time: i64) -> Result<DexValue> {
        let event = self.allocate_motion_event_with_source_and_times(
            MOTION_EVENT_ACTION_SCROLL,
            0.0,
            0.0,
            INPUT_SOURCE_ROTARY_ENCODER,
            event_time,
            event_time,
        )?;
        if !self.write_stored_instance_field(
            &event,
            &motion_event_pressure_field(),
            DexValue::Float(0.0),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent pressure field",
            ));
        }
        if !self.write_stored_instance_field(
            &event,
            &motion_event_scroll_axis_field(),
            DexValue::Float(axis_scroll),
        ) {
            return Err(WinedroidError::new(
                "failed to store MotionEvent AXIS_SCROLL value",
            ));
        }

        Ok(event)
    }

    fn dispatch_view_generic_motion_event(
        &self,
        view: &DexValue,
        event: &DexValue,
        depth: usize,
    ) -> Result<i32> {
        let Some(class_descriptor) = value_class_descriptor(view) else {
            return Err(WinedroidError::new(format!(
                "generic motion dispatch requires an object-backed View\nview: {view}"
            )));
        };

        if let Some(handled) = self.execute_optional_view_motion_method(
            view,
            class_descriptor,
            "dispatchGenericMotionEvent",
            event,
            depth,
        )? {
            return Ok(handled);
        }

        self.framework_view_dispatch_generic_motion_event(view, event, depth)
    }

    pub(in crate::dex_run) fn view_dispatch_generic_motion_event_call(
        &self,
        invocation: &DexInvocation,
        super_call: bool,
        depth: usize,
    ) -> Option<Result<i32>> {
        if invocation.arguments.len() != 2
            || !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_object_like(&invocation.arguments[1])
        {
            return None;
        }
        let Some(DexValue::Int(source)) =
            self.read_stored_instance_field(&invocation.arguments[1], &motion_event_source_field())
        else {
            return None;
        };
        if source & INPUT_SOURCE_CLASS_POINTER != 0 {
            return None;
        }

        let view = &invocation.arguments[0];
        let event = &invocation.arguments[1];
        Some(if super_call {
            self.framework_view_dispatch_generic_motion_event(view, event, depth)
        } else {
            self.dispatch_view_generic_motion_event(view, event, depth)
        })
    }

    // cite: frameworks-base View.java#dispatchGenericMotionEvent, dispatchGenericMotionEventInternal
    // cite: frameworks-base ViewGroup.java#dispatchGenericFocusedEvent
    fn framework_view_dispatch_generic_motion_event(
        &self,
        view: &DexValue,
        event: &DexValue,
        depth: usize,
    ) -> Result<i32> {
        if let Some(child) = self.focused_child_on_path(view) {
            if self.dispatch_view_generic_motion_event(&child, event, depth + 1)? != 0 {
                return Ok(1);
            }
        }

        self.view_dispatch_generic_motion_event_internal(view, event, depth)
    }

    fn view_dispatch_generic_motion_event_internal(
        &self,
        view: &DexValue,
        event: &DexValue,
        depth: usize,
    ) -> Result<i32> {
        if let Some(listener) =
            self.read_stored_instance_field(view, &view_generic_motion_listener_field())
        {
            if dex_value_is_object_like(&listener)
                && self.dispatch_generic_motion_listener(listener, view.clone(), event, depth)? != 0
            {
                return Ok(1);
            }
        }

        let Some(class_descriptor) = value_class_descriptor(view) else {
            return Ok(0);
        };
        Ok(self
            .execute_optional_view_motion_method(
                view,
                class_descriptor,
                "onGenericMotionEvent",
                event,
                depth,
            )?
            .unwrap_or(0))
    }

    fn dispatch_generic_motion_listener(
        &self,
        listener: DexValue,
        view: DexValue,
        event: &DexValue,
        depth: usize,
    ) -> Result<i32> {
        let Some(listener_descriptor) = value_class_descriptor(&listener) else {
            return Ok(0);
        };
        let on_generic_motion = DexMethodId {
            class_descriptor: listener_descriptor.to_owned(),
            name: "onGenericMotion".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "ZLL".to_owned(),
                return_type: "Z".to_owned(),
                parameters: vec![
                    "Landroid/view/View;".to_owned(),
                    "Landroid/view/MotionEvent;".to_owned(),
                ],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(listener_descriptor, &on_generic_motion)?
        else {
            return Err(WinedroidError::new(format!(
                "View.OnGenericMotionListener.onGenericMotion was not found\nlistener class: {listener_descriptor}"
            )));
        };

        match self.execute_method(
            None,
            &resolved,
            Some(&[listener.clone(), view, event.clone()]),
            depth + 1,
        )? {
            DexExecutionResult::Int(value) => Ok(i32::from(value != 0)),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "View.OnGenericMotionListener.onGenericMotion did not return boolean",
                ))
            }
            result => Err(WinedroidError::new(format!(
                "View.OnGenericMotionListener.onGenericMotion did not return boolean\nresult: {result}"
            ))),
        }
    }

    fn focused_child_on_path(&self, view: &DexValue) -> Option<DexValue> {
        let focused = self.focused_view()?;
        let view_key = map_identity_key(view);
        let path = self.focus_path_from_top(&focused);
        let index = path
            .iter()
            .position(|candidate| map_identity_key(candidate) == view_key)?;
        path.get(index + 1).cloned()
    }

    pub(in crate::dex_run) fn motion_event_get_axis_value(
        &self,
        invocation: &DexInvocation,
    ) -> Option<f32> {
        let event = invocation.arguments.first()?;
        let DexValue::Int(axis) = invocation.arguments.get(1)? else {
            return None;
        };
        if *axis != MOTION_EVENT_AXIS_SCROLL {
            return None;
        }

        match self.read_stored_instance_field(event, &motion_event_scroll_axis_field()) {
            Some(DexValue::Float(value)) => Some(value),
            _ => Some(0.0),
        }
    }
}
