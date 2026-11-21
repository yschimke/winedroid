mod helpers;
mod missing_support;

use std::path::{Path, PathBuf};

mod activity_window;
mod android_content_resources;
mod android_content_storage;
mod android_context_services;
mod android_graphics;
mod android_layout_menu;
mod android_platform_misc;
mod android_text_image;
mod android_view_core;
mod android_widget_adapters;
mod androidx_lifecycle_work;
mod androidx_runtime_widget;
mod bootstrap_fields;
mod class_new_instance_contract;
mod content_storage_smoke;
mod coroutine_contract;
mod generic_motion_contract;
mod input;
mod java_box_locale;
mod java_concurrency_atomic;
mod java_core_runtime;
mod java_io;
mod java_json_collection_extra;
mod java_lists;
mod java_maps;
mod java_math_util;
mod java_reflection;
mod java_regex_util;
mod java_sets_collections;
mod java_string_text;
mod runtime_core;
mod sqlite_security;
mod static_bootstrap;
mod third_party_runtime;

#[test]
fn installed_runtime_dir_only_applies_to_installed_source_apk() {
    assert_eq!(
        super::installed_runtime_dir(Path::new("/tmp/winedroid/apps/com.example/source.apk")),
        Some(PathBuf::from("/tmp/winedroid/apps/com.example/runtime"))
    );
    assert_eq!(
        super::installed_runtime_dir(Path::new("/tmp/winedroid/apps/com.example/base.apk")),
        None
    );
    assert_eq!(super::installed_runtime_dir(Path::new("source.apk")), None);
}
