use super::super::*;

const ANDROID_FEATURE_WATCH: &str = "android.hardware.type.watch";

impl DexProgram {
    pub(super) fn invoke_android_content_storage_builtin(
        &self,
        method: &DexMethodId,
        invocation: Option<&DexInvocation>,
    ) -> Option<DexExecutionResult> {
        if method.class_descriptor == "Landroid/content/ContentValues;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/ContentValues;"
            && method.name == "put"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;Ljava/lang/String;)V")
        {
            let invocation = invocation?;
            self.write_content_values_string(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/ContentValues;"
            && method.name == "put"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;Ljava/lang/Long;)V")
        {
            let invocation = invocation?;
            self.write_content_values_long(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "getFlags"
            && method.proto.descriptor_is("()I")
        {
            return Some(DexExecutionResult::Int(0));
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "putIntegerArrayListExtra"
            && method.proto.descriptor()
                == "(Ljava/lang/String;Ljava/util/ArrayList;)Landroid/content/Intent;"
        {
            let invocation = invocation?;
            let intent = self.put_intent_extra(invocation)?;

            return Some(DexExecutionResult::Value(intent));
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "getIntegerArrayListExtra"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)Ljava/util/ArrayList;")
        {
            let invocation = invocation?;
            let value = self.get_intent_extra(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "<init>"
            && method
                .proto
                .descriptor_is("(Landroid/content/Context;Ljava/lang/Class;)V")
        {
            let invocation = invocation?;
            self.initialize_intent_for_class(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "<init>"
            && method.proto.descriptor_is("(Ljava/lang/String;)V")
        {
            let invocation = invocation?;
            self.initialize_intent_for_action(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "<init>"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;Landroid/net/Uri;)V")
        {
            let invocation = invocation?;
            self.initialize_intent_for_action_and_uri(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "getData"
            && method.proto.descriptor_is("()Landroid/net/Uri;")
        {
            let invocation = invocation?;
            let value = self.intent_data(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "setData"
            && method
                .proto
                .descriptor_is("(Landroid/net/Uri;)Landroid/content/Intent;")
        {
            let invocation = invocation?;
            let value = self.set_intent_data(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base Intent.java#setAction
        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "setAction"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)Landroid/content/Intent;")
        {
            let invocation = invocation?;
            let value = self.set_intent_action(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "getAction"
            && method.proto.descriptor_is("()Ljava/lang/String;")
        {
            let invocation = invocation?;
            let value = self.intent_action(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base Intent.java#setDataAndType
        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "setDataAndType"
            && method.proto.descriptor()
                == "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;"
        {
            let invocation = invocation?;
            let value = self.set_intent_data_and_type(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base Intent.java#getType, mType
        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "getType"
            && method.proto.descriptor_is("()Ljava/lang/String;")
        {
            let invocation = invocation?;
            if !dex_value_is_object_like(invocation.arguments.first()?) {
                return None;
            }

            return Some(DexExecutionResult::Value(
                self.read_stored_instance_field(
                    &invocation.arguments[0],
                    &DexFieldId {
                        class_descriptor: "Landroid/content/Intent;".to_owned(),
                        type_descriptor: "Ljava/lang/String;".to_owned(),
                        name: "mType".to_owned(),
                    },
                )
                .unwrap_or(DexValue::Null),
            ));
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "getType"
            && method.proto.descriptor_is("()Ljava/lang/String;")
        {
            let invocation = invocation?;
            let value = self.intent_type(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Intent;"
            && method.name == "getExtras"
            && method.proto.descriptor_is("()Landroid/os/Bundle;")
        {
            return Some(DexExecutionResult::Value(DexValue::Null));
        }

        if method.class_descriptor == "Landroid/net/Uri;"
            && method.name == "parse"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)Landroid/net/Uri;")
        {
            let invocation = invocation?;
            let value = self.parse_uri(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base Uri.java#getScheme, getQuery, getFragment, getHost, getPath
        if method.class_descriptor == "Landroid/net/Uri;"
            && method.proto.descriptor_is("()Ljava/lang/String;")
            && matches!(
                method.name.as_str(),
                "getScheme"
                    | "getQuery"
                    | "getFragment"
                    | "getHost"
                    | "getAuthority"
                    | "getPath"
                    | "getLastPathSegment"
            )
        {
            let invocation = invocation?;
            let component = match method.name.as_str() {
                "getScheme" => "scheme",
                "getQuery" => "query",
                "getFragment" => "fragment",
                "getHost" => "host",
                "getAuthority" => "authority",
                "getPath" => "path",
                "getLastPathSegment" => "lastPathSegment",
                _ => return None,
            };
            let value = self.uri_component(invocation, component)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base Uri.java#getPathSegments
        if method.class_descriptor == "Landroid/net/Uri;"
            && method.name == "getPathSegments"
            && method.proto.descriptor_is("()Ljava/util/List;")
        {
            let invocation = invocation?;
            let value = self.uri_path_segments(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        // cite: frameworks-base Uri.java#getQueryParameterNames
        if method.class_descriptor == "Landroid/net/Uri;"
            && method.name == "getQueryParameterNames"
            && method.proto.descriptor_is("()Ljava/util/Set;")
        {
            let invocation = invocation?;
            let pairs = self.uri_query_pairs(invocation)?;
            let mut names = Vec::new();
            for (name, _) in pairs {
                let value = DexValue::String(std::rc::Rc::from(name));
                if !names.contains(&value) {
                    names.push(value);
                }
            }
            let set = self.allocate_object("Ljava/util/LinkedHashSet;".to_owned());
            self.runtime
                .borrow_mut()
                .list_entries
                .insert(map_identity_key(&set), names);

            return Some(DexExecutionResult::Value(set));
        }

        // cite: frameworks-base Uri.java#getQueryParameters, getQueryParameter
        if method.class_descriptor == "Landroid/net/Uri;"
            && method.name == "getQueryParameters"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)Ljava/util/List;")
        {
            let invocation = invocation?;
            let DexValue::String(wanted) = invocation.arguments.get(1)? else {
                return None;
            };
            let pairs = self.uri_query_pairs(invocation)?;
            let values: Vec<DexValue> = pairs
                .into_iter()
                .filter(|(name, _)| name.as_str() == wanted.as_ref())
                .map(|(_, value)| DexValue::String(std::rc::Rc::from(value)))
                .collect();
            let list = self.allocate_object("Ljava/util/ArrayList;".to_owned());
            self.runtime
                .borrow_mut()
                .list_entries
                .insert(map_identity_key(&list), values);

            return Some(DexExecutionResult::Value(list));
        }

        if method.class_descriptor == "Landroid/net/Uri;"
            && method.name == "getQueryParameter"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)Ljava/lang/String;")
        {
            let invocation = invocation?;
            let DexValue::String(wanted) = invocation.arguments.get(1)? else {
                return None;
            };
            let pairs = self.uri_query_pairs(invocation)?;
            let value = pairs
                .into_iter()
                .find(|(name, _)| name.as_str() == wanted.as_ref())
                .map(|(_, value)| DexValue::String(std::rc::Rc::from(value)))
                .unwrap_or(DexValue::Null);

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/net/Uri;"
            && method.name == "toString"
            && method.proto.descriptor_is("()Ljava/lang/String;")
        {
            let invocation = invocation?;
            let value = self.uri_to_string(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/IntentFilter;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/IntentFilter;"
            && method.name == "addAction"
            && method.proto.descriptor_is("(Ljava/lang/String;)V")
        {
            let invocation = invocation?;
            self.add_intent_filter_action(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/IntentFilter;"
            && method.name == "countActions"
            && method.proto.descriptor_is("()I")
        {
            let invocation = invocation?;
            let value = self.intent_filter_action_count(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/content/BroadcastReceiver;"
            && method.name == "<init>"
            && method.proto.descriptor_is("()V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/ComponentName;"
            && method.name == "<init>"
            && method
                .proto
                .descriptor_is("(Landroid/content/Context;Ljava/lang/Class;)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getApplicationContext"
            && method.proto.descriptor_is("()Landroid/content/Context;")
        {
            return Some(DexExecutionResult::Value(self.application_value()));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getCacheDir"
            && method.proto.descriptor_is("()Ljava/io/File;")
        {
            let invocation = invocation?;
            let value = self.context_private_directory_file(invocation, "cache")?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getFilesDir"
            && method.proto.descriptor_is("()Ljava/io/File;")
        {
            let invocation = invocation?;
            let value = self.context_private_directory_file(invocation, "files")?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getExternalFilesDirs"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)[Ljava/io/File;")
        {
            let invocation = invocation?;
            let value = self.context_external_files_dirs(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getDataDir"
            && method.proto.descriptor_is("()Ljava/io/File;")
        {
            let invocation = invocation?;
            let value = self.context_data_dir_file(invocation)?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getNoBackupFilesDir"
            && method.proto.descriptor_is("()Ljava/io/File;")
        {
            let invocation = invocation?;
            let value = self.context_private_directory_file(invocation, "no_backup")?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getCodeCacheDir"
            && method.proto.descriptor_is("()Ljava/io/File;")
        {
            let invocation = invocation?;
            let value = self.context_private_directory_file(invocation, "code_cache")?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getSharedPreferencesPath"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)Ljava/io/File;")
        {
            let invocation = invocation?;
            return self.context_shared_preferences_path_file(invocation);
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "createDeviceProtectedStorageContext"
            && method.proto.descriptor_is("()Landroid/content/Context;")
        {
            let invocation = invocation?;
            let context = context_receiver(invocation)?;
            let value = self.create_private_storage_context(
                context,
                crate::guest_filesystem::PrivateStorageKind::DeviceProtected,
            )?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "createCredentialProtectedStorageContext"
            && method.proto.descriptor_is("()Landroid/content/Context;")
        {
            let invocation = invocation?;
            let context = context_receiver(invocation)?;
            let value = self.create_private_storage_context(
                context,
                crate::guest_filesystem::PrivateStorageKind::CredentialProtected,
            )?;

            return Some(DexExecutionResult::Value(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "isDeviceProtectedStorage"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let context = context_receiver(invocation)?;
            let value = self.context_is_private_storage(
                context,
                crate::guest_filesystem::PrivateStorageKind::DeviceProtected,
            )?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "isCredentialProtectedStorage"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let context = context_receiver(invocation)?;
            let value = self.context_is_private_storage(
                context,
                crate::guest_filesystem::PrivateStorageKind::CredentialProtected,
            )?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "getDatabasePath"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;)Ljava/io/File;")
        {
            let invocation = invocation?;
            return self.context_database_path_file(invocation);
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "registerComponentCallbacks"
            && method
                .proto
                .descriptor_is("(Landroid/content/ComponentCallbacks;)V")
        {
            let invocation = invocation?;
            self.register_component_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "unregisterComponentCallbacks"
            && method
                .proto
                .descriptor_is("(Landroid/content/ComponentCallbacks;)V")
        {
            let invocation = invocation?;
            self.unregister_component_callback(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "unregisterReceiver"
            && method
                .proto
                .descriptor_is("(Landroid/content/BroadcastReceiver;)V")
        {
            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/Context;"
            && method.name == "registerReceiver"
            && method.proto.descriptor()
                == "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;"
        {
            return Some(DexExecutionResult::Value(DexValue::Null));
        }

        if method.class_descriptor == "Landroid/content/pm/PackageManager;"
            && method.name == "hasSystemFeature"
            && method.proto.descriptor_is("(Ljava/lang/String;)Z")
        {
            let value = invocation
                .map(|invocation| self.package_manager_has_system_feature(invocation))
                .unwrap_or(0);

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/content/pm/PackageManager;"
            && method.name == "checkPermission"
            && method
                .proto
                .descriptor_is("(Ljava/lang/String;Ljava/lang/String;)I")
        {
            let invocation = invocation?;
            let value = self.package_manager_check_permission(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/content/pm/PackageManager;"
            && method.name == "getActivityInfo"
            && method.proto.descriptor()
                == "(Landroid/content/ComponentName;I)Landroid/content/pm/ActivityInfo;"
        {
            return Some(DexExecutionResult::Value(DexValue::Reference(
                std::rc::Rc::from("Landroid/content/pm/ActivityInfo;.bootstrap"),
            )));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences;"
            && method.name == "contains"
            && method.proto.descriptor_is("(Ljava/lang/String;)Z")
        {
            let invocation = invocation?;
            let value = self.shared_preferences_contains(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences;"
            && method.name == "getBoolean"
            && method.proto.descriptor_is("(Ljava/lang/String;Z)Z")
        {
            let invocation = invocation?;
            let default_value = self.shared_preferences_get_boolean(invocation)?;

            return Some(DexExecutionResult::Int(default_value));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences;"
            && method.name == "getString"
            && method.proto.descriptor()
                == "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"
        {
            let invocation = invocation?;
            let default_value = self.shared_preferences_get_string(invocation)?;

            return Some(DexExecutionResult::Value(default_value));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences;"
            && method.name == "getInt"
            && method.proto.descriptor_is("(Ljava/lang/String;I)I")
        {
            let invocation = invocation?;
            let default_value = self.shared_preferences_get_int(invocation)?;

            return Some(DexExecutionResult::Int(default_value));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences;"
            && method.name == "getLong"
            && method.proto.descriptor_is("(Ljava/lang/String;J)J")
        {
            let invocation = invocation?;
            let default_value = self.shared_preferences_get_long(invocation)?;

            return Some(DexExecutionResult::Value(DexValue::Long(default_value)));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences;"
            && method.name == "edit"
            && method
                .proto
                .descriptor_is("()Landroid/content/SharedPreferences$Editor;")
        {
            let invocation = invocation?;
            let editor = self.shared_preferences_edit(invocation)?;

            return Some(DexExecutionResult::Value(editor));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "putBoolean"
            && method.proto.descriptor()
                == "(Ljava/lang/String;Z)Landroid/content/SharedPreferences$Editor;"
        {
            let invocation = invocation?;
            let editor = self.shared_preferences_editor_put_boolean(invocation)?;

            return Some(DexExecutionResult::Value(editor));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "putString"
            && method.proto.descriptor()
                == "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;"
        {
            let invocation = invocation?;
            let editor = self.shared_preferences_editor_put_string(invocation)?;

            return Some(DexExecutionResult::Value(editor));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "putInt"
            && method.proto.descriptor()
                == "(Ljava/lang/String;I)Landroid/content/SharedPreferences$Editor;"
        {
            let invocation = invocation?;
            let editor = self.shared_preferences_editor_put_int(invocation)?;

            return Some(DexExecutionResult::Value(editor));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "putLong"
            && method.proto.descriptor()
                == "(Ljava/lang/String;J)Landroid/content/SharedPreferences$Editor;"
        {
            let invocation = invocation?;
            let editor = self.shared_preferences_editor_put_long(invocation)?;

            return Some(DexExecutionResult::Value(editor));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "remove"
            && method.proto.descriptor()
                == "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;"
        {
            let invocation = invocation?;
            let editor = self.shared_preferences_editor_remove(invocation)?;

            return Some(DexExecutionResult::Value(editor));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "clear"
            && method
                .proto
                .descriptor_is("()Landroid/content/SharedPreferences$Editor;")
        {
            let invocation = invocation?;
            let editor = self.shared_preferences_editor_clear(invocation)?;

            return Some(DexExecutionResult::Value(editor));
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "apply"
            && method.proto.descriptor_is("()V")
        {
            let invocation = invocation?;
            self.shared_preferences_editor_apply(invocation)?;

            return Some(DexExecutionResult::Void);
        }

        if method.class_descriptor == "Landroid/content/SharedPreferences$Editor;"
            && method.name == "commit"
            && method.proto.descriptor_is("()Z")
        {
            let invocation = invocation?;
            let value = self.shared_preferences_editor_commit(invocation)?;

            return Some(DexExecutionResult::Int(value));
        }

        None
    }

    fn context_data_dir_file(&self, invocation: &DexInvocation) -> Option<DexValue> {
        // cite: frameworks-base ContextImpl.java#getDataDir
        let context = context_receiver(invocation)?;
        let storage = self.context_private_storage_kind(context)?;
        let path = self
            .guest_filesystem
            .as_ref()?
            .private_guest_path(storage, "");
        Some(DexValue::Reference(std::rc::Rc::from(format!(
            "Ljava/io/File;:{path}"
        ))))
    }

    fn context_private_directory_file(
        &self,
        invocation: &DexInvocation,
        child: &str,
    ) -> Option<DexValue> {
        // cite: frameworks-base ContextImpl.java#getFilesDir, getNoBackupFilesDir, getCacheDir, getCodeCacheDir, getDataDir
        let context = context_receiver(invocation)?;
        let path = self.ensure_context_private_directory(context, child)?;
        Some(DexValue::Reference(std::rc::Rc::from(format!(
            "Ljava/io/File;:{path}"
        ))))
    }

    // cite: frameworks-base ContextImpl.java#getExternalFilesDirs
    // cite: frameworks-base Environment.java#buildExternalStorageAppFilesDirs, buildPaths
    fn context_external_files_dirs(&self, invocation: &DexInvocation) -> Option<DexValue> {
        let _context = context_receiver(invocation)?;
        match &invocation.arguments[1] {
            DexValue::String(type_name) => {
                self.build_external_files_dirs_array(Some(type_name.as_ref()))
            }
            DexValue::Null | DexValue::Int(0) => self.build_external_files_dirs_array(None),
            _ => None,
        }
    }

    fn build_external_files_dirs_array(&self, type_name: Option<&str>) -> Option<DexValue> {
        let package_name = self.package_name.as_deref()?;
        let mut guest_path = format!(
            "{}/Android/data/{package_name}/files",
            crate::external_storage::GUEST_PRIMARY_EXTERNAL_ROOT
        );
        if let Some(type_name) = type_name {
            guest_path.push('/');
            guest_path.push_str(type_name);
        }

        if let Some(filesystem) = self.guest_filesystem.as_ref() {
            if let Ok(resolved) = filesystem.resolve(
                &guest_path,
                crate::external_storage::ExternalStorageAccess::Write,
                &self.external_storage_policy,
            ) {
                let _ = resolved.mkdir_all();
            }
        }

        Some(self.allocate_filled_array(
            "[Ljava/io/File;".to_owned(),
            vec![DexValue::Reference(std::rc::Rc::from(format!(
                "Ljava/io/File;:{guest_path}"
            )))],
        ))
    }

    fn context_database_path_file(&self, invocation: &DexInvocation) -> Option<DexExecutionResult> {
        if invocation.arguments.len() != 2 {
            return None;
        }
        let context = context_receiver(invocation)?;
        let storage = self.context_private_storage_kind(context)?;

        let DexValue::String(name) = &invocation.arguments[1] else {
            return None;
        };
        if name.is_empty() {
            return Some(DexExecutionResult::Throw(self.allocate_object(
                "Ljava/lang/StringIndexOutOfBoundsException;".to_owned(),
            )));
        }
        let filesystem = self.guest_filesystem.as_ref()?;

        // cite: frameworks-base ContextImpl.java#getDatabasePath, getDatabasesDir, makeFilename
        let path = if name.starts_with('/') {
            let path = crate::guest_filesystem::normalize_guest_file_path(name);
            self.try_create_absolute_database_parent(&path);
            path
        } else {
            if name.contains('/') {
                return Some(DexExecutionResult::Throw(
                    self.allocate_object("Ljava/lang/IllegalArgumentException;".to_owned()),
                ));
            }
            let database_dir = filesystem.private_guest_path(storage, "databases");
            if let Ok(directory) = filesystem.resolve(
                &database_dir,
                crate::external_storage::ExternalStorageAccess::Write,
                &self.external_storage_policy,
            ) {
                let _ = directory.mkdir_all();
            }
            format!("{database_dir}/{name}")
        };

        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from(format!("Ljava/io/File;:{path}")),
        )))
    }

    fn package_manager_has_system_feature(&self, invocation: &DexInvocation) -> i32 {
        let Some(DexValue::String(feature)) = invocation.arguments.get(1) else {
            return 0;
        };

        // cite: frameworks-base PackageManager.java#FEATURE_WATCH
        // cite: frameworks-base UiModeManagerService.java#mWatch
        let watch = self.config.device() == crate::config::WinedroidDevice::Watch;
        i32::from(watch && feature.as_ref() == ANDROID_FEATURE_WATCH)
    }

    fn package_manager_check_permission(&self, invocation: &DexInvocation) -> Option<i32> {
        if invocation.arguments.len() != 3 {
            return None;
        }
        let DexValue::String(permission) = &invocation.arguments[1] else {
            return None;
        };
        let DexValue::String(package_name) = &invocation.arguments[2] else {
            return None;
        };

        // cite: frameworks-base ApplicationPackageManager.java#checkPermission
        // cite: frameworks-base PermissionManager.java#checkPackageNamePermission
        if package_name.as_ref() != self.bootstrap_process_name() {
            return Some(ANDROID_PERMISSION_DENIED);
        }

        let granted = self
            .external_storage_policy
            .permission_granted(permission)
            .unwrap_or(true);

        Some(if granted {
            ANDROID_PERMISSION_GRANTED
        } else {
            ANDROID_PERMISSION_DENIED
        })
    }

    fn context_shared_preferences_path_file(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexExecutionResult> {
        if invocation.arguments.len() != 2 {
            return None;
        }
        let context = context_receiver(invocation)?;
        let DexValue::String(name) = &invocation.arguments[1] else {
            return None;
        };
        if name.contains('/') {
            return Some(DexExecutionResult::Throw(
                self.allocate_object("Ljava/lang/IllegalArgumentException;".to_owned()),
            ));
        }
        let directory = self.ensure_context_private_directory(context, "shared_prefs")?;
        let path = format!("{directory}/{name}.xml");

        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from(format!("Ljava/io/File;:{path}")),
        )))
    }

    fn ensure_context_private_directory(&self, context: &DexValue, child: &str) -> Option<String> {
        let filesystem = self.guest_filesystem.as_ref()?;
        let storage = self.context_private_storage_kind(context)?;
        let guest_path = filesystem.private_guest_path(storage, child);
        if let Ok(path) = filesystem.resolve(
            &guest_path,
            crate::external_storage::ExternalStorageAccess::Write,
            &self.external_storage_policy,
        ) {
            let _ = path.mkdir_all();
        }

        Some(guest_path)
    }

    fn try_create_absolute_database_parent(&self, guest_path: &str) {
        let Some((parent, _)) = guest_path.rsplit_once('/') else {
            return;
        };
        let parent = if parent.is_empty() { "/" } else { parent };
        let Some(filesystem) = self.guest_filesystem.as_ref() else {
            return;
        };
        let Ok(parent) = filesystem.resolve(
            parent,
            crate::external_storage::ExternalStorageAccess::Write,
            &self.external_storage_policy,
        ) else {
            return;
        };
        if !parent
            .metadata()
            .is_ok_and(|metadata| metadata.kind == crate::capability_fs::MetadataKind::Directory)
        {
            let _ = parent.mkdir();
        }
    }
}

fn context_receiver(invocation: &DexInvocation) -> Option<&DexValue> {
    if invocation.arguments.len() != 1 && invocation.arguments.len() != 2 {
        return None;
    }
    let context = invocation.arguments.first()?;
    dex_value_is_object_like(context).then_some(context)
}
