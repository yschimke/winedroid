use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::env;

use super::*;
use crate::dex_run::android::content_res::bitmap_drawable_resource_id_field;
use crate::dex_run::view::layout_id_from_reference_str;

macro_rules! rt_int_field {
    ($rt:expr, $obj:expr, $field:expr $(,)?) => {{
        thread_local! {
            static IC: std::cell::Cell<Option<(u32, u16)>> = const { std::cell::Cell::new(None) };
        }
        IC.with(|cell| {
            let mut ic = cell.get();
            let out = match $rt.value_field_cached($obj, &$field.display_signature(), &mut ic) {
                Some(DexValue::Int(value)) => Some(*value),
                _ => None,
            };
            cell.set(ic);
            out
        })
    }};
}

macro_rules! rt_float_field {
    ($rt:expr, $obj:expr, $field:expr $(,)?) => {{
        thread_local! {
            static IC: std::cell::Cell<Option<(u32, u16)>> = const { std::cell::Cell::new(None) };
        }
        IC.with(|cell| {
            let mut ic = cell.get();
            let out = match $rt.value_field_cached($obj, &$field.display_signature(), &mut ic) {
                Some(DexValue::Float(value)) => Some(*value),
                _ => None,
            };
            cell.set(ic);
            out
        })
    }};
}

impl DexProgram {
    pub(in crate::dex_run) fn view_snapshot(&self) -> DexViewSnapshot {
        let active_activity = self.runtime.borrow().activity_stack.last().cloned();
        let content_layout_start = active_activity
            .as_ref()
            .map(|activity| activity.content_layout_start)
            .unwrap_or(0);
        let view_lookup_start = active_activity
            .as_ref()
            .map(|activity| activity.view_lookup_start)
            .unwrap_or(0);
        let runtime_view_root_start = active_activity
            .as_ref()
            .map(|activity| activity.runtime_view_root_start)
            .unwrap_or(0);
        let theme_styles = active_activity
            .as_ref()
            .map(|activity| self.context_theme_style_snapshot(&activity.receiver))
            .unwrap_or_default();
        let runtime = self.runtime.borrow();
        let focused_view_key = runtime.focused_view.as_ref().map(map_identity_key);

        let content_layouts = runtime.content_layouts[content_layout_start..].to_vec();
        drop(runtime);
        let inflated_layouts = self.attached_inflated_layout_ids();
        let dynamic_children = self.dynamic_container_children_snapshot();
        let runtime = self.runtime.borrow();
        let view_field_snapshot =
            view_field_snapshot(&runtime, focused_view_key.as_deref(), &content_layouts);
        let display_size = self.display_size();

        DexViewSnapshot {
            content_layouts,
            inflated_layouts,
            theme_styles,
            resource_sdk_version: BOOTSTRAP_ANDROID_SDK_INT as u16,
            resource_ui_mode: self.config.android_configuration_ui_mode(),
            resource_display_width: display_size.width() as u16,
            resource_display_height: display_size.height() as u16,
            resource_screen_round: self.config.display_shape().is_round(),
            runtime_view_roots: runtime.runtime_view_roots[runtime_view_root_start..].to_vec(),
            runtime_canvas_frames: runtime_canvas_frame_snapshot(&runtime),
            view_lookup_ids: runtime.view_lookup_ids[view_lookup_start..].to_vec(),
            view_texts: view_field_snapshot.texts,
            view_text_cursors: view_field_snapshot.cursors,
            view_images: view_field_snapshot.images,
            view_visibilities: view_field_snapshot.visibilities,
            click_listeners: click_listener_snapshot(&runtime.view_click_listeners),
            declared_on_click_handlers: declared_on_click_snapshot(
                &runtime.view_declared_on_click_handlers,
            ),
            adapter_rows: adapter_rows_snapshot(&runtime.adapter_rows),
            dynamic_children,
            activity_overlays: activity_overlays_snapshot(&runtime),
            dialog_overlays: dialog_overlays_snapshot(&runtime),
            action_bar_title: runtime.action_bar_title.clone(),
            action_bar_nav_icon: runtime
                .action_bar_nav_icon
                .and_then(|id| u32::try_from(id).ok()),
            action_bar_overflow_open: runtime.action_bar_overflow_open,
            action_bar_present: runtime.action_bar_present,
            action_bar_home_as_up: runtime.action_bar_home_as_up,
            options_menu: options_menu_snapshot(&runtime.options_menu),
        }
    }

    pub(in crate::dex_run) fn activity_overlays(&self) -> Vec<DexActivityOverlay> {
        activity_overlays_snapshot(&self.runtime.borrow())
    }

    pub(in crate::dex_run) fn dialog_overlays(&self) -> Vec<DexDialogOverlay> {
        dialog_overlays_snapshot(&self.runtime.borrow())
    }

    pub(in crate::dex_run) fn dialog_content_layouts(&self) -> Vec<i32> {
        let runtime = self.runtime.borrow();
        let dialog_content_layout_start = runtime
            .activity_stack
            .last()
            .map(|activity| activity.dialog_content_layout_start)
            .unwrap_or(0);

        runtime.dialog_content_layouts[dialog_content_layout_start..].to_vec()
    }

    pub(in crate::dex_run) fn preference_snapshot(&self) -> Vec<DexPreferenceSnapshot> {
        let inflated = self.inflated_preference_snapshot();
        if !inflated.is_empty() {
            return inflated;
        }

        let runtime = self.runtime.borrow();
        let min_object_id = runtime
            .activity_stack
            .last()
            .map(|activity| activity.object_start)
            .unwrap_or(0);

        preference_snapshot_from_runtime(self, &runtime, min_object_id)
    }

    pub(in crate::dex_run) fn perform_preference_click(
        &self,
        preference_key: &str,
        depth: usize,
    ) -> Result<i32> {
        let preference = self.preference_value(preference_key).ok_or_else(|| {
            WinedroidError::new(format!(
                "preference click target was not found\npreference key: {preference_key}"
            ))
        })?;
        let Some(class_descriptor) = value_class_descriptor(&preference).map(str::to_owned) else {
            return Err(WinedroidError::new(format!(
                "preference click target has no class descriptor\npreference key: {preference_key}"
            )));
        };
        let mut resolved = None;
        for name in ["performClick", "l"] {
            let method = DexMethodId {
                class_descriptor: class_descriptor.clone(),
                name: name.to_owned(),
                proto: crate::dex::DexProto {
                    shorty: "V".to_owned(),
                    return_type: "V".to_owned(),
                    parameters: Vec::new(),
                },
            };
            resolved = self.resolve_exact_method_in_class_hierarchy(&class_descriptor, &method)?;
            if resolved.is_some() {
                break;
            }
        }
        let Some(resolved) = resolved else {
            return Err(WinedroidError::new(format!(
                "Preference click method was not found\nclass: {class_descriptor}\nnext missing primitive: AndroidX Preference click dispatch"
            )));
        };
        let dialog_count = self.runtime.borrow().active_dialog_overlays.len();
        let result = self.execute_method(None, &resolved, Some(&[preference]), depth + 1)?;

        match result {
            DexExecutionResult::Void => {
                self.show_list_preference_dialog_if_needed(
                    preference_key,
                    &class_descriptor,
                    dialog_count,
                )?;
                self.show_edit_text_preference_dialog_if_needed(
                    preference_key,
                    &class_descriptor,
                    dialog_count,
                )?;

                Ok(1)
            }
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "Preference click method did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "Preference click method did not complete normally",
                ))
            }
        }
    }

    fn preference_value(&self, preference_key: &str) -> Option<DexValue> {
        let runtime = self.runtime.borrow();
        let min_object_id = runtime
            .activity_stack
            .last()
            .map(|activity| activity.object_start)
            .unwrap_or(0);
        let group_children = preference_group_children(&runtime, min_object_id);

        for children in group_children.values() {
            for child in children {
                if map_identity_key(child) == preference_key {
                    return Some(child.clone());
                }
            }
        }

        None
    }

    fn show_list_preference_dialog_if_needed(
        &self,
        preference_key: &str,
        class_descriptor: &str,
        dialog_count_before_click: usize,
    ) -> Result<()> {
        if !self.descriptor_matches_type(class_descriptor, "Landroidx/preference/ListPreference;") {
            return Ok(());
        }
        if self.runtime.borrow().active_dialog_overlays.len() != dialog_count_before_click {
            return Ok(());
        }

        let (title, entries, entry_values, current_value) = {
            let runtime = self.runtime.borrow();
            let title = preference_field_text(
                &runtime,
                preference_key,
                &[
                    "Landroidx/preference/Preference;.mTitle:Ljava/lang/CharSequence;",
                    "Landroidx/preference/Preference;.g:Ljava/lang/CharSequence;",
                ],
            );
            let entries = preference_string_array_field(
                &runtime,
                preference_key,
                &[
                    "Landroidx/preference/ListPreference;.mEntries:[Ljava/lang/CharSequence;",
                    "Landroidx/preference/ListPreference;.S:[Ljava/lang/CharSequence;",
                ],
            );
            let entry_values = preference_string_array_field(
                &runtime,
                preference_key,
                &[
                    "Landroidx/preference/ListPreference;.mEntryValues:[Ljava/lang/CharSequence;",
                    "Landroidx/preference/ListPreference;.T:[Ljava/lang/CharSequence;",
                ],
            );
            let current_value = preference_field_string(
                &runtime,
                preference_key,
                &[
                    "Landroidx/preference/ListPreference;.mValue:Ljava/lang/String;",
                    "Landroidx/preference/ListPreference;.U:Ljava/lang/String;",
                ],
            );

            (title, entries, entry_values, current_value)
        };

        if entries.is_empty() {
            return Err(WinedroidError::new(format!(
                "ListPreference click reached no entries\npreference key: {preference_key}\nclass: {class_descriptor}\nnext missing primitive: Android ListPreference entries"
            )));
        }
        if entries.len() != entry_values.len() {
            return Err(WinedroidError::new(format!(
                "ListPreference entry and value arrays have different lengths\npreference key: {preference_key}\nentries: {}\nentry values: {}\nnext missing primitive: Android ListPreference entryValues",
                entries.len(),
                entry_values.len()
            )));
        }

        let items = entries
            .into_iter()
            .zip(entry_values)
            .map(|(text, value)| RuntimeDialogItem {
                selected: current_value.as_deref() == Some(value.as_str()),
                action: RuntimeDialogItemAction::ListPreferenceValue {
                    preference_key: preference_key.to_owned(),
                    value,
                },
                text,
                enabled: true,
            })
            .collect();

        let mut runtime = self.runtime.borrow_mut();
        let dialog_key = format!("androidx-preference-list-dialog:{preference_key}");
        runtime
            .active_dialog_overlays
            .retain(|dialog| dialog.dialog_key != dialog_key);
        runtime.active_dialog_overlays.push(RuntimeDialogOverlay {
            dialog_key,
            title,
            content_layouts: Vec::new(),
            items,
            buttons: Vec::new(),
            input_value: None,
        });

        Ok(())
    }

    fn show_edit_text_preference_dialog_if_needed(
        &self,
        preference_key: &str,
        class_descriptor: &str,
        dialog_count_before_click: usize,
    ) -> Result<()> {
        if !self
            .descriptor_matches_type(class_descriptor, "Landroidx/preference/EditTextPreference;")
        {
            return Ok(());
        }
        if self.runtime.borrow().active_dialog_overlays.len() != dialog_count_before_click {
            return Ok(());
        }

        let (title, current_value) = {
            let runtime = self.runtime.borrow();
            let title = preference_field_text(
                &runtime,
                preference_key,
                &[
                    "Landroidx/preference/Preference;.mTitle:Ljava/lang/CharSequence;",
                    "Landroidx/preference/Preference;.g:Ljava/lang/CharSequence;",
                ],
            );
            let current_value = preference_field_string(
                &runtime,
                preference_key,
                &["Landroidx/preference/EditTextPreference;.mText:Ljava/lang/String;"],
            );

            (title, current_value)
        };

        let mut runtime = self.runtime.borrow_mut();
        let dialog_key = format!("androidx-preference-edit-dialog:{preference_key}");
        runtime
            .active_dialog_overlays
            .retain(|dialog| dialog.dialog_key != dialog_key);
        runtime.active_dialog_overlays.push(RuntimeDialogOverlay {
            dialog_key,
            title,
            content_layouts: Vec::new(),
            items: Vec::new(),
            buttons: vec![
                RuntimeDialogButton {
                    text: "Cancel".to_owned(),
                    enabled: true,
                    action: RuntimeDialogButtonAction::Dismiss,
                },
                RuntimeDialogButton {
                    text: "OK".to_owned(),
                    enabled: true,
                    action: RuntimeDialogButtonAction::CommitEditTextPreference {
                        preference_key: preference_key.to_owned(),
                    },
                },
            ],
            input_value: Some(current_value.unwrap_or_default()),
        });

        Ok(())
    }

    pub(in crate::dex_run) fn edit_active_dialog_input(
        &self,
        characters: &str,
        key: crate::input::InputKey,
    ) -> i32 {
        let mut runtime = self.runtime.borrow_mut();
        let Some(dialog) = runtime
            .active_dialog_overlays
            .iter_mut()
            .rev()
            .find(|dialog| dialog.input_value.is_some())
        else {
            return 0;
        };
        let Some(value) = dialog.input_value.as_mut() else {
            return 0;
        };

        if key == crate::input::InputKey::DeleteBackward {
            value.pop();
            return 1;
        }

        let printable = characters
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>();
        if printable.is_empty() {
            return 0;
        }
        value.push_str(&printable);

        1
    }

    fn set_edit_text_preference_value(
        &self,
        preference_key: &str,
        value: &str,
        depth: usize,
    ) -> Result<()> {
        let preference = self.preference_value(preference_key).ok_or_else(|| {
            WinedroidError::new(format!(
                "EditTextPreference commit target was not found\npreference key: {preference_key}"
            ))
        })?;
        let Some(class_descriptor) = value_class_descriptor(&preference) else {
            return Err(WinedroidError::new(format!(
                "EditTextPreference commit target has no class descriptor\npreference key: {preference_key}"
            )));
        };

        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: "setText".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "VL".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec!["Ljava/lang/String;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Err(WinedroidError::new(format!(
                "EditTextPreference setText was not found\nclass: {class_descriptor}\nnext missing primitive: AndroidX EditTextPreference text commit"
            )));
        };
        let result = self.execute_method(
            None,
            &resolved,
            Some(&[
                preference,
                DexValue::String(std::rc::Rc::from(value.to_owned())),
            ]),
            depth + 1,
        )?;

        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "EditTextPreference setText did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "EditTextPreference setText did not complete normally",
                ))
            }
        }
    }

    fn set_list_preference_value(
        &self,
        preference_key: &str,
        value: &str,
        depth: usize,
    ) -> Result<()> {
        let preference = self.preference_value(preference_key).ok_or_else(|| {
            WinedroidError::new(format!(
                "ListPreference selection target was not found\npreference key: {preference_key}"
            ))
        })?;
        let Some(class_descriptor) = value_class_descriptor(&preference) else {
            return Err(WinedroidError::new(format!(
                "ListPreference selection target has no class descriptor\npreference key: {preference_key}"
            )));
        };
        if !self.descriptor_matches_type(class_descriptor, "Landroidx/preference/ListPreference;") {
            return Err(WinedroidError::new(format!(
                "dialog item target is not a ListPreference\npreference key: {preference_key}\nclass: {class_descriptor}"
            )));
        }

        let mut resolved = None;
        for name in ["setValue", "z"] {
            let method = DexMethodId {
                class_descriptor: class_descriptor.to_owned(),
                name: name.to_owned(),
                proto: crate::dex::DexProto {
                    shorty: "VL".to_owned(),
                    return_type: "V".to_owned(),
                    parameters: vec!["Ljava/lang/String;".to_owned()],
                },
            };
            resolved = self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?;
            if resolved.is_some() {
                break;
            }
        }
        let Some(resolved) = resolved else {
            return Err(WinedroidError::new(format!(
                "ListPreference value setter was not found\npreference key: {preference_key}\nclass: {class_descriptor}\nnext missing primitive: AndroidX ListPreference value commit"
            )));
        };
        let result = self.execute_method(
            None,
            &resolved,
            Some(&[
                preference,
                DexValue::String(std::rc::Rc::from(value.to_owned())),
            ]),
            depth + 1,
        )?;

        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(
                "ListPreference value setter did not complete normally",
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "ListPreference value setter did not complete normally",
                ))
            }
        }
    }

    pub(in crate::dex_run) fn active_activity_start(&self) -> Option<RuntimeActivityStart> {
        self.runtime
            .borrow()
            .activity_stack
            .last()
            .map(|activity| RuntimeActivityStart {
                class_descriptor: activity.class_descriptor.clone(),
                method_signature: activity.method_signature.clone(),
                result: activity.result.clone(),
            })
    }

    pub(in crate::dex_run) fn start_activity_on_create(
        &self,
        class_descriptor: &str,
        depth: usize,
    ) -> Result<RuntimeActivityStart> {
        self.runtime
            .borrow_mut()
            .post_main_thread_task(MainThreadTask::LaunchActivity {
                class_descriptor: class_descriptor.to_owned(),
            });
        let display_size = self.display_size();
        self.dispatch_pending_main_loop(display_size.width(), display_size.height(), depth)?;

        self.active_activity_start().ok_or_else(|| {
            WinedroidError::new(format!(
                "Activity launch did not produce an active Activity start record\nactivity: {class_descriptor}\nnext missing primitive: ActivityThread launch dispatch"
            ))
        })
    }

    pub(in crate::dex_run) fn dispatch_activity_launch(
        &self,
        class_descriptor: &str,
        depth: usize,
    ) -> Result<RuntimeActivityStart> {
        {
            let mut runtime = self.runtime.borrow_mut();
            let chrome = crate::dex_run::runtime::SavedActivityChrome {
                action_bar_title: runtime.action_bar_title.take(),
                action_bar_nav_icon: runtime.action_bar_nav_icon.take(),
                action_bar_nav_click: runtime.action_bar_nav_click.take(),
                action_bar_present: std::mem::take(&mut runtime.action_bar_present),
                action_bar_home_as_up: std::mem::take(&mut runtime.action_bar_home_as_up),
                options_menu: std::mem::take(&mut runtime.options_menu),
                inflated_preferences: std::mem::take(&mut runtime.inflated_preferences),
            };
            if let Some(active) = runtime.activity_stack.last_mut() {
                active.saved_chrome = Some(chrome);
            }
            runtime.action_bar_overflow_open = false;
            runtime.pending_fragment = None;
        }
        {
            let _span = trace::span("runtime", "ensure Application initialized");
            self.ensure_application_initialized(depth)?;
        }

        let resolved = {
            let _span = trace::span("runtime", "resolve Activity.onCreate");
            self.resolve_named_method(class_descriptor, "onCreate")?
        };
        let initial_arguments = {
            let _span = trace::span("runtime", "prepare Activity.onCreate arguments");
            self.prepare_entrypoint_initial_arguments(&resolved)?
        };
        let activity_receiver = activity_receiver_argument(&initial_arguments)?;
        let method_signature = resolved.method.display_signature();

        self.push_activity(
            resolved.class_descriptor.clone(),
            activity_receiver.clone(),
            method_signature.clone(),
        );
        self.apply_manifest_activity_theme(&resolved.class_descriptor, &activity_receiver);
        let result = {
            let _span = trace::span("runtime", "execute Activity.onCreate method");
            self.execute_method(None, &resolved, initial_arguments.as_deref(), depth)?
        };

        match result {
            DexExecutionResult::Void => {}
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                return Err(WinedroidError::new(format!(
                    "Activity onCreate parked without a guest-thread scheduler\nclass: {}\nmethod: {}",
                    resolved.class_descriptor, method_signature
                )));
            }
            result => {
                return Err(WinedroidError::new(format!(
                    "Activity onCreate returned a value\nclass: {}\nmethod: {}\nresult: {}",
                    resolved.class_descriptor, method_signature, result
                )));
            }
        }

        {
            let _span = trace::span("runtime", "dispatch onActivityCreated callbacks");
            self.dispatch_activity_lifecycle_callbacks(
                &activity_receiver,
                "onActivityCreated",
                Some(DexValue::Null),
                depth + 1,
            )?;
        }
        {
            let _span = trace::span("runtime", "dispatch onActivityPostCreated callbacks");
            self.dispatch_activity_lifecycle_callbacks(
                &activity_receiver,
                "onActivityPostCreated",
                Some(DexValue::Null),
                depth + 1,
            )?;
        }
        {
            let _span = trace::span("runtime", "dispatch Activity foreground lifecycle");
            self.dispatch_activity_foreground_lifecycle(
                &resolved.class_descriptor,
                &activity_receiver,
                depth + 1,
            )?;
        }

        self.auto_focus_default_view()?;

        self.capture_options_menu(&resolved.class_descriptor, &activity_receiver, depth + 1)?;

        let started = RuntimeActivityStart {
            class_descriptor: resolved.class_descriptor,
            method_signature,
            result: result.to_string(),
        };
        self.finish_active_activity(started.result.clone());

        Ok(started)
    }

    fn dispatch_activity_foreground_lifecycle(
        &self,
        class_descriptor: &str,
        activity_receiver: &DexValue,
        depth: usize,
    ) -> Result<()> {
        self.dispatch_optional_activity_lifecycle_callback(
            class_descriptor,
            activity_receiver,
            "onStart",
            depth + 1,
        )?;
        self.dispatch_activity_lifecycle_callbacks(
            activity_receiver,
            "onActivityStarted",
            None,
            depth + 1,
        )?;
        self.dispatch_activity_lifecycle_callbacks(
            activity_receiver,
            "onActivityPostStarted",
            None,
            depth + 1,
        )?;
        self.dispatch_optional_activity_lifecycle_callback(
            class_descriptor,
            activity_receiver,
            "onResume",
            depth + 1,
        )?;
        self.dispatch_activity_lifecycle_callbacks(
            activity_receiver,
            "onActivityResumed",
            None,
            depth + 1,
        )?;
        self.dispatch_activity_lifecycle_callbacks(
            activity_receiver,
            "onActivityPostResumed",
            None,
            depth + 1,
        )
    }

    fn dispatch_optional_activity_lifecycle_callback(
        &self,
        class_descriptor: &str,
        activity_receiver: &DexValue,
        method_name: &str,
        depth: usize,
    ) -> Result<()> {
        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto: crate::dex::DexProto {
                shorty: "V".to_owned(),
                return_type: "V".to_owned(),
                parameters: Vec::new(),
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Ok(());
        };
        let result =
            self.execute_method(None, &resolved, Some(&[activity_receiver.clone()]), depth)?;

        match result {
            DexExecutionResult::Void => return Ok(()),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                return Err(WinedroidError::new(format!(
                    "Activity lifecycle callback parked without a guest-thread scheduler\nclass: {}\nmethod: {}",
                    resolved.class_descriptor,
                    resolved.method.display_signature()
                )));
            }
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => {}
        }

        Err(WinedroidError::new(format!(
            "Activity lifecycle callback returned a value\nclass: {}\nmethod: {}\nresult: {}",
            resolved.class_descriptor,
            resolved.method.display_signature(),
            result
        )))
    }

    pub(in crate::dex_run) fn register_activity_lifecycle_callback(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }
        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        if !dex_value_is_object_like(&invocation.arguments[1]) {
            return None;
        }

        let activity_key = map_identity_key(&invocation.arguments[0]);
        let callback = invocation.arguments[1].clone();
        let mut runtime = self.runtime.borrow_mut();
        let callbacks = runtime
            .activity_lifecycle_callbacks
            .entry(activity_key)
            .or_default();
        if !callbacks.contains(&callback) {
            callbacks.push(callback);
        }

        Some(())
    }

    fn dispatch_activity_lifecycle_callbacks(
        &self,
        activity_receiver: &DexValue,
        method_name: &str,
        bundle: Option<DexValue>,
        depth: usize,
    ) -> Result<()> {
        let activity_key = map_identity_key(activity_receiver);
        let callbacks = self
            .runtime
            .borrow()
            .activity_lifecycle_callbacks
            .get(&activity_key)
            .cloned()
            .unwrap_or_default();

        for callback in callbacks {
            self.dispatch_single_activity_lifecycle_callback(
                callback,
                activity_receiver.clone(),
                method_name,
                bundle.clone(),
                depth + 1,
            )?;
        }

        Ok(())
    }

    fn dispatch_single_activity_lifecycle_callback(
        &self,
        callback: DexValue,
        activity_receiver: DexValue,
        method_name: &str,
        bundle: Option<DexValue>,
        depth: usize,
    ) -> Result<()> {
        let Some(callback_descriptor) = value_class_descriptor(&callback) else {
            return Err(WinedroidError::new(format!(
                "Activity lifecycle callback dispatch requires an object-backed callback\ncallback: {callback}"
            )));
        };
        let mut parameters = vec!["Landroid/app/Activity;".to_owned()];
        let mut arguments = vec![callback.clone(), activity_receiver];

        if let Some(bundle) = bundle {
            parameters.push("Landroid/os/Bundle;".to_owned());
            arguments.push(bundle);
        }

        let method = DexMethodId {
            class_descriptor: callback_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto: crate::dex::DexProto {
                shorty: format!("V{}", "L".repeat(parameters.len())),
                return_type: "V".to_owned(),
                parameters,
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(callback_descriptor, &method)?
        else {
            return Ok(());
        };
        let result = self.execute_method(None, &resolved, Some(&arguments), depth)?;

        match result {
            DexExecutionResult::Void => return Ok(()),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                return Err(WinedroidError::new(format!(
                    "Activity lifecycle callback parked without a guest-thread scheduler\nclass: {}\nmethod: {}",
                    resolved.class_descriptor,
                    resolved.method.display_signature()
                )));
            }
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => {}
        }

        Err(WinedroidError::new(format!(
            "Activity lifecycle callback returned a value\nclass: {}\nmethod: {}\nresult: {}",
            resolved.class_descriptor,
            resolved.method.display_signature(),
            result
        )))
    }

    pub(in crate::dex_run) fn push_activity(
        &self,
        class_descriptor: String,
        receiver: DexValue,
        method_signature: String,
    ) {
        // cite: frameworks-base Activity.java#attach
        self.reset_window_decor_tree();

        let mut runtime = self.runtime.borrow_mut();
        let object_start = runtime.next_object_id;
        let content_layout_start = runtime.content_layouts.len();
        let inflated_layout_start = runtime.inflated_layouts.len();
        let dialog_content_layout_start = runtime.dialog_content_layouts.len();
        let runtime_view_root_start = runtime.runtime_view_roots.len();
        let view_lookup_start = runtime.view_lookup_ids.len();
        let adapter_rows_snapshot = runtime.adapter_rows.clone();
        let adapter_item_clicks_snapshot = runtime.adapter_item_clicks.clone();

        runtime.activity_receiver = Some(receiver.clone());
        runtime.active_menu_overlays.clear();
        runtime.active_dialog_overlays.clear();
        runtime.activity_stack.push(RuntimeActivity {
            class_descriptor,
            receiver,
            method_signature,
            result: "pending".to_owned(),
            object_start,
            content_layout_start,
            inflated_layout_start,
            initial_inflated_layout_end: inflated_layout_start,
            dialog_content_layout_start,
            runtime_view_root_start,
            view_lookup_start,
            saved_chrome: None,
            result_target: None,
            result_code: 0,
            result_intent: None,
            adapter_rows_snapshot,
            adapter_item_clicks_snapshot,
        });
    }

    pub(in crate::dex_run) fn start_activity_for_result(
        &self,
        invocation: &DexInvocation,
        request_code: i32,
        depth: usize,
    ) -> Result<()> {
        let launcher = self.active_activity_receiver();
        self.start_activity_from_intent(invocation, depth)?;
        if let Some(launcher) = launcher {
            if let Some(active) = self.runtime.borrow_mut().activity_stack.last_mut() {
                active.result_target = Some((launcher, request_code));
            }
        }
        Ok(())
    }

    pub(in crate::dex_run) fn set_activity_result(
        &self,
        result_code: i32,
        result_intent: Option<DexValue>,
    ) {
        if let Some(active) = self.runtime.borrow_mut().activity_stack.last_mut() {
            active.result_code = result_code;
            if let Some(intent) = result_intent {
                active.result_intent = Some(intent);
            }
        }
    }

    pub(in crate::dex_run) fn deliver_activity_result(
        &self,
        target: (DexValue, i32),
        result_code: i32,
        result_intent: Option<DexValue>,
        depth: usize,
    ) -> Result<()> {
        let (launcher, request_code) = target;
        let Some(class_descriptor) = value_class_descriptor(&launcher).map(str::to_owned) else {
            return Ok(());
        };
        let method = DexMethodId {
            class_descriptor: class_descriptor.clone(),
            name: "onActivityResult".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "VIIL".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec![
                    "I".to_owned(),
                    "I".to_owned(),
                    "Landroid/content/Intent;".to_owned(),
                ],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(&class_descriptor, &method)?
        else {
            return Ok(());
        };
        let data = result_intent.unwrap_or(DexValue::Null);
        let result = self.execute_method(
            None,
            &resolved,
            Some(&[
                launcher,
                DexValue::Int(request_code),
                DexValue::Int(result_code),
                data,
            ]),
            depth + 1,
        )?;
        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "Activity.onActivityResult parked without a guest-thread scheduler",
                ))
            }
            result => Err(WinedroidError::new(format!(
                "Activity.onActivityResult did not complete normally\nresult: {result}"
            ))),
        }
    }

    pub(in crate::dex_run) fn finish_active_activity(&self, result: String) {
        let mut runtime = self.runtime.borrow_mut();
        let initial_inflated_layout_end = runtime.inflated_layouts.len();

        if let Some(activity) = runtime.activity_stack.last_mut() {
            activity.result = result;
            activity.initial_inflated_layout_end = initial_inflated_layout_end;
        }
    }

    pub(in crate::dex_run) fn finish_activity(
        &self,
        invocation: &DexInvocation,
        depth: usize,
    ) -> Result<()> {
        if invocation.arguments.len() != 1 {
            return Err(WinedroidError::new(format!(
                "Activity.finish expected one receiver argument, got {}",
                invocation.arguments.len()
            )));
        }

        let receiver = &invocation.arguments[0];
        if !dex_value_is_object_like(receiver) {
            return Err(WinedroidError::new(format!(
                "Activity.finish requires an object-backed Activity receiver\nreceiver: {receiver}"
            )));
        }

        let receiver_key = map_identity_key(receiver);
        let mut runtime = self.runtime.borrow_mut();
        let Some(active_activity) = runtime.activity_stack.last() else {
            return Err(WinedroidError::new(
                "Activity.finish has no active Activity on the runtime stack",
            ));
        };
        if map_identity_key(&active_activity.receiver) != receiver_key {
            return Err(WinedroidError::new(format!(
                "Activity.finish only supports finishing the active Activity\nreceiver: {receiver}"
            )));
        }

        let finished = runtime
            .activity_stack
            .pop()
            .expect("active Activity was checked before pop");
        let pending_result = finished
            .result_target
            .clone()
            .map(|target| (target, finished.result_code, finished.result_intent.clone()));
        runtime
            .activity_lifecycle_callbacks
            .remove(&map_identity_key(&finished.receiver));
        runtime
            .content_layouts
            .truncate(finished.content_layout_start);
        runtime
            .inflated_layouts
            .truncate(finished.inflated_layout_start);
        runtime
            .dialog_content_layouts
            .truncate(finished.dialog_content_layout_start);
        runtime.view_lookup_ids.truncate(finished.view_lookup_start);
        runtime.adapter_rows = finished.adapter_rows_snapshot;
        runtime.adapter_item_clicks = finished.adapter_item_clicks_snapshot;
        runtime.active_menu_overlays.clear();
        runtime.active_dialog_overlays.clear();
        runtime.action_bar_overflow_open = false;
        runtime.inflated_preferences.clear();
        runtime.pending_fragment = None;

        let resumed_chrome = runtime
            .activity_stack
            .last_mut()
            .and_then(|activity| activity.saved_chrome.take());
        if let Some(chrome) = resumed_chrome {
            runtime.action_bar_title = chrome.action_bar_title;
            runtime.action_bar_nav_icon = chrome.action_bar_nav_icon;
            runtime.action_bar_nav_click = chrome.action_bar_nav_click;
            runtime.action_bar_present = chrome.action_bar_present;
            runtime.action_bar_home_as_up = chrome.action_bar_home_as_up;
            runtime.options_menu = chrome.options_menu;
            runtime.inflated_preferences = chrome.inflated_preferences;
        } else {
            runtime.action_bar_title = None;
            runtime.action_bar_nav_icon = None;
            runtime.action_bar_nav_click = None;
            runtime.action_bar_present = false;
            runtime.action_bar_home_as_up = false;
            runtime.options_menu.clear();
        }

        runtime.activity_receiver = runtime
            .activity_stack
            .last()
            .map(|activity| activity.receiver.clone());

        drop(runtime);

        if let Some((target, result_code, result_intent)) = pending_result {
            self.deliver_activity_result(target, result_code, result_intent, depth)?;
        }

        Ok(())
    }

    // cite: androidx AppCompatDelegateImpl.java#setSupportActionBar
    // cite: androidx ToolbarWidgetWrapper.java#setWindowTitle
    // cite: frameworks-base ActivityThread.java#performLaunchActivity
    // cite: frameworks-base PackageItemInfo.java#loadLabel
    pub(in crate::dex_run) fn install_support_toolbar_action_bar(&self) -> Result<()> {
        let title = {
            let runtime = self.runtime.borrow();
            if runtime.action_bar_title.is_some() {
                None
            } else {
                runtime
                    .activity_stack
                    .last()
                    .map(|activity| activity.class_descriptor.clone())
            }
        };

        if let Some(class_descriptor) = title {
            let label_resource_id = self
                .activity_labels
                .get(&class_descriptor)
                .copied()
                .or(self.application_label);
            if let Some(label_resource_id) = label_resource_id {
                let label = self
                    .resolve_string_resource_text(label_resource_id)?
                    .ok_or_else(|| {
                        WinedroidError::new(format!(
                            "toolbar title string resource 0x{label_resource_id:08x} was not found"
                        ))
                    })?;
                self.runtime.borrow_mut().action_bar_title = Some(label);
            }
        }

        self.runtime.borrow_mut().action_bar_present = true;
        Ok(())
    }

    // cite: frameworks-base Activity.java#invalidateOptionsMenu
    // cite: frameworks-base PhoneWindow.java#invalidatePanelMenu
    pub(in crate::dex_run) fn post_invalidate_options_menu(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 1 || !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        let activity = invocation.arguments[0].clone();
        let activity_key = map_identity_key(&activity);
        let mut runtime = self.runtime.borrow_mut();
        let already_posted = runtime.main_thread_tasks.iter().any(|task| {
            matches!(
                task,
                MainThreadTask::InvalidateOptionsMenu { activity: pending }
                    if map_identity_key(pending) == activity_key
            )
        });
        if !already_posted {
            runtime.post_main_thread_task(MainThreadTask::InvalidateOptionsMenu { activity });
        }

        Some(())
    }

    pub(in crate::dex_run) fn capture_options_menu(
        &self,
        class_descriptor: &str,
        activity_receiver: &DexValue,
        depth: usize,
    ) -> Result<()> {
        self.runtime.borrow_mut().options_menu.clear();

        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: "onCreateOptionsMenu".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "ZL".to_owned(),
                return_type: "Z".to_owned(),
                parameters: vec!["Landroid/view/Menu;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Ok(());
        };

        let menu = self.allocate_object("Landroid/view/Menu;".to_owned());
        let result = self.execute_method(
            None,
            &resolved,
            Some(&[activity_receiver.clone(), menu.clone()]),
            depth + 1,
        )?;
        match result {
            DexExecutionResult::Void | DexExecutionResult::Int(_) => {}
            DexExecutionResult::Value(_) | DexExecutionResult::Throw(_) => {
                return Err(WinedroidError::new(
                    "Activity.onCreateOptionsMenu did not complete normally",
                ));
            }
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                return Err(WinedroidError::new(
                    "Activity.onCreateOptionsMenu did not complete normally",
                ));
            }
        }

        self.invoke_prepare_options_menu(class_descriptor, activity_receiver, &menu, depth)?;

        let menu_key = map_identity_key(&menu);
        let items = self
            .runtime
            .borrow()
            .menus
            .get(&menu_key)
            .map(|menu| menu.items.clone())
            .unwrap_or_default();
        self.runtime.borrow_mut().options_menu = items;

        Ok(())
    }

    fn invoke_prepare_options_menu(
        &self,
        class_descriptor: &str,
        activity_receiver: &DexValue,
        menu: &DexValue,
        depth: usize,
    ) -> Result<()> {
        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: "onPrepareOptionsMenu".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "ZL".to_owned(),
                return_type: "Z".to_owned(),
                parameters: vec!["Landroid/view/Menu;".to_owned()],
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Ok(());
        };

        let result = self.execute_method(
            None,
            &resolved,
            Some(&[activity_receiver.clone(), menu.clone()]),
            depth + 1,
        )?;
        match result {
            DexExecutionResult::Void | DexExecutionResult::Int(_) => Ok(()),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(
                    "Activity.onPrepareOptionsMenu parked without a guest-thread scheduler",
                ))
            }
            result => Err(WinedroidError::new(format!(
                "Activity.onPrepareOptionsMenu did not complete normally\nresult: {result}"
            ))),
        }
    }

    pub(in crate::dex_run) fn resolve_activity_title(
        &self,
        class_descriptor: &str,
    ) -> Result<Option<String>> {
        let Some(apk_path) = self.apk_path.as_ref() else {
            return Ok(None);
        };
        let mut apk = ApkArchive::open(apk_path)?;
        let manifest_bytes = apk.read_required_file("AndroidManifest.xml")?;
        let manifest = crate::manifest::parse_manifest(&manifest_bytes)?;

        let dotted_name = class_descriptor
            .strip_prefix('L')
            .and_then(|name| name.strip_suffix(';'))
            .unwrap_or(class_descriptor)
            .replace('/', ".");
        let Some(label) = manifest
            .activities
            .iter()
            .find(|activity| activity.name == dotted_name)
            .and_then(|activity| activity.label)
            .or(manifest.application_label)
        else {
            return Ok(None);
        };

        let title = self.resolve_string_resource_text(label)?.ok_or_else(|| {
            WinedroidError::new(format!(
                "activity label string resource 0x{label:08x} was not found"
            ))
        })?;
        Ok(Some(title))
    }

    pub(in crate::dex_run) fn active_activity_receiver(&self) -> Option<DexValue> {
        let runtime = self.runtime.borrow();

        runtime
            .activity_stack
            .last()
            .map(|activity| activity.receiver.clone())
            .or_else(|| runtime.activity_receiver.clone())
    }

    pub(in crate::dex_run) fn dialog_init(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 3 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        if !dex_value_is_object_like(&invocation.arguments[1]) {
            return None;
        }
        let DexValue::Int(theme_resource_id) = invocation.arguments[2] else {
            return None;
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &dialog_context_field(),
            invocation.arguments[1].clone(),
        );
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &dialog_theme_resource_field(),
            DexValue::Int(theme_resource_id),
        );
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &dialog_window_field(),
            dialog_window_reference(&invocation.arguments[0]),
        );

        Some(())
    }

    pub(in crate::dex_run) fn dialog_context(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        Some(
            self.read_stored_instance_field(&invocation.arguments[0], &dialog_context_field())
                .unwrap_or(DexValue::Null),
        )
    }

    // cite: frameworks-base Dialog.java#setContentView
    pub(in crate::dex_run) fn dialog_content_view(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        let dialog = invocation.arguments.first()?;
        if !dex_value_is_object_like(dialog) {
            return None;
        }

        self.read_stored_instance_field(dialog, &dialog_content_view_field())
    }

    // cite: frameworks-base Dialog.java#findViewById
    pub(in crate::dex_run) fn dialog_stored_content_view(
        &self,
        receiver: &DexValue,
    ) -> Option<DexValue> {
        if !dex_value_is_object_like(receiver) {
            return None;
        }

        match self.read_stored_instance_field(receiver, &dialog_content_view_field())? {
            value @ (DexValue::Object(_) | DexValue::Reference(_)) => Some(value),
            _ => None,
        }
    }

    // cite: frameworks-base WindowManagerGlobal.java#addView
    pub(in crate::dex_run) fn register_runtime_view_root(&self, view: &DexValue) {
        let Some(class_descriptor) = value_class_descriptor(view) else {
            return;
        };
        let view_key = map_identity_key(view);

        let mut runtime = self.runtime.borrow_mut();
        if !runtime
            .runtime_view_roots
            .iter()
            .any(|root| root.view_key == view_key)
        {
            runtime.runtime_view_roots.push(DexRuntimeViewRoot {
                view_key,
                class_descriptor: class_descriptor.to_owned(),
            });
        }
        runtime.request_layout_traversal();
    }

    pub(in crate::dex_run) fn write_dialog_content_view(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() < 2 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_object_like(&invocation.arguments[1])
        {
            return None;
        }

        if !self.write_stored_instance_field(
            &invocation.arguments[0],
            &dialog_content_view_field(),
            invocation.arguments[1].clone(),
        ) {
            return None;
        }

        // cite: frameworks-base Window.java#setContentView
        self.runtime.borrow_mut().view_parents.insert(
            map_identity_key(&invocation.arguments[1]),
            decor_view_reference(),
        );

        Some(())
    }

    // cite: frameworks-base Dialog.java#setContentView
    // cite: frameworks-base PhoneWindow.java#setContentView
    pub(in crate::dex_run) fn dialog_set_content_view_by_resource(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }
        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        let DexValue::Int(layout_id) = invocation.arguments[1] else {
            return None;
        };

        let root = self.layout_reference_for_id(layout_id);
        let _ = self.register_inflated_layout_tree(layout_id, &root);

        let content_invocation = DexInvocation {
            arguments: smallvec::smallvec![invocation.arguments[0].clone(), root],
            ..invocation.clone()
        };
        self.write_dialog_content_view(&content_invocation)
    }

    pub(in crate::dex_run) fn dialog_window(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        if let Some(value) =
            self.read_stored_instance_field(&invocation.arguments[0], &dialog_window_field())
        {
            return Some(value);
        }

        let value = dialog_window_reference(&invocation.arguments[0]);
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &dialog_window_field(),
            value.clone(),
        );

        Some(value)
    }

    pub(in crate::dex_run) fn write_dialog_bool_field(
        &self,
        invocation: &DexInvocation,
        field: &DexFieldId,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        let DexValue::Int(value) = invocation.arguments[1] else {
            return None;
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            field,
            DexValue::Int(if value == 0 { 0 } else { 1 }),
        )
        .then_some(())
    }

    pub(in crate::dex_run) fn write_dialog_cancelable(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        self.write_dialog_bool_field(invocation, &dialog_cancelable_field())
    }

    pub(in crate::dex_run) fn write_dialog_canceled_on_touch_outside(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        self.write_dialog_bool_field(invocation, &dialog_canceled_on_touch_outside_field())
    }

    pub(in crate::dex_run) fn write_dialog_nullable_object_field(
        &self,
        invocation: &DexInvocation,
        field: &DexFieldId,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        if !dex_value_is_nullable_object_like(&invocation.arguments[1]) {
            return None;
        }

        self.write_stored_instance_field(
            &invocation.arguments[0],
            field,
            invocation.arguments[1].clone(),
        )
        .then_some(())
    }

    pub(in crate::dex_run) fn write_dialog_on_cancel_listener(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        self.write_dialog_nullable_object_field(invocation, &dialog_on_cancel_listener_field())
    }

    pub(in crate::dex_run) fn write_dialog_on_dismiss_listener(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        self.write_dialog_nullable_object_field(invocation, &dialog_on_dismiss_listener_field())
    }

    pub(in crate::dex_run) fn dialog_show(&self, invocation: &DexInvocation) -> Option<()> {
        self.dialog_show_with_overlay(invocation, true)
    }

    // cite: frameworks-base Dialog.java#show
    pub(in crate::dex_run) fn dialog_show_with_overlay(
        &self,
        invocation: &DexInvocation,
        push_overlay: bool,
    ) -> Option<()> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        let dialog = invocation.arguments[0].clone();
        let overlay = push_overlay.then(|| {
            let runtime = self.runtime.borrow();
            runtime_dialog_overlay_from_graph(&runtime, &dialog)
        });

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &dialog_showing_field(),
            DexValue::Int(1),
        )
        .then_some(())?;

        let Some(overlay) = overlay else {
            return Some(());
        };

        let mut runtime = self.runtime.borrow_mut();
        runtime
            .active_dialog_overlays
            .retain(|active| active.dialog_key != overlay.dialog_key);
        let dialog_content_layout_start = runtime
            .activity_stack
            .last()
            .map(|activity| activity.dialog_content_layout_start)
            .unwrap_or(0);
        for layout_id in &overlay.content_layouts {
            if !runtime.dialog_content_layouts[dialog_content_layout_start..].contains(layout_id) {
                runtime.dialog_content_layouts.push(*layout_id);
            }
        }
        runtime.active_dialog_overlays.push(overlay);

        Some(())
    }

    // cite: frameworks-base Dialog.java#dismissDialog
    pub(in crate::dex_run) fn dialog_dismiss(
        &self,
        invocation: &DexInvocation,
        content_view: Option<&DexValue>,
    ) -> Option<()> {
        if invocation.arguments.is_empty() {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &dialog_showing_field(),
            DexValue::Int(0),
        );

        let dialog_key = map_identity_key(&invocation.arguments[0]);
        let mut runtime = self.runtime.borrow_mut();
        runtime
            .active_dialog_overlays
            .retain(|active| active.dialog_key != dialog_key);

        if let Some(content_view) = content_view {
            let view_key = map_identity_key(content_view);
            runtime
                .runtime_view_roots
                .retain(|root| root.view_key != view_key);
            runtime
                .runtime_canvas_frames
                .retain(|frame| frame.root_view_key != view_key);
            runtime.view_parents.remove(&view_key);
        }
        runtime.request_layout_traversal();

        Some(())
    }

    pub(in crate::dex_run) fn dialog_on_dismiss_listener(
        &self,
        dialog: &DexValue,
    ) -> Option<DexValue> {
        self.read_stored_instance_field(dialog, &dialog_on_dismiss_listener_field())
    }

    pub(in crate::dex_run) fn dialog_is_showing(&self, invocation: &DexInvocation) -> Option<i32> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        match self.read_stored_instance_field(&invocation.arguments[0], &dialog_showing_field()) {
            Some(DexValue::Int(value)) => Some(if value == 0 { 0 } else { 1 }),
            Some(_) => None,
            None => Some(0),
        }
    }

    pub(in crate::dex_run) fn click_dialog_button(
        &self,
        dialog_index: usize,
        button_index: usize,
    ) -> Result<i32> {
        let mut runtime = self.runtime.borrow_mut();
        let Some(dialog) = runtime.active_dialog_overlays.get(dialog_index) else {
            return Err(WinedroidError::new(format!(
                "dialog button click referenced missing dialog\ndialog index: {dialog_index}"
            )));
        };
        let Some(button) = dialog.buttons.get(button_index) else {
            return Err(WinedroidError::new(format!(
                "dialog button click referenced missing button\ndialog index: {dialog_index}\nbutton index: {button_index}"
            )));
        };
        if !button.enabled {
            return Ok(0);
        }

        let action = button.action.clone();
        let input_value = dialog.input_value.clone();
        let dialog_key = dialog.dialog_key.clone();
        runtime.active_dialog_overlays.remove(dialog_index);
        runtime.insert_field(
            (dialog_key, dialog_showing_field().display_signature()),
            DexValue::Int(0),
        );
        drop(runtime);

        if let RuntimeDialogButtonAction::CommitEditTextPreference { preference_key } = action {
            self.set_edit_text_preference_value(
                &preference_key,
                input_value.as_deref().unwrap_or_default(),
                0,
            )?;
        }

        Ok(1)
    }

    pub(in crate::dex_run) fn click_dialog_item(
        &self,
        dialog_index: usize,
        item_index: usize,
        depth: usize,
    ) -> Result<i32> {
        let (dialog_key, action) = {
            let runtime = self.runtime.borrow();
            let Some(dialog) = runtime.active_dialog_overlays.get(dialog_index) else {
                return Err(WinedroidError::new(format!(
                    "dialog item click referenced missing dialog\ndialog index: {dialog_index}"
                )));
            };
            let Some(item) = dialog.items.get(item_index) else {
                if dialog.input_value.is_some() {
                    return Ok(0);
                }
                return Err(WinedroidError::new(format!(
                    "dialog item click referenced missing item\ndialog index: {dialog_index}\nitem index: {item_index}"
                )));
            };
            if !item.enabled {
                return Ok(0);
            }

            (dialog.dialog_key.clone(), item.action.clone())
        };

        match action {
            RuntimeDialogItemAction::ListPreferenceValue {
                preference_key,
                value,
            } => {
                self.set_list_preference_value(&preference_key, &value, depth + 1)?;
            }
        }

        let mut runtime = self.runtime.borrow_mut();
        runtime
            .active_dialog_overlays
            .retain(|dialog| dialog.dialog_key != dialog_key);
        runtime.insert_field(
            (dialog_key, dialog_showing_field().display_signature()),
            DexValue::Int(0),
        );

        Ok(1)
    }

    pub(in crate::dex_run) fn activity_window(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        Some(DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/view/Window;:activity:{}",
            map_identity_key(&invocation.arguments[0])
        ))))
    }

    // cite: frameworks-base Activity.java#setRequestedOrientation
    pub(in crate::dex_run) fn activity_write_requested_orientation(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let DexValue::Int(requested_orientation) = invocation.arguments[1] else {
            return None;
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &activity_requested_orientation_field(),
            DexValue::Int(requested_orientation),
        )
        .then_some(())
    }

    // cite: frameworks-base Activity.java#setShowWhenLocked, setTurnScreenOn
    pub(in crate::dex_run) fn activity_write_window_bool(
        &self,
        invocation: &DexInvocation,
        method_name: &str,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let DexValue::Int(value) = invocation.arguments[1] else {
            return None;
        };
        let name = match method_name {
            "setShowWhenLocked" => "showWhenLocked",
            "setTurnScreenOn" => "turnScreenOn",
            _ => return None,
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &DexFieldId {
                class_descriptor: "Landroid/app/Activity;".to_owned(),
                type_descriptor: "Z".to_owned(),
                name: name.to_owned(),
            },
            DexValue::Int(i32::from(value != 0)),
        )
        .then_some(())
    }

    // cite: frameworks-base Activity.java#getRequestedOrientation
    // cite: frameworks-base ActivityInfo.java#SCREEN_ORIENTATION_UNSPECIFIED
    pub(in crate::dex_run) fn activity_read_requested_orientation(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }

        let value = self
            .read_stored_instance_field(
                &invocation.arguments[0],
                &activity_requested_orientation_field(),
            )
            .unwrap_or(DexValue::Int(-1));
        let DexValue::Int(requested_orientation) = value else {
            return None;
        };

        Some(requested_orientation)
    }

    pub(in crate::dex_run) fn active_activity_layout_roots(&self) -> Vec<DexValue> {
        let content_layouts = self.active_content_layout_ids();
        content_layouts
            .into_iter()
            .chain(self.attached_inflated_layout_ids())
            .map(|layout_id| self.layout_reference_for_id(layout_id))
            .collect()
    }

    pub(in crate::dex_run) fn active_content_layout_ids(&self) -> Vec<i32> {
        let runtime = self.runtime.borrow();
        let start = runtime
            .activity_stack
            .last()
            .map(|activity| activity.content_layout_start)
            .unwrap_or(0);
        runtime.content_layouts[start..].to_vec()
    }

    // cite: frameworks-base ViewGroup.java#addView
    pub(in crate::dex_run) fn attached_inflated_layout_ids(&self) -> Vec<i32> {
        let (content_layouts, inflated_layouts, adapter_item_layout_ids) = {
            let runtime = self.runtime.borrow();
            let active_activity = runtime.activity_stack.last();
            let content_layout_start = active_activity
                .map(|activity| activity.content_layout_start)
                .unwrap_or(0);
            let inflated_layout_start = active_activity
                .map(|activity| activity.inflated_layout_start)
                .unwrap_or(0);

            (
                runtime.content_layouts[content_layout_start..].to_vec(),
                runtime.inflated_layouts[inflated_layout_start..].to_vec(),
                runtime
                    .adapter_rows
                    .values()
                    .map(|rows| rows.item_layout_id)
                    .collect::<std::collections::BTreeSet<i32>>(),
            )
        };

        let content_set: std::collections::BTreeSet<i32> =
            content_layouts.iter().copied().collect();
        let reachable = self.content_reachable_view_keys();

        inflated_layouts
            .into_iter()
            // cite: frameworks-base ViewGroup.java#getChildAt
            // cite: frameworks-base ListView.java#obtainView
            .filter(|layout_id| !adapter_item_layout_ids.contains(layout_id))
            .filter(|layout_id| {
                content_set.contains(layout_id)
                    || reachable
                        .contains(&map_identity_key(&self.layout_reference_for_id(*layout_id)))
            })
            .collect()
    }

    fn content_reachable_view_keys(&self) -> HashSet<String> {
        let runtime = self.runtime.borrow();
        let mut visited = HashSet::new();
        let mut stack = vec![map_identity_key(&view_reference(ANDROID_CONTENT_VIEW_ID))];
        while let Some(key) = stack.pop() {
            if !visited.insert(key.clone()) {
                continue;
            }
            if let Some(children) = runtime.view_group_children.get(&key) {
                for child in children {
                    stack.push(map_identity_key(child));
                }
            }
        }
        visited
    }

    pub(in crate::dex_run) fn registered_view_for_receiver(
        &self,
        receiver: &DexValue,
        view_id: i32,
    ) -> Option<DexValue> {
        let receiver_key = map_identity_key(receiver);

        self.runtime
            .borrow()
            .inflated_view_children
            .get(&(receiver_key, view_id))
            .cloned()
    }

    pub(in crate::dex_run) fn active_registered_view_by_id(
        &self,
        view_id: i32,
    ) -> Option<DexValue> {
        let roots = self.active_activity_layout_roots();

        roots.into_iter().rev().find_map(|root| {
            let root_key = map_identity_key(&root);
            self.runtime
                .borrow()
                .inflated_view_children
                .get(&(root_key, view_id))
                .cloned()
        })
    }

    pub(in crate::dex_run) fn active_dialog_registered_view_by_id(
        &self,
        view_id: i32,
    ) -> Option<DexValue> {
        let content_layouts = {
            let runtime = self.runtime.borrow();
            runtime
                .active_dialog_overlays
                .iter()
                .rev()
                .flat_map(|overlay| overlay.content_layouts.iter().rev().copied())
                .collect::<Vec<_>>()
        };

        content_layouts.into_iter().find_map(|layout_id| {
            let root = self.layout_reference_for_id(layout_id);
            let root_key = map_identity_key(&root);
            self.runtime
                .borrow()
                .inflated_view_children
                .get(&(root_key, view_id))
                .cloned()
        })
    }

    pub(in crate::dex_run) fn find_registered_view_by_id(
        &self,
        receiver: &DexValue,
        view_id: i32,
    ) -> Option<DexValue> {
        self.registered_view_for_receiver(receiver, view_id)
            .or_else(|| self.active_registered_view_by_id(view_id))
    }

    pub(in crate::dex_run) fn capture_adapter_row(
        &self,
        row_view: &DexValue,
        item_layout_id: i32,
    ) -> Result<DexAdapterRow> {
        let mut row = DexAdapterRow::default();
        let Some(events) = self.layout_xml_events(item_layout_id)? else {
            return Ok(row);
        };

        for event in events {
            let crate::binary_xml::XmlEvent::Start(element) = event else {
                continue;
            };
            let Some(view_id) = xml_element_view_id(&element) else {
                continue;
            };
            let Some(child) = self.registered_view_for_receiver(row_view, view_id) else {
                continue;
            };

            if let Some(DexValue::String(text)) =
                self.read_stored_instance_field(&child, &text_view_text_field())
            {
                row.texts.push(DexViewText {
                    view_id,
                    text: text.to_string(),
                });
            }
            if let Some(value) =
                self.read_stored_instance_field(&child, &image_view_drawable_field())
            {
                if let Some(drawable_resource_id) = self.image_view_drawable_resource_id(&value) {
                    row.images.push(DexViewImage {
                        view_id,
                        drawable_resource_id,
                    });
                }
            }
            if let Some(DexValue::Int(visibility)) =
                self.read_stored_instance_field(&child, &view_visibility_field())
            {
                row.visibilities.push(DexViewVisibility {
                    view_id,
                    visibility,
                });
            }
        }

        Ok(row)
    }

    // cite: frameworks-base BitmapFactory.java#decodeResource
    // cite: frameworks-base BitmapDrawable.java#BitmapDrawable
    pub(in crate::dex_run) fn image_view_drawable_resource_id(
        &self,
        value: &DexValue,
    ) -> Option<u32> {
        match value {
            DexValue::Reference(_) => drawable_resource_id_from_value(value),
            DexValue::Object(_) => {
                match self.read_stored_instance_field(value, &bitmap_drawable_resource_id_field()) {
                    Some(DexValue::Int(resource_id)) => u32::try_from(resource_id).ok(),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(in crate::dex_run) fn register_adapter_item_click(
        &self,
        list_view: &DexValue,
        adapter_view_id: i32,
        position: i32,
    ) -> i32 {
        let mut runtime = self.runtime.borrow_mut();
        let click_id = runtime.allocate_adapter_item_click_id();
        runtime.adapter_item_clicks.insert(
            click_id,
            AdapterItemClick {
                list_view: list_view.clone(),
                position,
                adapter_view_id,
            },
        );
        click_id
    }

    pub(in crate::dex_run) fn clear_adapter_item_clicks(&self, adapter_view_id: i32) {
        self.runtime
            .borrow_mut()
            .adapter_item_clicks
            .retain(|_, item| item.adapter_view_id != adapter_view_id);
    }

    pub(in crate::dex_run) fn adapter_item_click(&self, view_id: i32) -> Option<(DexValue, i32)> {
        self.runtime
            .borrow()
            .adapter_item_clicks
            .get(&view_id)
            .map(|item| (item.list_view.clone(), item.position))
    }

    pub(in crate::dex_run) fn store_adapter_rows(&self, rows: DexAdapterRows) {
        self.runtime
            .borrow_mut()
            .adapter_rows
            .insert(rows.adapter_view_id, rows);
    }

    pub(in crate::dex_run) fn clear_adapter_rows(&self, adapter_view_id: i32) {
        self.runtime
            .borrow_mut()
            .adapter_rows
            .remove(&adapter_view_id);
    }

    #[cfg(test)]
    pub(in crate::dex_run) fn write_activity_receiver(&self, receiver: DexValue) {
        self.runtime.borrow_mut().activity_receiver = Some(receiver);
    }

    pub(in crate::dex_run) fn write_declared_on_click_handler(
        &self,
        view_id: i32,
        method_name: &str,
    ) -> Result<()> {
        if view_id <= 0 {
            return Err(WinedroidError::new(format!(
                "XML onClick handler requires a positive Android view ID\nview id: {view_id}"
            )));
        }

        if method_name.is_empty() {
            return Err(WinedroidError::new(
                "XML onClick handler method name is empty",
            ));
        }

        let view_key = map_identity_key(&self.canonical_view_for_id(view_id));
        self.runtime
            .borrow_mut()
            .view_declared_on_click_handlers
            .insert(view_key, method_name.to_owned());

        Ok(())
    }

    pub(in crate::dex_run) fn canonical_view_for_id(&self, view_id: i32) -> DexValue {
        self.active_registered_view_by_id(view_id)
            .unwrap_or_else(|| view_reference(view_id))
    }

    pub(in crate::dex_run) fn write_view_text(&self, view_id: i32, text: &str) -> Result<()> {
        if view_id <= 0 {
            return Err(WinedroidError::new(format!(
                "TextView text requires a positive Android view ID\nview id: {view_id}"
            )));
        }

        let view = self.canonical_view_for_id(view_id);
        if !self.write_stored_instance_field(
            &view,
            &text_view_text_field(),
            DexValue::String(std::rc::Rc::from(text.to_owned())),
        ) {
            return Err(WinedroidError::new(format!(
                "failed to store TextView text\nview id: {view_id}"
            )));
        }

        Ok(())
    }

    pub(in crate::dex_run) fn write_view_visibility_value(
        &self,
        view_id: i32,
        visibility: i32,
    ) -> Result<()> {
        if view_id <= 0 {
            return Err(WinedroidError::new(format!(
                "View visibility requires a positive Android view ID\nview id: {view_id}"
            )));
        }

        if !matches!(visibility, 0 | 4 | 8) {
            return Err(WinedroidError::new(format!(
                "View visibility must be one of Android VISIBLE(0), INVISIBLE(4), or GONE(8)\nview id: {view_id}\nvisibility: {visibility}"
            )));
        }

        let view = self.canonical_view_for_id(view_id);
        if !self.write_stored_instance_field(
            &view,
            &view_visibility_field(),
            DexValue::Int(visibility),
        ) {
            return Err(WinedroidError::new(format!(
                "failed to store View visibility\nview id: {view_id}"
            )));
        }

        Ok(())
    }

    pub(in crate::dex_run) fn view_text(&self, view_id: i32) -> Result<Option<String>> {
        if view_id <= 0 {
            return Err(WinedroidError::new(format!(
                "TextView text lookup requires a positive Android view ID\nview id: {view_id}"
            )));
        }

        let value = self
            .read_stored_instance_field(
                &self.canonical_view_for_id(view_id),
                &text_view_text_field(),
            )
            .or_else(|| {
                self.read_stored_instance_field(&view_reference(view_id), &text_view_text_field())
            });
        let Some(value) = value else {
            return Ok(None);
        };

        match value {
            DexValue::String(text) => Ok(Some(text.to_string())),
            other => Err(WinedroidError::new(format!(
                "stored TextView text is not a string\nview id: {view_id}\nvalue: {other}"
            ))),
        }
    }
}

pub(in crate::dex_run) fn dialog_context_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Landroid/content/Context;".to_owned(),
        name: "mContext".to_owned(),
    }
}

pub(in crate::dex_run) fn dialog_theme_resource_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "mThemeResource".to_owned(),
    }
}

pub(in crate::dex_run) fn dialog_cancelable_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "cancelable".to_owned(),
    }
}

fn dialog_canceled_on_touch_outside_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "canceledOnTouchOutside".to_owned(),
    }
}

fn dialog_on_cancel_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Landroid/content/DialogInterface$OnCancelListener;".to_owned(),
        name: "onCancelListener".to_owned(),
    }
}

fn dialog_on_dismiss_listener_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Landroid/content/DialogInterface$OnDismissListener;".to_owned(),
        name: "onDismissListener".to_owned(),
    }
}

// cite: frameworks-base ActivityInfo.java#screenOrientation
fn activity_requested_orientation_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Activity;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "requestedOrientation".to_owned(),
    }
}

fn dialog_showing_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Z".to_owned(),
        name: "showing".to_owned(),
    }
}

fn dialog_content_view_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Landroid/view/View;".to_owned(),
        name: "winedroidContentView".to_owned(),
    }
}

fn dialog_window_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/app/Dialog;".to_owned(),
        type_descriptor: "Landroid/view/Window;".to_owned(),
        name: "mWindow".to_owned(),
    }
}

fn dialog_window_reference(dialog: &DexValue) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/view/Window;:dialog:{}",
        map_identity_key(dialog)
    )))
}

fn runtime_dialog_overlay_from_graph(
    runtime: &DexRuntimeState,
    dialog: &DexValue,
) -> RuntimeDialogOverlay {
    let dialog_key = map_identity_key(dialog);
    let object_keys = dialog_reachable_object_keys(runtime, &dialog_key);
    let state_keys = dialog_state_object_keys(runtime, &object_keys);
    let text_values = dialog_text_values(runtime, &state_keys);
    let title = dialog_title(&text_values);
    let buttons = dialog_buttons(&text_values, title.as_deref());
    let content_layouts = dialog_content_layouts(runtime, &state_keys);

    RuntimeDialogOverlay {
        dialog_key,
        title,
        content_layouts,
        items: Vec::new(),
        buttons,
        input_value: None,
    }
}

fn dialog_reachable_object_keys(runtime: &DexRuntimeState, start_key: &str) -> BTreeSet<String> {
    const MAX_DIALOG_GRAPH_DEPTH: usize = 2;

    let mut visited = BTreeSet::new();
    let mut pending = VecDeque::new();
    visited.insert(start_key.to_owned());
    pending.push_back((start_key.to_owned(), 0));

    while let Some((owner_key, depth)) = pending.pop_front() {
        if depth >= MAX_DIALOG_GRAPH_DEPTH {
            continue;
        }

        for ((field_owner_key, field), value) in runtime.iter_fields() {
            if !field_owner_key.matches_key(&owner_key) {
                continue;
            }
            // cite: frameworks-base Dialog.java#mContext, mOwnerActivity
            // cite: frameworks-base ContextWrapper.java#mBase
            if dialog_graph_context_edge(field) {
                continue;
            }

            let Some(value_key) = dialog_reachable_object_key(value) else {
                continue;
            };
            if !visited.insert(value_key.clone()) {
                continue;
            }

            pending.push_back((value_key, depth + 1));
        }
    }

    // cite: frameworks-base Dialog.java#setContentView
    // cite: frameworks-base ViewGroup.java#addView
    if let Some(content_view) = runtime
        .instance_fields
        .get(&(
            start_key.to_owned(),
            dialog_content_view_field().display_signature(),
        ))
        .cloned()
    {
        if let Some(content_view_key) = dialog_reachable_object_key(&content_view) {
            if visited.insert(content_view_key.clone()) {
                pending.push_back((content_view_key, 0));
            }

            while let Some((owner_key, _)) = pending.pop_front() {
                let Some(children) = runtime.view_group_children.get(&owner_key) else {
                    continue;
                };

                for child in children {
                    let Some(child_key) = dialog_reachable_object_key(child) else {
                        continue;
                    };
                    if !visited.insert(child_key.clone()) {
                        continue;
                    }

                    pending.push_back((child_key, 0));
                }
            }
        }
    }

    visited
}

fn dialog_graph_context_edge(field: &str) -> bool {
    field.ends_with(".mContext:Landroid/content/Context;")
        || field == "Landroid/content/ContextWrapper;.mBase:Landroid/content/Context;"
        || field == "Landroid/app/Dialog;.mOwnerActivity:Landroid/app/Activity;"
}

fn dialog_state_object_keys(
    runtime: &DexRuntimeState,
    object_keys: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut state_keys = BTreeSet::new();

    for ((owner_key, field), value) in runtime.iter_fields() {
        if !object_keys.contains(&owner_key.to_string()) {
            continue;
        }

        if dialog_text_field(field) || layout_id_from_value(value).is_some() {
            state_keys.insert(owner_key.to_string());
        }
    }

    state_keys
}

fn dialog_reachable_object_key(value: &DexValue) -> Option<String> {
    match value {
        DexValue::Object(_) => Some(map_identity_key(value)),
        DexValue::Reference(_)
        | DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

fn dialog_text_values(runtime: &DexRuntimeState, object_keys: &BTreeSet<String>) -> Vec<String> {
    let mut values = Vec::new();

    for ((owner_key, field), value) in runtime.iter_fields() {
        if !object_keys.contains(&owner_key.to_string()) {
            continue;
        }
        if !dialog_text_field(field) {
            continue;
        }

        let Some(text) = runtime_char_sequence_text(runtime, value) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty()
            || !dialog_text_is_meaningful(text)
            || values.iter().any(|existing| existing == text)
        {
            continue;
        }

        values.push(text.to_owned());
    }

    values.sort();
    values
}

fn dialog_text_field(field: &str) -> bool {
    field.ends_with(":Ljava/lang/CharSequence;") || field.ends_with(":Ljava/lang/String;")
}

fn dialog_text_is_meaningful(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

fn dialog_title(values: &[String]) -> Option<String> {
    values
        .iter()
        .find(|value| !dialog_button_like(value))
        .or_else(|| values.first())
        .cloned()
}

fn dialog_buttons(values: &[String], title: Option<&str>) -> Vec<RuntimeDialogButton> {
    values
        .iter()
        .filter(|value| Some(value.as_str()) != title)
        .filter(|value| dialog_button_like(value) || value.chars().count() <= 24)
        .map(|value| RuntimeDialogButton {
            text: value.clone(),
            enabled: true,
            action: RuntimeDialogButtonAction::Dismiss,
        })
        .collect()
}

fn dialog_button_like(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "ok" | "close" | "cancel" | "dismiss" | "yes" | "no"
    )
}

fn dialog_content_layouts(runtime: &DexRuntimeState, object_keys: &BTreeSet<String>) -> Vec<i32> {
    let mut layout_ids = BTreeSet::new();

    for key in object_keys {
        if let Some(layout_id) = layout_id_from_identity_key(key) {
            layout_ids.insert(layout_id);
        }
    }

    for ((owner_key, _field), value) in runtime.iter_fields() {
        if !object_keys.contains(&owner_key.to_string()) {
            continue;
        }
        if let Some(layout_id) = layout_id_from_value(value) {
            layout_ids.insert(layout_id);
        }
    }

    for layout_id in active_activity_initial_inflated_layouts(runtime) {
        layout_ids.remove(&layout_id);
    }

    layout_ids.into_iter().collect()
}

fn active_activity_initial_inflated_layouts(runtime: &DexRuntimeState) -> BTreeSet<i32> {
    let Some(activity) = runtime.activity_stack.last() else {
        return BTreeSet::new();
    };

    runtime.inflated_layouts[activity.inflated_layout_start..activity.initial_inflated_layout_end]
        .iter()
        .copied()
        .collect()
}

fn layout_id_from_value(value: &DexValue) -> Option<i32> {
    let DexValue::Reference(reference) = value else {
        return None;
    };

    layout_id_from_reference(reference)
}

fn layout_id_from_identity_key(key: &str) -> Option<i32> {
    let reference = key.strip_prefix("reference:")?;
    layout_id_from_reference(reference)
}

fn layout_id_from_reference(reference: &str) -> Option<i32> {
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

fn preference_snapshot_from_runtime(
    program: &DexProgram,
    runtime: &DexRuntimeState,
    min_object_id: u64,
) -> Vec<DexPreferenceSnapshot> {
    let group_children = preference_group_children(runtime, min_object_id);
    if group_children.is_empty() {
        return Vec::new();
    }

    let child_group_keys = preference_child_group_keys(&group_children);
    let mut root_group_keys = group_children
        .keys()
        .filter(|key| !child_group_keys.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    root_group_keys.sort_by_key(|key| object_identity_sort_key(key));

    let mut snapshot = Vec::new();
    let mut visited_groups = HashSet::new();
    let mut visited_preferences = HashSet::new();

    for root_group_key in root_group_keys {
        append_preference_group_snapshot(
            program,
            runtime,
            &group_children,
            &root_group_key,
            min_object_id,
            0,
            &mut visited_groups,
            &mut visited_preferences,
            &mut snapshot,
        );
    }

    snapshot
}

fn preference_group_children(
    runtime: &DexRuntimeState,
    min_object_id: u64,
) -> HashMap<String, Vec<DexValue>> {
    let children_fields = [
        "Landroidx/preference/PreferenceGroup;.mPreferences:Ljava/util/List;",
        "Landroidx/preference/PreferenceGroup;.N:Ljava/util/ArrayList;",
    ];
    let mut groups = HashMap::new();

    for (owner_key, object_fields) in runtime.iter_field_owners() {
        let Some(value) = children_fields
            .iter()
            .find_map(|children_field| object_fields.get(*children_field))
        else {
            continue;
        };
        if !owner_key.is_in_active_object_scope(min_object_id) {
            continue;
        }

        let list_key = map_identity_key(value);
        let Some(children) = runtime.list_entries.get(&list_key) else {
            continue;
        };

        groups.insert(owner_key.to_string(), children.clone());
    }

    groups
}

fn preference_child_group_keys(groups: &HashMap<String, Vec<DexValue>>) -> HashSet<String> {
    let mut child_group_keys = HashSet::new();

    for children in groups.values() {
        for child in children {
            let child_key = map_identity_key(child);
            if groups.contains_key(&child_key) {
                child_group_keys.insert(child_key);
            }
        }
    }

    child_group_keys
}

fn append_preference_group_snapshot(
    program: &DexProgram,
    runtime: &DexRuntimeState,
    group_children: &HashMap<String, Vec<DexValue>>,
    group_key: &str,
    min_object_id: u64,
    depth: usize,
    visited_groups: &mut HashSet<String>,
    visited_preferences: &mut HashSet<String>,
    snapshot: &mut Vec<DexPreferenceSnapshot>,
) {
    if !visited_groups.insert(group_key.to_owned()) {
        return;
    }

    let Some(children) = group_children.get(group_key) else {
        return;
    };

    for child in children {
        append_preference_snapshot(
            program,
            runtime,
            group_children,
            child,
            min_object_id,
            depth,
            visited_groups,
            visited_preferences,
            snapshot,
        );
    }
}

fn append_preference_snapshot(
    program: &DexProgram,
    runtime: &DexRuntimeState,
    group_children: &HashMap<String, Vec<DexValue>>,
    preference: &DexValue,
    min_object_id: u64,
    depth: usize,
    visited_groups: &mut HashSet<String>,
    visited_preferences: &mut HashSet<String>,
    snapshot: &mut Vec<DexPreferenceSnapshot>,
) {
    let preference_key = map_identity_key(preference);
    if !identity_key_is_in_active_object_scope(&preference_key, min_object_id) {
        return;
    }
    if !visited_preferences.insert(preference_key.clone()) {
        return;
    }

    if let Some(item) =
        preference_snapshot_item(program, runtime, preference, &preference_key, depth)
    {
        snapshot.push(item);
    }

    if group_children.contains_key(&preference_key) {
        append_preference_group_snapshot(
            program,
            runtime,
            group_children,
            &preference_key,
            min_object_id,
            depth + 1,
            visited_groups,
            visited_preferences,
            snapshot,
        );
    }
}

fn preference_snapshot_item(
    program: &DexProgram,
    runtime: &DexRuntimeState,
    preference: &DexValue,
    preference_key: &str,
    depth: usize,
) -> Option<DexPreferenceSnapshot> {
    let class_descriptor = value_class_descriptor(preference)?.to_owned();
    if !class_descriptor.starts_with("Landroidx/preference/")
        && !program.descriptor_matches_type(&class_descriptor, "Landroidx/preference/Preference;")
    {
        return None;
    }

    let title = preference_field_text(
        runtime,
        preference_key,
        &[
            "Landroidx/preference/Preference;.mTitle:Ljava/lang/CharSequence;",
            "Landroidx/preference/Preference;.g:Ljava/lang/CharSequence;",
        ],
    )?;
    let summary = preference_field_text(
        runtime,
        preference_key,
        &[
            "Landroidx/preference/Preference;.mSummary:Ljava/lang/CharSequence;",
            "Landroidx/preference/Preference;.h:Ljava/lang/CharSequence;",
        ],
    )
    .filter(|summary| summary != &title);
    let key = preference_field_string(
        runtime,
        preference_key,
        &[
            "Landroidx/preference/Preference;.mKey:Ljava/lang/String;",
            "Landroidx/preference/Preference;.k:Ljava/lang/String;",
        ],
    );
    let icon_resource_id = preference_icon_resource_id(runtime, preference_key);
    let kind = preference_kind(program, runtime, preference_key, &class_descriptor);

    Some(DexPreferenceSnapshot {
        runtime_key: preference_key.to_owned(),
        key,
        title,
        summary,
        icon_resource_id,
        class_descriptor,
        kind,
        depth,
    })
}

fn preference_kind(
    program: &DexProgram,
    runtime: &DexRuntimeState,
    preference_key: &str,
    class_descriptor: &str,
) -> DexPreferenceKind {
    if program.descriptor_matches_type(class_descriptor, "Landroidx/preference/PreferenceCategory;")
    {
        return DexPreferenceKind::Category;
    }

    if program.descriptor_matches_type(class_descriptor, "Landroidx/preference/TwoStatePreference;")
    {
        return DexPreferenceKind::Switch {
            checked: preference_bool_field(
                runtime,
                preference_key,
                &[
                    "Landroidx/preference/TwoStatePreference;.mChecked:Z",
                    "Landroidx/preference/TwoStatePreference;.M:Z",
                ],
            )
            .unwrap_or(false),
        };
    }

    if program.descriptor_matches_type(class_descriptor, "Landroidx/preference/ListPreference;") {
        return DexPreferenceKind::List {
            value: preference_field_string(
                runtime,
                preference_key,
                &[
                    "Landroidx/preference/ListPreference;.mValue:Ljava/lang/String;",
                    "Landroidx/preference/ListPreference;.U:Ljava/lang/String;",
                ],
            ),
            entries: preference_string_array_field(
                runtime,
                preference_key,
                &[
                    "Landroidx/preference/ListPreference;.mEntries:[Ljava/lang/CharSequence;",
                    "Landroidx/preference/ListPreference;.S:[Ljava/lang/CharSequence;",
                ],
            ),
            entry_values: preference_string_array_field(
                runtime,
                preference_key,
                &[
                    "Landroidx/preference/ListPreference;.mEntryValues:[Ljava/lang/CharSequence;",
                    "Landroidx/preference/ListPreference;.T:[Ljava/lang/CharSequence;",
                ],
            ),
        };
    }

    DexPreferenceKind::Plain
}

fn preference_bool_field(
    runtime: &DexRuntimeState,
    owner_key: &str,
    field_signatures: &[&str],
) -> Option<bool> {
    let value = preference_field_value(runtime, owner_key, field_signatures)?;
    let DexValue::Int(value) = value else {
        return None;
    };

    Some(*value != 0)
}

fn preference_field_value<'runtime>(
    runtime: &'runtime DexRuntimeState,
    owner_key: &str,
    field_signatures: &[&str],
) -> Option<&'runtime DexValue> {
    field_signatures.iter().find_map(|field_signature| {
        runtime
            .instance_fields
            .get(&(owner_key.to_owned(), (*field_signature).to_owned()))
    })
}

fn preference_string_array_field(
    runtime: &DexRuntimeState,
    owner_key: &str,
    field_signatures: &[&str],
) -> Vec<String> {
    let Some(value) = preference_field_value(runtime, owner_key, field_signatures) else {
        return Vec::new();
    };
    let array_key = map_identity_key(value);
    let Some(values) = runtime.array_entries.get(&array_key) else {
        return Vec::new();
    };

    values
        .iter()
        .filter_map(|value| runtime_char_sequence_text(runtime, value))
        .collect()
}

fn preference_icon_resource_id(runtime: &DexRuntimeState, owner_key: &str) -> Option<u32> {
    let drawable = preference_field_value(
        runtime,
        owner_key,
        &[
            "Landroidx/preference/Preference;.mIcon:Landroid/graphics/drawable/Drawable;",
            "Landroidx/preference/Preference;.j:Landroid/graphics/drawable/Drawable;",
        ],
    )
    .and_then(drawable_resource_id_from_value);

    if drawable.is_some() {
        return drawable;
    }

    preference_field_value(
        runtime,
        owner_key,
        &[
            "Landroidx/preference/Preference;.mIconResId:I",
            "Landroidx/preference/Preference;.i:I",
        ],
    )
    .and_then(dex_int_value)
    .and_then(|resource_id| u32::try_from(resource_id).ok())
    .filter(|resource_id| *resource_id != 0)
}

fn preference_field_text(
    runtime: &DexRuntimeState,
    owner_key: &str,
    field_signatures: &[&str],
) -> Option<String> {
    let value = preference_field_value(runtime, owner_key, field_signatures)?;

    runtime_char_sequence_text(runtime, value)
}

fn preference_field_string(
    runtime: &DexRuntimeState,
    owner_key: &str,
    field_signatures: &[&str],
) -> Option<String> {
    let value = preference_field_value(runtime, owner_key, field_signatures)?;
    let DexValue::String(text) = value else {
        return None;
    };

    Some(text.clone().to_string())
}

fn runtime_canvas_frame_snapshot(runtime: &DexRuntimeState) -> Vec<DexRuntimeCanvasFrame> {
    runtime
        .runtime_canvas_frames
        .iter()
        .map(|frame| DexRuntimeCanvasFrame {
            root_view_key: frame.root_view_key.clone(),
            class_descriptor: frame.class_descriptor.clone(),
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
            ops: runtime_canvas_ops_snapshot(runtime, &frame.canvas_key),
        })
        .collect()
}

fn runtime_canvas_ops_snapshot(
    runtime: &DexRuntimeState,
    canvas_key: &str,
) -> Vec<DexRuntimeCanvasOp> {
    let Some(ops) = runtime.canvas_draw_ops.get(canvas_key) else {
        return Vec::new();
    };

    print_runtime_canvas_raw_op_summary_if_requested(ops);
    runtime_canvas_ops_from_slice(runtime, ops, 0)
}

fn print_runtime_canvas_raw_op_summary_if_requested(ops: &[CanvasDrawOp]) {
    static RAW_OP_SUMMARY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*RAW_OP_SUMMARY
        .get_or_init(|| env::var_os("WINEDROID_RUNTIME_CANVAS_RAW_OP_SUMMARY").is_some())
    {
        return;
    }

    let mut summary = RuntimeCanvasRawOpSummary::default();
    summary.record_ops(ops);
    eprintln!(
        "Runtime canvas raw ops: total:{} save:{} restore:{} translate:{} scale:{} concat_matrix:{} clip_rect:{} clip_path:{} line:{} rect:{} round_rect:{} text_layout:{} path:{} bitmap:{} render_node:{}",
        ops.len(),
        summary.save,
        summary.restore,
        summary.translate,
        summary.scale,
        summary.concat_matrix,
        summary.clip_rect,
        summary.clip_path,
        summary.line,
        summary.rect,
        summary.round_rect,
        summary.text_layout,
        summary.path,
        summary.bitmap,
        summary.render_node
    );
}

#[derive(Debug, Default)]
struct RuntimeCanvasRawOpSummary {
    save: usize,
    restore: usize,
    translate: usize,
    scale: usize,
    rotate: usize,
    concat_matrix: usize,
    clip_rect: usize,
    clip_path: usize,
    line: usize,
    rect: usize,
    circle: usize,
    round_rect: usize,
    text_layout: usize,
    path: usize,
    bitmap: usize,
    render_node: usize,
}

impl RuntimeCanvasRawOpSummary {
    fn record_ops(&mut self, ops: &[CanvasDrawOp]) {
        for op in ops {
            self.record_op(op);
        }
    }

    fn record_op(&mut self, op: &CanvasDrawOp) {
        match op {
            CanvasDrawOp::Save => self.save += 1,
            CanvasDrawOp::Restore => self.restore += 1,
            CanvasDrawOp::Translate { .. } => self.translate += 1,
            CanvasDrawOp::Scale { .. } => self.scale += 1,
            CanvasDrawOp::Rotate { .. } => self.rotate += 1,
            CanvasDrawOp::ConcatMatrix { .. } => self.concat_matrix += 1,
            CanvasDrawOp::ClipRect { .. } => self.clip_rect += 1,
            CanvasDrawOp::ClipPath { .. } => self.clip_path += 1,
            CanvasDrawOp::Line { .. } => self.line += 1,
            CanvasDrawOp::Rect { .. } => self.rect += 1,
            CanvasDrawOp::Circle { .. } => self.circle += 1,
            CanvasDrawOp::RoundRect { .. } => self.round_rect += 1,
            CanvasDrawOp::TextLayout { .. } => self.text_layout += 1,
            CanvasDrawOp::Path { .. } => self.path += 1,
            CanvasDrawOp::Bitmap { .. } => self.bitmap += 1,
            CanvasDrawOp::RenderNode { .. } => self.render_node += 1,
        }
    }
}

fn runtime_canvas_ops_from_slice(
    runtime: &DexRuntimeState,
    ops: &[CanvasDrawOp],
    depth: usize,
) -> Vec<DexRuntimeCanvasOp> {
    if depth > 16 {
        return Vec::new();
    }

    ops.iter()
        .filter_map(|op| runtime_canvas_op_snapshot(runtime, op, depth))
        .collect()
}

fn runtime_canvas_op_snapshot(
    runtime: &DexRuntimeState,
    op: &CanvasDrawOp,
    depth: usize,
) -> Option<DexRuntimeCanvasOp> {
    match op {
        CanvasDrawOp::Save => Some(DexRuntimeCanvasOp::Save),
        CanvasDrawOp::Restore => Some(DexRuntimeCanvasOp::Restore),
        CanvasDrawOp::Translate { dx, dy } => {
            Some(DexRuntimeCanvasOp::Translate { dx: *dx, dy: *dy })
        }
        CanvasDrawOp::Scale { sx, sy } => Some(DexRuntimeCanvasOp::Scale { sx: *sx, sy: *sy }),
        CanvasDrawOp::Rotate { degrees } => Some(DexRuntimeCanvasOp::Rotate { degrees: *degrees }),
        CanvasDrawOp::ConcatMatrix { matrix } => Some(DexRuntimeCanvasOp::ConcatMatrix {
            values: runtime_matrix_values(runtime, matrix),
        }),
        CanvasDrawOp::ClipRect {
            left,
            top,
            right,
            bottom,
            op,
        } => Some(DexRuntimeCanvasOp::ClipRect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            op: runtime_canvas_clip_op_snapshot(op),
        }),
        CanvasDrawOp::ClipPath { ops, op } => Some(DexRuntimeCanvasOp::ClipPath {
            ops: runtime_canvas_path_elements_snapshot(ops)?,
            op: runtime_canvas_clip_op_snapshot(op),
        }),
        CanvasDrawOp::Line {
            x1,
            y1,
            x2,
            y2,
            paint,
        } => Some(DexRuntimeCanvasOp::Line {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
            color: runtime_paint_color(runtime, paint),
            blend_mode: runtime_paint_blend_mode(runtime, paint),
            stroke_width: runtime_paint_stroke_width(runtime, paint),
            stroke_cap: runtime_paint_stroke_cap(runtime, paint),
        }),
        CanvasDrawOp::Rect {
            left,
            top,
            right,
            bottom,
            paint,
        } => Some(DexRuntimeCanvasOp::Rect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            color: runtime_paint_color(runtime, paint),
            blend_mode: runtime_paint_blend_mode(runtime, paint),
            paint_style: runtime_paint_style(runtime, paint),
        }),
        CanvasDrawOp::Circle {
            cx,
            cy,
            radius,
            paint,
        } => Some(DexRuntimeCanvasOp::Circle {
            cx: *cx,
            cy: *cy,
            radius: *radius,
            color: runtime_paint_color(runtime, paint),
            blend_mode: runtime_paint_blend_mode(runtime, paint),
            paint_style: runtime_paint_style(runtime, paint),
            stroke_width: runtime_paint_stroke_width(runtime, paint),
        }),
        CanvasDrawOp::RoundRect {
            left,
            top,
            right,
            bottom,
            rx,
            ry,
            paint,
        } => Some(DexRuntimeCanvasOp::RoundRect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            rx: *rx,
            ry: *ry,
            color: runtime_paint_color(runtime, paint),
            blend_mode: runtime_paint_blend_mode(runtime, paint),
            paint_style: runtime_paint_style(runtime, paint),
        }),
        CanvasDrawOp::TextLayout { text, paint, .. } => {
            let text_size = runtime_paint_text_size(runtime, paint);

            Some(DexRuntimeCanvasOp::Text {
                text: text.clone(),
                x: 0.0,
                y: text_size,
                color: runtime_paint_color(runtime, paint),
                text_size,
            })
        }
        CanvasDrawOp::Path { ops, paint } => Some(DexRuntimeCanvasOp::Path {
            ops: runtime_canvas_path_elements_snapshot(ops)?,
            color: runtime_paint_color(runtime, paint),
            blend_mode: runtime_paint_blend_mode(runtime, paint),
            paint_style: runtime_paint_style(runtime, paint),
        }),
        CanvasDrawOp::Bitmap {
            bitmap,
            source,
            destination,
            paint,
        } => runtime_canvas_bitmap_op_snapshot(runtime, bitmap, source, destination, paint, depth),
        CanvasDrawOp::RenderNode { render_node } => {
            let render_node_key = map_identity_key(render_node);
            let ops = runtime
                .render_node_draw_ops
                .get(&render_node_key)
                .map(|ops| runtime_canvas_ops_from_slice(runtime, ops, depth + 1))
                .unwrap_or_default();

            // cite: frameworks-base RenderNode.java#setPosition, setAlpha, setScaleX, setScaleY, setClipToBounds
            Some(DexRuntimeCanvasOp::RenderNode {
                left: rt_int_field!(
                    runtime,
                    render_node,
                    &render_node_position_field("positionLeft"),
                )
                .unwrap_or(0) as f32,
                top: rt_int_field!(
                    runtime,
                    render_node,
                    &render_node_position_field("positionTop"),
                )
                .unwrap_or(0) as f32,
                right: rt_int_field!(
                    runtime,
                    render_node,
                    &render_node_position_field("positionRight"),
                )
                .unwrap_or(0) as f32,
                bottom: rt_int_field!(
                    runtime,
                    render_node,
                    &render_node_position_field("positionBottom"),
                )
                .unwrap_or(0) as f32,
                alpha: rt_float_field!(runtime, render_node, &render_node_alpha_field())
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0),
                scale_x: rt_float_field!(runtime, render_node, &render_node_scale_x_field())
                    .unwrap_or(1.0),
                scale_y: rt_float_field!(runtime, render_node, &render_node_scale_y_field())
                    .unwrap_or(1.0),
                clip_to_bounds: rt_int_field!(
                    runtime,
                    render_node,
                    &render_node_clip_to_bounds_field(),
                )
                .map(|value| value != 0)
                .unwrap_or(true),
                ops,
            })
        }
    }
}

fn runtime_canvas_path_elements_snapshot(
    path_ops: &[PathOp],
) -> Option<Vec<DexRuntimeCanvasPathOp>> {
    path_ops
        .iter()
        .map(runtime_canvas_path_element_snapshot)
        .collect()
}

fn runtime_canvas_path_element_snapshot(op: &PathOp) -> Option<DexRuntimeCanvasPathOp> {
    match op {
        PathOp::MoveTo { x, y } => Some(DexRuntimeCanvasPathOp::MoveTo { x: *x, y: *y }),
        PathOp::LineTo { x, y } => Some(DexRuntimeCanvasPathOp::LineTo { x: *x, y: *y }),
        PathOp::RLineTo { dx, dy } => Some(DexRuntimeCanvasPathOp::RLineTo { dx: *dx, dy: *dy }),
        PathOp::QuadTo { x1, y1, x2, y2 } => Some(DexRuntimeCanvasPathOp::QuadTo {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
        }),
        PathOp::CubicTo {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
        } => Some(DexRuntimeCanvasPathOp::CubicTo {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
            x3: *x3,
            y3: *y3,
        }),
        PathOp::RCubicTo {
            dx1,
            dy1,
            dx2,
            dy2,
            dx3,
            dy3,
        } => Some(DexRuntimeCanvasPathOp::RCubicTo {
            dx1: *dx1,
            dy1: *dy1,
            dx2: *dx2,
            dy2: *dy2,
            dx3: *dx3,
            dy3: *dy3,
        }),
        PathOp::BooleanOp { left, right, op } => Some(DexRuntimeCanvasPathOp::BooleanOp {
            left: runtime_canvas_path_elements_snapshot(left)?,
            right: runtime_canvas_path_elements_snapshot(right)?,
            op: runtime_canvas_path_boolean_op_snapshot(*op),
        }),
        PathOp::Rect {
            left,
            top,
            right,
            bottom,
            clockwise,
        } => Some(DexRuntimeCanvasPathOp::Rect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            clockwise: *clockwise,
        }),
        PathOp::RoundRect {
            left,
            top,
            right,
            bottom,
            radii,
            clockwise,
        } => Some(DexRuntimeCanvasPathOp::RoundRect {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            radii: radii.clone(),
            clockwise: *clockwise,
        }),
        PathOp::Close => Some(DexRuntimeCanvasPathOp::Close),
    }
}

fn runtime_canvas_path_boolean_op_snapshot(op: PathBooleanOp) -> DexRuntimeCanvasPathBooleanOp {
    match op {
        PathBooleanOp::Difference => DexRuntimeCanvasPathBooleanOp::Difference,
        PathBooleanOp::Intersect => DexRuntimeCanvasPathBooleanOp::Intersect,
        PathBooleanOp::Union => DexRuntimeCanvasPathBooleanOp::Union,
        PathBooleanOp::Xor => DexRuntimeCanvasPathBooleanOp::Xor,
        PathBooleanOp::ReverseDifference => DexRuntimeCanvasPathBooleanOp::ReverseDifference,
    }
}

fn runtime_canvas_clip_op_snapshot(op: &DexValue) -> DexRuntimeCanvasClipOp {
    match op {
        DexValue::Reference(value) if value.ends_with(".DIFFERENCE") => {
            DexRuntimeCanvasClipOp::Difference
        }
        _ => DexRuntimeCanvasClipOp::Intersect,
    }
}

fn runtime_matrix_values(runtime: &DexRuntimeState, matrix: &DexValue) -> [f32; 9] {
    let matrix_key = map_identity_key(matrix);

    runtime
        .matrix_values
        .get(&matrix_key)
        .cloned()
        .unwrap_or_else(AndroidMatrixState::identity)
        .values
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct RuntimeCanvasRect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

fn runtime_canvas_bitmap_op_snapshot(
    runtime: &DexRuntimeState,
    bitmap: &DexValue,
    source: &DexValue,
    destination: &DexValue,
    paint: &DexValue,
    depth: usize,
) -> Option<DexRuntimeCanvasOp> {
    let bitmap_width = runtime_bitmap_dimension(runtime, bitmap, "width")?;
    let bitmap_height = runtime_bitmap_dimension(runtime, bitmap, "height")?;
    let destination = runtime_canvas_rect(runtime, destination)?;
    let source = runtime_canvas_source_rect(runtime, source, bitmap_width, bitmap_height)?;
    let ops = runtime_bitmap_canvas_ops_snapshot(runtime, bitmap, depth + 1);
    let encoded_image = runtime
        .bitmap_encoded_images
        .get(&map_identity_key(bitmap))
        .cloned();
    // cite: frameworks-base BaseCanvas.java#drawBitmap
    let (color, blend_mode, color_filter) = if matches!(paint, DexValue::Null | DexValue::Int(0)) {
        (0xff00_0000, DexRuntimeCanvasBlendMode::SrcOver, None)
    } else {
        (
            runtime_paint_color(runtime, paint),
            runtime_paint_blend_mode(runtime, paint),
            runtime_paint_color_filter(runtime, paint)?,
        )
    };

    Some(DexRuntimeCanvasOp::Bitmap {
        left: destination.left,
        top: destination.top,
        right: destination.right,
        bottom: destination.bottom,
        source_left: source.left,
        source_top: source.top,
        source_right: source.right,
        source_bottom: source.bottom,
        bitmap_width,
        bitmap_height,
        color,
        blend_mode,
        color_filter,
        alpha_mask: runtime_bitmap_is_alpha_mask(runtime, bitmap),
        encoded_image,
        ops,
    })
}

fn runtime_bitmap_is_alpha_mask(runtime: &DexRuntimeState, bitmap: &DexValue) -> bool {
    // cite: frameworks-base Bitmap.java#ALPHA_8
    matches!(
        runtime_instance_field(
            runtime,
            bitmap,
            &bitmap_object_field("config", "Landroid/graphics/Bitmap$Config;"),
        ),
        Some(DexValue::Reference(value)) if value.ends_with(".ALPHA_8")
    )
}

fn runtime_paint_color_filter(
    runtime: &DexRuntimeState,
    paint: &DexValue,
) -> Option<Option<DexRuntimeCanvasColorFilter>> {
    // cite: frameworks-base Paint.java#setColorFilter
    let filter = match runtime_instance_field(runtime, paint, &paint_color_filter_field()) {
        None | Some(DexValue::Null) | Some(DexValue::Int(0)) => return Some(None),
        Some(filter) => filter.clone(),
    };
    // cite: frameworks-base BlendModeColorFilter.java#BlendModeColorFilter
    let color = rt_int_field!(runtime, &filter, &blend_mode_color_filter_color_field())? as u32;
    let DexValue::Reference(mode) =
        runtime_instance_field(runtime, &filter, &blend_mode_color_filter_mode_field())?
    else {
        return None;
    };

    Some(Some(DexRuntimeCanvasColorFilter {
        color,
        blend_mode: runtime_canvas_blend_mode_value(mode)?,
    }))
}

fn runtime_bitmap_dimension(
    runtime: &DexRuntimeState,
    bitmap: &DexValue,
    name: &str,
) -> Option<u32> {
    let value = runtime_instance_int_field(runtime, bitmap, &bitmap_int_field(name))?;
    u32::try_from(value).ok().filter(|value| *value > 0)
}

fn runtime_canvas_source_rect(
    runtime: &DexRuntimeState,
    source: &DexValue,
    bitmap_width: u32,
    bitmap_height: u32,
) -> Option<RuntimeCanvasRect> {
    if matches!(source, DexValue::Null) {
        return Some(RuntimeCanvasRect {
            left: 0.0,
            top: 0.0,
            right: bitmap_width as f32,
            bottom: bitmap_height as f32,
        });
    }

    runtime_canvas_rect(runtime, source)
}

fn runtime_canvas_rect(runtime: &DexRuntimeState, rect: &DexValue) -> Option<RuntimeCanvasRect> {
    Some(RuntimeCanvasRect {
        left: runtime_canvas_rect_field(runtime, rect, "left")?,
        top: runtime_canvas_rect_field(runtime, rect, "top")?,
        right: runtime_canvas_rect_field(runtime, rect, "right")?,
        bottom: runtime_canvas_rect_field(runtime, rect, "bottom")?,
    })
}

fn runtime_canvas_rect_field(
    runtime: &DexRuntimeState,
    rect: &DexValue,
    name: &str,
) -> Option<f32> {
    if let Some(value) = runtime_instance_int_field(runtime, rect, &rect_int_field(name)) {
        return Some(value as f32);
    }

    runtime_instance_float_field(runtime, rect, &rectf_float_field(name))
}

fn runtime_bitmap_canvas_ops_snapshot(
    runtime: &DexRuntimeState,
    bitmap: &DexValue,
    depth: usize,
) -> Vec<DexRuntimeCanvasOp> {
    if depth > 16 {
        return Vec::new();
    }

    let Some(canvas_key) = runtime_bitmap_canvas_key(runtime, bitmap) else {
        return Vec::new();
    };
    let Some(ops) = runtime.canvas_draw_ops.get(&canvas_key) else {
        return Vec::new();
    };

    runtime_canvas_ops_from_slice(runtime, ops, depth)
}

fn runtime_bitmap_canvas_key(runtime: &DexRuntimeState, bitmap: &DexValue) -> Option<String> {
    let bitmap_key = map_identity_key(bitmap);
    let canvas_bitmap_signature = canvas_bitmap_field().display_signature();

    thread_local! {
        static BITMAP_OWNER_MEMO: std::cell::RefCell<HashMap<String, String>> =
            std::cell::RefCell::new(HashMap::new());
    }
    if let Some(owner) = BITMAP_OWNER_MEMO.with(|m| m.borrow().get(&bitmap_key).cloned()) {
        let still_bound = runtime
            .field(&owner, &canvas_bitmap_signature)
            .map(map_identity_key)
            == Some(bitmap_key.clone());
        if still_bound {
            return Some(owner);
        }
        BITMAP_OWNER_MEMO.with(|m| {
            m.borrow_mut().remove(&bitmap_key);
        });
    }

    let found = runtime
        .iter_field_owners()
        .find_map(|(owner_key, object_fields)| {
            let value = object_fields.get(&canvas_bitmap_signature)?;
            (map_identity_key(value) == bitmap_key).then(|| owner_key.to_string())
        });
    if let Some(owner) = &found {
        BITMAP_OWNER_MEMO.with(|m| {
            m.borrow_mut().insert(bitmap_key.clone(), owner.clone());
        });
    }
    found
}

fn runtime_paint_color(runtime: &DexRuntimeState, paint: &DexValue) -> u32 {
    let color =
        rt_int_field!(runtime, paint, &paint_color_field()).unwrap_or(0xff00_0000u32 as i32) as u32;
    let alpha = rt_int_field!(runtime, paint, &paint_alpha_field())
        .unwrap_or(((color >> 24) & 0xff) as i32)
        .clamp(0, 255) as u32;

    (color & 0x00ff_ffff) | (alpha << 24)
}

fn runtime_paint_blend_mode(
    runtime: &DexRuntimeState,
    paint: &DexValue,
) -> DexRuntimeCanvasBlendMode {
    // cite: frameworks-base Paint.java#setBlendMode
    match runtime_instance_field(runtime, paint, &paint_blend_mode_field()) {
        Some(DexValue::Reference(value)) => {
            runtime_canvas_blend_mode_value(value).unwrap_or(DexRuntimeCanvasBlendMode::SrcOver)
        }
        _ => DexRuntimeCanvasBlendMode::SrcOver,
    }
}

fn runtime_canvas_blend_mode_value(value: &str) -> Option<DexRuntimeCanvasBlendMode> {
    // cite: frameworks-base BlendMode.java#CLEAR, SRC, DST, SRC_OVER, DST_OVER, SRC_IN, DST_IN, SRC_OUT, DST_OUT, SRC_ATOP, DST_ATOP, XOR, PLUS, MODULATE, SCREEN, OVERLAY, DARKEN, LIGHTEN, COLOR_DODGE, COLOR_BURN, HARD_LIGHT, SOFT_LIGHT, DIFFERENCE, EXCLUSION, MULTIPLY, HUE, SATURATION, COLOR, LUMINOSITY
    Some(match value.rsplit('.').next().unwrap_or(value) {
        "CLEAR" => DexRuntimeCanvasBlendMode::Clear,
        "SRC" => DexRuntimeCanvasBlendMode::Src,
        "DST" => DexRuntimeCanvasBlendMode::Dst,
        "SRC_OVER" => DexRuntimeCanvasBlendMode::SrcOver,
        "DST_OVER" => DexRuntimeCanvasBlendMode::DstOver,
        "SRC_IN" => DexRuntimeCanvasBlendMode::SrcIn,
        "DST_IN" => DexRuntimeCanvasBlendMode::DstIn,
        "SRC_OUT" => DexRuntimeCanvasBlendMode::SrcOut,
        "DST_OUT" => DexRuntimeCanvasBlendMode::DstOut,
        "SRC_ATOP" => DexRuntimeCanvasBlendMode::SrcAtop,
        "DST_ATOP" => DexRuntimeCanvasBlendMode::DstAtop,
        "XOR" => DexRuntimeCanvasBlendMode::Xor,
        "PLUS" => DexRuntimeCanvasBlendMode::Plus,
        "MODULATE" => DexRuntimeCanvasBlendMode::Modulate,
        "SCREEN" => DexRuntimeCanvasBlendMode::Screen,
        "OVERLAY" => DexRuntimeCanvasBlendMode::Overlay,
        "DARKEN" => DexRuntimeCanvasBlendMode::Darken,
        "LIGHTEN" => DexRuntimeCanvasBlendMode::Lighten,
        "COLOR_DODGE" => DexRuntimeCanvasBlendMode::ColorDodge,
        "COLOR_BURN" => DexRuntimeCanvasBlendMode::ColorBurn,
        "HARD_LIGHT" => DexRuntimeCanvasBlendMode::HardLight,
        "SOFT_LIGHT" => DexRuntimeCanvasBlendMode::SoftLight,
        "DIFFERENCE" => DexRuntimeCanvasBlendMode::Difference,
        "EXCLUSION" => DexRuntimeCanvasBlendMode::Exclusion,
        "MULTIPLY" => DexRuntimeCanvasBlendMode::Multiply,
        "HUE" => DexRuntimeCanvasBlendMode::Hue,
        "SATURATION" => DexRuntimeCanvasBlendMode::Saturation,
        "COLOR" => DexRuntimeCanvasBlendMode::Color,
        "LUMINOSITY" => DexRuntimeCanvasBlendMode::Luminosity,
        _ => return None,
    })
}

fn runtime_paint_style(runtime: &DexRuntimeState, paint: &DexValue) -> DexRuntimeCanvasPaintStyle {
    match runtime_instance_field(runtime, paint, &paint_style_field()) {
        Some(DexValue::Reference(value)) if value.ends_with(".STROKE") => {
            DexRuntimeCanvasPaintStyle::Stroke
        }
        Some(DexValue::Reference(value)) if value.ends_with(".FILL_AND_STROKE") => {
            DexRuntimeCanvasPaintStyle::FillAndStroke
        }
        _ => DexRuntimeCanvasPaintStyle::Fill,
    }
}

fn runtime_paint_stroke_width(runtime: &DexRuntimeState, paint: &DexValue) -> f32 {
    runtime_instance_float_field(runtime, paint, &paint_stroke_width_field()).unwrap_or(0.0)
}

fn runtime_paint_stroke_cap(
    runtime: &DexRuntimeState,
    paint: &DexValue,
) -> DexRuntimeCanvasStrokeCap {
    match runtime_instance_field(runtime, paint, &paint_stroke_cap_field()) {
        Some(DexValue::Reference(value)) if value.ends_with(".ROUND") => {
            DexRuntimeCanvasStrokeCap::Round
        }
        Some(DexValue::Reference(value)) if value.ends_with(".SQUARE") => {
            DexRuntimeCanvasStrokeCap::Square
        }
        _ => DexRuntimeCanvasStrokeCap::Butt,
    }
}

fn runtime_paint_text_size(runtime: &DexRuntimeState, paint: &DexValue) -> f32 {
    runtime_instance_float_field(runtime, paint, &paint_text_size_field()).unwrap_or(16.0)
}

fn runtime_instance_int_field(
    runtime: &DexRuntimeState,
    object: &DexValue,
    field: &DexFieldId,
) -> Option<i32> {
    let DexValue::Int(value) = runtime_instance_field(runtime, object, field)? else {
        return None;
    };

    Some(*value)
}

fn runtime_instance_float_field(
    runtime: &DexRuntimeState,
    object: &DexValue,
    field: &DexFieldId,
) -> Option<f32> {
    match runtime_instance_field(runtime, object, field)? {
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

fn runtime_instance_field<'a>(
    runtime: &'a DexRuntimeState,
    object: &DexValue,
    field: &DexFieldId,
) -> Option<&'a DexValue> {
    runtime.value_field(object, &field.display_signature())
}

fn runtime_char_sequence_text(runtime: &DexRuntimeState, value: &DexValue) -> Option<String> {
    match value {
        DexValue::String(text) => Some(text.clone().to_string()),
        DexValue::Object(_) | DexValue::Reference(_) => {
            let key = map_identity_key(value);
            runtime
                .string_values
                .get(&key)
                .or_else(|| runtime.string_builder_values.get(&key))
                .cloned()
        }
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_) => None,
    }
}

struct ViewFieldSnapshot {
    texts: Vec<DexViewText>,
    cursors: Vec<DexViewTextCursor>,
    images: Vec<DexViewImage>,
    visibilities: Vec<DexViewVisibility>,
}

fn view_field_snapshot(
    runtime: &DexRuntimeState,
    focused_view_key: Option<&str>,
    active_content_layout_ids: &[i32],
) -> ViewFieldSnapshot {
    let text_field = text_view_text_field().display_signature();
    let visibility_field = view_visibility_field().display_signature();
    let drawable_field = image_view_drawable_field().display_signature();
    let start_field = text_view_selection_start_field().display_signature();
    let end_field = text_view_selection_end_field().display_signature();
    let cursor_visible_field = text_view_cursor_visible_field().display_signature();

    let mut texts: BTreeMap<i32, (u8, String)> = BTreeMap::new();
    let mut visibilities: BTreeMap<i32, (u8, i32)> = BTreeMap::new();
    let mut images: BTreeMap<i32, (u8, u32)> = BTreeMap::new();
    let mut cursors = Vec::new();

    let mut slot_cache: Option<(u32, [Option<u16>; 6])> = None;
    let signatures = [
        text_field.as_str(),
        visibility_field.as_str(),
        drawable_field.as_str(),
        start_field.as_str(),
        end_field.as_str(),
        cursor_visible_field.as_str(),
    ];

    for (reference, object_fields) in runtime.iter_reference_fields() {
        let Some(view_id) = view_id_from_reference(reference) else {
            continue;
        };
        let priority = view_reference_priority(reference, active_content_layout_ids);

        let slots = match object_fields.layout_index() {
            Some(layout) => match &slot_cache {
                Some((cached_layout, slots)) if *cached_layout == layout => *slots,
                _ => {
                    let slots = signatures.map(|signature| object_fields.slot_of(signature));
                    slot_cache = Some((layout, slots));
                    slots
                }
            },
            None => [None; 6],
        };
        let field = |index: usize, signature: &str| -> Option<&DexValue> {
            match slots[index] {
                Some(slot) => object_fields.get_slot(slot),
                None => object_fields.get(signature),
            }
        };

        if let Some(DexValue::String(text)) = field(0, &text_field) {
            match texts.get(&view_id) {
                Some((existing, _)) if *existing > priority => {}
                _ => {
                    texts.insert(view_id, (priority, text.clone().to_string()));
                }
            }
        }

        if let Some(DexValue::Int(visibility)) = field(1, &visibility_field) {
            match visibilities.get(&view_id) {
                Some((existing, _)) if *existing > priority => {}
                _ => {
                    visibilities.insert(view_id, (priority, *visibility));
                }
            }
        }

        if let Some(value) = field(2, &drawable_field) {
            if let Some(drawable_resource_id) =
                drawable_resource_id_from_snapshot_field(runtime, value)
            {
                match images.get(&view_id) {
                    Some((existing, _)) if *existing > priority => {}
                    _ => {
                        images.insert(view_id, (priority, drawable_resource_id));
                    }
                }
            }
        }

        if focused_view_key.is_some_and(|focused| {
            focused
                .strip_prefix("reference:")
                .is_some_and(|rest| rest == reference)
        }) {
            if let Some(DexValue::Int(selection_start)) = field(3, &start_field) {
                let selection_end = field(4, &end_field)
                    .and_then(dex_int_value)
                    .unwrap_or(*selection_start);
                let cursor_visible = field(5, &cursor_visible_field)
                    .and_then(dex_int_value)
                    .unwrap_or(1)
                    != 0;
                cursors.push(DexViewTextCursor {
                    view_id,
                    selection_start: *selection_start,
                    selection_end,
                    cursor_visible,
                });
            }
        }
    }

    cursors.sort_by_key(|cursor| cursor.view_id);

    ViewFieldSnapshot {
        texts: texts
            .into_iter()
            .map(|(view_id, (_, text))| DexViewText { view_id, text })
            .collect(),
        cursors,
        images: images
            .into_iter()
            .map(|(view_id, (_, drawable_resource_id))| DexViewImage {
                view_id,
                drawable_resource_id,
            })
            .collect(),
        visibilities: visibilities
            .into_iter()
            .map(|(view_id, (_, visibility))| DexViewVisibility {
                view_id,
                visibility,
            })
            .collect(),
    }
}

// cite: frameworks-base ActivityThread.java#performLaunchActivity
fn view_reference_priority(reference: &str, active_content_layout_ids: &[i32]) -> u8 {
    if reference.starts_with("Landroid/view/View;:id/") {
        return 0;
    }

    match layout_id_from_reference_str(reference) {
        Some(layout_id) if active_content_layout_ids.contains(&layout_id) => 2,
        _ => 1,
    }
}

fn dex_int_value(value: &DexValue) -> Option<i32> {
    let DexValue::Int(value) = value else {
        return None;
    };

    Some(*value)
}

pub(in crate::dex_run) fn drawable_resource_id_from_value(value: &DexValue) -> Option<u32> {
    let DexValue::Reference(reference) = value else {
        return None;
    };
    let resource_id = reference.strip_prefix("Landroid/graphics/drawable/Drawable;:resource:")?;
    let resource_id = resource_id.parse::<i32>().ok()?;

    u32::try_from(resource_id).ok()
}

// cite: frameworks-base BitmapFactory.java#decodeResource
// cite: frameworks-base BitmapDrawable.java#BitmapDrawable
fn drawable_resource_id_from_snapshot_field(
    runtime: &DexRuntimeState,
    value: &DexValue,
) -> Option<u32> {
    match value {
        DexValue::Reference(_) => drawable_resource_id_from_value(value),
        DexValue::Object(object) => {
            let signature = bitmap_drawable_resource_id_field().display_signature();
            match runtime
                .instance_fields
                .object_field(object.id, &signature)?
            {
                DexValue::Int(resource_id) => u32::try_from(*resource_id).ok(),
                _ => None,
            }
        }
        _ => None,
    }
}

fn options_menu_snapshot(items: &[RuntimeMenuItem]) -> Vec<DexMenuItem> {
    items
        .iter()
        .filter(|item| item.visible)
        .map(|item| DexMenuItem {
            item_id: item.item_id,
            title: item.title.clone(),
            icon_resource: item.icon_resource.and_then(|id| u32::try_from(id).ok()),
            show_as_action: item.show_as_action,
            enabled: item.enabled,
            visible: item.visible,
        })
        .collect()
}

fn activity_overlays_snapshot(runtime: &DexRuntimeState) -> Vec<DexActivityOverlay> {
    runtime
        .active_menu_overlays
        .iter()
        .map(|overlay| DexActivityOverlay {
            anchor_view_id: overlay.anchor_view_id,
            items: overlay
                .items
                .iter()
                .map(|item| DexActivityOverlayItem {
                    item_id: item.item_id,
                    text: item.title.clone(),
                    enabled: item.enabled,
                })
                .collect(),
        })
        .collect()
}

fn dialog_overlays_snapshot(runtime: &DexRuntimeState) -> Vec<DexDialogOverlay> {
    runtime
        .active_dialog_overlays
        .iter()
        .map(|overlay| DexDialogOverlay {
            title: overlay.title.clone(),
            content_layouts: overlay.content_layouts.clone(),
            items: overlay
                .items
                .iter()
                .map(|item| DexDialogItem {
                    text: item.text.clone(),
                    enabled: item.enabled,
                    selected: item.selected,
                })
                .chain(overlay.input_value.iter().map(|value| DexDialogItem {
                    text: format!("{value}\u{2038}"),
                    enabled: true,
                    selected: false,
                }))
                .collect(),
            buttons: overlay
                .buttons
                .iter()
                .map(|button| DexDialogButton {
                    text: button.text.clone(),
                    enabled: button.enabled,
                })
                .collect(),
        })
        .collect()
}

fn adapter_rows_snapshot(adapter_rows: &HashMap<i32, DexAdapterRows>) -> Vec<DexAdapterRows> {
    let mut snapshot: Vec<DexAdapterRows> = adapter_rows.values().cloned().collect();
    snapshot.sort_by_key(|rows| rows.adapter_view_id);
    snapshot
}

fn click_listener_snapshot(listeners: &HashMap<String, DexValue>) -> Vec<DexViewClickListener> {
    let mut snapshot = Vec::new();

    for (view_key, listener) in listeners {
        let Some(view_id) = view_id_from_identity_key(view_key) else {
            continue;
        };

        snapshot.push(DexViewClickListener {
            view_id,
            listener: listener.to_string(),
        });
    }

    snapshot.sort_by_key(|listener| listener.view_id);
    snapshot
}

fn declared_on_click_snapshot(
    handlers: &HashMap<String, String>,
) -> Vec<DexDeclaredOnClickHandler> {
    let mut snapshot = Vec::new();

    for (view_key, method_name) in handlers {
        let Some(view_id) = view_id_from_identity_key(view_key) else {
            continue;
        };

        snapshot.push(DexDeclaredOnClickHandler {
            view_id,
            method_name: method_name.clone(),
        });
    }

    snapshot.sort_by_key(|handler| handler.view_id);
    snapshot
}

fn view_id_from_identity_key(value: &str) -> Option<i32> {
    view_id_from_reference(value.strip_prefix("reference:")?)
}

fn view_id_from_reference(reference: &str) -> Option<i32> {
    if let Some((descriptor, view_id)) = reference.rsplit_once(":id/") {
        if reference_descriptor(descriptor) == Some(descriptor) {
            return view_id.parse().ok();
        }
    }
    let (head, view_id) = reference.rsplit_once("/id/")?;
    if !head.contains(":layout/") {
        return None;
    }
    view_id.parse().ok()
}
