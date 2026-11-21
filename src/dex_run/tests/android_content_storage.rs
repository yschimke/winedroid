use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::*;
use super::helpers::*;

#[test]
fn builtin_context_get_shared_preferences_returns_named_reference() {
    let program = DexProgram::new(Vec::new());
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getSharedPreferences".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLI".to_owned(),
            return_type: "Landroid/content/SharedPreferences;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    };
    let invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap")),
        DexValue::String(std::rc::Rc::from("prefs")),
        DexValue::Int(0),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Landroid/content/SharedPreferences;:ce:7072656673")
        )))
    );
}

#[test]
fn context_get_shared_preferences_rejects_path_separator_names() {
    // cite: frameworks-base ContextImpl.java#getSharedPreferencesPath, makeFilename
    let program = DexProgram::new(Vec::new());
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getSharedPreferences".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLI".to_owned(),
            return_type: "Landroid/content/SharedPreferences;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    };
    let invocation = test_invocation(vec![
        bootstrap_context(),
        DexValue::String(std::rc::Rc::from("../prefs")),
        DexValue::Int(0),
    ]);

    let Some(DexExecutionResult::Throw(exception)) =
        program.invoke_builtin_method(&method, Some(&invocation))
    else {
        panic!("SharedPreferences name with a separator should throw");
    };
    assert!(program.value_matches_type(&exception, "Ljava/lang/IllegalArgumentException;"));
}

#[test]
fn fabricated_shared_preferences_cannot_create_an_editor() {
    let program = DexProgram::new(Vec::new());
    let preferences = DexValue::Reference(std::rc::Rc::from(
        "Landroid/content/SharedPreferences;:fabricated",
    ));

    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_edit_method(),
            Some(&test_invocation(vec![preferences])),
        ),
        None
    );
}

#[test]
fn builtin_context_get_application_context_returns_application() {
    let program = DexProgram::new(Vec::new());
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getApplicationContext".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Landroid/content/Context;".to_owned(),
            parameters: Vec::new(),
        },
    };
    let invocation = test_invocation(vec![DexValue::Reference(std::rc::Rc::from(
        "Landroid/content/Context;.bootstrap",
    ))]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Landroid/app/Application;.bootstrap")
        )))
    );
}

#[test]
fn builtin_context_get_application_context_uses_manifest_application_class() {
    let program = DexProgram::with_application_class_descriptor(
        Vec::new(),
        Some("Lcom/example/App;".to_owned()),
    );
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getApplicationContext".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Landroid/content/Context;".to_owned(),
            parameters: Vec::new(),
        },
    };
    let invocation = test_invocation(vec![DexValue::Reference(std::rc::Rc::from(
        "Landroid/content/Context;.bootstrap",
    ))]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Lcom/example/App;.bootstrap")
        )))
    );
}

#[test]
fn context_impl_get_cache_dir_returns_android_guest_path() {
    let app_dir = test_app_data_dir("context_cache_dir");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir.clone(), shared_dir)
            .expect("program should load guest filesystem");
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getCacheDir".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Ljava/io/File;".to_owned(),
            parameters: Vec::new(),
        },
    };
    let invocation = test_invocation(vec![DexValue::Reference(std::rc::Rc::from(
        "Landroid/content/Context;.bootstrap",
    ))]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Ljava/io/File;:/data/user/0/com.example/cache")
        )))
    );
    assert!(app_dir.join("apps/com.example/data/cache").is_dir());

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn context_impl_get_files_dir_returns_android_guest_path() {
    let app_dir = test_app_data_dir("context_files_dir");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir.clone(), shared_dir)
            .expect("program should load guest filesystem");
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getFilesDir".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Ljava/io/File;".to_owned(),
            parameters: Vec::new(),
        },
    };
    let invocation = test_invocation(vec![DexValue::Reference(std::rc::Rc::from(
        "Landroid/content/Context;.bootstrap",
    ))]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Ljava/io/File;:/data/user/0/com.example/files")
        )))
    );
    assert!(app_dir.join("apps/com.example/data/files").is_dir());

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn package_manager_check_permission_matches_package_name_and_grant_state() {
    // cite: frameworks-base ApplicationPackageManager.java#checkPermission
    // cite: frameworks-base PermissionManager.java#checkPackageNamePermission
    let mut program = DexProgram::new(Vec::new());
    program.external_storage_policy = ExternalStoragePolicy::new(
        34,
        34,
        vec![crate::external_storage::READ_MEDIA_IMAGES.to_owned()],
        crate::external_storage::ExternalStorageRuntimeAccess::new(
            vec![crate::external_storage::READ_MEDIA_IMAGES.to_owned()],
            false,
            false,
        ),
    );
    let method = DexMethodId {
        class_descriptor: "Landroid/content/pm/PackageManager;".to_owned(),
        name: "checkPermission".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ILL".to_owned(),
            return_type: "I".to_owned(),
            parameters: vec![
                "Ljava/lang/String;".to_owned(),
                "Ljava/lang/String;".to_owned(),
            ],
        },
    };
    let result = |permission: &str, package_name: &str| {
        program.invoke_builtin_method(
            &method,
            Some(&test_invocation(vec![
                DexValue::Reference(std::rc::Rc::from("package_manager")),
                DexValue::String(std::rc::Rc::from(permission)),
                DexValue::String(std::rc::Rc::from(package_name)),
            ])),
        )
    };

    assert_eq!(
        result(
            crate::external_storage::READ_MEDIA_IMAGES,
            "winedroid.bootstrap"
        ),
        Some(DexExecutionResult::Int(ANDROID_PERMISSION_GRANTED))
    );
    assert_eq!(
        result(
            crate::external_storage::READ_MEDIA_VIDEO,
            "winedroid.bootstrap"
        ),
        Some(DexExecutionResult::Int(ANDROID_PERMISSION_DENIED))
    );
    assert_eq!(
        result(crate::external_storage::READ_MEDIA_IMAGES, "com.other.app"),
        Some(DexExecutionResult::Int(ANDROID_PERMISSION_DENIED))
    );
    assert_eq!(
        result("android.permission.INTERNET", "winedroid.bootstrap"),
        Some(DexExecutionResult::Int(ANDROID_PERMISSION_GRANTED))
    );
}

#[test]
fn context_impl_get_external_files_dirs_returns_single_primary_volume_entry() {
    // cite: frameworks-base ContextImpl.java#getExternalFilesDirs
    // cite: frameworks-base Environment.java#buildExternalStorageAppFilesDirs, buildPaths
    let app_dir = test_app_data_dir("context_external_files_dirs");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir.clone(), shared_dir)
            .expect("program should load guest filesystem");
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getExternalFilesDirs".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "[Ljava/io/File;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    // cite: art interpreter_common.h#DoInvoke
    let invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap")),
        DexValue::Int(0),
    ]);

    let Some(DexExecutionResult::Value(array)) =
        program.invoke_builtin_method(&method, Some(&invocation))
    else {
        panic!("getExternalFilesDirs should return a File[] value");
    };

    assert_eq!(
        program
            .runtime
            .borrow()
            .array_entries
            .get(&map_identity_key(&array)),
        Some(&vec![DexValue::Reference(std::rc::Rc::from(
            "Ljava/io/File;:/storage/emulated/0/Android/data/com.example/files"
        ))])
    );
    assert!(app_dir
        .join("storage/Android/data/com.example/files")
        .is_dir());

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn context_impl_get_database_path_returns_android_guest_path() {
    let app_dir = test_app_data_dir("context_database_path");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir, shared_dir)
            .expect("program should load guest filesystem");
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getDatabasePath".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "Ljava/io/File;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap")),
        DexValue::String(std::rc::Rc::from("timer.db")),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Ljava/io/File;:/data/user/0/com.example/databases/timer.db")
        )))
    );
    assert!(app_dir.join("apps/com.example/data/databases").is_dir());

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn context_impl_make_filename_rejects_database_names_with_path_separators() {
    let app_dir = test_app_data_dir("context_database_separator");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir, shared_dir)
            .expect("program should load guest filesystem");
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getDatabasePath".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "Ljava/io/File;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap")),
        DexValue::String(std::rc::Rc::from("../outside.db")),
    ]);

    let Some(DexExecutionResult::Throw(exception)) =
        program.invoke_builtin_method(&method, Some(&invocation))
    else {
        panic!("database name with a separator should throw");
    };
    assert!(program.value_matches_type(&exception, "Ljava/lang/IllegalArgumentException;"));

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn context_impl_get_database_path_keeps_an_absolute_name_in_the_guest_namespace() {
    let app_dir = test_app_data_dir("context_database_absolute");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir, shared_dir)
            .expect("program should load guest filesystem");
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getDatabasePath".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "Ljava/io/File;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap")),
        DexValue::String(std::rc::Rc::from("/tmp/host.db")),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Ljava/io/File;:/tmp/host.db")
        )))
    );
    let private_absolute = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap")),
        DexValue::String(std::rc::Rc::from(
            "/data/user/0/com.example/custom/timer.db",
        )),
    ]);
    assert_eq!(
        program.invoke_builtin_method(&method, Some(&private_absolute)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Ljava/io/File;:/data/user/0/com.example/custom/timer.db")
        )))
    );
    assert!(app_dir.join("apps/com.example/data/custom").is_dir());

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn cts_device_protected_context_uses_a_distinct_private_storage_root() {
    // cite: cts ContextTest.java#testCreateDeviceProtectedStorageContext
    // cite: frameworks-base ContextImpl.java#createDeviceProtectedStorageContext, createCredentialProtectedStorageContext, isDeviceProtectedStorage, isCredentialProtectedStorage, getDataDir, getFilesDir, getNoBackupFilesDir, getCacheDir, getCodeCacheDir, getDatabasePath, getDatabasesDir, getSharedPreferencesPath, getPreferencesDir
    // cite: frameworks-base ContextThemeWrapper.java#ContextThemeWrapper
    // cite: frameworks-base ContextWrapper.java#getDataDir
    let app_dir = test_app_data_dir("device_protected_context");
    let data_dir = app_dir.join("apps/com.example/data");
    let device_data_dir = app_dir.join("apps/com.example/data_de");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("credential data dir should be created");
    fs::create_dir_all(&device_data_dir).expect("device data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir.clone(), shared_dir)
            .expect("program should load guest filesystem");
    let credential_context =
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap"));
    let device_context = invoke_context_value(
        &program,
        &context_no_arg_method(
            "createDeviceProtectedStorageContext",
            "Landroid/content/Context;",
        ),
        credential_context.clone(),
    );
    let explicit_credential_context = invoke_context_value(
        &program,
        &context_no_arg_method(
            "createCredentialProtectedStorageContext",
            "Landroid/content/Context;",
        ),
        device_context.clone(),
    );
    let wrapper = program.allocate_object("Landroid/view/ContextThemeWrapper;".to_owned());
    program
        .context_theme_wrapper_init(&test_invocation(vec![
            wrapper.clone(),
            device_context.clone(),
            DexValue::Int(0),
        ]))
        .expect("context wrapper should retain its base Context");

    assert!(program.value_matches_type(&device_context, "Landroid/content/Context;"));

    assert_eq!(
        program.invoke_builtin_method(
            &context_no_arg_method("isDeviceProtectedStorage", "Z"),
            Some(&test_invocation(vec![credential_context.clone()])),
        ),
        Some(DexExecutionResult::Int(0))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &context_no_arg_method("isDeviceProtectedStorage", "Z"),
            Some(&test_invocation(vec![device_context.clone()])),
        ),
        Some(DexExecutionResult::Int(1))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &context_no_arg_method("isCredentialProtectedStorage", "Z"),
            Some(&test_invocation(vec![explicit_credential_context.clone()])),
        ),
        Some(DexExecutionResult::Int(1))
    );
    assert_eq!(
        invoke_context_value(
            &program,
            &context_no_arg_method("getDataDir", "Ljava/io/File;"),
            credential_context.clone(),
        ),
        DexValue::Reference(std::rc::Rc::from("Ljava/io/File;:/data/user/0/com.example"))
    );
    assert_eq!(
        invoke_context_value(
            &program,
            &context_no_arg_method("getDataDir", "Ljava/io/File;"),
            device_context.clone(),
        ),
        DexValue::Reference(std::rc::Rc::from(
            "Ljava/io/File;:/data/user_de/0/com.example"
        ))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &context_string_file_method("getDatabasePath"),
            Some(&test_invocation(vec![
                device_context.clone(),
                DexValue::String(std::rc::Rc::from("timer.db")),
            ])),
        ),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Ljava/io/File;:/data/user_de/0/com.example/databases/timer.db")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &context_string_file_method("getSharedPreferencesPath"),
            Some(&test_invocation(vec![
                device_context.clone(),
                DexValue::String(std::rc::Rc::from("prefs")),
            ])),
        ),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Ljava/io/File;:/data/user_de/0/com.example/shared_prefs/prefs.xml")
        )))
    );
    assert!(device_data_dir.join("databases").is_dir());
    assert!(device_data_dir.join("shared_prefs").is_dir());
    assert_eq!(
        invoke_context_value(
            &program,
            &context_no_arg_method("getDataDir", "Ljava/io/File;"),
            wrapper,
        ),
        DexValue::Reference(std::rc::Rc::from(
            "Ljava/io/File;:/data/user_de/0/com.example"
        ))
    );

    for (method_name, directory_name) in [
        ("getFilesDir", "files"),
        ("getCacheDir", "cache"),
        ("getCodeCacheDir", "code_cache"),
        ("getNoBackupFilesDir", "no_backup"),
    ] {
        assert_eq!(
            invoke_context_value(
                &program,
                &context_no_arg_method(method_name, "Ljava/io/File;"),
                device_context.clone(),
            ),
            DexValue::Reference(std::rc::Rc::from(format!(
                "Ljava/io/File;:/data/user_de/0/com.example/{directory_name}"
            )))
        );
        assert!(device_data_dir.join(directory_name).is_dir());
        assert!(!data_dir.join(directory_name).exists());
    }

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn context_shared_preferences_persist_in_the_receivers_storage_domain() {
    // cite: frameworks-base ContextImpl.java#getSharedPreferences, getSharedPreferencesPath, getPreferencesDir, getDataDir
    let app_dir = test_app_data_dir("device_protected_preferences");
    let data_dir = app_dir.join("apps/com.example/data");
    let device_data_dir = app_dir.join("apps/com.example/data_de");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("credential data dir should be created");
    fs::create_dir_all(&device_data_dir).expect("device data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let credential_context =
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap"));

    {
        let program = DexProgram::with_guest_filesystem(
            Vec::new(),
            "com.example",
            data_dir.clone(),
            shared_dir.clone(),
        )
        .expect("program should load guest filesystem");
        let device_context = invoke_context_value(
            &program,
            &context_no_arg_method(
                "createDeviceProtectedStorageContext",
                "Landroid/content/Context;",
            ),
            credential_context.clone(),
        );
        let credential_preferences =
            context_shared_preferences(&program, credential_context.clone(), "prefs");
        let device_preferences =
            context_shared_preferences(&program, device_context.clone(), "prefs");
        let credential_collision =
            context_shared_preferences(&program, credential_context.clone(), "device:prefs");

        assert_ne!(credential_preferences, device_preferences);
        assert_ne!(credential_collision, device_preferences);
        let editor = shared_preferences_editor(&program, device_preferences.clone());
        program.invoke_builtin_method(
            &shared_preferences_editor_put_int_method(),
            Some(&test_invocation(vec![
                editor.clone(),
                DexValue::String(std::rc::Rc::from("answer")),
                DexValue::Int(42),
            ])),
        );
        assert_eq!(
            program.invoke_builtin_method(
                &shared_preferences_editor_commit_method(),
                Some(&test_invocation(vec![editor])),
            ),
            Some(DexExecutionResult::Int(1))
        );
        assert_eq!(
            program.invoke_builtin_method(
                &shared_preferences_get_int_method(),
                Some(&test_invocation(vec![
                    credential_preferences,
                    DexValue::String(std::rc::Rc::from("answer")),
                    DexValue::Int(0),
                ])),
            ),
            Some(DexExecutionResult::Int(0))
        );
        assert_eq!(
            program.invoke_builtin_method(
                &shared_preferences_get_int_method(),
                Some(&test_invocation(vec![
                    device_preferences,
                    DexValue::String(std::rc::Rc::from("answer")),
                    DexValue::Int(0),
                ])),
            ),
            Some(DexExecutionResult::Int(42))
        );
    }

    let program = DexProgram::with_guest_filesystem(
        Vec::new(),
        "com.example",
        data_dir.clone(),
        shared_dir.clone(),
    )
    .expect("program should reload both preference stores");
    let device_context = invoke_context_value(
        &program,
        &context_no_arg_method(
            "createDeviceProtectedStorageContext",
            "Landroid/content/Context;",
        ),
        credential_context.clone(),
    );
    let device_preferences = context_shared_preferences(&program, device_context, "prefs");
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_int_method(),
            Some(&test_invocation(vec![
                device_preferences.clone(),
                DexValue::String(std::rc::Rc::from("answer")),
                DexValue::Int(0),
            ])),
        ),
        Some(DexExecutionResult::Int(42))
    );
    assert!(data_dir.join("shared_prefs/store.tsv").is_file());
    assert!(device_data_dir.join("shared_prefs/store.tsv").is_file());
    let editor = shared_preferences_editor(&program, device_preferences);
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_clear_method(),
            Some(&test_invocation(vec![editor.clone()])),
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_commit_method(),
            Some(&test_invocation(vec![editor])),
        ),
        Some(DexExecutionResult::Int(1))
    );
    drop(program);

    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir, shared_dir)
            .expect("cleared device preference store should reload");
    let device_context = invoke_context_value(
        &program,
        &context_no_arg_method(
            "createDeviceProtectedStorageContext",
            "Landroid/content/Context;",
        ),
        credential_context,
    );
    let device_preferences = context_shared_preferences(&program, device_context, "prefs");
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_int_method(),
            Some(&test_invocation(vec![
                device_preferences,
                DexValue::String(std::rc::Rc::from("answer")),
                DexValue::Int(0),
            ])),
        ),
        Some(DexExecutionResult::Int(0))
    );

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn cts_application_info_exposes_both_private_roots_and_credential_default() {
    // cite: cts ApplicationInfoTest.java#verifyDefaultValues
    // cite: frameworks-base ScanPackageUtils.java#applyPolicy
    // cite: frameworks-base ApplicationInfo.java#initForUser
    let app_dir = test_app_data_dir("application_info_private_roots");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("credential data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let mut program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir, shared_dir)
            .expect("program should load guest filesystem");
    let manifest_context = ApkManifestContext::from_manifest(crate::manifest::AndroidManifest {
        default_to_device_protected_storage: Some(true),
        direct_boot_aware: Some(true),
        ..crate::manifest::AndroidManifest::default()
    })
    .expect("manifest context should apply package policy");
    program.apply_apk_manifest_context(manifest_context);
    let application_info = DexValue::Reference(std::rc::Rc::from(
        "Landroid/content/pm/ApplicationInfo;.bootstrap",
    ));

    for (field_name, expected) in [
        ("dataDir", "/data/user/0/com.example"),
        ("credentialProtectedDataDir", "/data/user/0/com.example"),
        ("deviceProtectedDataDir", "/data/user_de/0/com.example"),
    ] {
        let field = DexFieldId {
            class_descriptor: "Landroid/content/pm/ApplicationInfo;".to_owned(),
            type_descriptor: "Ljava/lang/String;".to_owned(),
            name: field_name.to_owned(),
        };
        assert_eq!(
            program.read_bootstrap_instance_field(&application_info, &field),
            Some(DexValue::String(std::rc::Rc::from(expected)))
        );
    }

    fs::remove_dir_all(app_dir).ok();
}

#[test]
fn sqlite_open_helper_uses_its_context_storage_domain() {
    // cite: frameworks-base SQLiteOpenHelper.java#getDatabaseLocked
    // cite: frameworks-base ContextImpl.java#getDatabasePath, getDatabasesDir, getDataDir
    let app_dir = test_app_data_dir("sqlite_context_storage");
    let data_dir = app_dir.join("apps/com.example/data");
    let device_data_dir = app_dir.join("apps/com.example/data_de");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("credential data dir should be created");
    fs::create_dir_all(&device_data_dir).expect("device data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir.clone(), shared_dir)
            .expect("program should load guest filesystem");
    let credential_context =
        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap"));
    let device_context = invoke_context_value(
        &program,
        &context_no_arg_method(
            "createDeviceProtectedStorageContext",
            "Landroid/content/Context;",
        ),
        credential_context.clone(),
    );
    let credential_database = DexValue::Reference(std::rc::Rc::from("credential-database"));
    let device_database = DexValue::Reference(std::rc::Rc::from("device-database"));

    assert_eq!(
        program
            .register_sqlite_database_name(
                &credential_database,
                &credential_context,
                Some("timer.db"),
            )
            .expect("credential database path should resolve"),
        fs::canonicalize(&data_dir)
            .expect("credential data dir should canonicalize")
            .join("databases/timer.db")
    );
    assert_eq!(
        program
            .register_sqlite_database_name(&device_database, &device_context, Some("timer.db"),)
            .expect("device database path should resolve"),
        fs::canonicalize(&device_data_dir)
            .expect("device data dir should canonicalize")
            .join("databases/timer.db")
    );
    assert!(program
        .register_sqlite_database_name(
            &DexValue::Reference(std::rc::Rc::from("invalid-database")),
            &credential_context,
            Some("../timer.db"),
        )
        .is_err());
    assert!(program
        .sqlite_database_path(&DexValue::Reference(std::rc::Rc::from(
            "unregistered-database",
        )))
        .is_err());
    assert!(DexProgram::new(Vec::new())
        .register_sqlite_database_name(
            &DexValue::Reference(std::rc::Rc::from("unbacked-database")),
            &bootstrap_context(),
            Some("timer.db"),
        )
        .is_err());

    fs::remove_dir_all(app_dir).ok();
}

#[cfg(unix)]
#[test]
fn sqlite_guest_vfs_uses_the_held_private_root_after_visible_replacement() {
    // cite: frameworks-base SQLiteOpenHelper.java#getDatabaseLocked
    // cite: frameworks-base ContextImpl.java#getDatabasePath, getDatabasesDir, getDataDir
    let app_dir = test_app_data_dir("sqlite_capability_root");
    let data_dir = app_dir.join("apps/com.example/data");
    let device_data_dir = app_dir.join("apps/com.example/data_de");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("credential data dir should be created");
    fs::create_dir_all(&device_data_dir).expect("device data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let program =
        DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir.clone(), shared_dir)
            .expect("program should load guest filesystem");
    let database = DexValue::Reference(std::rc::Rc::from("capability-database"));
    program
        .register_sqlite_database_name(
            &database,
            &DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap")),
            Some("held.db"),
        )
        .expect("database path should register");
    let moved = app_dir.join("original-data");
    let outside = app_dir.join("outside-data");
    fs::create_dir_all(outside.join("databases")).expect("outside database dir should exist");
    fs::rename(&data_dir, &moved).expect("credential root should move");
    std::os::unix::fs::symlink(&outside, &data_dir)
        .expect("replacement credential root symlink should be created");

    assert_eq!(
        program.sqlite_execute_database_sql(
            &database,
            "CREATE TABLE values_table(value INTEGER)",
            &[],
        ),
        Some(())
    );
    assert!(moved.join("databases/held.db").is_file());
    assert!(!outside.join("databases/held.db").exists());

    drop(program);
    fs::remove_file(&data_dir).ok();
    fs::remove_dir_all(app_dir).ok();
}

#[cfg(unix)]
#[test]
fn cts_sqlite_transactions_persist_through_guest_vfs_reopen() {
    // cite: cts SQLiteDatabaseTest.java#testTransaction
    // cite: frameworks-base SQLiteOpenHelper.java#getDatabaseLocked
    let app_dir = test_app_data_dir("sqlite_transaction_reopen");
    let data_dir = app_dir.join("apps/com.example/data");
    let shared_dir = app_dir.join("storage");
    fs::create_dir_all(&data_dir).expect("credential data dir should be created");
    fs::create_dir_all(&shared_dir).expect("shared dir should be created");
    let begin = guest_vfs_transaction_method("beginTransactionNonExclusive");
    let successful = guest_vfs_transaction_method("setTransactionSuccessful");
    let end = guest_vfs_transaction_method("endTransaction");

    {
        let program = DexProgram::with_guest_filesystem(
            Vec::new(),
            "com.example",
            data_dir.clone(),
            shared_dir.clone(),
        )
        .expect("program should load guest filesystem");
        let database = DexValue::Reference(std::rc::Rc::from("transaction-database"));
        program
            .register_sqlite_database_name(&database, &bootstrap_context(), Some("state.db"))
            .expect("database path should register");
        program
            .sqlite_execute_database_sql(&database, "CREATE TABLE values_table(value INTEGER)", &[])
            .expect("transaction table should be created");
        program
            .sqlite_execute_database_sql(&database, "INSERT INTO values_table VALUES (0)", &[])
            .expect("transaction row should be created");

        assert_guest_vfs_transaction_void(&program, &begin, &database);
        program
            .sqlite_execute_database_sql(&database, "UPDATE values_table SET value = 1", &[])
            .expect("committed update should execute");
        assert_guest_vfs_transaction_void(&program, &successful, &database);
        assert_guest_vfs_transaction_void(&program, &end, &database);

        assert_guest_vfs_transaction_void(&program, &begin, &database);
        program
            .sqlite_execute_database_sql(&database, "UPDATE values_table SET value = 2", &[])
            .expect("rolled back update should execute");
        assert_guest_vfs_transaction_void(&program, &end, &database);
    }

    {
        let program =
            DexProgram::with_guest_filesystem(Vec::new(), "com.example", data_dir, shared_dir)
                .expect("reopened program should load guest filesystem");
        let database = DexValue::Reference(std::rc::Rc::from("reopened-transaction-database"));
        program
            .register_sqlite_database_name(&database, &bootstrap_context(), Some("state.db"))
            .expect("reopened database path should register");
        let cursor = program
            .sqlite_raw_query_result(&database, "SELECT value FROM values_table", &[])
            .expect("reopened transaction value should be readable");
        assert_eq!(cursor.rows, vec![vec![DexValue::Long(1)]]);
    }

    fs::remove_dir_all(app_dir).ok();
}

fn guest_vfs_transaction_method(name: &str) -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/database/sqlite/SQLiteDatabase;".to_owned(),
        name: name.to_owned(),
        proto: crate::dex::DexProto {
            shorty: "V".to_owned(),
            return_type: "V".to_owned(),
            parameters: Vec::new(),
        },
    }
}

fn assert_guest_vfs_transaction_void(
    program: &DexProgram,
    method: &DexMethodId,
    database: &DexValue,
) {
    assert_eq!(
        program.invoke_builtin_method(method, Some(&test_invocation(vec![database.clone()])),),
        Some(DexExecutionResult::Void)
    );
}

#[test]
fn builtin_context_component_callbacks_are_tracked_by_context() {
    let program = DexProgram::new(Vec::new());
    let register = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "registerComponentCallbacks".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Landroid/content/ComponentCallbacks;".to_owned()],
        },
    };
    let unregister = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "unregisterComponentCallbacks".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Landroid/content/ComponentCallbacks;".to_owned()],
        },
    };
    let context = DexValue::Reference(std::rc::Rc::from("context"));
    let callback = DexValue::Reference(std::rc::Rc::from("callback"));
    let register_invocation = test_invocation(vec![context.clone(), callback.clone()]);

    assert_eq!(
        program.invoke_builtin_method(&register, Some(&register_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.invoke_builtin_method(&register, Some(&register_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program
            .runtime
            .borrow()
            .component_callbacks
            .get("reference:context"),
        Some(&vec![callback.clone()])
    );

    let unregister_invocation = test_invocation(vec![context, callback]);

    assert_eq!(
        program.invoke_builtin_method(&unregister, Some(&unregister_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert!(program
        .runtime
        .borrow()
        .component_callbacks
        .get("reference:context")
        .is_none());
}

#[test]
fn builtin_context_unregister_receiver_returns_void() {
    let program = DexProgram::new(Vec::new());
    let unregister = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "unregisterReceiver".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Landroid/content/BroadcastReceiver;".to_owned()],
        },
    };
    let register = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "registerReceiver".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLL".to_owned(),
            return_type: "Landroid/content/Intent;".to_owned(),
            parameters: vec![
                "Landroid/content/BroadcastReceiver;".to_owned(),
                "Landroid/content/IntentFilter;".to_owned(),
            ],
        },
    };
    let unregister_invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("context")),
        DexValue::Null,
    ]);
    let register_invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("context")),
        DexValue::Reference(std::rc::Rc::from("receiver")),
        DexValue::Reference(std::rc::Rc::from("filter")),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&unregister, Some(&unregister_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.invoke_builtin_method(&register, Some(&register_invocation)),
        Some(DexExecutionResult::Value(DexValue::Null))
    );
}

#[test]
fn builtin_intent_filter_constructor_returns_void() {
    let program = DexProgram::new(Vec::new());
    let constructor = DexMethodId {
        class_descriptor: "Landroid/content/IntentFilter;".to_owned(),
        name: "<init>".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "V".to_owned(),
            return_type: "V".to_owned(),
            parameters: Vec::new(),
        },
    };
    let add_action = DexMethodId {
        class_descriptor: "Landroid/content/IntentFilter;".to_owned(),
        name: "addAction".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let count_actions = DexMethodId {
        class_descriptor: "Landroid/content/IntentFilter;".to_owned(),
        name: "countActions".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "I".to_owned(),
            return_type: "I".to_owned(),
            parameters: Vec::new(),
        },
    };
    let filter = DexValue::Reference(std::rc::Rc::from("intent-filter"));
    let count_invocation = test_invocation(vec![filter.clone()]);
    let add_action_invocation = test_invocation(vec![
        filter,
        DexValue::String(std::rc::Rc::from("android.intent.action.TIME_SET")),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&constructor, None),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.invoke_builtin_method(&count_actions, Some(&count_invocation)),
        Some(DexExecutionResult::Int(0))
    );
    assert_eq!(
        program.invoke_builtin_method(&add_action, Some(&add_action_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.invoke_builtin_method(&count_actions, Some(&count_invocation)),
        Some(DexExecutionResult::Int(1))
    );
}

#[test]
fn builtin_broadcast_receiver_constructor_returns_void() {
    let program = DexProgram::new(Vec::new());
    let method = DexMethodId {
        class_descriptor: "Landroid/content/BroadcastReceiver;".to_owned(),
        name: "<init>".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "V".to_owned(),
            return_type: "V".to_owned(),
            parameters: Vec::new(),
        },
    };

    assert_eq!(
        program.invoke_builtin_method(&method, None),
        Some(DexExecutionResult::Void)
    );
}

#[test]
fn builtin_component_name_constructor_returns_void() {
    let program = DexProgram::new(Vec::new());
    let method = DexMethodId {
        class_descriptor: "Landroid/content/ComponentName;".to_owned(),
        name: "<init>".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VLL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec![
                "Landroid/content/Context;".to_owned(),
                "Ljava/lang/Class;".to_owned(),
            ],
        },
    };
    let invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from("context")),
        DexValue::Reference(std::rc::Rc::from(
            "Landroid/content/ComponentName;.bootstrap",
        )),
        DexValue::Reference(std::rc::Rc::from(
            "Ljava/lang/Class;:Lcom/example/MainActivity;",
        )),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&method, Some(&invocation)),
        Some(DexExecutionResult::Void)
    );
}

#[test]
fn builtin_content_values_constructor_returns_void() {
    let program = DexProgram::new(Vec::new());
    let method = DexMethodId {
        class_descriptor: "Landroid/content/ContentValues;".to_owned(),
        name: "<init>".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "V".to_owned(),
            return_type: "V".to_owned(),
            parameters: Vec::new(),
        },
    };

    assert_eq!(
        program.invoke_builtin_method(&method, None),
        Some(DexExecutionResult::Void)
    );
}

#[test]
fn builtin_content_values_put_string_stores_value() {
    let program = DexProgram::new(Vec::new());
    let put = DexMethodId {
        class_descriptor: "Landroid/content/ContentValues;".to_owned(),
        name: "put".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VLL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec![
                "Ljava/lang/String;".to_owned(),
                "Ljava/lang/String;".to_owned(),
            ],
        },
    };
    let values = DexValue::Reference(std::rc::Rc::from("content-values"));
    let string_invocation = test_invocation(vec![
        values.clone(),
        DexValue::String(std::rc::Rc::from("name")),
        DexValue::String(std::rc::Rc::from("calculator")),
    ]);
    let null_invocation = test_invocation(vec![
        values,
        DexValue::String(std::rc::Rc::from("missing")),
        DexValue::Null,
    ]);

    assert_eq!(
        program.invoke_builtin_method(&put, Some(&string_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.invoke_builtin_method(&put, Some(&null_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program
            .runtime
            .borrow()
            .content_values_entries
            .get(&("reference:content-values".to_owned(), "name".to_owned())),
        Some(&DexValue::String(std::rc::Rc::from("calculator")))
    );
    assert_eq!(
        program
            .runtime
            .borrow()
            .content_values_entries
            .get(&("reference:content-values".to_owned(), "missing".to_owned())),
        Some(&DexValue::Null)
    );
}

#[test]
fn builtin_content_values_put_long_stores_value() {
    let program = DexProgram::new(Vec::new());
    let put = DexMethodId {
        class_descriptor: "Landroid/content/ContentValues;".to_owned(),
        name: "put".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VLL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec![
                "Ljava/lang/String;".to_owned(),
                "Ljava/lang/Long;".to_owned(),
            ],
        },
    };
    let values = DexValue::Reference(std::rc::Rc::from("content-values"));
    let long_invocation = test_invocation(vec![
        values.clone(),
        DexValue::String(std::rc::Rc::from("timestamp")),
        DexValue::Reference(std::rc::Rc::from("Ljava/lang/Long;:42")),
    ]);
    let null_invocation = test_invocation(vec![
        values,
        DexValue::String(std::rc::Rc::from("optional")),
        DexValue::Null,
    ]);

    assert_eq!(
        program.invoke_builtin_method(&put, Some(&long_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.invoke_builtin_method(&put, Some(&null_invocation)),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.runtime.borrow().content_values_entries.get(&(
            "reference:content-values".to_owned(),
            "timestamp".to_owned()
        )),
        Some(&DexValue::Long(42))
    );
    assert_eq!(
        program
            .runtime
            .borrow()
            .content_values_entries
            .get(&("reference:content-values".to_owned(), "optional".to_owned())),
        Some(&DexValue::Null)
    );
}

#[test]
fn builtin_context_package_manager_reports_no_features() {
    let program = DexProgram::new(Vec::new());
    let get_package_manager = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getPackageManager".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Landroid/content/pm/PackageManager;".to_owned(),
            parameters: Vec::new(),
        },
    };
    let has_feature = DexMethodId {
        class_descriptor: "Landroid/content/pm/PackageManager;".to_owned(),
        name: "hasSystemFeature".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ZL".to_owned(),
            return_type: "Z".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let get_activity_info = DexMethodId {
        class_descriptor: "Landroid/content/pm/PackageManager;".to_owned(),
        name: "getActivityInfo".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLI".to_owned(),
            return_type: "Landroid/content/pm/ActivityInfo;".to_owned(),
            parameters: vec!["Landroid/content/ComponentName;".to_owned(), "I".to_owned()],
        },
    };
    let get_activity_info_invocation = test_invocation(vec![
        DexValue::Reference(std::rc::Rc::from(
            "Landroid/content/pm/PackageManager;.bootstrap",
        )),
        DexValue::Reference(std::rc::Rc::from(
            "Landroid/content/ComponentName;.bootstrap",
        )),
        DexValue::Int(0),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&get_package_manager, None),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Landroid/content/pm/PackageManager;.bootstrap")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(&has_feature, None),
        Some(DexExecutionResult::Int(0))
    );
    assert_eq!(
        program.invoke_builtin_method(&get_activity_info, Some(&get_activity_info_invocation)),
        Some(DexExecutionResult::Value(DexValue::Reference(
            std::rc::Rc::from("Landroid/content/pm/ActivityInfo;.bootstrap")
        )))
    );
}

// cite: frameworks-base PackageManager.java#FEATURE_WATCH
// cite: frameworks-base UiModeManagerService.java#mWatch
#[test]
fn package_manager_reports_watch_feature_only_on_watch_device() {
    let has_feature = DexMethodId {
        class_descriptor: "Landroid/content/pm/PackageManager;".to_owned(),
        name: "hasSystemFeature".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ZL".to_owned(),
            return_type: "Z".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let feature_invocation = |feature: &str| {
        test_invocation(vec![
            DexValue::Reference(std::rc::Rc::from(
                "Landroid/content/pm/PackageManager;.bootstrap",
            )),
            DexValue::String(std::rc::Rc::from(feature)),
        ])
    };
    let watch = DexProgram::with_config(Vec::new(), crate::config::WinedroidConfig::watch());
    let phone = DexProgram::new(Vec::new());

    for (program, feature, expected) in [
        (&watch, "android.hardware.type.watch", 1),
        (&watch, "android.software.leanback", 0),
        (&phone, "android.hardware.type.watch", 0),
    ] {
        assert_eq!(
            program.invoke_builtin_method(&has_feature, Some(&feature_invocation(feature))),
            Some(DexExecutionResult::Int(expected))
        );
    }
}

#[test]
fn builtin_empty_shared_preferences_returns_absent_and_defaults() {
    let program = DexProgram::new(Vec::new());
    let preferences = context_shared_preferences(&program, bootstrap_context(), "prefs");
    let contains = DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "contains".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ZL".to_owned(),
            return_type: "Z".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let get_boolean = DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "getBoolean".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ZLZ".to_owned(),
            return_type: "Z".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "Z".to_owned()],
        },
    };
    let get_int = DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "getInt".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ILI".to_owned(),
            return_type: "I".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    };
    let get_string = DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "getString".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLL".to_owned(),
            return_type: "Ljava/lang/String;".to_owned(),
            parameters: vec![
                "Ljava/lang/String;".to_owned(),
                "Ljava/lang/String;".to_owned(),
            ],
        },
    };
    let get_long = DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "getLong".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "JLJ".to_owned(),
            return_type: "J".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "J".to_owned()],
        },
    };
    let contains_invocation = test_invocation(vec![
        preferences.clone(),
        DexValue::String(std::rc::Rc::from("enabled")),
    ]);
    let get_boolean_invocation = test_invocation(vec![
        preferences.clone(),
        DexValue::String(std::rc::Rc::from("enabled")),
        DexValue::Int(1),
    ]);
    let get_int_invocation = test_invocation(vec![
        preferences.clone(),
        DexValue::String(std::rc::Rc::from("count")),
        DexValue::Int(7),
    ]);
    let get_string_invocation = test_invocation(vec![
        preferences.clone(),
        DexValue::String(std::rc::Rc::from("theme")),
        DexValue::String(std::rc::Rc::from("system")),
    ]);
    let get_null_string_invocation = test_invocation(vec![
        preferences.clone(),
        DexValue::String(std::rc::Rc::from("accent")),
        DexValue::Null,
    ]);
    let get_zero_string_invocation = test_invocation(vec![
        preferences.clone(),
        DexValue::String(std::rc::Rc::from("history")),
        DexValue::Int(0),
    ]);
    let get_long_invocation = test_invocation(vec![
        preferences,
        DexValue::String(std::rc::Rc::from("timestamp")),
        DexValue::Long(42),
        DexValue::Long(42),
    ]);

    assert_eq!(
        program.invoke_builtin_method(&contains, Some(&contains_invocation)),
        Some(DexExecutionResult::Int(0))
    );
    assert_eq!(
        program.invoke_builtin_method(&get_boolean, Some(&get_boolean_invocation)),
        Some(DexExecutionResult::Int(1))
    );
    assert_eq!(
        program.invoke_builtin_method(&get_int, Some(&get_int_invocation)),
        Some(DexExecutionResult::Int(7))
    );
    assert_eq!(
        program.invoke_builtin_method(&get_string, Some(&get_string_invocation)),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("system")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(&get_string, Some(&get_null_string_invocation)),
        Some(DexExecutionResult::Value(DexValue::Null))
    );
    assert_eq!(
        program.invoke_builtin_method(&get_string, Some(&get_zero_string_invocation)),
        Some(DexExecutionResult::Value(DexValue::Null))
    );
    assert_eq!(
        program.invoke_builtin_method(&get_long, Some(&get_long_invocation)),
        Some(DexExecutionResult::Value(DexValue::Long(42)))
    );
}

#[test]
fn builtin_shared_preferences_editor_flow_completes() {
    let data_dir = test_app_data_dir("shared_preferences_editor_flow");
    let program = DexProgram::with_app_data_dir(Vec::new(), data_dir.clone())
        .expect("program should create app data backed runtime");
    let preferences = context_shared_preferences(&program, bootstrap_context(), "prefs");
    let edit_invocation = test_invocation(vec![preferences.clone()]);
    let DexExecutionResult::Value(editor) = program
        .invoke_builtin_method(&shared_preferences_edit_method(), Some(&edit_invocation))
        .expect("edit should return an editor")
    else {
        panic!("edit should return an editor value");
    };
    let put_invocation = test_invocation(vec![
        editor.clone(),
        DexValue::String(std::rc::Rc::from("enabled")),
        DexValue::Int(1),
    ]);
    let put_string_invocation = test_invocation(vec![
        editor.clone(),
        DexValue::String(std::rc::Rc::from("history")),
        DexValue::String(std::rc::Rc::from("[{}]")),
    ]);
    let put_int_invocation = test_invocation(vec![
        editor.clone(),
        DexValue::String(std::rc::Rc::from("theme")),
        DexValue::Int(-1),
    ]);
    let put_long_invocation = test_invocation(vec![
        editor.clone(),
        DexValue::String(std::rc::Rc::from("timestamp")),
        DexValue::Long(0),
        DexValue::Long(0),
    ]);
    let get_string_invocation = test_invocation(vec![
        preferences.clone(),
        DexValue::String(std::rc::Rc::from("history")),
        DexValue::String(std::rc::Rc::from("default")),
    ]);
    let get_int_invocation = test_invocation(vec![
        preferences,
        DexValue::String(std::rc::Rc::from("theme")),
        DexValue::Int(0),
    ]);
    let apply_invocation = test_invocation(vec![editor.clone()]);
    let commit_invocation = test_invocation(vec![editor.clone()]);

    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_put_boolean_method(),
            Some(&put_invocation)
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_put_string_method(),
            Some(&put_string_invocation)
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_put_int_method(),
            Some(&put_int_invocation)
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_put_long_method(),
            Some(&put_long_invocation)
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_string_method(),
            Some(&get_string_invocation)
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("default")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_int_method(),
            Some(&get_int_invocation)
        ),
        Some(DexExecutionResult::Int(0))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_apply_method(),
            Some(&apply_invocation)
        ),
        Some(DexExecutionResult::Void)
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_string_method(),
            Some(&get_string_invocation)
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("[{}]")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_int_method(),
            Some(&get_int_invocation)
        ),
        Some(DexExecutionResult::Int(-1))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_commit_method(),
            Some(&commit_invocation)
        ),
        Some(DexExecutionResult::Int(1))
    );

    fs::remove_dir_all(data_dir).ok();
}

#[test]
fn builtin_shared_preferences_remove_and_clear_apply_through_editor() {
    let data_dir = test_app_data_dir("shared_preferences_remove_clear");
    let program = DexProgram::with_app_data_dir(Vec::new(), data_dir.clone())
        .expect("program should create app data backed runtime");
    let preferences = context_shared_preferences(&program, bootstrap_context(), "prefs");
    let editor = shared_preferences_editor(&program, preferences.clone());

    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_put_string_method(),
            Some(&test_invocation(vec![
                editor.clone(),
                DexValue::String(std::rc::Rc::from("history")),
                DexValue::String(std::rc::Rc::from("[1]")),
            ])),
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_put_int_method(),
            Some(&test_invocation(vec![
                editor.clone(),
                DexValue::String(std::rc::Rc::from("theme")),
                DexValue::Int(4),
            ])),
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_apply_method(),
            Some(&test_invocation(vec![editor.clone()])),
        ),
        Some(DexExecutionResult::Void)
    );

    let editor = shared_preferences_editor(&program, preferences.clone());
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_remove_method(),
            Some(&test_invocation(vec![
                editor.clone(),
                DexValue::String(std::rc::Rc::from("history")),
            ])),
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_clear_method(),
            Some(&test_invocation(vec![editor.clone()])),
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_put_int_method(),
            Some(&test_invocation(vec![
                editor.clone(),
                DexValue::String(std::rc::Rc::from("theme")),
                DexValue::Int(7),
            ])),
        ),
        Some(DexExecutionResult::Value(editor.clone()))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_editor_commit_method(),
            Some(&test_invocation(vec![editor])),
        ),
        Some(DexExecutionResult::Int(1))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_string_method(),
            Some(&test_invocation(vec![
                preferences.clone(),
                DexValue::String(std::rc::Rc::from("history")),
                DexValue::String(std::rc::Rc::from("default")),
            ])),
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("default")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_int_method(),
            Some(&test_invocation(vec![
                preferences,
                DexValue::String(std::rc::Rc::from("theme")),
                DexValue::Int(0),
            ])),
        ),
        Some(DexExecutionResult::Int(7))
    );

    fs::remove_dir_all(data_dir).ok();
}

#[test]
fn builtin_shared_preferences_persist_across_programs_when_app_data_dir_is_set() {
    let data_dir = test_app_data_dir("shared_preferences_persist");

    {
        let program = DexProgram::with_app_data_dir(Vec::new(), data_dir.clone())
            .expect("program should create app data backed runtime");
        let preferences = context_shared_preferences(&program, bootstrap_context(), "prefs");
        let editor = shared_preferences_editor(&program, preferences.clone());

        assert_eq!(
            program.invoke_builtin_method(
                &shared_preferences_editor_put_boolean_method(),
                Some(&test_invocation(vec![
                    editor.clone(),
                    DexValue::String(std::rc::Rc::from("enabled")),
                    DexValue::Int(1),
                ])),
            ),
            Some(DexExecutionResult::Value(editor.clone()))
        );
        assert_eq!(
            program.invoke_builtin_method(
                &shared_preferences_editor_put_string_method(),
                Some(&test_invocation(vec![
                    editor.clone(),
                    DexValue::String(std::rc::Rc::from("name")),
                    DexValue::String(std::rc::Rc::from("OpenCalculator")),
                ])),
            ),
            Some(DexExecutionResult::Value(editor.clone()))
        );
        assert_eq!(
            program.invoke_builtin_method(
                &shared_preferences_editor_commit_method(),
                Some(&test_invocation(vec![editor])),
            ),
            Some(DexExecutionResult::Int(1))
        );
    }

    let program = DexProgram::with_app_data_dir(Vec::new(), data_dir.clone())
        .expect("program should reload persisted app data");
    let preferences = context_shared_preferences(&program, bootstrap_context(), "prefs");
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_boolean_method(),
            Some(&test_invocation(vec![
                preferences.clone(),
                DexValue::String(std::rc::Rc::from("enabled")),
                DexValue::Int(0),
            ])),
        ),
        Some(DexExecutionResult::Int(1))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_string_method(),
            Some(&test_invocation(vec![
                preferences,
                DexValue::String(std::rc::Rc::from("name")),
                DexValue::String(std::rc::Rc::from("missing")),
            ])),
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("OpenCalculator")
        )))
    );

    fs::remove_dir_all(data_dir).ok();
}

#[test]
fn shared_preferences_v1_store_migrates_legacy_credential_identity() {
    let data_dir = test_app_data_dir("shared_preferences_v1_migration");
    let store_dir = data_dir.join("shared_prefs");
    fs::create_dir_all(&store_dir).expect("legacy preferences directory should exist");
    fs::write(
        store_dir.join("store.tsv"),
        "winedroid-shared-preferences-v1\n7265666572656e63653a4c616e64726f69642f636f6e74656e742f536861726564507265666572656e6365733b3a7072656673\t656e61626c6564\tint\t1\n",
    )
    .expect("legacy preferences store should exist");
    let program = DexProgram::with_app_data_dir(Vec::new(), data_dir.clone())
        .expect("legacy preferences store should load");
    let preferences = context_shared_preferences(&program, bootstrap_context(), "prefs");

    assert_eq!(
        program.invoke_builtin_method(
            &shared_preferences_get_boolean_method(),
            Some(&test_invocation(vec![
                preferences,
                DexValue::String(std::rc::Rc::from("enabled")),
                DexValue::Int(0),
            ])),
        ),
        Some(DexExecutionResult::Int(1))
    );

    fs::remove_dir_all(data_dir).ok();
}

fn shared_preferences_editor(program: &DexProgram, preferences: DexValue) -> DexValue {
    let DexExecutionResult::Value(editor) = program
        .invoke_builtin_method(
            &shared_preferences_edit_method(),
            Some(&test_invocation(vec![preferences])),
        )
        .expect("edit should return an editor")
    else {
        panic!("edit should return an editor value");
    };

    editor
}

fn shared_preferences_edit_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "edit".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Landroid/content/SharedPreferences$Editor;".to_owned(),
            parameters: Vec::new(),
        },
    }
}

fn shared_preferences_get_boolean_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "getBoolean".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ZLZ".to_owned(),
            return_type: "Z".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "Z".to_owned()],
        },
    }
}

fn shared_preferences_get_string_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "getString".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLL".to_owned(),
            return_type: "Ljava/lang/String;".to_owned(),
            parameters: vec![
                "Ljava/lang/String;".to_owned(),
                "Ljava/lang/String;".to_owned(),
            ],
        },
    }
}

fn shared_preferences_get_int_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences;".to_owned(),
        name: "getInt".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ILI".to_owned(),
            return_type: "I".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    }
}

fn shared_preferences_editor_put_boolean_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "putBoolean".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLZ".to_owned(),
            return_type: "Landroid/content/SharedPreferences$Editor;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "Z".to_owned()],
        },
    }
}

fn shared_preferences_editor_put_string_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "putString".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLL".to_owned(),
            return_type: "Landroid/content/SharedPreferences$Editor;".to_owned(),
            parameters: vec![
                "Ljava/lang/String;".to_owned(),
                "Ljava/lang/String;".to_owned(),
            ],
        },
    }
}

fn shared_preferences_editor_put_int_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "putInt".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLI".to_owned(),
            return_type: "Landroid/content/SharedPreferences$Editor;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    }
}

fn shared_preferences_editor_put_long_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "putLong".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLJ".to_owned(),
            return_type: "Landroid/content/SharedPreferences$Editor;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "J".to_owned()],
        },
    }
}

fn shared_preferences_editor_remove_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "remove".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "Landroid/content/SharedPreferences$Editor;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    }
}

fn shared_preferences_editor_clear_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "clear".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Landroid/content/SharedPreferences$Editor;".to_owned(),
            parameters: Vec::new(),
        },
    }
}

fn shared_preferences_editor_apply_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "apply".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "V".to_owned(),
            return_type: "V".to_owned(),
            parameters: Vec::new(),
        },
    }
}

fn shared_preferences_editor_commit_method() -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/SharedPreferences$Editor;".to_owned(),
        name: "commit".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "Z".to_owned(),
            return_type: "Z".to_owned(),
            parameters: Vec::new(),
        },
    }
}

fn context_no_arg_method(name: &str, return_type: &str) -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: name.to_owned(),
        proto: crate::dex::DexProto {
            shorty: if return_type == "Z" { "Z" } else { "L" }.to_owned(),
            return_type: return_type.to_owned(),
            parameters: Vec::new(),
        },
    }
}

fn context_string_file_method(name: &str) -> DexMethodId {
    DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: name.to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "Ljava/io/File;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    }
}

fn invoke_context_value(program: &DexProgram, method: &DexMethodId, context: DexValue) -> DexValue {
    let Some(DexExecutionResult::Value(value)) =
        program.invoke_builtin_method(method, Some(&test_invocation(vec![context])))
    else {
        panic!("Context method should return a value");
    };
    value
}

fn context_shared_preferences(program: &DexProgram, context: DexValue, name: &str) -> DexValue {
    let method = DexMethodId {
        class_descriptor: "Landroid/content/Context;".to_owned(),
        name: "getSharedPreferences".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLI".to_owned(),
            return_type: "Landroid/content/SharedPreferences;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    };
    let Some(DexExecutionResult::Value(preferences)) = program.invoke_builtin_method(
        &method,
        Some(&test_invocation(vec![
            context,
            DexValue::String(std::rc::Rc::from(name)),
            DexValue::Int(0),
        ])),
    ) else {
        panic!("Context.getSharedPreferences should return a value");
    };
    preferences
}

fn bootstrap_context() -> DexValue {
    DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap"))
}

fn test_app_data_dir(name: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("winedroid-{name}-{}-{id}", std::process::id()));

    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).expect("test app data dir should be created");

    dir
}

#[test]
fn builtin_uri_component_accessors_parse_scheme_authority_path_query_fragment() {
    // cite: frameworks-base Uri.java#getScheme, getAuthority, getPath
    let program = DexProgram::new(Vec::new());
    let parse = DexMethodId {
        class_descriptor: "Landroid/net/Uri;".to_owned(),
        name: "parse".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "Landroid/net/Uri;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    let Some(DexExecutionResult::Value(uri)) = program.invoke_builtin_method(
        &parse,
        Some(&test_invocation(vec![DexValue::String(std::rc::Rc::from(
            "app://host/notes/edit?noteId=1&noteColor=-657931#top",
        ))])),
    ) else {
        panic!("Uri.parse did not return a uri");
    };

    let component = |name: &str, descriptor: &str| DexMethodId {
        class_descriptor: "Landroid/net/Uri;".to_owned(),
        name: name.to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: descriptor.to_owned(),
            parameters: Vec::new(),
        },
    };

    assert_eq!(
        program.invoke_builtin_method(
            &component("getScheme", "Ljava/lang/String;"),
            Some(&test_invocation(vec![uri.clone()]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("app")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &component("getAuthority", "Ljava/lang/String;"),
            Some(&test_invocation(vec![uri.clone()]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("host")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &component("getPath", "Ljava/lang/String;"),
            Some(&test_invocation(vec![uri.clone()]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("/notes/edit")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &component("getLastPathSegment", "Ljava/lang/String;"),
            Some(&test_invocation(vec![uri.clone()]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("edit")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &component("getFragment", "Ljava/lang/String;"),
            Some(&test_invocation(vec![uri.clone()]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("top")
        )))
    );

    let get_query_parameter = DexMethodId {
        class_descriptor: "Landroid/net/Uri;".to_owned(),
        name: "getQueryParameter".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LL".to_owned(),
            return_type: "Ljava/lang/String;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &get_query_parameter,
            Some(&test_invocation(vec![
                uri.clone(),
                DexValue::String(std::rc::Rc::from("noteId"))
            ]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("1")
        )))
    );

    let get_path_segments = DexMethodId {
        class_descriptor: "Landroid/net/Uri;".to_owned(),
        name: "getPathSegments".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Ljava/util/List;".to_owned(),
            parameters: Vec::new(),
        },
    };
    let Some(DexExecutionResult::Value(segments)) =
        program.invoke_builtin_method(&get_path_segments, Some(&test_invocation(vec![uri])))
    else {
        panic!("Uri.getPathSegments did not return a list");
    };
    let list_get = DexMethodId {
        class_descriptor: "Ljava/util/List;".to_owned(),
        name: "get".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LI".to_owned(),
            return_type: "Ljava/lang/Object;".to_owned(),
            parameters: vec!["I".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &list_get,
            Some(&test_invocation(vec![segments.clone(), DexValue::Int(0)]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("notes")
        )))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &list_get,
            Some(&test_invocation(vec![segments, DexValue::Int(1)]))
        ),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("edit")
        )))
    );
}

#[test]
fn builtin_bundle_get_scalar_and_string_round_trip_with_defaults() {
    // cite: frameworks-base BaseBundle.java#putInt, getInt, getString
    let program = DexProgram::new(Vec::new());
    let bundle = DexValue::Object(DexObjectRef {
        id: 501,
        class_descriptor: std::rc::Rc::from("Landroid/os/Bundle;"),
    });
    let put_int = DexMethodId {
        class_descriptor: "Landroid/os/BaseBundle;".to_owned(),
        name: "putInt".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VLI".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &put_int,
            Some(&test_invocation(vec![
                bundle.clone(),
                DexValue::String(std::rc::Rc::from("count")),
                DexValue::Int(7),
            ]))
        ),
        Some(DexExecutionResult::Void)
    );

    let get_int = DexMethodId {
        class_descriptor: "Landroid/os/BaseBundle;".to_owned(),
        name: "getInt".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ILI".to_owned(),
            return_type: "I".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &get_int,
            Some(&test_invocation(vec![
                bundle.clone(),
                DexValue::String(std::rc::Rc::from("count"))
            ]))
        ),
        Some(DexExecutionResult::Int(7))
    );
    assert_eq!(
        program.invoke_builtin_method(
            &get_int,
            Some(&test_invocation(vec![
                bundle.clone(),
                DexValue::String(std::rc::Rc::from("missing"))
            ]))
        ),
        Some(DexExecutionResult::Int(0))
    );

    let get_string = DexMethodId {
        class_descriptor: "Landroid/os/BaseBundle;".to_owned(),
        name: "getString".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLL".to_owned(),
            return_type: "Ljava/lang/String;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &get_string,
            Some(&test_invocation(vec![
                bundle,
                DexValue::String(std::rc::Rc::from("missing"))
            ]))
        ),
        Some(DexExecutionResult::Value(DexValue::Null))
    );
}

#[test]
fn builtin_bundle_scalars_are_stored_boxed_for_object_reads() {
    // cite: frameworks-base BaseBundle.java#putInt, get
    let program = DexProgram::new(Vec::new());
    let bundle = DexValue::Object(DexObjectRef {
        id: 701,
        class_descriptor: std::rc::Rc::from("Landroid/os/Bundle;"),
    });
    let scalar_method =
        |name: &str, shorty: &str, return_type: &str, parameter: &str| DexMethodId {
            class_descriptor: "Landroid/os/BaseBundle;".to_owned(),
            name: name.to_owned(),
            proto: crate::dex::DexProto {
                shorty: shorty.to_owned(),
                return_type: return_type.to_owned(),
                parameters: if parameter.is_empty() {
                    vec!["Ljava/lang/String;".to_owned()]
                } else {
                    vec!["Ljava/lang/String;".to_owned(), parameter.to_owned()]
                },
            },
        };
    let put = |method: &DexMethodId, key: &str, value: DexValue| {
        assert_eq!(
            program.invoke_builtin_method(
                method,
                Some(&test_invocation(vec![
                    bundle.clone(),
                    DexValue::String(std::rc::Rc::from(key)),
                    value,
                ]))
            ),
            Some(DexExecutionResult::Void)
        );
    };
    let get = |method: &DexMethodId, key: &str| {
        program.invoke_builtin_method(
            method,
            Some(&test_invocation(vec![
                bundle.clone(),
                DexValue::String(std::rc::Rc::from(key)),
            ])),
        )
    };

    put(
        &scalar_method("putInt", "VLI", "V", "I"),
        "selection",
        DexValue::Int(-1),
    );
    put(
        &scalar_method("putBoolean", "VLZ", "V", "Z"),
        "expanded",
        DexValue::Int(1),
    );
    put(
        &scalar_method("putLong", "VLJ", "V", "J"),
        "timestamp",
        DexValue::Long(-9),
    );
    put(
        &scalar_method("putFloat", "VLF", "V", "F"),
        "scale",
        DexValue::Float(1.5),
    );
    put(
        &scalar_method("putDouble", "VLD", "V", "D"),
        "ratio",
        DexValue::Long(2.25f64.to_bits() as i64),
    );

    let get_object = scalar_method("get", "LL", "Ljava/lang/Object;", "");
    let Some(DexExecutionResult::Value(boxed_int)) = get(&get_object, "selection") else {
        panic!("get should return a value for a putInt entry");
    };
    assert_eq!(
        boxed_int,
        DexValue::Reference(std::rc::Rc::from("Ljava/lang/Integer;:-1"))
    );
    assert!(program.value_matches_type(&boxed_int, "Ljava/lang/Integer;"));
    let Some(DexExecutionResult::Value(boxed_boolean)) = get(&get_object, "expanded") else {
        panic!("get should return a value for a putBoolean entry");
    };
    assert!(program.value_matches_type(&boxed_boolean, "Ljava/lang/Boolean;"));

    assert_eq!(
        get(&scalar_method("getInt", "ILI", "I", ""), "selection"),
        Some(DexExecutionResult::Int(-1))
    );
    assert_eq!(
        get(&scalar_method("getBoolean", "ZL", "Z", ""), "expanded"),
        Some(DexExecutionResult::Int(1))
    );
    assert_eq!(
        get(&scalar_method("getLong", "JL", "J", ""), "timestamp"),
        Some(DexExecutionResult::Value(DexValue::Long(-9)))
    );
    assert_eq!(
        get(&scalar_method("getFloat", "FL", "F", ""), "scale"),
        Some(DexExecutionResult::Value(DexValue::Float(1.5)))
    );
    assert_eq!(
        get(&scalar_method("getDouble", "DL", "D", ""), "ratio"),
        Some(DexExecutionResult::Value(DexValue::Long(
            2.25f64.to_bits() as i64
        )))
    );

    // cite: frameworks-base BaseBundle.java#getInt
    put(
        &scalar_method("putInt", "VLI", "V", "I"),
        "mismatch",
        DexValue::Int(3),
    );
    assert_eq!(
        get(&scalar_method("getLong", "JL", "J", ""), "mismatch"),
        Some(DexExecutionResult::Value(DexValue::Long(0)))
    );
}

#[test]
fn builtin_bundle_copy_constructor_clones_source_entries() {
    // cite: frameworks-base Bundle.java#Bundle
    let program = DexProgram::new(Vec::new());
    let source = DexValue::Object(DexObjectRef {
        id: 601,
        class_descriptor: std::rc::Rc::from("Landroid/os/Bundle;"),
    });
    let copy = DexValue::Object(DexObjectRef {
        id: 602,
        class_descriptor: std::rc::Rc::from("Landroid/os/Bundle;"),
    });
    let put_int = DexMethodId {
        class_descriptor: "Landroid/os/BaseBundle;".to_owned(),
        name: "putInt".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VLI".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned(), "I".to_owned()],
        },
    };
    program
        .invoke_builtin_method(
            &put_int,
            Some(&test_invocation(vec![
                source.clone(),
                DexValue::String(std::rc::Rc::from("noteId")),
                DexValue::Int(42),
            ])),
        )
        .expect("putInt should succeed");

    let copy_constructor = DexMethodId {
        class_descriptor: "Landroid/os/Bundle;".to_owned(),
        name: "<init>".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Landroid/os/Bundle;".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &copy_constructor,
            Some(&test_invocation(vec![copy.clone(), source]))
        ),
        Some(DexExecutionResult::Void)
    );

    let get_int = DexMethodId {
        class_descriptor: "Landroid/os/BaseBundle;".to_owned(),
        name: "getInt".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "ILI".to_owned(),
            return_type: "I".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &get_int,
            Some(&test_invocation(vec![
                copy,
                DexValue::String(std::rc::Rc::from("noteId"))
            ]))
        ),
        Some(DexExecutionResult::Int(42))
    );
}

#[test]
fn builtin_intent_set_action_and_set_data_and_type_accept_null_as_zero() {
    // cite: frameworks-base Intent.java#setAction, setDataAndType
    let program = DexProgram::new(Vec::new());
    let intent = DexValue::Object(DexObjectRef {
        id: 701,
        class_descriptor: std::rc::Rc::from("Landroid/content/Intent;"),
    });
    let set_action = DexMethodId {
        class_descriptor: "Landroid/content/Intent;".to_owned(),
        name: "setAction".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLL".to_owned(),
            return_type: "Landroid/content/Intent;".to_owned(),
            parameters: vec!["Ljava/lang/String;".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &set_action,
            Some(&test_invocation(vec![
                intent.clone(),
                DexValue::String(std::rc::Rc::from("android.intent.action.VIEW"))
            ]))
        ),
        Some(DexExecutionResult::Value(intent.clone()))
    );

    let get_action = DexMethodId {
        class_descriptor: "Landroid/content/Intent;".to_owned(),
        name: "getAction".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "L".to_owned(),
            return_type: "Ljava/lang/String;".to_owned(),
            parameters: Vec::new(),
        },
    };
    assert_eq!(
        program.invoke_builtin_method(&get_action, Some(&test_invocation(vec![intent.clone()]))),
        Some(DexExecutionResult::Value(DexValue::String(
            std::rc::Rc::from("android.intent.action.VIEW")
        )))
    );

    let set_data_and_type = DexMethodId {
        class_descriptor: "Landroid/content/Intent;".to_owned(),
        name: "setDataAndType".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "LLLL".to_owned(),
            return_type: "Landroid/content/Intent;".to_owned(),
            parameters: vec![
                "Landroid/net/Uri;".to_owned(),
                "Ljava/lang/String;".to_owned(),
            ],
        },
    };
    let uri = DexValue::Reference(std::rc::Rc::from("Landroid/net/Uri;:content://notes/1"));
    assert_eq!(
        program.invoke_builtin_method(
            &set_data_and_type,
            Some(&test_invocation(vec![
                intent.clone(),
                uri,
                DexValue::Int(0),
            ]))
        ),
        Some(DexExecutionResult::Value(intent))
    );
}
