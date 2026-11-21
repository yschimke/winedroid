use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::apk::ApkArchive;
#[cfg(test)]
use crate::binary_xml::{self, XmlEvent};
use crate::config::{DisplaySize, WinedroidConfig};
use crate::dex::{
    self, DexAnnotation, DexAnnotationValue, DexClassDef, DexCodeItem, DexEncodedMethod,
    DexFieldId, DexFile, DexMethodId, DEX_VISIBILITY_RUNTIME,
};
#[cfg(all(feature = "jit", not(target_family = "wasm")))]
use crate::dex_interpreter::DexFrame;
use crate::dex_interpreter::{
    self, DexArrayLength, DexArrayRead, DexArrayWrite, DexCheckCastResult, DexClassAccess,
    DexExecutionResult, DexFieldAccess, DexFieldWrite, DexFilledNewArray, DexGeneratedException,
    DexInstanceOf, DexInvocation, DexInvocationFrame, DexInvocationHandler, DexInvocationRegisters,
    DexInvocationResult, DexInvokeKind, DexNewArray, DexNewInstance, DexObjectRef,
    DexParkedExecution, DexParkedStep, DexPendingResult, DexStaticFieldAccess,
    DexStaticFieldReadResult, DexStaticFieldWrite, DexStaticFieldWriteResult, DexStringAccess,
    DexTypeCheck, DexValue,
};
use crate::error::{Result, WinedroidError};
use crate::external_storage::ExternalStoragePolicy;
use crate::input::InputKey;
use crate::manifest;
use crate::resource_table::{self, ResourceFile, ResourceValue};
use crate::trace;

mod android;
mod bootstrap;
mod bootstrap_fields;
mod builtins;
mod collections;
#[cfg(all(feature = "jit", not(target_family = "wasm")))]
mod dex3oat_bridge;
mod heap;
mod helpers;
mod host_actions;
mod invocation;
mod java_big_decimal;
mod java_primitives;
mod java_text;
mod method_filters;
mod missing_support;
mod model;
mod reflection;
mod runtime;
mod sqlite;
mod storage;
mod view;
mod vm;

use android::content_res::*;
use android::os::*;
use bootstrap::*;
use bootstrap_fields::*;
use collections::*;
use heap::*;
use helpers::*;
use java_big_decimal::*;
use java_primitives::*;
use java_text::*;
use method_filters::*;
pub use model::*;
use reflection::*;
use runtime::*;
use sqlite::{SqliteSessionKey, SqliteTransactionError, SqliteTransactionState};
use view::*;
use vm::*;

const MAX_INVOCATION_DEPTH: usize = 512;
const DEX_ACC_STATIC: u32 = 0x0008;
const DEX_ACC_VOLATILE: u32 = 0x0040;
const JAVA_ACC_PUBLIC: u32 = 0x0001;
const JAVA_ACC_PRIVATE: u32 = 0x0002;
const JAVA_ACC_PROTECTED: u32 = 0x0004;
const JAVA_ACC_FINAL: u32 = 0x0010;
const JAVA_ACC_TRANSIENT: u32 = 0x0080;
const JAVA_ACC_INTERFACE: u32 = 0x0200;
const JAVA_ACC_ABSTRACT: u32 = 0x0400;
const JAVA_ACC_SYNTHETIC: u32 = 0x1000;
const BOOTSTRAP_ANDROID_SDK_INT: i32 = 35;
const ANDROID_PERMISSION_GRANTED: i32 = 0;
const ANDROID_PERMISSION_DENIED: i32 = -1;
const ANDROID_CALENDAR_HOUR_OF_DAY: i32 = 11;
const BOOTSTRAP_HOUR_OF_DAY: i32 = 12;
const BOOTSTRAP_PROCESS_NAME: &str = "winedroid.bootstrap";
const BOOTSTRAP_PROCESS_ID: i32 = 1000;
const BOOTSTRAP_PROCESS_UID: i32 = 10000;
#[cfg(test)]
const BOOTSTRAP_DISPLAY_WIDTH: i32 = 420;
#[cfg(test)]
const BOOTSTRAP_DISPLAY_HEIGHT: i32 = 760;
const BOOTSTRAP_DISPLAY_DENSITY: f32 = 1.0;
const BOOTSTRAP_DISPLAY_DENSITY_DPI: i32 = 160;
const BOOTSTRAP_FONT_SCALE: f32 = 1.0;
const ANDROID_CONTENT_VIEW_ID: i32 = 16908290;
// cite: frameworks-base screen_toolbar.xml#action_bar
const ANDROID_ACTION_BAR_VIEW_ID: i32 = 0x01ff_0001;
const XML_PULL_START_DOCUMENT: i32 = 0;
const XML_PULL_END_DOCUMENT: i32 = 1;
const XML_PULL_START_TAG: i32 = 2;
const XML_PULL_END_TAG: i32 = 3;
const ANDROID_INTENT_ACTION_VIEW: &str = "android.intent.action.VIEW";
const MAX_UI_THREAD_PUMP_PASSES: usize = 8;
const FRAME_INTERVAL_NANOS: i64 = 16_666_666;

type MethodCacheKey = u64;

fn method_cache_key(dex_file_index: usize, method_index: u32) -> MethodCacheKey {
    ((dex_file_index as u64) << 32) | u64::from(method_index)
}

// cite: art class_linker.cc#FieldTypeOrderFromFirstDescriptorCharacter
fn runtime_field_type_order(type_descriptor: &str) -> u8 {
    match type_descriptor.as_bytes().first() {
        Some(b'L') | Some(b'[') => 0,
        Some(b'J') => 1,
        Some(b'D') => 2,
        Some(b'I') => 3,
        Some(b'F') => 4,
        Some(b'C') => 5,
        Some(b'S') => 6,
        Some(b'Z') => 7,
        Some(b'B') => 8,
        _ => 0,
    }
}

pub fn run_dex_method(apk_path: &Path, class_name: &str, method_name: &str) -> Result<()> {
    let program = DexProgram::load(apk_path)?;
    let class_descriptor = normalize_class_name(class_name);
    let resolved = program.resolve_named_method(&class_descriptor, method_name)?;
    let initial_arguments = program.prepare_entrypoint_initial_arguments(&resolved)?;

    let result =
        program.execute_method(Some(apk_path), &resolved, initial_arguments.as_deref(), 0)?;
    if let DexExecutionResult::Parked(execution) = &result {
        program.discard_parked_execution(execution);
        return Err(WinedroidError::new(format!(
            "DEX entrypoint parked without a guest-thread scheduler\nclass: {class_descriptor}\nmethod: {method_name}"
        )));
    }
    println!("DEX result");
    println!("  {result}");

    Ok(())
}

pub(crate) fn start_activity_on_create_runtime_with_app_data_and_config(
    apk_path: &Path,
    activity_name: &str,
    app_data_dir: Option<&Path>,
    app_device_data_dir: Option<&Path>,
    shared_storage_dir: Option<&Path>,
    config: WinedroidConfig,
) -> Result<DexActivityRuntime> {
    let program = {
        let _span = trace::span("runtime", "load DEX program");
        DexProgram::load_with_app_data_dir_and_config(
            apk_path,
            app_data_dir,
            app_device_data_dir,
            shared_storage_dir,
            config,
        )?
    };
    let class_descriptor = normalize_class_name(activity_name);
    let started = {
        let _span = trace::span("runtime", "run Activity.onCreate lifecycle");
        program.start_activity_on_create(&class_descriptor, 0)?
    };

    Ok(DexActivityRuntime {
        program,
        class_descriptor: started.class_descriptor,
        method_signature: started.method_signature,
        result: started.result,
        last_view_snapshot: std::cell::RefCell::new(None),
    })
}

fn activity_receiver_argument(initial_arguments: &Option<Vec<DexValue>>) -> Result<DexValue> {
    let Some(arguments) = initial_arguments else {
        return Err(WinedroidError::new(
            "Activity onCreate did not receive an Activity receiver argument",
        ));
    };
    let Some(receiver) = arguments.first() else {
        return Err(WinedroidError::new(
            "Activity onCreate argument list is empty",
        ));
    };

    match receiver {
        DexValue::Object(_) | DexValue::Reference(_) => Ok(receiver.clone()),
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => Err(WinedroidError::new(format!(
            "Activity onCreate receiver is not object-backed\nreceiver: {receiver}"
        ))),
    }
}

impl DexActivityRuntime {
    pub fn activity_start(&self) -> DexActivityStart {
        let active = self.program.active_activity_start();

        DexActivityStart {
            class_descriptor: active
                .as_ref()
                .map(|activity| activity.class_descriptor.clone())
                .unwrap_or_else(|| self.class_descriptor.clone()),
            method_signature: active
                .as_ref()
                .map(|activity| activity.method_signature.clone())
                .unwrap_or_else(|| self.method_signature.clone()),
            result: active
                .map(|activity| activity.result)
                .unwrap_or_else(|| self.result.clone()),
            view_snapshot: self.view_snapshot(),
            resources: self.program.resources.clone(),
        }
    }

    pub fn view_snapshot(&self) -> DexViewSnapshot {
        self.program.view_snapshot()
    }

    pub fn click_view_id(&self, view_id: i32) -> Result<i32> {
        self.program.perform_view_click_by_id(view_id, 0)
    }

    pub fn click_preference(&self, preference_key: &str) -> Result<i32> {
        self.program.perform_preference_click(preference_key, 0)
    }

    pub fn click_touch(&self, x: u32, y: u32) -> Result<i32> {
        self.program
            .perform_runtime_view_touch(x as f32, y as f32, 0)
    }

    pub fn touch_hold(&self, x: u32, y: u32, hold_ms: u64) -> Result<i32> {
        self.program
            .perform_runtime_view_touch_hold(x as f32, y as f32, hold_ms, 0)
    }

    pub fn touch_swipe(
        &self,
        start: (u32, u32),
        end: (u32, u32),
        duration_ms: u64,
        steps: Option<u32>,
    ) -> Result<i32> {
        self.program.perform_runtime_view_swipe(
            start.0 as f32,
            start.1 as f32,
            end.0 as f32,
            end.1 as f32,
            duration_ms,
            steps,
            0,
        )
    }

    pub fn focus_view(&self, view_id: i32) -> Result<i32> {
        self.program.focus_view_by_id(view_id)
    }

    pub fn key_input(&self, characters: &str, key: InputKey) -> Result<i32> {
        self.program.handle_key_input(characters, key, 0)
    }

    pub fn click_dialog_button(&self, dialog_index: usize, button_index: usize) -> Result<i32> {
        self.program.click_dialog_button(dialog_index, button_index)
    }

    pub fn click_dialog_item(&self, dialog_index: usize, item_index: usize) -> Result<i32> {
        self.program.click_dialog_item(dialog_index, item_index, 0)
    }

    pub fn take_host_actions(&self) -> Vec<DexHostAction> {
        self.program.take_host_actions()
    }

    pub fn register_view_text(&self, view_id: i32, text: &str) -> Result<()> {
        // cite: frameworks-base TextView.java#TextView_text
        if self.program.view_text(view_id)?.is_some() {
            return Ok(());
        }

        self.program.write_view_text(view_id, text)
    }

    pub fn register_view_visibility(&self, view_id: i32, visibility: i32) -> Result<()> {
        self.program
            .write_view_visibility_value(view_id, visibility)
    }

    pub fn view_text(&self, view_id: i32) -> Result<Option<String>> {
        self.program.view_text(view_id)
    }

    pub fn activity_overlays(&self) -> Vec<DexActivityOverlay> {
        self.program.activity_overlays()
    }

    pub fn dialog_overlays(&self) -> Vec<DexDialogOverlay> {
        self.program.dialog_overlays()
    }

    pub fn dialog_content_layouts(&self) -> Vec<i32> {
        self.program.dialog_content_layouts()
    }

    pub fn preference_snapshot(&self) -> Vec<DexPreferenceSnapshot> {
        self.program.preference_snapshot()
    }

    pub fn dispatch_view_tree_draw_listeners(&self) -> Result<usize> {
        self.program.dispatch_view_tree_draw_listeners(0)
    }

    pub fn pump_ui_thread(&self, width: u32, height: u32) -> Result<DexUiPumpResult> {
        self.pump_ui_thread_with_budget(width, height, MAX_UI_THREAD_PUMP_PASSES, None)
    }

    pub fn pump_ui_thread_with_budget(
        &self,
        width: u32,
        height: u32,
        max_passes: usize,
        max_work_items_per_pass: Option<usize>,
    ) -> Result<DexUiPumpResult> {
        // cite: frameworks-base View.java#updateDisplayListIfDirty
        let before = self.last_view_snapshot.borrow_mut().take();
        let dispatch = self.program.dispatch_pending_main_loop_with_budget(
            width,
            height,
            0,
            max_passes,
            max_work_items_per_pass,
        )?;
        let view_snapshot_changed = if dispatch.dispatched == 0 && before.is_some() {
            *self.last_view_snapshot.borrow_mut() = before;
            false
        } else {
            let after = self.view_snapshot();
            let changed = before.as_ref().map_or(true, |before| before != &after);
            *self.last_view_snapshot.borrow_mut() = Some(after);
            changed
        };
        let runtime = self.program.runtime.borrow();
        let pending_non_frame_work = runtime.has_pending_non_frame_main_work();
        let pending_main_work = runtime.has_pending_main_work();

        Ok(DexUiPumpResult {
            dispatched: dispatch.dispatched,
            view_snapshot_changed,
            pending_non_frame_work,
            pending_main_work,
            work_budget_exhausted: dispatch.work_budget_exhausted,
        })
    }

    pub fn register_declared_on_click_handler(
        &self,
        view_id: i32,
        method_name: &str,
    ) -> Result<()> {
        self.program
            .write_declared_on_click_handler(view_id, method_name)
    }

    pub fn set_frame_interval_nanos(&self, frame_interval_nanos: i64) {
        self.program
            .runtime
            .borrow_mut()
            .set_frame_interval_nanos(frame_interval_nanos);
    }

    pub fn advance_frame_clock(&self, skipped_intervals: i64) {
        self.program
            .runtime
            .borrow_mut()
            .begin_driven_frame(skipped_intervals);
    }

    pub fn finish_driven_frame(&self) {
        self.program.runtime.borrow_mut().finish_driven_frame();
    }

    pub fn wants_frames(&self) -> bool {
        let runtime = self.program.runtime.borrow();
        runtime.has_pending_main_work() || runtime.has_delayed_handler_posts()
    }

    pub fn main_thread_idle(&self) -> bool {
        self.program.runtime.borrow().main_thread_is_idle()
    }

    pub fn next_wake_delay_millis(&self) -> Option<i64> {
        self.program.runtime.borrow().next_wake_delay_millis()
    }
}

struct DexProgram {
    strict_missing: bool,
    dex_files: Vec<DexFile>,
    package_name: Option<String>,
    target_sdk_version: i32,
    external_storage_policy: ExternalStoragePolicy,
    application_class_descriptor: Option<String>,
    activity_themes: HashMap<String, u32>,
    application_theme: Option<u32>,
    activity_labels: HashMap<String, u32>,
    application_label: Option<u32>,
    apk_path: Option<PathBuf>,
    #[cfg(test)]
    _test_app_data_dir: Option<PathBuf>,
    #[cfg(test)]
    app_device_data_dir: Option<PathBuf>,
    default_private_storage: crate::guest_filesystem::PrivateStorageKind,
    guest_filesystem: Option<crate::guest_filesystem::GuestFilesystem>,
    resources: Option<Vec<u8>>,
    config: WinedroidConfig,
    caches: RefCell<DexRuntimeCaches>,
    runtime: RefCell<DexRuntimeState>,
    sqlite_guest_database_paths:
        RefCell<HashMap<String, crate::guest_filesystem::ResolvedGuestPath>>,
    sqlite_sessions: RefCell<HashMap<SqliteSessionKey, RuntimeSqliteSession>>,
    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    jit_cache: RefCell<crate::jit::JitCache>,
}

enum RuntimeSqliteConnection {
    #[cfg(not(target_os = "emscripten"))]
    Guest(crate::capability_sqlite::CapabilityConnection),
    #[cfg(target_os = "emscripten")]
    Browser {
        connection: std::mem::ManuallyDrop<rusqlite::Connection>,
        path: BrowserSqlitePath,
    },
    #[cfg(test)]
    Host(rusqlite::Connection),
}

#[cfg(target_os = "emscripten")]
struct BrowserSqlitePath {
    host_path: PathBuf,
    _resolved: crate::guest_filesystem::ResolvedGuestPath,
    parent: crate::capability_fs::Directory,
    leaf: std::ffi::CString,
    parent_identity: crate::capability_fs::FileIdentity,
    database_identity: crate::capability_fs::FileIdentity,
}

impl RuntimeSqliteConnection {
    fn connection(&self) -> &rusqlite::Connection {
        match self {
            #[cfg(not(target_os = "emscripten"))]
            Self::Guest(connection) => connection.connection(),
            #[cfg(target_os = "emscripten")]
            Self::Browser { connection, .. } => connection,
            #[cfg(test)]
            Self::Host(connection) => connection,
        }
    }

    #[cfg(target_os = "emscripten")]
    fn validate_path(&self) -> std::io::Result<()> {
        match self {
            Self::Browser { path, .. } => path.validate(),
            #[cfg(test)]
            Self::Host(_) => Ok(()),
        }
    }

    #[cfg(not(target_os = "emscripten"))]
    fn validate_path(&self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for RuntimeSqliteConnection {
    fn drop(&mut self) {
        #[cfg(target_os = "emscripten")]
        match self {
            Self::Browser { connection, path } => match path.validate() {
                Ok(()) => unsafe { std::mem::ManuallyDrop::drop(connection) },
                Err(error) => {
                    eprintln!("SQLite browser connection retained after capability validation failed\n  error: {error}");
                }
            },
            #[cfg(test)]
            Self::Host(_) => {}
        }
    }
}

struct RuntimeSqliteSession {
    connection: RuntimeSqliteConnection,
    transaction: Option<SqliteTransactionState>,
}

struct MainLoopDispatchResult {
    dispatched: usize,
    work_budget_exhausted: bool,
}

impl DexProgram {
    fn display_size(&self) -> DisplaySize {
        self.config.display_size()
    }

    fn resource_resolution_config(&self) -> resource_table::ResourceResolutionConfig {
        let display_size = self.display_size();
        resource_table::ResourceResolutionConfig::android_with_display(
            BOOTSTRAP_ANDROID_SDK_INT as u16,
            self.config.android_configuration_ui_mode(),
            display_size.width() as u16,
            display_size.height() as u16,
        )
        .with_screen_round(self.config.display_shape().is_round())
    }

    #[cfg(test)]
    fn new(dex_files: Vec<DexFile>) -> Self {
        Self::with_package_name(dex_files, None)
    }

    #[cfg(test)]
    fn with_package_name(dex_files: Vec<DexFile>, package_name: Option<String>) -> Self {
        Self::with_resource_context(dex_files, package_name, None, None, None, None)
    }

    #[cfg(test)]
    fn with_config(dex_files: Vec<DexFile>, config: WinedroidConfig) -> Self {
        Self::with_resource_context_and_config(dex_files, None, None, None, None, None, config)
    }

    #[cfg(test)]
    fn with_application_class_descriptor(
        dex_files: Vec<DexFile>,
        application_class_descriptor: Option<String>,
    ) -> Self {
        Self::with_resource_context(
            dex_files,
            None,
            application_class_descriptor,
            None,
            None,
            None,
        )
    }

    #[cfg(test)]
    fn with_app_data_dir(dex_files: Vec<DexFile>, app_data_dir: PathBuf) -> Result<Self> {
        let program =
            Self::with_resource_context(dex_files, None, None, None, Some(app_data_dir), None);
        program.load_persisted_shared_preferences()?;

        Ok(program)
    }

    #[cfg(test)]
    fn with_guest_filesystem(
        dex_files: Vec<DexFile>,
        package_name: &str,
        app_data_dir: PathBuf,
        shared_storage_dir: PathBuf,
    ) -> Result<Self> {
        let app_device_data_dir = app_data_dir
            .parent()
            .ok_or_else(|| WinedroidError::new("app data directory has no app parent"))?
            .join("data_de");
        fs::create_dir_all(&app_device_data_dir).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to create device-protected app data directory `{}`",
                    app_device_data_dir.display()
                ),
                error,
            )
        })?;
        let mut program = Self::with_resource_context(
            dex_files,
            Some(package_name.to_owned()),
            None,
            None,
            Some(app_data_dir.clone()),
            None,
        );
        program.app_device_data_dir = Some(app_device_data_dir.clone());
        program.guest_filesystem = Some(crate::guest_filesystem::GuestFilesystem::new(
            package_name,
            app_data_dir,
            app_device_data_dir,
            shared_storage_dir,
        )?);
        program.load_persisted_shared_preferences()?;

        Ok(program)
    }

    #[cfg(test)]
    fn with_guest_filesystem_and_external_storage_policy(
        dex_files: Vec<DexFile>,
        package_name: &str,
        app_data_dir: PathBuf,
        shared_storage_dir: PathBuf,
        external_storage_policy: ExternalStoragePolicy,
    ) -> Result<Self> {
        let mut program =
            Self::with_guest_filesystem(dex_files, package_name, app_data_dir, shared_storage_dir)?;
        program.external_storage_policy = external_storage_policy;

        Ok(program)
    }

    #[cfg(test)]
    fn with_resource_context(
        dex_files: Vec<DexFile>,
        package_name: Option<String>,
        application_class_descriptor: Option<String>,
        apk_path: Option<PathBuf>,
        _app_data_dir: Option<PathBuf>,
        resources: Option<Vec<u8>>,
    ) -> Self {
        Self::with_resource_context_and_config(
            dex_files,
            package_name,
            application_class_descriptor,
            apk_path,
            _app_data_dir,
            resources,
            WinedroidConfig::default(),
        )
    }

    fn with_resource_context_and_config(
        dex_files: Vec<DexFile>,
        package_name: Option<String>,
        application_class_descriptor: Option<String>,
        apk_path: Option<PathBuf>,
        _app_data_dir: Option<PathBuf>,
        resources: Option<Vec<u8>>,
        config: WinedroidConfig,
    ) -> Self {
        let program = Self {
            strict_missing: crate::missing::strict_enabled(),
            dex_files,
            package_name,
            target_sdk_version: BOOTSTRAP_ANDROID_SDK_INT,
            external_storage_policy: ExternalStoragePolicy::restricted(BOOTSTRAP_ANDROID_SDK_INT),
            application_class_descriptor,
            activity_themes: HashMap::new(),
            application_theme: None,
            activity_labels: HashMap::new(),
            application_label: None,
            apk_path,
            #[cfg(test)]
            _test_app_data_dir: _app_data_dir,
            #[cfg(test)]
            app_device_data_dir: None,
            default_private_storage:
                crate::guest_filesystem::PrivateStorageKind::CredentialProtected,
            guest_filesystem: None,
            resources,
            config,
            caches: RefCell::new(DexRuntimeCaches::default()),
            #[cfg(all(feature = "jit", not(target_family = "wasm")))]
            jit_cache: RefCell::new(crate::jit::JitCache::new()),
            runtime: RefCell::new(DexRuntimeState::new()),
            sqlite_guest_database_paths: RefCell::new(HashMap::new()),
            sqlite_sessions: RefCell::new(HashMap::new()),
        };

        program.apply_configured_frame_interval();
        program.write_bootstrap_configuration_fields();

        program
    }

    fn apply_configured_frame_interval(&self) {
        if let crate::config::WinedroidFrameRate::Fixed { frames_per_second } =
            self.config.frame_rate()
        {
            let interval_nanos =
                crate::frame_clock::frame_interval_nanos_for_fps(frames_per_second);
            self.runtime
                .borrow_mut()
                .set_frame_interval_nanos(interval_nanos);
        }
    }

    fn load_class(&self, class_descriptor: &str) -> Option<Rc<LoadedClass>> {
        if let Some(cached) = self
            .caches
            .borrow()
            .class_defs
            .get(class_descriptor)
            .cloned()
        {
            return cached;
        }

        // cite: libcore ClassLoader.java#loadClass
        // cite: art class_linker.cc#ClassLinker::FindClassInBaseDexClassLoader
        let class_def = if is_bootstrap_class_descriptor(class_descriptor) {
            None
        } else {
            self.dex_files
                .iter()
                .enumerate()
                .find_map(|(dex_file_index, dex_file)| {
                    dex_file
                        .find_class(class_descriptor)
                        .cloned()
                        .map(|class_def| LoadedClass {
                            dex_file_index,
                            class_def,
                        })
                })
                .map(Rc::new)
        };

        self.caches
            .borrow_mut()
            .class_defs
            .insert(class_descriptor.to_owned(), class_def.clone());

        class_def
    }

    fn code_item(&self, resolved: &ResolvedMethod) -> Result<Rc<DexCodeItem>> {
        let key = CodeItemKey {
            dex_file_index: resolved.dex_file_index,
            code_offset: resolved.encoded_method.code_offset,
        };

        if let Some(code) = self.caches.borrow().code_items.get(&key).cloned() {
            return Ok(code);
        }

        let code = {
            let _span = trace::stat_span("perf", "dex parse code item");
            Rc::new(
                self.dex_files[resolved.dex_file_index]
                    .parse_code_item(resolved.encoded_method.code_offset)
                    .map_err(|error| method_execution_error(resolved, error))?,
            )
        };

        self.caches
            .borrow_mut()
            .code_items
            .insert(key, Rc::clone(&code));

        Ok(code)
    }

    fn method_intrinsic(&self, resolved: &ResolvedMethod) -> Result<Option<MethodIntrinsic>> {
        let key = method_intrinsic_key(resolved);
        if let Some(intrinsic) = self.caches.borrow().method_intrinsics.get(&key).copied() {
            return Ok(intrinsic);
        }

        let intrinsic = if resolved.encoded_method.code_offset == 0 {
            None
        } else {
            let code = self.code_item(resolved)?;
            detect_method_intrinsic(self, resolved, &code)?
        };

        self.caches
            .borrow_mut()
            .method_intrinsics
            .insert(key, intrinsic);

        Ok(intrinsic)
    }

    fn type_descriptor(&self, dex_file_index: usize, type_index: u32) -> Result<String> {
        Ok(self
            .dex_files
            .get(dex_file_index)
            .ok_or_else(|| WinedroidError::new("dex file index out of range"))?
            .type_descriptor(type_index)?
            .to_owned())
    }

    fn field_id(&self, dex_file_index: usize, field_index: u32) -> Result<Rc<DexFieldId>> {
        if let Some(Some(field)) = self
            .caches
            .borrow()
            .field_ids
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(field_index as usize))
        {
            return Ok(Rc::clone(field));
        }

        let field = Rc::new(self.dex_files[dex_file_index].field(field_index)?.clone());
        let mut caches = self.caches.borrow_mut();
        if caches.field_ids.len() <= dex_file_index {
            caches.field_ids.resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.field_ids[dex_file_index];
        if per_dex.len() <= field_index as usize {
            per_dex.resize(field_index as usize + 1, None);
        }
        per_dex[field_index as usize] = Some(Rc::clone(&field));

        Ok(field)
    }

    fn method_id(&self, dex_file_index: usize, method_index: u32) -> Result<Rc<DexMethodId>> {
        if let Some(Some(method)) = self
            .caches
            .borrow()
            .method_ids
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(method_index as usize))
        {
            return Ok(Rc::clone(method));
        }

        let method = Rc::new(self.dex_files[dex_file_index].method(method_index)?.clone());
        let mut caches = self.caches.borrow_mut();
        if caches.method_ids.len() <= dex_file_index {
            caches.method_ids.resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.method_ids[dex_file_index];
        if per_dex.len() <= method_index as usize {
            per_dex.resize(method_index as usize + 1, None);
        }
        per_dex[method_index as usize] = Some(Rc::clone(&method));

        Ok(method)
    }

    fn method_proto_descriptor(&self, dex_file_index: usize, method_index: u32) -> Result<Rc<str>> {
        if let Some(Some(descriptor)) = self
            .caches
            .borrow()
            .method_proto_descriptors
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(method_index as usize))
        {
            return Ok(Rc::clone(descriptor));
        }
        let method = self.method_id(dex_file_index, method_index)?;
        let descriptor: Rc<str> = Rc::from(method.proto.descriptor());
        let mut caches = self.caches.borrow_mut();
        if caches.method_proto_descriptors.len() <= dex_file_index {
            caches
                .method_proto_descriptors
                .resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.method_proto_descriptors[dex_file_index];
        if per_dex.len() <= method_index as usize {
            per_dex.resize(method_index as usize + 1, None);
        }
        per_dex[method_index as usize] = Some(Rc::clone(&descriptor));
        Ok(descriptor)
    }

    fn field_inline_cache(&self, dex_file_index: usize, field_index: u32) -> Option<(u32, u16)> {
        self.caches
            .borrow()
            .field_inline_caches
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(field_index as usize).copied().flatten())
    }

    fn store_field_inline_cache(
        &self,
        dex_file_index: usize,
        field_index: u32,
        cache: Option<(u32, u16)>,
    ) {
        let mut caches = self.caches.borrow_mut();
        if caches.field_inline_caches.len() <= dex_file_index {
            caches
                .field_inline_caches
                .resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.field_inline_caches[dex_file_index];
        if per_dex.len() <= field_index as usize {
            per_dex.resize(field_index as usize + 1, None);
        }
        per_dex[field_index as usize] = cache;
    }

    fn read_stored_instance_field_ic(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
        signature: &str,
    ) -> Option<DexValue> {
        let DexValue::Object(object_ref) = object else {
            return self.read_stored_instance_field_by_signature(object, signature);
        };
        let mut cache = self.field_inline_cache(dex_file_index, field_index);
        let before = cache;
        let value = self
            .runtime
            .borrow()
            .instance_fields
            .object_field_cached(object_ref.id, signature, &mut cache)
            .cloned();
        if cache != before {
            self.store_field_inline_cache(dex_file_index, field_index, cache);
        }
        value
    }

    fn read_stored_instance_field_resolved(
        &self,
        resolved: &ResolvedInstanceField,
        object: &DexValue,
    ) -> Result<Option<DexValue>> {
        let DexValue::Object(object_ref) = object else {
            return Ok(self.read_stored_instance_field_by_signature(object, &resolved.signature));
        };
        let Some(class_descriptor) = self
            .runtime
            .borrow()
            .instance_fields
            .object_class_descriptor(object_ref.id)
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        self.ensure_instance_class_layout(&class_descriptor)?;
        let Some(slot) = self.resolved_slot_for_class(resolved, &class_descriptor) else {
            return Ok(self
                .runtime
                .borrow()
                .instance_fields
                .object_field(object_ref.id, &resolved.signature)
                .cloned());
        };
        Ok(self
            .runtime
            .borrow()
            .instance_fields
            .object_field_slot(object_ref.id, slot)
            .cloned())
    }

    fn write_stored_instance_field_ic(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
        signature: &str,
        value: DexValue,
    ) -> bool {
        let DexValue::Object(object_ref) = object else {
            return self.write_stored_instance_field_by_signature(object, signature, value);
        };
        let mut cache = self.field_inline_cache(dex_file_index, field_index);
        let before = cache;
        self.runtime
            .borrow_mut()
            .instance_fields
            .set_object_field_cached(object_ref.id, signature, value, &mut cache);
        if cache != before {
            self.store_field_inline_cache(dex_file_index, field_index, cache);
        }
        true
    }

    fn write_stored_instance_field_resolved(
        &self,
        resolved: &ResolvedInstanceField,
        object: &DexValue,
        value: DexValue,
    ) -> Result<bool> {
        let DexValue::Object(object_ref) = object else {
            return Ok(self.write_stored_instance_field_by_signature(
                object,
                &resolved.signature,
                value,
            ));
        };
        let Some(class_descriptor) = self
            .runtime
            .borrow()
            .instance_fields
            .object_class_descriptor(object_ref.id)
            .map(str::to_owned)
        else {
            return Ok(false);
        };
        self.ensure_instance_class_layout(&class_descriptor)?;
        let Some(slot) = self.resolved_slot_for_class(resolved, &class_descriptor) else {
            let mut cache = None;
            self.runtime
                .borrow_mut()
                .instance_fields
                .set_object_field_cached(object_ref.id, &resolved.signature, value, &mut cache);
            return Ok(true);
        };
        Ok(self
            .runtime
            .borrow_mut()
            .instance_fields
            .set_object_field_slot(object_ref.id, slot, value))
    }

    fn resolved_slot_for_class(
        &self,
        resolved: &ResolvedInstanceField,
        class_descriptor: &str,
    ) -> Option<u16> {
        if class_descriptor == resolved.declaring_class {
            return Some(resolved.slot);
        }
        self.runtime
            .borrow()
            .instance_fields
            .class_field_slot(class_descriptor, &resolved.signature)
            .map(|(_layout, slot)| slot)
    }

    fn field_id_and_signature(
        &self,
        dex_file_index: usize,
        field_index: u32,
    ) -> Result<(Rc<DexFieldId>, Rc<str>)> {
        if let Some(Some(signature)) = self
            .caches
            .borrow()
            .field_signatures_by_index
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(field_index as usize))
        {
            let field = self.field_id(dex_file_index, field_index)?;
            return Ok((field, Rc::clone(signature)));
        }

        let field = self.field_id(dex_file_index, field_index)?;
        let signature = self.field_signature(&field);
        let mut caches = self.caches.borrow_mut();
        if caches.field_signatures_by_index.len() <= dex_file_index {
            caches
                .field_signatures_by_index
                .resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.field_signatures_by_index[dex_file_index];
        if per_dex.len() <= field_index as usize {
            per_dex.resize(field_index as usize + 1, None);
        }
        per_dex[field_index as usize] = Some(Rc::clone(&signature));

        Ok((field, signature))
    }

    fn field_signature_by_index(&self, dex_file_index: usize, field_index: u32) -> Result<Rc<str>> {
        if let Some(Some(signature)) = self
            .caches
            .borrow()
            .field_signatures_by_index
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(field_index as usize))
        {
            return Ok(Rc::clone(signature));
        }

        let field = self.field_id(dex_file_index, field_index)?;
        let signature = self.field_signature(&field);
        let mut caches = self.caches.borrow_mut();
        if caches.field_signatures_by_index.len() <= dex_file_index {
            caches
                .field_signatures_by_index
                .resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.field_signatures_by_index[dex_file_index];
        if per_dex.len() <= field_index as usize {
            per_dex.resize(field_index as usize + 1, None);
        }
        per_dex[field_index as usize] = Some(Rc::clone(&signature));

        Ok(signature)
    }

    fn field_signature(&self, field: &DexFieldId) -> Rc<str> {
        if let Some(signature) = self.caches.borrow().field_signatures.get(field) {
            return Rc::clone(signature);
        }

        let signature: Rc<str> = match framework_field_declaring_class(field) {
            Some(declaring_class) => Rc::from(format!(
                "{}.{}:{}",
                declaring_class, field.name, field.type_descriptor
            )),
            None => Rc::from(field.display_signature()),
        };
        self.caches
            .borrow_mut()
            .field_signatures
            .insert(field.clone(), Rc::clone(&signature));

        signature
    }

    fn resolved_instance_field(
        &self,
        dex_file_index: usize,
        field_index: u32,
    ) -> Result<Option<Rc<ResolvedInstanceField>>> {
        if let Some(Some(resolved)) = self
            .caches
            .borrow()
            .resolved_instance_fields
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(field_index as usize))
        {
            return Ok(Some(Rc::clone(resolved)));
        }

        let (field, signature) = self.field_id_and_signature(dex_file_index, field_index)?;
        self.ensure_instance_class_layout(&field.class_descriptor)?;
        let Some((_layout, slot)) = self
            .runtime
            .borrow()
            .instance_fields
            .class_field_slot(&field.class_descriptor, &signature)
        else {
            return Ok(None);
        };
        let access_flags = self.instance_field_access_flags(&field).unwrap_or(0);
        let resolved = Rc::new(ResolvedInstanceField {
            declaring_class: field.class_descriptor.clone(),
            type_descriptor: field.type_descriptor.clone(),
            access_flags,
            is_volatile: access_flags & DEX_ACC_VOLATILE != 0,
            slot,
            default_value: default_field_value(&field),
            field,
            signature,
        });
        self.store_resolved_instance_field(dex_file_index, field_index, &resolved);
        Ok(Some(resolved))
    }

    fn store_resolved_instance_field(
        &self,
        dex_file_index: usize,
        field_index: u32,
        resolved: &Rc<ResolvedInstanceField>,
    ) {
        let mut caches = self.caches.borrow_mut();
        if caches.resolved_instance_fields.len() <= dex_file_index {
            caches
                .resolved_instance_fields
                .resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.resolved_instance_fields[dex_file_index];
        if per_dex.len() <= field_index as usize {
            per_dex.resize(field_index as usize + 1, None);
        }
        per_dex[field_index as usize] = Some(Rc::clone(resolved));
    }

    fn resolved_static_field(
        &self,
        dex_file_index: usize,
        field_index: u32,
    ) -> Result<Option<Rc<ResolvedStaticField>>> {
        if let Some(Some(resolved)) = self
            .caches
            .borrow()
            .resolved_static_fields
            .get(dex_file_index)
            .and_then(|per_dex| per_dex.get(field_index as usize))
        {
            return Ok(Some(Rc::clone(resolved)));
        }

        let (field, signature) = self.field_id_and_signature(dex_file_index, field_index)?;
        self.ensure_static_class_layout(&field.class_descriptor)?;
        let Some(slot) = self
            .runtime
            .borrow()
            .static_fields
            .class_field_slot(&field.class_descriptor, &signature)
        else {
            return Ok(None);
        };
        let access_flags = self.static_field_access_flags(&field).unwrap_or(0);
        let resolved = Rc::new(ResolvedStaticField {
            declaring_class: field.class_descriptor.clone(),
            type_descriptor: field.type_descriptor.clone(),
            access_flags,
            is_volatile: access_flags & DEX_ACC_VOLATILE != 0,
            slot,
            default_value: default_field_value(&field),
            field,
            signature,
        });
        self.store_resolved_static_field(dex_file_index, field_index, &resolved);
        Ok(Some(resolved))
    }

    fn store_resolved_static_field(
        &self,
        dex_file_index: usize,
        field_index: u32,
        resolved: &Rc<ResolvedStaticField>,
    ) {
        let mut caches = self.caches.borrow_mut();
        if caches.resolved_static_fields.len() <= dex_file_index {
            caches
                .resolved_static_fields
                .resize(dex_file_index + 1, Vec::new());
        }
        let per_dex = &mut caches.resolved_static_fields[dex_file_index];
        if per_dex.len() <= field_index as usize {
            per_dex.resize(field_index as usize + 1, None);
        }
        per_dex[field_index as usize] = Some(Rc::clone(resolved));
    }

    fn ensure_instance_class_layout(&self, class_descriptor: &str) -> Result<()> {
        if self
            .runtime
            .borrow()
            .instance_fields
            .class_layout_defined(class_descriptor)
        {
            return Ok(());
        }

        let mut signatures = Vec::new();
        let mut visited = HashSet::new();
        self.collect_instance_layout_signatures(class_descriptor, &mut visited, &mut signatures)?;
        let mut runtime = self.runtime.borrow_mut();
        if !runtime
            .instance_fields
            .class_layout_defined(class_descriptor)
        {
            runtime
                .instance_fields
                .define_class_layout(class_descriptor, &signatures);
        }
        Ok(())
    }

    fn collect_instance_layout_signatures(
        &self,
        class_descriptor: &str,
        visited: &mut HashSet<String>,
        signatures: &mut Vec<Rc<str>>,
    ) -> Result<()> {
        if !visited.insert(class_descriptor.to_owned()) {
            return Ok(());
        }
        let Some(loaded_class) = self.load_class(class_descriptor) else {
            return Ok(());
        };
        if let Some(superclass) = loaded_class.class_def.superclass_descriptor.as_deref() {
            self.collect_instance_layout_signatures(superclass, visited, signatures)?;
        }
        for encoded_field in &loaded_class.class_def.instance_fields {
            let field = self.field_id(loaded_class.dex_file_index, encoded_field.field_index)?;
            signatures.push(self.field_signature(&field));
        }
        Ok(())
    }

    fn instance_field_access_flags(&self, field: &DexFieldId) -> Option<u32> {
        let loaded_class = self.load_class(&field.class_descriptor)?;
        for encoded_field in &loaded_class.class_def.instance_fields {
            let Ok(candidate) =
                self.field_id(loaded_class.dex_file_index, encoded_field.field_index)
            else {
                continue;
            };
            if candidate.as_ref() == field {
                return Some(encoded_field.access_flags);
            }
        }
        None
    }

    fn read_stored_static_field_resolved(
        &self,
        resolved: &ResolvedStaticField,
    ) -> Option<DexValue> {
        self.runtime
            .borrow()
            .static_fields
            .get_slot(&resolved.declaring_class, resolved.slot)
            .cloned()
    }

    fn write_stored_static_field_resolved(
        &self,
        resolved: &ResolvedStaticField,
        value: DexValue,
    ) -> bool {
        self.runtime.borrow_mut().static_fields.set_slot(
            &resolved.declaring_class,
            resolved.slot,
            value,
        )
    }

    fn ensure_static_class_layout(&self, class_descriptor: &str) -> Result<()> {
        if self
            .runtime
            .borrow()
            .static_fields
            .class_layout_defined(class_descriptor)
        {
            return Ok(());
        }

        let Some(loaded_class) = self.load_class(class_descriptor) else {
            return Ok(());
        };
        let mut ordered = Vec::new();
        for (index, encoded_field) in loaded_class.class_def.static_fields.iter().enumerate() {
            let field = self.field_id(loaded_class.dex_file_index, encoded_field.field_index)?;
            let signature = self.field_signature(&field);
            ordered.push((
                runtime_field_type_order(&field.type_descriptor),
                index,
                signature,
            ));
        }
        ordered.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let signatures = ordered
            .into_iter()
            .map(|(_order, _index, signature)| signature)
            .collect::<Vec<_>>();
        let mut runtime = self.runtime.borrow_mut();
        if !runtime.static_fields.class_layout_defined(class_descriptor) {
            runtime
                .static_fields
                .define_class_layout(class_descriptor, &signatures);
        }
        Ok(())
    }

    fn static_field_access_flags(&self, field: &DexFieldId) -> Option<u32> {
        let loaded_class = self.load_class(&field.class_descriptor)?;
        for encoded_field in &loaded_class.class_def.static_fields {
            let Ok(candidate) =
                self.field_id(loaded_class.dex_file_index, encoded_field.field_index)
            else {
                continue;
            };
            if candidate.as_ref() == field {
                return Some(encoded_field.access_flags);
            }
        }
        None
    }

    fn string_value(&self, dex_file_index: usize, string_index: u32) -> Result<String> {
        let key = StringIdKey {
            dex_file_index,
            string_index,
        };

        if let Some(value) = self.caches.borrow().strings.get(&key).cloned() {
            return Ok(value);
        }

        let value = self.dex_files[dex_file_index]
            .string(string_index)?
            .to_owned();
        self.caches.borrow_mut().strings.insert(key, value.clone());

        Ok(value)
    }

    fn type_descriptor_value(&self, dex_file_index: usize, type_index: u32) -> Result<String> {
        let key = TypeIdKey {
            dex_file_index,
            type_index,
        };

        if let Some(value) = self.caches.borrow().type_descriptors.get(&key).cloned() {
            return Ok(value);
        }

        let value = self.dex_files[dex_file_index]
            .type_descriptor(type_index)?
            .to_owned();
        self.caches
            .borrow_mut()
            .type_descriptors
            .insert(key, value.clone());

        Ok(value)
    }

    // cite: frameworks-base Resources.java#getIdentifier
    fn resource_identifier(&self, name: &str, def_type: &str, def_package: &str) -> i32 {
        let (package, rest) = name
            .split_once(':')
            .map_or((def_package, name), |(package, rest)| (package, rest));
        let (type_name, entry_name) = rest.split_once('/').unwrap_or((def_type, rest));
        if type_name.is_empty() || entry_name.is_empty() {
            return 0;
        }

        if package == "android" {
            // cite: frameworks-base public-final.xml#content, title, home
            return match (type_name, entry_name) {
                ("id", "content") => ANDROID_CONTENT_VIEW_ID,
                ("id", "title") => 0x0102_0016,
                ("id", "home") => 0x0102_002c,
                ("id", "action_bar") => ANDROID_ACTION_BAR_VIEW_ID,
                _ => 0,
            };
        }

        let Some(resources) = self.resources.as_ref() else {
            return 0;
        };
        resource_table::find_resource_id(resources, type_name, entry_name)
            .ok()
            .flatten()
            .and_then(|id| i32::try_from(id).ok())
            .unwrap_or(0)
    }

    fn resolve_resource_value(&self, resource_id: u32) -> Result<Option<ResourceValue>> {
        if let Some(cached) = self
            .caches
            .borrow()
            .resource_values
            .get(&resource_id)
            .cloned()
        {
            return Ok(cached);
        }

        let Some(resources) = self.resources.as_ref() else {
            return Ok(None);
        };
        let config = self.resource_resolution_config();
        let value =
            resource_table::resolve_resource_value_for_config(resources, resource_id, config)?;
        self.caches
            .borrow_mut()
            .resource_values
            .insert(resource_id, value.clone());

        Ok(value)
    }

    fn resolve_resource_file(&self, resource_id: u32) -> Result<Option<ResourceFile>> {
        if let Some(cached) = self
            .caches
            .borrow()
            .resource_files
            .get(&resource_id)
            .cloned()
        {
            return Ok(cached);
        }

        let Some(resources) = self.resources.as_ref() else {
            return Ok(None);
        };
        let file = resource_table::resolve_resource_file_for_config(
            resources,
            resource_id,
            self.resource_resolution_config(),
        )?;
        self.caches
            .borrow_mut()
            .resource_files
            .insert(resource_id, file.clone());

        Ok(file)
    }

    fn load(apk_path: &Path) -> Result<Self> {
        Self::load_with_app_data_dir(apk_path, None)
    }

    fn load_with_app_data_dir(apk_path: &Path, app_data_dir: Option<&Path>) -> Result<Self> {
        Self::load_with_app_data_dir_and_config(
            apk_path,
            app_data_dir,
            None,
            None,
            WinedroidConfig::default(),
        )
    }

    fn load_with_app_data_dir_and_config(
        apk_path: &Path,
        app_data_dir: Option<&Path>,
        app_device_data_dir: Option<&Path>,
        shared_storage_dir: Option<&Path>,
        config: WinedroidConfig,
    ) -> Result<Self> {
        #[cfg(not(all(feature = "jit", not(target_family = "wasm"))))]
        if let Some(flag) = requested_compiled_runtime_flag() {
            return Err(WinedroidError::new(format!(
                "compiled DEX execution requested by `{flag}`, but this build is interpreter-only\nunset `{flag}` or use a native build with the `jit` feature"
            )));
        }
        let manifest_context = {
            let _span = trace::span("runtime", "load manifest context");
            load_apk_manifest_context(apk_path)?
        };
        let dex_files = {
            let _span = trace::span("runtime", "load full DEX files");
            load_dex_files(apk_path)?
        };
        trace::event("runtime", format!("loaded {} DEX files", dex_files.len()));
        let resources = {
            let _span = trace::span("runtime", "load resources.arsc");
            load_apk_resources(apk_path)?
        };
        let mut program = Self::with_resource_context_and_config(
            dex_files,
            manifest_context.package_name.clone(),
            manifest_context.application_class_descriptor.clone(),
            Some(apk_path.to_path_buf()),
            app_data_dir.map(Path::to_path_buf),
            resources,
            config,
        );
        #[cfg(test)]
        {
            program.app_device_data_dir = app_device_data_dir.map(Path::to_path_buf);
        }
        program.apply_apk_manifest_context(manifest_context);
        crate::permissions::grant_declared_permissions(&mut program.external_storage_policy);
        program.guest_filesystem = match (
            program.package_name.as_deref(),
            app_data_dir,
            app_device_data_dir,
            shared_storage_dir,
        ) {
            (
                Some(package_name),
                Some(app_data_dir),
                Some(app_device_data_dir),
                Some(shared_storage_dir),
            ) => Some(crate::guest_filesystem::GuestFilesystem::new(
                package_name,
                app_data_dir,
                app_device_data_dir,
                shared_storage_dir,
            )?),
            _ => None,
        };
        {
            let _span = trace::span("runtime", "load persisted shared preferences");
            program.load_persisted_shared_preferences()?;
        }

        Ok(program)
    }

    fn apply_apk_manifest_context(&mut self, context: ApkManifestContext) {
        self.package_name = context.package_name;
        self.application_class_descriptor = context.application_class_descriptor;
        self.activity_themes = context.activity_themes;
        self.application_theme = context.application_theme;
        self.activity_labels = context.activity_labels;
        self.application_label = context.application_label;
        self.target_sdk_version = context.target_sdk_version;
        self.default_private_storage = context.default_private_storage;
        self.external_storage_policy = ExternalStoragePolicy::for_fresh_install(
            context.min_sdk_version,
            context.target_sdk_version,
            context.request_legacy_external_storage,
            context.requested_permissions,
        );
    }

    // cite: art interpreter.cc#Execute
    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    fn try_jit_execute(
        &self,
        resolved: &ResolvedMethod,
        code: &DexCodeItem,
        initial_arguments: Option<&[DexValue]>,
        depth: usize,
    ) -> Option<Result<DexExecutionResult>> {
        if !jit_dispatch_enabled() {
            return None;
        }
        if let Some(result) = self.try_jit_execute_integer(resolved, code, initial_arguments) {
            return Some(Ok(result));
        }
        self.try_jit_execute_object(resolved, code, initial_arguments, depth)
    }

    // cite: art quick_exception_handler.cc#DeoptimizeStackVisitor
    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    fn code_for_inlined_method(
        &self,
        dex_file_index: usize,
        method_index: u32,
        declaring_class_descriptor: &str,
    ) -> Result<Rc<DexCodeItem>> {
        let mut id = self
            .method_id(dex_file_index, method_index)?
            .as_ref()
            .clone();
        id.class_descriptor = declaring_class_descriptor.to_owned();
        let resolved = self
            .resolve_exact_method(&id)?
            .ok_or_else(|| WinedroidError::new("inlined callee unresolved on deopt"))?;
        self.code_item(&resolved)
    }

    // cite: art interpreter.cc#Execute
    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    fn exec_to_invocation(result: DexExecutionResult) -> DexInvocationResult {
        match result {
            DexExecutionResult::Void => DexInvocationResult::Void,
            DexExecutionResult::Int(value) => {
                DexInvocationResult::RawValue(DexPendingResult::from_value(DexValue::Int(value)))
            }
            DexExecutionResult::Value(value) => {
                DexInvocationResult::RawValue(DexPendingResult::from_value(value))
            }
            DexExecutionResult::Throw(value) => DexInvocationResult::Throw(value),
            DexExecutionResult::Parked(execution) => DexInvocationResult::Parked(execution),
        }
    }

    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    fn frame_window(
        flat: &DexFrame,
        frame: &crate::dex3oat::deopt::ResumePoint,
    ) -> Result<Vec<DexValue>> {
        let base = frame.reg_base as usize;
        flat.read_values_window(base, frame.reg_count as usize)
    }

    // cite: art quick_exception_handler.cc#DeoptimizeStackVisitor
    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    fn resume_deopt_stack(
        &self,
        stack: &[crate::dex3oat::deopt::ResumePoint],
        dex_file_index: usize,
        top_code: &DexCodeItem,
        flat: &DexFrame,
        innermost_result: Option<DexInvocationResult>,
        handler: &mut DexRunInvocationHandler,
    ) -> Result<DexExecutionResult> {
        let saved_class_descriptor = handler.current_class_descriptor.clone();
        let result = (|| {
            let inner_idx = stack
                .len()
                .checked_sub(1)
                .ok_or_else(|| WinedroidError::new("empty deopt stack"))?;
            let mut codes: Vec<Option<Rc<DexCodeItem>>> = Vec::with_capacity(stack.len());
            let mut class_descriptors = Vec::with_capacity(stack.len());
            for (idx, frame) in stack.iter().enumerate() {
                let class_descriptor = match frame.declaring_class_descriptor.as_deref() {
                    Some(descriptor) => descriptor.to_owned(),
                    None if idx == 0 => saved_class_descriptor.clone().ok_or_else(|| {
                        WinedroidError::new("outer deopt frame has no declaring class metadata")
                    })?,
                    None => {
                        return Err(WinedroidError::new(
                            "inlined deopt frame has no declaring class metadata",
                        ))
                    }
                };
                codes.push(if idx == 0 {
                    None
                } else {
                    Some(self.code_for_inlined_method(
                        dex_file_index,
                        frame.method_index,
                        &class_descriptor,
                    )?)
                });
                class_descriptors.push(class_descriptor);
            }
            let code_at =
                |idx: usize| -> &DexCodeItem { codes[idx].as_deref().unwrap_or(top_code) };

            let inner = &stack[inner_idx];
            handler.current_class_descriptor = Some(class_descriptors[inner_idx].clone());
            let mut result = match innermost_result {
                Some(invocation_result) => dex_interpreter::complete_invocation_from_pc(
                    code_at(inner_idx),
                    dex_file_index,
                    Self::frame_window(flat, inner)?,
                    inner.dex_pc,
                    invocation_result,
                    handler,
                )?,
                None => dex_interpreter::interpret_code_from_pc(
                    code_at(inner_idx),
                    dex_file_index,
                    Self::frame_window(flat, inner)?,
                    inner.dex_pc,
                    handler,
                )?,
            };
            for idx in (0..inner_idx).rev() {
                handler.current_class_descriptor = Some(class_descriptors[idx].clone());
                result = dex_interpreter::complete_invocation_from_pc(
                    code_at(idx),
                    dex_file_index,
                    Self::frame_window(flat, &stack[idx])?,
                    stack[idx].dex_pc,
                    Self::exec_to_invocation(result),
                    handler,
                )?;
            }
            Ok(result)
        })();
        handler.current_class_descriptor = saved_class_descriptor;
        result
    }

    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    fn try_jit_execute_object(
        &self,
        resolved: &ResolvedMethod,
        code: &DexCodeItem,
        initial_arguments: Option<&[DexValue]>,
        depth: usize,
    ) -> Option<Result<DexExecutionResult>> {
        match initial_arguments {
            Some(arguments) => {
                if arguments.len() != code.ins_size as usize {
                    return None;
                }
            }
            None => {
                if code.ins_size != 0 {
                    return None;
                }
            }
        }

        if crate::trace::enabled("dex3oat") {
            dex3oat_bridge::probe_coverage(self, resolved, code, initial_arguments);
        }

        let abi = crate::jit::ObjectMethodAbi {
            get_int: jit_rt_get_int,
            set_int: jit_rt_set_int,
            get_long: jit_rt_get_long,
            set_long: jit_rt_set_long,
            get_float_bits: jit_rt_get_float_bits,
            set_float_bits: jit_rt_set_float_bits,
            iget_int: jit_rt_iget_int,
            aget: jit_rt_aget,
            move_reg: jit_rt_move,
            iget_object: jit_rt_iget_object,
            iput_field: jit_rt_iput_field,
            sget_int: jit_rt_sget_int,
            sget_object: jit_rt_sget_object,
            invoke: jit_rt_invoke,
            move_result: jit_rt_move_result,
            return_int: jit_rt_return_int,
            return_void: jit_rt_return_void,
            return_object: jit_rt_return_object,
            new_instance: jit_rt_new_instance,
            const_string: jit_rt_const_string,
            const_class: jit_rt_const_class,
            instance_of: jit_rt_instance_of,
            bail: jit_rt_bail,
        };
        let first_param_reg = code.registers_size.saturating_sub(code.ins_size);
        let ready = self.jit_cache.borrow_mut().advance_object(
            resolved.dex_file_index,
            resolved.encoded_method.code_offset,
            &code.instructions,
            first_param_reg,
            abi,
            || {
                if std::env::var_os("WINEDROID_DEX3OAT_GRAPH").is_some() {
                    dex3oat_bridge::Dex3oatBridge::new(self, resolved.dex_file_index)
                        .object_ops_graph(resolved, code)
                } else if std::env::var_os("WINEDROID_DEX3OAT_JIT").is_some() {
                    dex3oat_bridge::Dex3oatBridge::new(self, resolved.dex_file_index)
                        .object_ops(resolved, code)
                        .map(|ops| (ops, crate::jit::CompiledMeta::default()))
                } else {
                    None
                }
            },
        );
        if !ready {
            return None;
        }

        let cgdiff_expected = if std::env::var_os("WD_CGDIFF").is_some() {
            let bridge = dex3oat_bridge::Dex3oatBridge::new(self, resolved.dex_file_index);
            match bridge.object_ops(resolved, code) {
                Some(ops)
                    if ops.iter().all(|op| {
                        !matches!(
                            op,
                            crate::jit::ObjectOp::Invoke { .. }
                                | crate::jit::ObjectOp::IputField { .. }
                        )
                    }) =>
                {
                    let mut cg_handler = DexRunInvocationHandler {
                        program: self,
                        current_dex_file_index: resolved.dex_file_index,
                        current_class_descriptor: Some(resolved.class_descriptor.clone()),
                        depth,
                    };
                    dex_interpreter::interpret_code_with_initial_arguments_in_context(
                        code,
                        initial_arguments,
                        &mut cg_handler,
                        depth,
                    )
                    .ok()
                }
                _ => None,
            }
        } else {
            None
        };

        let inline_headroom = if std::env::var_os("WINEDROID_DEX3OAT_JIT").is_some()
            || std::env::var_os("WINEDROID_DEX3OAT_GRAPH").is_some()
        {
            dex3oat_bridge::INLINE_REGISTER_BUDGET
        } else {
            0
        };
        let register_count = code.registers_size.saturating_add(inline_headroom);
        let mut frame = DexFrame::new_zeroed(register_count);
        if let Some(arguments) = initial_arguments {
            let Some(first_parameter) = code.registers_size.checked_sub(code.ins_size) else {
                return Some(Err(WinedroidError::new(format!(
                    "DEX code item has {} input register(s) but only {} total register(s)",
                    code.ins_size, code.registers_size
                ))));
            };
            for (index, value) in arguments.iter().enumerate() {
                let register = first_parameter + index as u16;
                if let Err(error) = frame.set_register_from_value(register, value) {
                    return Some(Err(error));
                }
            }
        }

        let mut handler = DexRunInvocationHandler {
            program: self,
            current_dex_file_index: resolved.dex_file_index,
            current_class_descriptor: Some(resolved.class_descriptor.clone()),
            depth,
        };
        let (compiled, meta) = self
            .jit_cache
            .borrow()
            .peek_object(resolved.dex_file_index, resolved.encoded_method.code_offset)?;
        let mut env = JitObjectEnv {
            frame: &mut frame,
            handler: &mut handler,
            pending_writes: Vec::new(),
            outcome: None,
            last_result: None,
            resume_pc: None,
            stop_result: None,
            stop_pc: 0,
            hard_error: None,
            invoked: false,
        };
        let status =
            unsafe { compiled.call(&mut env as *mut JitObjectEnv as *mut std::ffi::c_void) };
        let JitObjectEnv {
            pending_writes,
            outcome,
            resume_pc,
            stop_result,
            stop_pc,
            hard_error,
            invoked,
            ..
        } = env;
        if let Some(error) = hard_error {
            return Some(Err(error));
        }
        match status {
            0 => {
                for write in pending_writes {
                    if let Err(error) = handler.write_instance_field(write) {
                        return Some(Err(error));
                    }
                }
                if let (Some(expected), Some(actual)) = (&cgdiff_expected, &outcome) {
                    if expected != actual {
                        trace::event(
                            "cgdiff",
                            format!(
                                "MISMATCH {} expected={expected:?} got={actual:?}",
                                resolved.method.display_signature()
                            ),
                        );
                    }
                }
                outcome.map(Ok)
            }
            1 => {
                if !invoked {
                    return None;
                }
                for write in pending_writes {
                    if let Err(error) = handler.write_instance_field(write) {
                        return Some(Err(error));
                    }
                }
                let resume_pc = resume_pc?;
                trace::count("jit", "object method interpreter resumes", 1);
                match meta.deopt.get(&resume_pc).filter(|s| s.len() > 1) {
                    Some(stack) => Some(self.resume_deopt_stack(
                        stack,
                        resolved.dex_file_index,
                        code,
                        &frame,
                        None,
                        &mut handler,
                    )),
                    None => {
                        let pc = meta
                            .deopt
                            .get(&resume_pc)
                            .and_then(|s| s.first())
                            .map(|r| r.dex_pc)
                            .unwrap_or(resume_pc as usize);
                        let registers = match frame.read_all_values() {
                            Ok(registers) => registers,
                            Err(error) => return Some(Err(error)),
                        };
                        Some(dex_interpreter::interpret_code_from_pc(
                            code,
                            resolved.dex_file_index,
                            registers,
                            pc,
                            &mut handler,
                        ))
                    }
                }
            }
            3 => match stop_result {
                Some(DexInvocationResult::Parked(mut execution)) => {
                    match meta.deopt.get(&stop_pc).filter(|s| s.len() > 1) {
                        Some(stack) => {
                            for idx in (0..stack.len()).rev() {
                                let frame_class_descriptor =
                                    match stack[idx].declaring_class_descriptor.as_deref() {
                                        Some(descriptor) => descriptor.to_owned(),
                                        None if idx == 0 => resolved.class_descriptor.clone(),
                                        None => return Some(Err(WinedroidError::new(
                                            "parked deopt frame has no declaring class metadata",
                                        ))),
                                    };
                                let resolved_code = if idx == 0 {
                                    None
                                } else {
                                    match self.code_for_inlined_method(
                                        resolved.dex_file_index,
                                        stack[idx].method_index,
                                        &frame_class_descriptor,
                                    ) {
                                        Ok(code) => Some(code),
                                        Err(error) => return Some(Err(error)),
                                    }
                                };
                                let frame_code: &DexCodeItem =
                                    resolved_code.as_deref().unwrap_or(code);
                                dex_interpreter::park_caller_from_registers(
                                    &mut execution,
                                    resolved.dex_file_index,
                                    Some(&frame_class_descriptor),
                                    frame_code,
                                    match Self::frame_window(&frame, &stack[idx]) {
                                        Ok(registers) => registers,
                                        Err(error) => return Some(Err(error)),
                                    },
                                    stack[idx].dex_pc,
                                );
                            }
                        }
                        None => {
                            let pc = meta
                                .deopt
                                .get(&stop_pc)
                                .and_then(|s| s.first())
                                .map(|r| r.dex_pc)
                                .unwrap_or(stop_pc as usize);
                            let registers = match frame.read_all_values() {
                                Ok(registers) => registers,
                                Err(error) => return Some(Err(error)),
                            };
                            dex_interpreter::park_caller_from_registers(
                                &mut execution,
                                resolved.dex_file_index,
                                Some(&resolved.class_descriptor),
                                code,
                                registers,
                                pc,
                            );
                        }
                    }
                    Some(Ok(DexExecutionResult::Parked(execution)))
                }
                Some(other) => {
                    trace::count("jit", "object method invoke stops", 1);
                    match meta.deopt.get(&stop_pc).filter(|s| s.len() > 1) {
                        Some(stack) => Some(self.resume_deopt_stack(
                            stack,
                            resolved.dex_file_index,
                            code,
                            &frame,
                            Some(other),
                            &mut handler,
                        )),
                        None => {
                            let pc = meta
                                .deopt
                                .get(&stop_pc)
                                .and_then(|s| s.first())
                                .map(|r| r.dex_pc)
                                .unwrap_or(stop_pc as usize);
                            let registers = match frame.read_all_values() {
                                Ok(registers) => registers,
                                Err(error) => return Some(Err(error)),
                            };
                            Some(dex_interpreter::complete_invocation_from_pc(
                                code,
                                resolved.dex_file_index,
                                registers,
                                pc,
                                other,
                                &mut handler,
                            ))
                        }
                    }
                }
                None => Some(Err(WinedroidError::new(
                    "JIT invoke stop without a stop result",
                ))),
            },
            _ => {
                if invoked {
                    return Some(Err(WinedroidError::new(format!(
                        "JIT compiled method returned unknown status {status} after an invoke"
                    ))));
                }
                None
            }
        }
    }

    #[cfg(all(feature = "jit", not(target_family = "wasm")))]
    fn try_jit_execute_integer(
        &self,
        resolved: &ResolvedMethod,
        code: &DexCodeItem,
        initial_arguments: Option<&[DexValue]>,
    ) -> Option<DexExecutionResult> {
        match initial_arguments {
            Some(arguments) => {
                if arguments.len() != code.ins_size as usize
                    || !arguments
                        .iter()
                        .all(|value| matches!(value, DexValue::Int(_)))
                {
                    return None;
                }
            }
            None => {
                if code.ins_size != 0 {
                    return None;
                }
            }
        }

        let register_count = code.registers_size;
        let ready = self.jit_cache.borrow_mut().advance_integer(
            resolved.dex_file_index,
            resolved.encoded_method.code_offset,
            &code.instructions,
            register_count,
        );
        if !ready {
            return None;
        }

        let mut registers = vec![0i32; register_count as usize];
        if let Some(arguments) = initial_arguments {
            let first_parameter = register_count.checked_sub(code.ins_size)?;
            for (index, value) in arguments.iter().enumerate() {
                let DexValue::Int(int_value) = value else {
                    return None;
                };
                registers[first_parameter as usize + index] = *int_value;
            }
        }

        let cache = self.jit_cache.borrow();
        let compiled =
            cache.peek_integer(resolved.dex_file_index, resolved.encoded_method.code_offset)?;
        Some(DexExecutionResult::Int(compiled.call(&registers)))
    }

    fn execute_method(
        &self,
        apk_path: Option<&Path>,
        resolved: &ResolvedMethod,
        initial_arguments: Option<&[DexValue]>,
        depth: usize,
    ) -> Result<DexExecutionResult> {
        let method_started_at = trace::enabled("perf").then(Instant::now);
        let _method_stat = trace::stat_span("perf", "dex execute_method wall");
        trace::count("perf", "dex execute_method calls", 1);
        record_method_count_histogram(resolved);
        if depth > MAX_INVOCATION_DEPTH {
            return Err(WinedroidError::new(format!(
                "DEX invocation depth exceeded {MAX_INVOCATION_DEPTH}"
            )));
        }

        if resolved.encoded_method.code_offset == 0 {
            if let Some(result) = self.invoke_builtin_method(&resolved.method, None) {
                trace_method_result(depth, resolved, &result);
                return Ok(result);
            }

            return self.missing_code_method(&resolved.method);
        }

        let code = self.code_item(resolved)?;

        if let Some(apk_path) = apk_path {
            print_dex_run_summary(apk_path, resolved, &code);
        }

        let mut invocation_handler = DexRunInvocationHandler {
            program: self,
            current_dex_file_index: resolved.dex_file_index,
            current_class_descriptor: Some(resolved.class_descriptor.clone()),
            depth,
        };

        let entrypoint_arguments = if initial_arguments.is_none() && depth == 0 {
            self.entrypoint_initial_arguments(resolved, &code)
        } else {
            None
        };
        let effective_initial_arguments = initial_arguments.or(entrypoint_arguments.as_deref());

        #[cfg(all(feature = "jit", not(target_family = "wasm")))]
        if let Some(result) =
            self.try_jit_execute(resolved, &code, effective_initial_arguments, depth)
        {
            let result = result.map_err(|error| method_execution_error(resolved, error));
            if let Ok(result) = &result {
                trace_method_result(depth, resolved, result);
            }
            return result;
        }

        let _method_time_scope =
            method_wall_histogram_enabled().then(|| MethodTimeScope::enter(resolved));
        let result = {
            let _span = trace::stat_span("perf", "dex interpret code");
            dex_interpreter::interpret_code_with_initial_arguments_in_context(
                &code,
                effective_initial_arguments,
                &mut invocation_handler,
                resolved.dex_file_index,
            )
            .map_err(|error| method_execution_error(resolved, error))
        };
        let elapsed = method_started_at.map(|started| started.elapsed());
        if let Some(elapsed) = elapsed {
            if elapsed >= Duration::from_millis(100) {
                trace::event(
                    "perf",
                    format!(
                        "slow interpreted method class={} method={} depth={} took {:.2}ms",
                        resolved.class_descriptor,
                        resolved.method.display_signature(),
                        depth,
                        elapsed.as_secs_f64() * 1000.0
                    ),
                );
            }
        }

        if let Ok(result) = &result {
            trace_method_result(depth, resolved, result);
        }

        result
    }

    fn execute_method_from_invocation_frame(
        &self,
        resolved: &ResolvedMethod,
        invocation: &DexInvocationFrame<'_>,
        depth: usize,
    ) -> Result<DexExecutionResult> {
        if !method_execution_observers_enabled() {
            return self.execute_method_from_invocation_frame_plain(resolved, invocation, depth);
        }

        let method_started_at = trace::enabled("perf").then(Instant::now);
        let _method_stat = trace::stat_span("perf", "dex execute_method wall");
        trace::count("perf", "dex execute_method calls", 1);
        record_method_count_histogram(resolved);
        let _method_time_scope =
            method_wall_histogram_enabled().then(|| MethodTimeScope::enter(resolved));
        let result = self.execute_method_from_invocation_frame_plain(resolved, invocation, depth);
        let elapsed = method_started_at.map(|started| started.elapsed());
        if let Some(elapsed) = elapsed {
            if elapsed >= Duration::from_millis(100) {
                trace::event(
                    "perf",
                    format!(
                        "slow interpreted method class={} method={} depth={} took {:.2}ms",
                        resolved.class_descriptor,
                        resolved.method.display_signature(),
                        depth,
                        elapsed.as_secs_f64() * 1000.0
                    ),
                );
            }
        }

        if let Ok(result) = &result {
            trace_method_result(depth, resolved, result);
        }

        result
    }

    fn execute_method_from_invocation_frame_plain(
        &self,
        resolved: &ResolvedMethod,
        invocation: &DexInvocationFrame<'_>,
        depth: usize,
    ) -> Result<DexExecutionResult> {
        if depth > MAX_INVOCATION_DEPTH {
            return Err(WinedroidError::new(format!(
                "DEX invocation depth exceeded {MAX_INVOCATION_DEPTH}"
            )));
        }
        if resolved.encoded_method.code_offset == 0 {
            return self.missing_code_method(&resolved.method);
        }

        let code = self.code_item(resolved)?;
        let frame = dex_interpreter::frame_from_invocation_arguments(&code, invocation)?;
        let mut invocation_handler = DexRunInvocationHandler {
            program: self,
            current_dex_file_index: resolved.dex_file_index,
            current_class_descriptor: Some(resolved.class_descriptor.clone()),
            depth,
        };
        dex_interpreter::interpret_code_with_frame_in_context(
            &code,
            frame,
            &mut invocation_handler,
            resolved.dex_file_index,
        )
        .map_err(|error| method_execution_error(resolved, error))
    }

    fn bootstrap_process_name(&self) -> String {
        self.package_name
            .clone()
            .unwrap_or_else(|| BOOTSTRAP_PROCESS_NAME.to_owned())
    }

    fn application_value(&self) -> DexValue {
        if let Some(application) = self.runtime.borrow().application_instance.clone() {
            return application;
        }

        let descriptor = self
            .application_class_descriptor
            .as_deref()
            .unwrap_or("Landroid/app/Application;");

        DexValue::Reference(std::rc::Rc::from(format!("{descriptor}.bootstrap")))
    }

    fn active_context_value(&self) -> DexValue {
        if let Some(activity) = self.active_activity_receiver() {
            return activity;
        }

        if let Some(application) = self.runtime.borrow().application_instance.clone() {
            return application;
        }

        DexValue::Reference(std::rc::Rc::from("Landroid/content/Context;.bootstrap"))
    }

    fn ensure_application_initialized(&self, depth: usize) -> Result<()> {
        if self.runtime.borrow().application_instance.is_some() {
            return Ok(());
        }

        let Some(class_descriptor) = self.application_class_descriptor.clone() else {
            return Ok(());
        };

        let application = self.allocate_object(class_descriptor.clone());
        self.runtime.borrow_mut().application_instance = Some(application.clone());

        self.execute_optional_void_method(
            &class_descriptor,
            "<init>",
            &[application.clone()],
            depth + 1,
        )?;
        self.execute_optional_void_method(&class_descriptor, "onCreate", &[application], depth + 1)
    }

    fn execute_optional_void_method(
        &self,
        class_descriptor: &str,
        method_name: &str,
        arguments: &[DexValue],
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
        let Some(resolved) = self.resolve_exact_method(&method)? else {
            return Ok(());
        };
        let result = self.execute_method(None, &resolved, Some(arguments), depth)?;

        match &result {
            DexExecutionResult::Void => return Ok(()),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(execution);
            }
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => {}
        }

        Err(WinedroidError::new(format!(
            "Application method returned a value\nclass: {class_descriptor}\nmethod: {}\nresult: {result}",
            resolved.method.display_signature()
        )))
    }

    fn dispatch_pending_main_loop(&self, width: u32, height: u32, depth: usize) -> Result<usize> {
        Ok(self
            .dispatch_pending_main_loop_with_budget(
                width,
                height,
                depth,
                MAX_UI_THREAD_PUMP_PASSES,
                None,
            )?
            .dispatched)
    }

    fn dispatch_pending_main_loop_with_budget(
        &self,
        width: u32,
        height: u32,
        depth: usize,
        max_passes: usize,
        max_work_items_per_pass: Option<usize>,
    ) -> Result<MainLoopDispatchResult> {
        // cite: frameworks-base ActivityThread.java#main
        self.with_current_guest_thread(main_guest_thread_reference(), || {
            self.dispatch_pending_main_loop_as_main_thread(
                width,
                height,
                depth,
                max_passes,
                max_work_items_per_pass,
            )
        })
    }

    fn dispatch_pending_main_loop_as_main_thread(
        &self,
        width: u32,
        height: u32,
        depth: usize,
        max_passes: usize,
        max_work_items_per_pass: Option<usize>,
    ) -> Result<MainLoopDispatchResult> {
        let mut dispatched = 0usize;
        let mut dispatched_frame_batch = false;
        let mut work_budget_exhausted = false;

        for pass in 0..max_passes {
            let work = self
                .runtime
                .borrow_mut()
                .take_main_thread_work_with_limit(!dispatched_frame_batch, max_work_items_per_pass);
            let work_items = work.tasks.len()
                + work.handler_posts.len()
                + work.choreographer_callbacks.len()
                + work.frame_callbacks.len()
                + work.thread_resumes.len();
            let has_frame_callbacks =
                !work.choreographer_callbacks.is_empty() || !work.frame_callbacks.is_empty();
            let traversal_pending = self.runtime.borrow().has_dispatchable_traversal_request();

            if work.tasks.is_empty()
                && work.handler_posts.is_empty()
                && work.choreographer_callbacks.is_empty()
                && work.frame_callbacks.is_empty()
                && work.thread_resumes.is_empty()
                && !traversal_pending
            {
                let frame_interval_millis_ceil = self.runtime.borrow().frame_interval_millis_ceil();
                let advanced_to_timed_event = self
                    .runtime
                    .borrow_mut()
                    .advance_main_thread_uptime_to_next_timed_event(frame_interval_millis_ceil);
                if advanced_to_timed_event {
                    self.runtime.borrow_mut().resume_expired_parked_threads();
                    trace::event(
                        "runtime",
                        format!("main loop pass {pass} advanced uptime to the next timed event"),
                    );
                    continue;
                }

                trace::event(
                    "runtime",
                    format!("main loop pass {pass} stopped with no pending main-thread work"),
                );
                break;
            }

            trace::event(
                "runtime",
                format!(
                    "main loop pass {pass} tasks={} handler_posts={} frame_callbacks={} thread_resumes={} traversal_pending={}",
                    work.tasks.len(),
                    work.handler_posts.len(),
                    work.choreographer_callbacks.len() + work.frame_callbacks.len(),
                    work.thread_resumes.len(),
                    traversal_pending
                ),
            );

            for task in work.tasks {
                trace::event(
                    "runtime",
                    format!("main loop pass {pass} dispatch task {task:?}"),
                );
                {
                    let _span = trace::span("runtime", "dispatch main-thread task");
                    self.dispatch_main_thread_task(task, depth + 1)?;
                }
                dispatched += 1;
            }

            for post in work.handler_posts {
                trace::event(
                    "runtime",
                    format!(
                        "main loop pass {pass} dispatch Handler.post handler={} runnable={} delay_ms={} due_time_ms={}",
                        post.handler_key, post.runnable, post.delay_millis, post.due_time_millis
                    ),
                );
                {
                    let _span = trace::span("runtime", "dispatch Handler.post callback");
                    self.dispatch_runnable(&post.runnable, "Handler.post", depth + 1)?;
                }
                dispatched += 1;
            }

            if has_frame_callbacks {
                let frame_time_nanos = {
                    let mut runtime = self.runtime.borrow_mut();
                    let frame_time_nanos = runtime.take_frame_time_nanos();
                    runtime.advance_main_thread_uptime_to_frame(frame_time_nanos);
                    frame_time_nanos
                };
                for callback in work.choreographer_callbacks {
                    trace::event(
                        "runtime",
                        format!(
                            "main loop pass {pass} dispatch Choreographer callback type={} action={} frame_time_nanos={frame_time_nanos}",
                            callback.callback_type, callback.action
                        ),
                    );
                    {
                        let method_metric_start = crate::method_metrics::start_timing();
                        let _span =
                            trace::span("runtime", "dispatch Choreographer runnable callback");
                        let result = self.dispatch_runnable(
                            &callback.action,
                            "Choreographer callback",
                            depth + 1,
                        );
                        crate::method_metrics::record_render_callback_elapsed(method_metric_start);
                        result?;
                    }
                    dispatched += 1;
                }
                for callback in work.frame_callbacks {
                    trace::event(
                        "runtime",
                        format!(
                            "main loop pass {pass} dispatch frame callback {callback} frame_time_nanos={frame_time_nanos}"
                        ),
                    );
                    {
                        let method_metric_start = crate::method_metrics::start_timing();
                        let _span = trace::span("runtime", "dispatch Choreographer frame callback");
                        let result =
                            self.dispatch_frame_callback(&callback, frame_time_nanos, depth + 1);
                        crate::method_metrics::record_render_callback_elapsed(method_metric_start);
                        result?;
                    }
                    dispatched += 1;
                }
            }
            if has_frame_callbacks {
                dispatched_frame_batch = true;
            }

            for resume in work.thread_resumes {
                trace::event(
                    "runtime",
                    format!(
                        "main loop pass {pass} resume parked thread {}",
                        resume.thread
                    ),
                );
                {
                    let _span = trace::span("runtime", "resume parked Java thread");
                    self.resume_parked_thread(resume.thread, resume.execution, depth + 1)?;
                }
                dispatched += 1;
            }

            if self.runtime.borrow().has_dispatchable_traversal_request() {
                trace::event(
                    "runtime",
                    format!("main loop pass {pass} dispatch ViewRoot traversal"),
                );
                // cite: frameworks-base ViewRootImpl.java#performTraversals
                let cancel_draw = {
                    let _span = trace::span("runtime", "dispatch ViewTreeObserver pre-draw");
                    self.dispatch_view_tree_pre_draw_listeners(depth + 1)?
                };
                if cancel_draw {
                    trace::event(
                        "runtime",
                        format!("main loop pass {pass} traversal canceled by pre-draw listener"),
                    );
                } else {
                    let render_metric_start = crate::render_metrics::start_timing();
                    let frames = {
                        let _span = trace::span("runtime", "dispatch ViewRoot traversal");
                        self.dispatch_runtime_view_frames(width, height, depth + 1)?
                    };
                    crate::render_metrics::record_traversal_elapsed(render_metric_start);
                    dispatched += frames;
                    if frames > 0 {
                        let listeners = {
                            let _span =
                                trace::span("runtime", "dispatch ViewTreeObserver global layout");
                            self.dispatch_view_tree_global_layout_listeners(depth + 1)?
                        };
                        dispatched += listeners;
                    }
                }
            }

            trace::event(
                "runtime",
                format!("main loop pass {pass} callbacks complete for {width}x{height} surface"),
            );

            if let Some(max_work_items) = max_work_items_per_pass {
                if work_items >= max_work_items && self.runtime.borrow().has_pending_main_work() {
                    work_budget_exhausted = true;
                }
            }
        }

        Ok(MainLoopDispatchResult {
            dispatched,
            work_budget_exhausted,
        })
    }

    fn dispatch_main_thread_task(&self, task: MainThreadTask, depth: usize) -> Result<()> {
        match task {
            MainThreadTask::LaunchActivity { class_descriptor } => {
                self.dispatch_activity_launch(&class_descriptor, depth)?;
            }
            MainThreadTask::InvalidateOptionsMenu { activity } => {
                let Some(class_descriptor) = value_class_descriptor(&activity).map(str::to_owned)
                else {
                    return Err(WinedroidError::new(format!(
                        "invalidateOptionsMenu requires an object-backed activity\nactivity: {activity}"
                    )));
                };
                self.capture_options_menu(&class_descriptor, &activity, depth)?;
            }
            MainThreadTask::StartThread { thread } => {
                self.run_started_thread(&thread, depth)?;
            }
        }

        Ok(())
    }

    // cite: libcore Thread.java#start, run
    fn run_started_thread(&self, thread: &DexValue, depth: usize) -> Result<()> {
        self.with_current_guest_thread(thread.clone(), || {
            self.run_started_thread_as_current(thread, depth)
        })
    }

    fn run_started_thread_as_current(&self, thread: &DexValue, depth: usize) -> Result<()> {
        let Some(class_descriptor) = value_class_descriptor(thread).map(str::to_owned) else {
            return Err(WinedroidError::new(format!(
                "Thread.start requires an object-backed Thread receiver\nreceiver: {thread}"
            )));
        };
        let run_method = DexMethodId {
            class_descriptor: class_descriptor.clone(),
            name: "run".to_owned(),
            proto: dex::DexProto {
                shorty: "V".to_owned(),
                return_type: "V".to_owned(),
                parameters: Vec::new(),
            },
        };

        if let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(&class_descriptor, &run_method)?
        {
            let result =
                self.execute_method(None, &resolved, Some(&[thread.clone()]), depth + 1)?;
            return self.finish_started_thread(thread, result, &class_descriptor);
        }

        if let Some(target) = self.thread_target(thread) {
            if target != DexValue::Null {
                let Some(target_descriptor) = value_class_descriptor(&target).map(str::to_owned)
                else {
                    return Err(WinedroidError::new(format!(
                        "Thread.start target requires an object-backed Runnable\ntarget: {target}"
                    )));
                };
                let target_run_method = DexMethodId {
                    class_descriptor: target_descriptor.clone(),
                    name: "run".to_owned(),
                    proto: dex::DexProto {
                        shorty: "V".to_owned(),
                        return_type: "V".to_owned(),
                        parameters: Vec::new(),
                    },
                };
                let Some(resolved) = self.resolve_exact_method_in_class_hierarchy(
                    &target_descriptor,
                    &target_run_method,
                )?
                else {
                    return Err(WinedroidError::new(format!(
                        "Thread.start target Runnable.run() was not found\nrunnable class: {target_descriptor}"
                    )));
                };
                let result = self.execute_method(None, &resolved, Some(&[target]), depth + 1)?;
                return self.finish_started_thread(thread, result, &target_descriptor);
            }
        }

        Ok(())
    }

    fn finish_started_thread(
        &self,
        thread: &DexValue,
        result: DexExecutionResult,
        class_descriptor: &str,
    ) -> Result<()> {
        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Parked(execution) => {
                let cleanup_execution = execution.clone();
                if self
                    .runtime
                    .borrow_mut()
                    .store_parked_thread(thread, execution)
                    .is_some()
                {
                    return Ok(());
                }
                self.discard_parked_execution(&cleanup_execution);
                Err(WinedroidError::new(format!(
                    "Thread.start could not store parked thread\nthread: {thread}"
                )))
            }
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(format!(
                "Thread run() did not complete normally\nclass: {class_descriptor}"
            ))),
        }
    }

    fn dispatch_runnable(&self, runnable: &DexValue, context: &str, depth: usize) -> Result<()> {
        // cite: libcore FutureTask.java#run
        if value_class_descriptor(runnable) == Some("Ljava/util/concurrent/FutureTask;") {
            return self.future_task_run(runnable, depth);
        }

        self.dispatch_object_void_method(
            runnable,
            "run",
            dex::DexProto {
                shorty: "V".to_owned(),
                return_type: "V".to_owned(),
                parameters: Vec::new(),
            },
            Vec::new(),
            context,
            depth,
        )
    }

    // cite: libcore FutureTask.java#run
    fn future_task_run(&self, receiver: &DexValue, depth: usize) -> Result<()> {
        self.with_current_guest_thread(background_pool_thread_reference(), || {
            self.future_task_run_as_current(receiver, depth)
        })
    }

    fn future_task_run_as_current(&self, receiver: &DexValue, depth: usize) -> Result<()> {
        if self.read_stored_instance_field(receiver, &future_task_done_field())
            == Some(DexValue::Int(1))
        {
            return Ok(());
        }

        if let Some(callable) =
            self.read_stored_instance_field(receiver, &future_task_callable_field())
        {
            if callable != DexValue::Null {
                return self.future_task_run_callable(receiver, &callable, depth);
            }
        }
        if let Some(runnable) =
            self.read_stored_instance_field(receiver, &future_task_runnable_field())
        {
            if runnable != DexValue::Null {
                return self.future_task_run_runnable(receiver, &runnable, depth);
            }
        }

        Err(WinedroidError::new(
            "FutureTask.run has neither a callable nor a runnable",
        ))
    }

    // cite: libcore FutureTask.java#run
    fn future_task_run_callable(
        &self,
        receiver: &DexValue,
        callable: &DexValue,
        depth: usize,
    ) -> Result<()> {
        let Some(class_descriptor) = value_class_descriptor(callable).map(str::to_owned) else {
            return Err(WinedroidError::new(
                "FutureTask.run requires an object-backed Callable",
            ));
        };
        let call_method = DexMethodId {
            class_descriptor: class_descriptor.clone(),
            name: "call".to_owned(),
            proto: dex::DexProto {
                shorty: "L".to_owned(),
                return_type: "Ljava/lang/Object;".to_owned(),
                parameters: Vec::new(),
            },
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(&class_descriptor, &call_method)?
        else {
            return Err(WinedroidError::new(format!(
                "FutureTask.run Callable.call() was not found\ncallable class: {class_descriptor}"
            )));
        };

        let result = self.execute_method(None, &resolved, Some(&[callable.clone()]), depth + 1)?;
        self.future_task_store_result(receiver, result)
    }

    // cite: libcore Executors.java#callable, RunnableAdapter
    fn future_task_run_runnable(
        &self,
        receiver: &DexValue,
        runnable: &DexValue,
        depth: usize,
    ) -> Result<()> {
        self.dispatch_runnable(runnable, "FutureTask.run", depth)?;

        let fixed_result = self
            .read_stored_instance_field(receiver, &future_task_runnable_result_field())
            .unwrap_or(DexValue::Null);
        self.write_stored_instance_field(receiver, &future_task_outcome_field(), fixed_result);
        self.write_stored_instance_field(receiver, &future_task_done_field(), DexValue::Int(1));

        Ok(())
    }

    // cite: libcore FutureTask.java#set, setException
    fn future_task_store_result(
        &self,
        receiver: &DexValue,
        result: DexExecutionResult,
    ) -> Result<()> {
        match result {
            DexExecutionResult::Value(value) => {
                self.write_stored_instance_field(receiver, &future_task_outcome_field(), value);
            }
            DexExecutionResult::Int(value) => {
                self.write_stored_instance_field(
                    receiver,
                    &future_task_outcome_field(),
                    DexValue::Int(value),
                );
            }
            DexExecutionResult::Void => {
                self.write_stored_instance_field(
                    receiver,
                    &future_task_outcome_field(),
                    DexValue::Null,
                );
            }
            DexExecutionResult::Throw(exception) => {
                self.write_stored_instance_field(
                    receiver,
                    &future_task_exception_field(),
                    exception,
                );
            }
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                return Err(WinedroidError::new(
                    "FutureTask.run Callable.call() did not complete normally",
                ));
            }
        }
        self.write_stored_instance_field(receiver, &future_task_done_field(), DexValue::Int(1));

        Ok(())
    }

    // cite: libcore FutureTask.java#get, report
    fn future_task_get(
        &self,
        invocation: &DexInvocation,
        depth: usize,
    ) -> Result<DexExecutionResult> {
        let Some(receiver) = invocation.arguments.first().cloned() else {
            return Err(WinedroidError::new("FutureTask.get requires a receiver"));
        };

        let already_done = self.read_stored_instance_field(&receiver, &future_task_done_field())
            == Some(DexValue::Int(1));
        if !already_done {
            self.future_task_run(&receiver, depth)?;
        }

        let cancelled = self.read_stored_instance_field(&receiver, &future_task_cancelled_field())
            == Some(DexValue::Int(1));
        if cancelled {
            let exception =
                self.allocate_object("Ljava/util/concurrent/CancellationException;".to_owned());
            return Ok(DexExecutionResult::Throw(exception));
        }

        if let Some(cause) =
            self.read_stored_instance_field(&receiver, &future_task_exception_field())
        {
            if cause != DexValue::Null {
                let exception =
                    self.allocate_object("Ljava/util/concurrent/ExecutionException;".to_owned());
                self.write_stored_instance_field(&exception, &future_task_get_cause_field(), cause);
                return Ok(DexExecutionResult::Throw(exception));
            }
        }

        let outcome = self
            .read_stored_instance_field(&receiver, &future_task_outcome_field())
            .unwrap_or(DexValue::Null);
        Ok(DexExecutionResult::Value(outcome))
    }

    fn dispatch_frame_callback(
        &self,
        callback: &DexValue,
        frame_time_nanos: i64,
        depth: usize,
    ) -> Result<()> {
        self.dispatch_object_void_method(
            callback,
            "doFrame",
            dex::DexProto {
                shorty: "VJ".to_owned(),
                return_type: "V".to_owned(),
                parameters: vec!["J".to_owned()],
            },
            vec![
                DexValue::Long(frame_time_nanos),
                DexValue::Long(frame_time_nanos),
            ],
            "Choreographer.FrameCallback",
            depth,
        )
    }

    fn resume_parked_thread(
        &self,
        thread: DexValue,
        execution: DexParkedExecution,
        depth: usize,
    ) -> Result<()> {
        self.with_current_guest_thread(thread.clone(), || {
            self.resume_parked_thread_as_current(thread, execution, depth)
        })
    }

    fn current_guest_thread_is_main(&self) -> bool {
        self.runtime.borrow().current_guest_thread == main_guest_thread_reference()
    }

    fn with_current_guest_thread<T>(&self, thread: DexValue, operation: impl FnOnce() -> T) -> T {
        let previous_thread = {
            let mut runtime = self.runtime.borrow_mut();
            std::mem::replace(&mut runtime.current_guest_thread, thread)
        };
        let result = operation();
        self.runtime.borrow_mut().current_guest_thread = previous_thread;
        result
    }

    fn resume_parked_thread_as_current(
        &self,
        thread: DexValue,
        mut execution: DexParkedExecution,
        depth: usize,
    ) -> Result<()> {
        let mut invocation_result = DexInvocationResult::Void;

        while let Some(step) = execution.pop_next_step() {
            let step_result = match step {
                DexParkedStep::Frame(frame) => {
                    let resume_result =
                        std::mem::replace(&mut invocation_result, DexInvocationResult::Void);
                    let current_class_descriptor =
                        frame.current_class_descriptor().map(str::to_owned);
                    let mut invocation_handler = DexRunInvocationHandler {
                        program: self,
                        current_dex_file_index: frame.dex_file_index(),
                        current_class_descriptor,
                        depth,
                    };
                    dex_interpreter::resume_parked_frame(
                        frame,
                        resume_result,
                        &mut invocation_handler,
                    )
                    .map_err(|error| {
                        WinedroidError::new(format!(
                            "while resuming parked Java thread\nthread: {thread}\n{error}"
                        ))
                    })
                    .map(Some)
                }
                DexParkedStep::ReplaceVoidWithValue(value) => {
                    let replacement_result =
                        std::mem::replace(&mut invocation_result, DexInvocationResult::Void);
                    match replacement_result {
                        DexInvocationResult::Void => {
                            invocation_result = DexInvocationResult::Value(value);
                            Ok(None)
                        }
                        DexInvocationResult::Throw(exception) => {
                            invocation_result = DexInvocationResult::Throw(exception);
                            Ok(None)
                        }
                        DexInvocationResult::Value(_)
                        | DexInvocationResult::RawValue(_)
                        | DexInvocationResult::Parked(_) => Err(WinedroidError::new(
                            "parked result replacement received a non-void result",
                        )),
                    }
                }
                DexParkedStep::RuntimeAction(action_id) => {
                    let action_result =
                        std::mem::replace(&mut invocation_result, DexInvocationResult::Void);
                    let action = self
                        .runtime
                        .borrow_mut()
                        .take_parked_runtime_action(action_id)
                        .ok_or_else(|| {
                            WinedroidError::new(format!(
                                "parked runtime action is missing\naction: {action_id}"
                            ))
                        });
                    match action {
                        Ok(action) => self
                            .resume_parked_runtime_action(&action, action_result, depth)
                            .map(Some),
                        Err(error) => Err(error),
                    }
                }
            };
            let result = match step_result {
                Ok(Some(result)) => result,
                Ok(None) => continue,
                Err(error) => {
                    self.runtime
                        .borrow_mut()
                        .discard_parked_runtime_actions(&execution);
                    return Err(error);
                }
            };

            invocation_result = match result {
                DexExecutionResult::Void => DexInvocationResult::Void,
                DexExecutionResult::Int(value) => DexInvocationResult::RawValue(
                    DexPendingResult::from_value(DexValue::Int(value)),
                ),
                DexExecutionResult::Value(value) => {
                    DexInvocationResult::RawValue(DexPendingResult::from_value(value))
                }
                DexExecutionResult::Throw(exception) => DexInvocationResult::Throw(exception),
                DexExecutionResult::Parked(mut resumed_execution) => {
                    resumed_execution.append_remaining_steps(execution);
                    let cleanup_execution = resumed_execution.clone();
                    if self
                        .runtime
                        .borrow_mut()
                        .store_parked_thread(&thread, resumed_execution)
                        .is_none()
                    {
                        self.runtime
                            .borrow_mut()
                            .discard_parked_runtime_actions(&cleanup_execution);
                        return Err(WinedroidError::new(format!(
                            "parked Java thread could not be stored\nthread: {thread}"
                        )));
                    }
                    return Ok(());
                }
            };
        }

        match invocation_result {
            DexInvocationResult::Throw(exception) => Err(WinedroidError::new(format!(
                "parked Java thread threw while resuming\nthread: {thread}\nexception: {exception}"
            ))),
            DexInvocationResult::Void
            | DexInvocationResult::Value(_)
            | DexInvocationResult::RawValue(_) => Ok(()),
            DexInvocationResult::Parked(_) => unreachable!(),
        }
    }

    fn resume_parked_runtime_action(
        &self,
        action: &ParkedRuntimeAction,
        invocation_result: DexInvocationResult,
        depth: usize,
    ) -> Result<DexExecutionResult> {
        if let DexInvocationResult::Throw(exception) = invocation_result {
            match action {
                ParkedRuntimeAction::ContinueClassInitialization(continuation) => {
                    self.abort_class_initialization(&continuation.class_descriptor);
                }
                ParkedRuntimeAction::FinishClassInitialization(class_descriptor) => {
                    self.abort_class_initialization(class_descriptor);
                }
                ParkedRuntimeAction::InvokeExactVoid { .. }
                | ParkedRuntimeAction::InvokeBuiltinVoid { .. } => {}
            }
            return Ok(DexExecutionResult::Throw(exception));
        }
        if !matches!(invocation_result, DexInvocationResult::Void) {
            self.abort_parked_runtime_action(action);
            return Err(WinedroidError::new(
                "parked runtime action received a non-void result",
            ));
        }

        let result = (|| {
            match action {
                ParkedRuntimeAction::ContinueClassInitialization(continuation) => {
                    match self.resume_class_initialization(continuation.clone(), depth + 1)? {
                        ClassInitializationResult::Complete => Ok(DexExecutionResult::Void),
                        ClassInitializationResult::Throw(exception) => {
                            Ok(DexExecutionResult::Throw(exception))
                        }
                        ClassInitializationResult::Parked(execution) => {
                            Ok(DexExecutionResult::Parked(execution))
                        }
                    }
                }
                ParkedRuntimeAction::FinishClassInitialization(class_descriptor) => {
                    // cite: art class_linker.cc#ClassLinker::InitializeClass
                    self.finish_class_initialization(class_descriptor);
                    Ok(DexExecutionResult::Void)
                }
                ParkedRuntimeAction::InvokeExactVoid { method, arguments } => {
                    let resolved = self.resolve_exact_method(method)?.ok_or_else(|| {
                        WinedroidError::new(format!(
                            "parked exact invocation target is missing\nmethod: {}",
                            method.display_signature()
                        ))
                    })?;
                    self.execute_method(None, &resolved, Some(arguments), depth + 1)
                }
                ParkedRuntimeAction::InvokeBuiltinVoid { method, invocation } => self
                    .invoke_builtin_method(method, Some(invocation))
                    .ok_or_else(|| {
                        WinedroidError::new(format!(
                            "parked builtin invocation target is missing\nmethod: {}",
                            method.display_signature()
                        ))
                    }),
            }
        })();
        if result.is_err() {
            self.abort_parked_runtime_action(action);
        }
        result
    }

    fn abort_parked_runtime_action(&self, action: &ParkedRuntimeAction) {
        match action {
            ParkedRuntimeAction::ContinueClassInitialization(continuation) => {
                self.abort_class_initialization(&continuation.class_descriptor);
            }
            ParkedRuntimeAction::FinishClassInitialization(class_descriptor) => {
                self.abort_class_initialization(class_descriptor);
            }
            ParkedRuntimeAction::InvokeExactVoid { .. }
            | ParkedRuntimeAction::InvokeBuiltinVoid { .. } => {}
        }
    }

    fn dispatch_object_void_method(
        &self,
        receiver: &DexValue,
        method_name: &str,
        proto: dex::DexProto,
        mut arguments: Vec<DexValue>,
        context: &str,
        depth: usize,
    ) -> Result<()> {
        let Some(class_descriptor) = value_class_descriptor(receiver) else {
            return Err(WinedroidError::new(format!(
                "{context} requires an object-backed callback\nargument: {receiver}"
            )));
        };

        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto,
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Err(WinedroidError::new(format!(
                "{context} callback method was not found\ncallback class: {class_descriptor}\nmethod: {method_name}{}\nnext missing primitive: Android UI thread callback dispatch",
                method.proto.descriptor()
            )));
        };

        let mut initial_arguments = Vec::with_capacity(arguments.len() + 1);
        initial_arguments.push(receiver.clone());
        initial_arguments.append(&mut arguments);

        let result = self.execute_method(None, &resolved, Some(&initial_arguments), depth + 1)?;
        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Throw(exception) => Err(WinedroidError::new(format!(
                "{context} callback `{method_name}` threw uncaught {}",
                self.describe_uncaught_throw(&exception)
            ))),
            DexExecutionResult::Int(_) | DexExecutionResult::Value(_) => Err(WinedroidError::new(
                format!("{context} callback `{method_name}` did not complete normally"),
            )),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "{context} callback `{method_name}` did not complete normally"
                )))
            }
        }
    }

    fn dispatch_object_value_method(
        &self,
        receiver: &DexValue,
        method_name: &str,
        proto: dex::DexProto,
        arguments: Vec<DexValue>,
        context: &str,
        depth: usize,
    ) -> Result<DexValue> {
        let Some(class_descriptor) = value_class_descriptor(receiver) else {
            return Err(WinedroidError::new(format!(
                "{context} requires an object-backed receiver\nargument: {receiver}"
            )));
        };
        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto,
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Err(WinedroidError::new(format!(
                "{context} method was not found\nclass: {class_descriptor}\nmethod: {method_name}{}\nnext missing primitive: adapter method dispatch",
                method.proto.descriptor()
            )));
        };

        let mut initial_arguments = Vec::with_capacity(arguments.len() + 1);
        initial_arguments.push(receiver.clone());
        initial_arguments.extend(arguments);

        match self.execute_method(None, &resolved, Some(&initial_arguments), depth + 1)? {
            DexExecutionResult::Value(value) => Ok(value),
            DexExecutionResult::Int(value) => Ok(DexValue::Int(value)),
            DexExecutionResult::Void => Ok(DexValue::Null),
            DexExecutionResult::Throw(exception) => Err(WinedroidError::new(format!(
                "{context} method `{method_name}` threw uncaught {}",
                self.describe_uncaught_throw(&exception)
            ))),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "{context} method `{method_name}` did not complete normally"
                )))
            }
        }
    }

    fn dispatch_optional_object_void_method(
        &self,
        receiver: &DexValue,
        method_name: &str,
        proto: dex::DexProto,
        arguments: Vec<DexValue>,
        depth: usize,
    ) -> Result<()> {
        let Some(class_descriptor) = value_class_descriptor(receiver) else {
            return Ok(());
        };
        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto,
        };
        let Some(resolved) =
            self.resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Ok(());
        };
        let mut initial_arguments = Vec::with_capacity(arguments.len() + 1);
        initial_arguments.push(receiver.clone());
        initial_arguments.extend(arguments);
        match self.execute_method(None, &resolved, Some(&initial_arguments), depth + 1)? {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "optional callback `{method_name}` parked without a guest-thread scheduler"
                )))
            }
            result => Err(WinedroidError::new(format!(
                "optional callback `{method_name}` did not complete normally\nresult: {result}"
            ))),
        }
    }

    fn describe_uncaught_throw(&self, exception: &DexValue) -> String {
        let class_descriptor = value_class_descriptor(exception).unwrap_or("unknown class");
        let field = DexFieldId {
            class_descriptor: "Ljava/lang/Throwable;".to_owned(),
            type_descriptor: "Ljava/lang/String;".to_owned(),
            name: "detailMessage".to_owned(),
        };
        match self.read_stored_instance_field(exception, &field) {
            Some(DexValue::String(message)) => format!("{class_descriptor}: {message}"),
            _ => class_descriptor.to_owned(),
        }
    }

    fn new_sax_attributes(&self, attributes: &[crate::binary_xml::XmlAttribute]) -> DexValue {
        let id = {
            let mut runtime = self.runtime.borrow_mut();
            let id = runtime.next_object_id;
            runtime.next_object_id += 1;
            id
        };
        let reference =
            DexValue::Reference(std::rc::Rc::from(format!("Lorg/xml/sax/Attributes;:{id}")));
        let entries = attributes
            .iter()
            .map(|attribute| (attribute.name.clone(), attribute.value.as_text()))
            .collect();
        self.runtime
            .borrow_mut()
            .sax_attributes
            .insert(map_identity_key(&reference), entries);
        reference
    }

    fn sax_parse(&self, invocation: &DexInvocation, depth: usize) -> Result<()> {
        let stream = invocation
            .arguments
            .first()
            .ok_or_else(|| WinedroidError::new("Xml.parse requires an InputStream argument"))?;
        let handler = invocation
            .arguments
            .get(2)
            .ok_or_else(|| WinedroidError::new("Xml.parse requires a ContentHandler argument"))?
            .clone();

        let stream_key = map_identity_key(stream);
        let bytes = self
            .runtime
            .borrow()
            .input_stream_values
            .get(&stream_key)
            .map(|stream| stream.bytes[stream.position.min(stream.bytes.len())..].to_vec())
            .ok_or_else(|| WinedroidError::new("Xml.parse stream has no readable bytes"))?;
        let events = crate::binary_xml::parse_plain_xml(&bytes)?;

        let string_param = || "Ljava/lang/String;".to_owned();
        let void = |parameters: Vec<String>| dex::DexProto {
            shorty: format!("V{}", "L".repeat(parameters.len())),
            return_type: "V".to_owned(),
            parameters,
        };

        self.dispatch_optional_object_void_method(
            &handler,
            "startDocument",
            void(Vec::new()),
            Vec::new(),
            depth,
        )?;

        for event in &events {
            match event {
                crate::binary_xml::XmlEvent::Start(element) => {
                    let attributes = self.new_sax_attributes(&element.attributes);
                    self.dispatch_optional_object_void_method(
                        &handler,
                        "startElement",
                        void(vec![
                            string_param(),
                            string_param(),
                            string_param(),
                            "Lorg/xml/sax/Attributes;".to_owned(),
                        ]),
                        vec![
                            DexValue::String(std::rc::Rc::from(String::new())),
                            DexValue::String(std::rc::Rc::from(
                                crate::binary_xml::local_name(&element.name).to_owned(),
                            )),
                            DexValue::String(std::rc::Rc::from(element.name.clone())),
                            attributes,
                        ],
                        depth,
                    )?;
                }
                crate::binary_xml::XmlEvent::End(element) => {
                    self.dispatch_optional_object_void_method(
                        &handler,
                        "endElement",
                        void(vec![string_param(), string_param(), string_param()]),
                        vec![
                            DexValue::String(std::rc::Rc::from(String::new())),
                            DexValue::String(std::rc::Rc::from(
                                crate::binary_xml::local_name(&element.name).to_owned(),
                            )),
                            DexValue::String(std::rc::Rc::from(element.name.clone())),
                        ],
                        depth,
                    )?;
                }
            }
        }

        self.dispatch_optional_object_void_method(
            &handler,
            "endDocument",
            void(Vec::new()),
            Vec::new(),
            depth,
        )
    }

    fn resolve_named_method(
        &self,
        class_descriptor: &str,
        method_name: &str,
    ) -> Result<ResolvedMethod> {
        let _span = trace::stat_span("perf", "dex resolve named method");
        let key = NamedMethodLookupKey {
            class_descriptor: class_descriptor.to_owned(),
            name: method_name.to_owned(),
        };

        let cached = self.caches.borrow().named_methods.get(&key).cloned();
        let mut matches = if let Some(cached) = cached {
            cached.clone()
        } else {
            let mut matches = Vec::new();

            if let Some(loaded_class) = self.load_class(class_descriptor) {
                collect_method_matches(
                    self,
                    &loaded_class,
                    &loaded_class.class_def.direct_methods,
                    method_name,
                    &mut matches,
                )?;
                collect_method_matches(
                    self,
                    &loaded_class,
                    &loaded_class.class_def.virtual_methods,
                    method_name,
                    &mut matches,
                )?;
            }

            self.caches
                .borrow_mut()
                .named_methods
                .insert(key, matches.clone());

            matches
        };

        if matches.is_empty() {
            return Err(WinedroidError::new(format!(
                "DEX method `{method_name}` was not found in class `{class_descriptor}`"
            )));
        }

        if matches.len() > 1 {
            return Err(WinedroidError::new(format!(
                "DEX method `{method_name}` is ambiguous in class `{class_descriptor}`\nmatching prototypes: {}",
                matching_prototypes_display(&matches)
            )));
        }

        Ok(matches.remove(0))
    }

    fn resolve_exact_method(&self, method: &DexMethodId) -> Result<Option<Rc<ResolvedMethod>>> {
        let _span = trace::stat_span("perf", "dex resolve exact method");
        let key = MethodLookupKey::from_method(method);
        if let Some(cached) = self.caches.borrow().exact_methods.get(&key).cloned() {
            return Ok(cached);
        }

        let mut matches = Vec::new();
        let proto_descriptor = method.proto.descriptor();

        if let Some(loaded_class) = self.load_class(&method.class_descriptor) {
            collect_exact_method_matches(
                self,
                &loaded_class,
                &loaded_class.class_def.direct_methods,
                method,
                &proto_descriptor,
                &mut matches,
            )?;
            collect_exact_method_matches(
                self,
                &loaded_class,
                &loaded_class.class_def.virtual_methods,
                method,
                &proto_descriptor,
                &mut matches,
            )?;
        }

        if matches.len() > 1 {
            return Err(WinedroidError::new(format!(
                "DEX method reference is ambiguous\nmethod: {}.{}",
                method.class_descriptor,
                method.display_signature()
            )));
        }

        let resolved = matches.pop().map(Rc::new);
        self.caches
            .borrow_mut()
            .exact_methods
            .insert(key, resolved.clone());

        Ok(resolved)
    }

    fn resolve_exact_method_in_class_hierarchy(
        &self,
        class_descriptor: &str,
        method: &DexMethodId,
    ) -> Result<Option<Rc<ResolvedMethod>>> {
        let _span = trace::stat_span("perf", "dex resolve class hierarchy method");
        let key = HierarchyMethodLookupKey::from_method(class_descriptor, method);
        if let Some(cached) = self
            .caches
            .borrow()
            .class_hierarchy_methods
            .get(&key)
            .cloned()
        {
            return Ok(cached);
        }

        let mut current_descriptor = Some(class_descriptor.to_owned());
        let mut visited_descriptors = Vec::new();
        let mut resolved = None;

        while let Some(descriptor) = current_descriptor {
            if visited_descriptors.contains(&descriptor) {
                break;
            }
            visited_descriptors.push(descriptor.clone());

            let mut current_method = method.clone();
            current_method.class_descriptor = descriptor.clone();
            if let Some(method) = self.resolve_exact_method(&current_method)? {
                resolved = Some(method);
                break;
            }

            current_descriptor = self.superclass_descriptor(&descriptor);
        }

        self.caches
            .borrow_mut()
            .class_hierarchy_methods
            .insert(key, resolved.clone());

        Ok(resolved)
    }

    fn resolve_interface_method_in_class_hierarchy(
        &self,
        class_descriptor: &str,
        method: &DexMethodId,
    ) -> Result<Option<Rc<ResolvedMethod>>> {
        let _span = trace::stat_span("perf", "dex resolve interface hierarchy method");
        let key = HierarchyMethodLookupKey::from_method(class_descriptor, method);
        if let Some(cached) = self
            .caches
            .borrow()
            .interface_hierarchy_methods
            .get(&key)
            .cloned()
        {
            return Ok(cached);
        }

        let mut current_descriptor = Some(class_descriptor.to_owned());
        let mut visited_classes = Vec::new();
        let mut visited_interfaces = Vec::new();
        let mut resolved = None;

        while let Some(descriptor) = current_descriptor {
            if visited_classes.contains(&descriptor) {
                break;
            }
            visited_classes.push(descriptor.clone());

            for interface_descriptor in self.class_interface_descriptors(&descriptor) {
                if let Some(method) = self.resolve_interface_default_method(
                    &interface_descriptor,
                    method,
                    &mut visited_interfaces,
                )? {
                    resolved = Some(method);
                    break;
                }
            }
            if resolved.is_some() {
                break;
            }

            current_descriptor = self.superclass_descriptor(&descriptor);
        }

        self.caches
            .borrow_mut()
            .interface_hierarchy_methods
            .insert(key, resolved.clone());

        Ok(resolved)
    }

    fn resolve_interface_default_method(
        &self,
        interface_descriptor: &str,
        method: &DexMethodId,
        visited_interfaces: &mut Vec<String>,
    ) -> Result<Option<Rc<ResolvedMethod>>> {
        let _span = trace::stat_span("perf", "dex resolve interface default method");
        if visited_interfaces.contains(&interface_descriptor.to_owned()) {
            return Ok(None);
        }
        visited_interfaces.push(interface_descriptor.to_owned());

        let mut interface_method = method.clone();
        interface_method.class_descriptor = interface_descriptor.to_owned();
        if let Some(resolved) = self.resolve_exact_method(&interface_method)? {
            if resolved.encoded_method.code_offset != 0 {
                return Ok(Some(resolved));
            }
        }

        for parent_interface in self.class_interface_descriptors(interface_descriptor) {
            if let Some(resolved) = self.resolve_interface_default_method(
                &parent_interface,
                method,
                visited_interfaces,
            )? {
                return Ok(Some(resolved));
            }
        }

        Ok(None)
    }

    pub(super) fn is_activity_class(&self, class_descriptor: &str) -> bool {
        let mut current = Some(class_descriptor.to_owned());
        let mut depth = 0;
        while let Some(descriptor) = current {
            if descriptor == "Landroid/app/Activity;" {
                return true;
            }
            depth += 1;
            if depth > 32 {
                return false;
            }
            current = self.superclass_descriptor(&descriptor);
        }
        false
    }

    fn superclass_descriptor(&self, class_descriptor: &str) -> Option<String> {
        if let Some(cached) = self
            .caches
            .borrow()
            .superclass_descriptors
            .get(class_descriptor)
            .cloned()
        {
            return cached;
        }

        let superclass = self
            .load_class(class_descriptor)
            .and_then(|loaded_class| loaded_class.class_def.superclass_descriptor.clone())
            .or_else(|| bootstrap_superclass_descriptor(class_descriptor).map(str::to_owned));

        self.caches
            .borrow_mut()
            .superclass_descriptors
            .insert(class_descriptor.to_owned(), superclass.clone());

        superclass
    }

    fn prepare_entrypoint_initial_arguments(
        &self,
        resolved: &ResolvedMethod,
    ) -> Result<Option<Vec<DexValue>>> {
        if resolved.encoded_method.access_flags & DEX_ACC_STATIC != 0
            || resolved.method.name == "<init>"
            || resolved.encoded_method.code_offset == 0
        {
            return Ok(None);
        }

        let code = self.code_item(resolved)?;
        let receiver = self.allocate_object(resolved.class_descriptor.clone());

        self.initialize_entrypoint_receiver(resolved, receiver.clone())?;

        self.entrypoint_initial_arguments_with_receiver(resolved, &code, Some(receiver))
            .map(Some)
            .ok_or_else(|| {
                WinedroidError::new(format!(
                    "DEX entrypoint argument preparation failed\nclass: {}\nmethod: {}",
                    resolved.class_descriptor,
                    resolved.method.display_signature()
                ))
            })
    }

    fn initialize_entrypoint_receiver(
        &self,
        resolved: &ResolvedMethod,
        receiver: DexValue,
    ) -> Result<()> {
        let constructor_method = DexMethodId {
            class_descriptor: resolved.class_descriptor.clone(),
            name: "<init>".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "V".to_owned(),
                return_type: "V".to_owned(),
                parameters: Vec::new(),
            },
        };
        let Some(constructor) = self.resolve_exact_method(&constructor_method)? else {
            return Ok(());
        };
        let result = self.execute_method(None, &constructor, Some(&[receiver]), 0)?;

        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(format!(
                "DEX entrypoint constructor did not complete normally\nclass: {}\nmethod: {}",
                resolved.class_descriptor,
                constructor.method.display_signature()
            ))),
            DexExecutionResult::Parked(execution) => {
                self.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "DEX entrypoint constructor did not complete normally\nclass: {}\nmethod: {}",
                    resolved.class_descriptor,
                    constructor.method.display_signature()
                )))
            }
        }
    }

    fn entrypoint_initial_arguments(
        &self,
        resolved: &ResolvedMethod,
        code: &DexCodeItem,
    ) -> Option<Vec<DexValue>> {
        self.entrypoint_initial_arguments_with_receiver(resolved, code, None)
    }

    fn entrypoint_initial_arguments_with_receiver(
        &self,
        resolved: &ResolvedMethod,
        code: &DexCodeItem,
        receiver: Option<DexValue>,
    ) -> Option<Vec<DexValue>> {
        let mut arguments = Vec::new();

        if resolved.encoded_method.access_flags & DEX_ACC_STATIC == 0 {
            arguments.push(
                receiver.unwrap_or_else(|| self.allocate_object(resolved.class_descriptor.clone())),
            );
        }

        for parameter in &resolved.method.proto.parameters {
            push_default_entrypoint_argument(&mut arguments, parameter)?;
        }

        if arguments.len() != code.ins_size as usize {
            return None;
        }

        Some(arguments)
    }

    fn initialize_intent_for_class(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 3 {
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
        match &invocation.arguments[1] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        class_reference_descriptor_from_value(&invocation.arguments[2])?;
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &intent_target_class_field(),
            invocation.arguments[2].clone(),
        )
        .then_some(())
    }

    fn initialize_intent_for_action(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        instance_field_owner_key(&invocation.arguments[0])?;
        let DexValue::String(action) = &invocation.arguments[1] else {
            return None;
        };

        self.set_intent_action_value(&invocation.arguments[0], action)
    }

    fn initialize_intent_for_action_and_uri(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 3 {
            return None;
        }

        let DexValue::String(action) = &invocation.arguments[1] else {
            return None;
        };

        self.set_intent_action_value(&invocation.arguments[0], action)?;
        self.set_intent_data_value(&invocation.arguments[0], &invocation.arguments[2])
    }

    fn set_intent_action_value(&self, intent: &DexValue, action: &str) -> Option<()> {
        instance_field_owner_key(intent)?;

        self.write_stored_instance_field(
            intent,
            &intent_action_field(),
            DexValue::String(std::rc::Rc::from(action.to_owned())),
        )
        .then_some(())
    }

    fn set_intent_data(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        self.set_intent_data_value(&invocation.arguments[0], &invocation.arguments[1])?;
        Some(invocation.arguments[0].clone())
    }

    // cite: frameworks-base Intent.java#setDataAndType
    fn set_intent_data_and_type(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 3 {
            return None;
        }

        self.set_intent_data_value(&invocation.arguments[0], &invocation.arguments[1])?;
        let mime_type = match &invocation.arguments[2] {
            DexValue::String(_) | DexValue::Null => invocation.arguments[2].clone(),
            DexValue::Int(0) => DexValue::Null,
            DexValue::Uninitialized
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::Object(_)
            | DexValue::Reference(_) => return None,
        };
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &DexFieldId {
                class_descriptor: "Landroid/content/Intent;".to_owned(),
                type_descriptor: "Ljava/lang/String;".to_owned(),
                name: "mType".to_owned(),
            },
            mime_type,
        );

        Some(invocation.arguments[0].clone())
    }

    fn set_intent_data_value(&self, intent: &DexValue, data: &DexValue) -> Option<()> {
        instance_field_owner_key(intent)?;

        match data {
            DexValue::Object(_) | DexValue::Reference(_) => self
                .write_stored_instance_field(intent, &intent_data_uri_field(), data.clone())
                .then_some(()),
            DexValue::Null => {
                self.remove_stored_instance_field(intent, &intent_data_uri_field());
                Some(())
            }
            DexValue::Uninitialized
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => None,
        }
    }

    fn intent_data(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        instance_field_owner_key(&invocation.arguments[0])?;

        Some(
            self.read_stored_instance_field(&invocation.arguments[0], &intent_data_uri_field())
                .unwrap_or(DexValue::Null),
        )
    }

    // cite: frameworks-base Intent.java#getType
    fn intent_type(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        instance_field_owner_key(&invocation.arguments[0])?;

        Some(
            self.read_stored_instance_field(&invocation.arguments[0], &intent_type_field())
                .unwrap_or(DexValue::Null),
        )
    }

    // cite: frameworks-base Intent.java#setAction
    fn set_intent_action(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let value = match &invocation.arguments[1] {
            DexValue::String(_) => invocation.arguments[1].clone(),
            DexValue::Null | DexValue::Int(0) => DexValue::Null,
            DexValue::Uninitialized
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::Object(_)
            | DexValue::Reference(_) => return None,
        };

        self.write_stored_instance_field(&invocation.arguments[0], &intent_action_field(), value)
            .then_some(invocation.arguments[0].clone())
    }

    fn intent_action(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        instance_field_owner_key(&invocation.arguments[0])?;

        Some(
            self.read_stored_instance_field(&invocation.arguments[0], &intent_action_field())
                .unwrap_or(DexValue::Null),
        )
    }

    fn parse_uri(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        let DexValue::String(uri_string) = &invocation.arguments[0] else {
            return None;
        };

        let uri = self.allocate_object("Landroid/net/Uri;".to_owned());
        self.write_stored_instance_field(
            &uri,
            &uri_string_field(),
            DexValue::String(uri_string.clone()),
        )
        .then_some(uri)
    }

    fn uri_text(&self, value: &DexValue) -> Option<String> {
        match self.read_stored_instance_field(value, &uri_string_field())? {
            DexValue::String(text) => Some(text.to_string()),
            _ => None,
        }
    }

    // cite: frameworks-base Uri.java#getScheme, getQuery, getFragment, getHost, getPath
    fn uri_component(&self, invocation: &DexInvocation, component: &str) -> Option<DexValue> {
        let text = self.uri_text(invocation.arguments.first()?)?;
        let (without_fragment, fragment) = match text.split_once('#') {
            Some((rest, fragment)) => (rest, Some(fragment)),
            None => (text.as_str(), None),
        };
        let (without_query, query) = match without_fragment.split_once('?') {
            Some((rest, query)) => (rest, Some(query)),
            None => (without_fragment, None),
        };
        let (scheme, after_scheme) = match without_query.split_once(':') {
            Some((scheme, rest)) => (Some(scheme), rest),
            None => (None, without_query),
        };
        let (authority, path) = if let Some(rest) = after_scheme.strip_prefix("//") {
            match rest.find('/') {
                Some(index) => (Some(&rest[..index]), &rest[index..]),
                None => (Some(rest), ""),
            }
        } else {
            (None, after_scheme)
        };

        let value = match component {
            "scheme" => scheme.map(str::to_owned),
            "query" => query.map(str::to_owned),
            "fragment" => fragment.map(str::to_owned),
            "authority" | "host" => authority.map(str::to_owned),
            "path" => Some(path.to_owned()),
            "lastPathSegment" => path
                .split('/')
                .filter(|segment| !segment.is_empty())
                .next_back()
                .map(str::to_owned),
            _ => return None,
        };

        Some(match value {
            Some(text) => DexValue::String(std::rc::Rc::from(text)),
            None => DexValue::Null,
        })
    }

    // cite: frameworks-base Uri.java#getPathSegments
    fn uri_path_segments(&self, invocation: &DexInvocation) -> Option<DexValue> {
        let DexValue::String(path) = self.uri_component(invocation, "path")? else {
            return Some(self.allocate_tracked_list(Vec::new()));
        };
        let segments: Vec<DexValue> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| DexValue::String(std::rc::Rc::from(segment.to_owned())))
            .collect();

        Some(self.allocate_tracked_list(segments))
    }

    fn allocate_tracked_list(&self, values: Vec<DexValue>) -> DexValue {
        let list = self.allocate_object("Ljava/util/ArrayList;".to_owned());
        self.runtime
            .borrow_mut()
            .list_entries
            .insert(map_identity_key(&list), values);

        list
    }

    // cite: frameworks-base Uri.java#getQueryParameterNames, getQueryParameters
    fn uri_query_pairs(&self, invocation: &DexInvocation) -> Option<Vec<(String, String)>> {
        let query = match self.uri_component(invocation, "query")? {
            DexValue::String(query) => query,
            _ => return Some(Vec::new()),
        };

        Some(
            query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| match pair.split_once('=') {
                    Some((name, value)) => (name.to_owned(), value.to_owned()),
                    None => (pair.to_owned(), String::new()),
                })
                .collect(),
        )
    }

    fn uri_to_string(&self, invocation: &DexInvocation) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        let value =
            self.read_stored_instance_field(&invocation.arguments[0], &uri_string_field())?;

        match value {
            DexValue::String(_) => Some(value),
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::Object(_)
            | DexValue::Reference(_) => None,
        }
    }

    fn start_activity_target_class(&self, invocation: &DexInvocation) -> Option<String> {
        if invocation.arguments.len() < 2 {
            return None;
        }

        let target_class = self
            .read_stored_instance_field(&invocation.arguments[1], &intent_target_class_field())?;
        class_reference_descriptor_from_value(&target_class).map(ToOwned::to_owned)
    }

    fn start_activity_from_intent(&self, invocation: &DexInvocation, depth: usize) -> Result<()> {
        if let Some(target) = self.start_activity_target_class(invocation) {
            self.start_activity_on_create(&target, depth + 1)?;
            return Ok(());
        }

        self.enqueue_host_action_for_implicit_intent(invocation)
    }

    fn enqueue_host_action_for_implicit_intent(&self, invocation: &DexInvocation) -> Result<()> {
        let Some(intent) = invocation.arguments.get(1) else {
            return Err(WinedroidError::new(
                "Context.startActivity expected an Intent argument",
            ));
        };
        let Some(DexValue::String(action)) =
            self.read_stored_instance_field(intent, &intent_action_field())
        else {
            return Err(WinedroidError::new(
                "Context.startActivity only supports explicit class Intents or ACTION_VIEW web Intents right now\nnext missing primitive: Android implicit Intent activity resolution",
            ));
        };

        if action != std::rc::Rc::from(ANDROID_INTENT_ACTION_VIEW) {
            return Err(WinedroidError::new(format!(
                "Context.startActivity implicit Intent action is not implemented\naction: {action}\nnext missing primitive: Android implicit Intent activity resolution"
            )));
        }

        let Some(uri) = self.read_stored_instance_field(intent, &intent_data_uri_field()) else {
            return Err(WinedroidError::new(
                "ACTION_VIEW Intent is missing a data Uri\nnext missing primitive: Android implicit Intent activity resolution",
            ));
        };
        let Some(DexValue::String(url)) =
            self.read_stored_instance_field(&uri, &uri_string_field())
        else {
            return Err(WinedroidError::new(
                "ACTION_VIEW Intent data Uri has no parsed string\nnext missing primitive: Android Uri runtime storage",
            ));
        };

        if !is_host_browser_url(&url) {
            return Err(WinedroidError::new(format!(
                "ACTION_VIEW Intent data is not a web URL\nurl: {url}\nnext missing primitive: Android non-browser ACTION_VIEW handling"
            )));
        }

        self.enqueue_host_action(DexHostAction::OpenBrowser {
            url: url.to_string(),
        });
        Ok(())
    }

    fn add_intent_filter_action(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let filter_key = map_identity_key(&invocation.arguments[0]);
        let DexValue::String(action) = &invocation.arguments[1] else {
            return None;
        };

        self.runtime
            .borrow_mut()
            .intent_filter_actions
            .entry(filter_key)
            .or_default()
            .push(action.clone().to_string());

        Some(())
    }

    fn intent_filter_action_count(&self, invocation: &DexInvocation) -> Option<i32> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        let filter_key = map_identity_key(&invocation.arguments[0]);
        let count = self
            .runtime
            .borrow()
            .intent_filter_actions
            .get(&filter_key)
            .map(Vec::len)
            .unwrap_or(0);

        i32::try_from(count).ok()
    }

    fn recycler_view_set_adapter(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let field = DexFieldId {
            class_descriptor: "Landroidx/recyclerview/widget/RecyclerView;".to_owned(),
            type_descriptor: "Landroidx/recyclerview/widget/RecyclerView$Adapter;".to_owned(),
            name: "mAdapter".to_owned(),
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &field,
            invocation.arguments[1].clone(),
        )
        .then_some(())
    }

    fn tab_layout_new_tab(&self) -> DexValue {
        let mut runtime = self.runtime.borrow_mut();
        let id = runtime.next_object_id;
        runtime.next_object_id += 1;

        DexValue::Reference(std::rc::Rc::from(format!(
            "Lcom/google/android/material/tabs/TabLayout$Tab;:{id}"
        )))
    }
}

struct DexRunInvocationHandler<'a> {
    program: &'a DexProgram,
    current_dex_file_index: usize,
    current_class_descriptor: Option<String>,
    depth: usize,
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
struct JitObjectEnv<'a, 'b> {
    frame: &'a mut DexFrame,
    handler: &'a mut DexRunInvocationHandler<'b>,
    pending_writes: Vec<DexFieldWrite>,
    outcome: Option<DexExecutionResult>,
    last_result: Option<DexValue>,
    resume_pc: Option<u32>,
    stop_result: Option<DexInvocationResult>,
    stop_pc: u32,
    hard_error: Option<WinedroidError>,
    invoked: bool,
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
fn jit_pending_field_value(
    env: &JitObjectEnv,
    object: &DexValue,
    field_index: u32,
) -> Option<DexValue> {
    let object_key = instance_field_owner_key(object)?;
    env.pending_writes.iter().rev().find_map(|write| {
        if write.field_index == field_index
            && instance_field_owner_key(&write.object).as_deref() == Some(object_key.as_str())
        {
            Some(write.value.clone())
        } else {
            None
        }
    })
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
fn jit_register(reg: u32, env: &mut JitObjectEnv, pc: u32) -> Option<u16> {
    match u16::try_from(reg) {
        Ok(register) => Some(register),
        Err(_) => {
            env.resume_pc = Some(pc);
            None
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
fn jit_frame_value(env: &mut JitObjectEnv, reg: u32, pc: u32) -> Option<DexValue> {
    let register = jit_register(reg, env, pc)?;
    match env.frame.read_value(register) {
        Ok(value) => Some(value),
        Err(_) => {
            env.resume_pc = Some(pc);
            None
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
fn jit_frame_int(env: &mut JitObjectEnv, reg: u32, pc: u32) -> Option<i32> {
    let register = jit_register(reg, env, pc)?;
    match env.frame.read_int(register) {
        Ok(value) => Some(value),
        Err(_) => {
            env.resume_pc = Some(pc);
            None
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
fn jit_store_value(env: &mut JitObjectEnv, dst: u32, value: DexValue, pc: u32) -> i32 {
    let Some(register) = jit_register(dst, env, pc) else {
        return 1;
    };
    match env.frame.set_register(register, value) {
        Ok(()) => 0,
        Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_get_int(env: *mut std::ffi::c_void, reg: u32, pc: u32) -> i64 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    match jit_frame_int(env, reg, pc) {
        Some(value) => i64::from(value as u32),
        None => 1i64 << 32,
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_set_int(env: *mut std::ffi::c_void, reg: u32, val: i32) {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    if let Ok(register) = u16::try_from(reg) {
        let _ = env.frame.set_int(register, val);
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_iget_int(
    env: *mut std::ffi::c_void,
    dst: u32,
    obj: u32,
    field_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let Some(object) = jit_frame_value(env, obj, pc) else {
        return 1;
    };
    if let Some(value) = jit_pending_field_value(env, &object, field_index) {
        return match value {
            DexValue::Int(value) => jit_store_value(env, dst, DexValue::Int(value), pc),
            _ => {
                env.resume_pc = Some(pc);
                1
            }
        };
    }
    let access = DexFieldAccess {
        field_index,
        destination_register: dst as u16,
        object_register: obj as u16,
        object,
    };
    match env.handler.read_instance_field(access) {
        Ok(DexValue::Int(value)) => jit_store_value(env, dst, DexValue::Int(value), pc),
        _ => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_aget(
    env: *mut std::ffi::c_void,
    dst: u32,
    array: u32,
    index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let Some(array_val) = jit_frame_value(env, array, pc) else {
        return 1;
    };
    let Some(index_i32) = jit_frame_int(env, index, pc) else {
        return 1;
    };
    let read = DexArrayRead {
        destination_register: dst as u16,
        array_register: array as u16,
        index_register: index as u16,
        array: array_val,
        index: index_i32,
    };
    match env.handler.read_array(read) {
        Ok(value) => jit_store_value(env, dst, value, pc),
        Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_move(env: *mut std::ffi::c_void, dst: u32, src: u32) {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    if let (Ok(dst), Ok(src)) = (u16::try_from(dst), u16::try_from(src)) {
        if let Ok(value) = env.frame.read_value(src) {
            let _ = env.frame.set_register(dst, value);
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_iget_object(
    env: *mut std::ffi::c_void,
    dst: u32,
    obj: u32,
    field_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let Some(object) = jit_frame_value(env, obj, pc) else {
        return 1;
    };
    if let Some(value) = jit_pending_field_value(env, &object, field_index) {
        return jit_store_value(env, dst, value, pc);
    }
    let access = DexFieldAccess {
        field_index,
        destination_register: dst as u16,
        object_register: obj as u16,
        object,
    };
    match env.handler.read_instance_field(access) {
        Ok(value) => jit_store_value(env, dst, value, pc),
        Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_iput_field(
    env: *mut std::ffi::c_void,
    val: u32,
    obj: u32,
    field_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let Some(value) = jit_frame_value(env, val, pc) else {
        return 1;
    };
    let Some(object) = jit_frame_value(env, obj, pc) else {
        return 1;
    };
    if instance_field_owner_key(&object).is_none() {
        env.resume_pc = Some(pc);
        return 1;
    }
    env.pending_writes.push(DexFieldWrite {
        field_index,
        value_register: val as u16,
        object_register: obj as u16,
        value,
        object,
    });
    0
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_sget_int(
    env: *mut std::ffi::c_void,
    dst: u32,
    field_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let access = DexStaticFieldAccess {
        field_index,
        destination_register: dst as u16,
    };
    match env.handler.read_static_field(access) {
        Ok(DexStaticFieldReadResult::Value(DexValue::Int(value))) => {
            jit_store_value(env, dst, DexValue::Int(value), pc)
        }
        _ => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_sget_object(
    env: *mut std::ffi::c_void,
    dst: u32,
    field_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let access = DexStaticFieldAccess {
        field_index,
        destination_register: dst as u16,
    };
    match env.handler.read_static_field(access) {
        Ok(DexStaticFieldReadResult::Value(value)) => jit_store_value(env, dst, value, pc),
        Ok(DexStaticFieldReadResult::Throw(_))
        | Ok(DexStaticFieldReadResult::Parked(_))
        | Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_instance_of(
    env: *mut std::ffi::c_void,
    dst: u32,
    reg: u32,
    type_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let Some(value) = jit_frame_value(env, reg, pc) else {
        return 1;
    };
    let check = DexInstanceOf {
        type_index,
        destination_register: dst as u16,
        value_register: reg as u16,
        value,
    };
    match env.handler.instance_of(check) {
        Ok(value) => jit_store_value(env, dst, DexValue::Int(value), pc),
        Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

// cite: art dex_instruction_list.h#THROW
#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_bail(env: *mut std::ffi::c_void, pc: u32) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    env.resume_pc = Some(pc);
    1
}

// cite: art quick_exception_handler.cc#DeoptimizeStackVisitor
#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_invoke(env: *mut std::ffi::c_void, pc: u32) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    for write in env.pending_writes.drain(..) {
        if let Err(error) = env.handler.write_instance_field(write) {
            env.hard_error = Some(error);
            return 3;
        }
    }
    env.invoked = true;
    env.resume_pc = Some(pc);
    1
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_move_result(env: *mut std::ffi::c_void, dst: u32, wide: u32, pc: u32) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let Some(value) = env.last_result.take() else {
        env.resume_pc = Some(pc);
        return 1;
    };
    if wide != 0 {
        let Some(register) = jit_register(dst, env, pc) else {
            return 1;
        };
        if env.frame.set_register(register, value.clone()).is_err() {
            env.resume_pc = Some(pc);
            return 1;
        }
        if env.frame.set_register(register + 1, value).is_err() {
            env.resume_pc = Some(pc);
            return 1;
        }
    } else if jit_store_value(env, dst, value, pc) != 0 {
        return 1;
    }
    0
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_return_int(env: *mut std::ffi::c_void, src: u32, pc: u32) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    match jit_frame_value(env, src, pc) {
        Some(DexValue::Int(value)) => {
            env.outcome = Some(DexExecutionResult::Int(value));
            0
        }
        Some(DexValue::Float(value)) => {
            env.outcome = Some(DexExecutionResult::Value(DexValue::Float(value)));
            0
        }
        Some(DexValue::Long(value)) => {
            env.outcome = Some(DexExecutionResult::Value(DexValue::Long(value)));
            0
        }
        _ => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_get_long(
    env: *mut std::ffi::c_void,
    reg: u32,
    pc: u32,
    status: *mut i32,
) -> i64 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let Some(register) = jit_register(reg, env, pc) else {
        unsafe { *status = 1 };
        return 0;
    };
    match env.frame.read_long(register) {
        Ok(value) => {
            unsafe { *status = 0 };
            value
        }
        Err(_) => {
            env.resume_pc = Some(pc);
            unsafe { *status = 1 };
            0
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_set_long(env: *mut std::ffi::c_void, reg: u32, val: i64) {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    if let Ok(register) = u16::try_from(reg) {
        let _ = env.frame.set_long(register, val);
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_get_float_bits(env: *mut std::ffi::c_void, reg: u32, pc: u32) -> i64 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    match jit_frame_value(env, reg, pc) {
        Some(DexValue::Float(value)) => i64::from(value.to_bits()),
        Some(DexValue::Int(value)) => i64::from(value as u32),
        _ => {
            env.resume_pc = Some(pc);
            1i64 << 32
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_set_float_bits(env: *mut std::ffi::c_void, reg: u32, bits: i32) {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    if let Ok(register) = u16::try_from(reg) {
        let _ = env.frame.set_float(register, f32::from_bits(bits as u32));
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_return_void(env: *mut std::ffi::c_void) {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    env.outcome = Some(DexExecutionResult::Void);
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
fn jit_store_or_bail(env: &mut JitObjectEnv, dst: u32, value: DexValue, pc: u32) -> i32 {
    jit_store_value(env, dst, value, pc)
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_new_instance(
    env: *mut std::ffi::c_void,
    dst: u32,
    type_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    for write in env.pending_writes.drain(..) {
        if let Err(error) = env.handler.write_instance_field(write) {
            env.hard_error = Some(error);
            return 3;
        }
    }
    env.invoked = true;
    match env
        .handler
        .allocate_object(dex_interpreter::DexNewInstance {
            type_index,
            destination_register: dst as u16,
        }) {
        Ok(value) => jit_store_or_bail(env, dst, value, pc),
        Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_const_string(
    env: *mut std::ffi::c_void,
    dst: u32,
    string_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    match env.handler.load_string(dex_interpreter::DexStringAccess {
        string_index,
        destination_register: dst as u16,
    }) {
        Ok(value) => jit_store_or_bail(env, dst, value, pc),
        Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_const_class(
    env: *mut std::ffi::c_void,
    dst: u32,
    type_index: u32,
    pc: u32,
) -> i32 {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    for write in env.pending_writes.drain(..) {
        if let Err(error) = env.handler.write_instance_field(write) {
            env.hard_error = Some(error);
            return 3;
        }
    }
    env.invoked = true;
    match env.handler.load_class(dex_interpreter::DexClassAccess {
        type_index,
        destination_register: dst as u16,
    }) {
        Ok(value) => jit_store_or_bail(env, dst, value, pc),
        Err(_) => {
            env.resume_pc = Some(pc);
            1
        }
    }
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
extern "C" fn jit_rt_return_object(env: *mut std::ffi::c_void, src: u32) {
    let env = unsafe { &mut *(env as *mut JitObjectEnv) };
    let value = u16::try_from(src)
        .ok()
        .and_then(|src| env.frame.read_value(src).ok())
        .unwrap_or(DexValue::Null);
    env.outcome = Some(DexExecutionResult::Value(value));
}

trait MethodIntrinsicArguments {
    fn invocation_kind(&self) -> DexInvokeKind;
    fn argument_count(&self) -> usize;
    fn argument_value(&self, index: usize) -> Result<Option<DexValue>>;
}

impl MethodIntrinsicArguments for DexInvocation {
    fn invocation_kind(&self) -> DexInvokeKind {
        self.kind
    }

    fn argument_count(&self) -> usize {
        self.arguments.len()
    }

    fn argument_value(&self, index: usize) -> Result<Option<DexValue>> {
        Ok(self.arguments.get(index).cloned())
    }
}

impl MethodIntrinsicArguments for DexInvocationFrame<'_> {
    fn invocation_kind(&self) -> DexInvokeKind {
        self.kind()
    }

    fn argument_count(&self) -> usize {
        DexInvocationFrame::argument_count(self)
    }

    fn argument_value(&self, index: usize) -> Result<Option<DexValue>> {
        self.read_argument(index)
    }
}

// cite: art interpreter_switch_impl-inl.h#IF_EQZ
fn null_assert_argument_is_non_null<A: MethodIntrinsicArguments + ?Sized>(
    invocation: &A,
    index: usize,
) -> Result<bool> {
    Ok(matches!(
        invocation.argument_value(index)?,
        Some(DexValue::String(_) | DexValue::Object(_) | DexValue::Reference(_))
    ))
}

// cite: art method_verifier.cc#SetTypesFromSignature, VerifyInvocationArgsFromIterator
fn null_assert_shortcut_applies<A: MethodIntrinsicArguments + ?Sized>(
    invocation: &A,
    resolved: &ResolvedMethod,
    argument_index: usize,
) -> Result<bool> {
    if invocation.invocation_kind() != DexInvokeKind::Static
        || method_parameter_word_count(&resolved.method.proto.parameters)
            != Some(invocation.argument_count())
    {
        return Ok(false);
    }

    null_assert_argument_is_non_null(invocation, argument_index)
}

fn invocation_int_argument<A: MethodIntrinsicArguments + ?Sized>(
    invocation: &A,
    index: usize,
) -> Result<Option<i32>> {
    Ok(invocation
        .argument_value(index)?
        .as_ref()
        .and_then(read_int_argument))
}

fn invocation_long_argument<A: MethodIntrinsicArguments + ?Sized>(
    invocation: &A,
    index: usize,
) -> Result<Option<i64>> {
    Ok(invocation
        .argument_value(index)?
        .as_ref()
        .and_then(read_long_argument))
}

fn invocation_float_argument<A: MethodIntrinsicArguments + ?Sized>(
    invocation: &A,
    index: usize,
) -> Result<Option<f32>> {
    Ok(invocation
        .argument_value(index)?
        .as_ref()
        .and_then(read_float_argument))
}

struct IntrinsicProgressableFeature {
    progress: f32,
    feature: DexValue,
}

struct IntrinsicMeasuredCubic {
    cubic: DexValue,
    size: f32,
    start_progress: f32,
    end_progress: f32,
}

#[derive(Clone)]
struct IntrinsicCubicObject {
    object: DexValue,
    values: [f32; 8],
}

impl DexRunInvocationHandler<'_> {
    fn invoke_resolved_intrinsic(
        &self,
        resolved: &ResolvedMethod,
        invocation: &DexInvocation,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(intrinsic) = self.program.method_intrinsic(resolved)? else {
            return Ok(None);
        };

        self.apply_method_intrinsic(resolved, intrinsic, invocation)
    }

    fn invoke_resolved_intrinsic_from_frame(
        &self,
        resolved: &ResolvedMethod,
        intrinsic: MethodIntrinsic,
        invocation: &DexInvocationFrame<'_>,
    ) -> Result<Option<DexInvocationResult>> {
        self.apply_method_intrinsic(resolved, intrinsic, invocation)
    }

    fn apply_method_intrinsic<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        resolved: &ResolvedMethod,
        intrinsic: MethodIntrinsic,
        invocation: &A,
    ) -> Result<Option<DexInvocationResult>> {
        match intrinsic {
            MethodIntrinsic::InstanceFieldGetter {
                receiver_arg,
                field_index,
                value_type,
            } => {
                let Some(receiver) = invocation.argument_value(receiver_arg)? else {
                    return Ok(None);
                };
                let Some(value) = self.read_intrinsic_instance_field(
                    resolved.dex_file_index,
                    field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                if !intrinsic_value_matches_type(value_type, &value) {
                    return Ok(None);
                }
                Ok(Some(DexInvocationResult::Value(value)))
            }
            MethodIntrinsic::FloatPack { high_arg, low_arg } => {
                let Some(high) = invocation_float_argument(invocation, high_arg)? else {
                    return Ok(None);
                };
                let Some(low) = invocation_float_argument(invocation, low_arg)? else {
                    return Ok(None);
                };
                Ok(Some(DexInvocationResult::Value(DexValue::Long(
                    pack_float_pair(high, low),
                ))))
            }
            MethodIntrinsic::AffineFloatPack {
                receiver_arg,
                x_arg,
                y_arg,
                x_offset_field_index,
                scale_field_index,
                y_offset_field_index,
            } => {
                let Some(receiver) = invocation.argument_value(receiver_arg)? else {
                    return Ok(None);
                };
                let Some(x) = invocation_float_argument(invocation, x_arg)? else {
                    return Ok(None);
                };
                let Some(y) = invocation_float_argument(invocation, y_arg)? else {
                    return Ok(None);
                };
                let Some(x_offset) = self.read_intrinsic_instance_float(
                    resolved.dex_file_index,
                    x_offset_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some(scale) = self.read_intrinsic_instance_float(
                    resolved.dex_file_index,
                    scale_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some(y_offset) = self.read_intrinsic_instance_float(
                    resolved.dex_file_index,
                    y_offset_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                Ok(Some(DexInvocationResult::Value(DexValue::Long(
                    pack_float_pair((x + x_offset) / scale, (y + y_offset) / scale),
                ))))
            }
            // cite: androidx Matrix.kt#map
            MethodIntrinsic::MatrixPointTransform {
                receiver_arg,
                x_arg,
                y_arg,
                matrix_field_index,
            } => self.apply_matrix_point_transform(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                x_arg,
                y_arg,
                matrix_field_index,
            ),
            MethodIntrinsic::LongHighFloat { value_arg } => {
                let Some(value) = invocation_long_argument(invocation, value_arg)? else {
                    return Ok(None);
                };
                let bits = (value >> 32) as i32;
                Ok(Some(DexInvocationResult::Value(DexValue::Float(
                    f32::from_bits(bits as u32),
                ))))
            }
            MethodIntrinsic::LongLowFloat { value_arg } => {
                let Some(value) = invocation_long_argument(invocation, value_arg)? else {
                    return Ok(None);
                };
                let bits = (value & 0xffff_ffff) as i32;
                Ok(Some(DexInvocationResult::Value(DexValue::Float(
                    f32::from_bits(bits as u32),
                ))))
            }
            MethodIntrinsic::FloatArrayGetter {
                receiver_arg,
                field_index,
                element_index,
            } => {
                let Some(receiver) = invocation.argument_value(receiver_arg)? else {
                    return Ok(None);
                };
                let Some(array) = self.read_intrinsic_instance_field(
                    resolved.dex_file_index,
                    field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some(value) = self.program.read_array_value(&array, element_index) else {
                    return Ok(None);
                };
                let Some(value) = read_float_argument(&value) else {
                    return Ok(None);
                };
                Ok(Some(DexInvocationResult::Value(DexValue::Float(value))))
            }
            MethodIntrinsic::FloatArrayPairTransform {
                receiver_arg,
                transformer_arg,
                index_arg,
                array_field_index,
                transform_method_index,
                transform_is_interface,
            } => {
                let Some(receiver) = invocation.argument_value(receiver_arg)? else {
                    return Ok(None);
                };
                let Some(transformer) = invocation.argument_value(transformer_arg)? else {
                    return Ok(None);
                };
                let Some(index) = invocation_int_argument(invocation, index_arg)? else {
                    return Ok(None);
                };
                let Some(array) = self.read_intrinsic_instance_field(
                    resolved.dex_file_index,
                    array_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some(x) = self
                    .program
                    .read_array_value(&array, index)
                    .and_then(|value| read_float_argument(&value))
                else {
                    return Ok(None);
                };
                let Some(next_index) = index.checked_add(1) else {
                    return Ok(None);
                };
                let Some(y) = self
                    .program
                    .read_array_value(&array, next_index)
                    .and_then(|value| read_float_argument(&value))
                else {
                    return Ok(None);
                };
                let kind = if transform_is_interface {
                    DexInvokeKind::Interface
                } else {
                    DexInvokeKind::Virtual
                };
                let Some(DexInvocationResult::Value(DexValue::Long(packed))) = self
                    .invoke_receiver_intrinsic_only(
                        resolved.dex_file_index,
                        transform_method_index,
                        kind,
                        vec![transformer, DexValue::Float(x), DexValue::Float(y)],
                    )?
                else {
                    return Ok(None);
                };
                let high = f32::from_bits(((packed >> 32) as i32) as u32);
                let low = f32::from_bits((packed as i32) as u32);
                if self
                    .program
                    .write_array_value(&array, index, DexValue::Float(high))
                    .is_none()
                {
                    return Ok(None);
                }
                if self
                    .program
                    .write_array_value(&array, next_index, DexValue::Float(low))
                    .is_none()
                {
                    return Ok(None);
                }
                Ok(Some(DexInvocationResult::Void))
            }
            MethodIntrinsic::FloatRemAddRem {
                left_arg,
                right_arg,
            } => {
                let Some(left) = invocation_float_argument(invocation, left_arg)? else {
                    return Ok(None);
                };
                let Some(right) = invocation_float_argument(invocation, right_arg)? else {
                    return Ok(None);
                };
                Ok(Some(DexInvocationResult::Value(DexValue::Float(
                    ((left % right) + right) % right,
                ))))
            }
            MethodIntrinsic::FloatLerp {
                start_arg,
                end_arg,
                fraction_arg,
            } => {
                let Some(start) = invocation_float_argument(invocation, start_arg)? else {
                    return Ok(None);
                };
                let Some(end) = invocation_float_argument(invocation, end_arg)? else {
                    return Ok(None);
                };
                let Some(fraction) = invocation_float_argument(invocation, fraction_arg)? else {
                    return Ok(None);
                };
                Ok(Some(DexInvocationResult::Value(DexValue::Float(
                    (fraction * end) + ((1.0 - fraction) * start),
                ))))
            }
            MethodIntrinsic::FloatAtan2Normalize {
                x_arg,
                y_arg,
                limit_field_index,
            } => {
                let Some(x) = invocation_float_argument(invocation, x_arg)? else {
                    return Ok(None);
                };
                let Some(y) = invocation_float_argument(invocation, y_arg)? else {
                    return Ok(None);
                };
                let Some(limit) =
                    self.read_intrinsic_static_float(resolved.dex_file_index, limit_field_index)?
                else {
                    return Ok(None);
                };
                let angle = (f64::from(y)).atan2(f64::from(x)) as f32;
                Ok(Some(DexInvocationResult::Value(DexValue::Float(
                    (angle + limit) % limit,
                ))))
            }
            MethodIntrinsic::CubicFloatArrayPack {
                receiver_arg,
                t_arg,
                field_index,
            } => {
                let Some(receiver) = invocation.argument_value(receiver_arg)? else {
                    return Ok(None);
                };
                let Some(t) = invocation_float_argument(invocation, t_arg)? else {
                    return Ok(None);
                };
                let Some((x, y)) = self.compute_cubic_float_array_point(
                    resolved.dex_file_index,
                    field_index,
                    &receiver,
                    t,
                )?
                else {
                    return Ok(None);
                };
                Ok(Some(DexInvocationResult::Value(DexValue::Long(
                    pack_float_pair(x, y),
                ))))
            }
            MethodIntrinsic::PathAngleDelta {
                receiver_arg,
                t_arg,
                curve_field_index,
                curve_array_field_index,
                anchor_field_index,
                anchor_x_field_index,
                anchor_y_field_index,
                start_angle_field_index,
                offset_field_index,
                limit_field_index,
            } => {
                let Some(receiver) = invocation.argument_value(receiver_arg)? else {
                    return Ok(None);
                };
                let Some(t) = invocation_float_argument(invocation, t_arg)? else {
                    return Ok(None);
                };
                let Some(curve) = self.read_intrinsic_instance_field(
                    resolved.dex_file_index,
                    curve_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some(anchor) = self.read_intrinsic_instance_field(
                    resolved.dex_file_index,
                    anchor_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some((x, y)) = self.compute_cubic_float_array_point(
                    resolved.dex_file_index,
                    curve_array_field_index,
                    &curve,
                    t,
                )?
                else {
                    return Ok(None);
                };
                let Some(anchor_x) = self.read_intrinsic_instance_float(
                    resolved.dex_file_index,
                    anchor_x_field_index,
                    &anchor,
                )?
                else {
                    return Ok(None);
                };
                let Some(anchor_y) = self.read_intrinsic_instance_float(
                    resolved.dex_file_index,
                    anchor_y_field_index,
                    &anchor,
                )?
                else {
                    return Ok(None);
                };
                let Some(start_angle) = self.read_intrinsic_instance_float(
                    resolved.dex_file_index,
                    start_angle_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some(offset) = self.read_intrinsic_instance_float(
                    resolved.dex_file_index,
                    offset_field_index,
                    &receiver,
                )?
                else {
                    return Ok(None);
                };
                let Some(limit) =
                    self.read_intrinsic_static_float(resolved.dex_file_index, limit_field_index)?
                else {
                    return Ok(None);
                };
                let angle = (f64::from(y - anchor_y)).atan2(f64::from(x - anchor_x)) as f32;
                let normalized = (angle + limit) % limit;
                let delta = ((normalized - start_angle) % limit + limit) % limit;
                Ok(Some(DexInvocationResult::Value(DexValue::Float(
                    (delta - offset).abs(),
                ))))
            }
            // cite: androidx Cubic.kt#transformed, transform, transformOnePoint
            MethodIntrinsic::CubicTransform {
                receiver_arg,
                transformer_arg,
                points_field_index,
                mutable_cubic_type_index,
                transform_method_index,
            } => self.apply_cubic_transform(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                transformer_arg,
                points_field_index,
                mutable_cubic_type_index,
                transform_method_index,
            ),
            // cite: androidx Features.kt#Edge, transformed
            MethodIntrinsic::FeatureEdgeTransform {
                receiver_arg,
                transformer_arg,
                cubics_field_index,
                edge_type_index,
                cubic_transform_method_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            } => self.apply_feature_edge_transform(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                transformer_arg,
                cubics_field_index,
                edge_type_index,
                cubic_transform_method_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            ),
            // cite: androidx Features.kt#Corner, transformed
            MethodIntrinsic::FeatureCornerTransform {
                receiver_arg,
                transformer_arg,
                cubics_field_index,
                vertex_field_index,
                rounded_center_field_index,
                convex_field_index,
                corner_type_index,
                cubic_transform_method_index,
                point_transform_method_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            } => self.apply_feature_corner_transform(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                transformer_arg,
                cubics_field_index,
                vertex_field_index,
                rounded_center_field_index,
                convex_field_index,
                corner_type_index,
                cubic_transform_method_index,
                point_transform_method_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            ),
            // cite: androidx FeatureMapping.kt#featureDistSquared, featureRepresentativePoint
            // cite: androidx Point.kt#getDistanceSquared
            MethodIntrinsic::FeatureDistanceSquared {
                left_arg,
                right_arg,
                corner_type_index,
                convex_field_index,
                cubics_field_index,
                cubic_points_field_index,
            } => self.apply_feature_distance_squared(
                resolved.dex_file_index,
                invocation,
                left_arg,
                right_arg,
                corner_type_index,
                convex_field_index,
                cubics_field_index,
                cubic_points_field_index,
            ),
            // cite: androidx PolygonMeasure.kt#measurePolygon, MeasuredCubic
            MethodIntrinsic::MeasuredPolygonMeasure {
                measurer_arg,
                polygon_arg,
                polygon_features_field_index,
                feature_cubics_field_index,
                cubic_points_field_index,
                corner_type_index,
                center_x_field_index,
                center_y_field_index,
                measured_polygon_measurer_field_index,
                measured_polygon_cubics_field_index,
                measured_polygon_features_field_index,
                measured_cubic_owner_field_index,
                measured_cubic_cubic_field_index,
                measured_cubic_size_field_index,
                measured_cubic_start_field_index,
                measured_cubic_end_field_index,
                progress_feature_progress_field_index,
                progress_feature_feature_field_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            } => self.apply_measured_polygon_measure(
                resolved.dex_file_index,
                invocation,
                measurer_arg,
                polygon_arg,
                polygon_features_field_index,
                feature_cubics_field_index,
                cubic_points_field_index,
                corner_type_index,
                center_x_field_index,
                center_y_field_index,
                measured_polygon_measurer_field_index,
                measured_polygon_cubics_field_index,
                measured_polygon_features_field_index,
                measured_cubic_owner_field_index,
                measured_cubic_cubic_field_index,
                measured_cubic_size_field_index,
                measured_cubic_start_field_index,
                measured_cubic_end_field_index,
                progress_feature_progress_field_index,
                progress_feature_feature_field_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            ),
            // cite: androidx RoundedPolygon.kt#RoundedPolygon, cubics
            // cite: androidx Cubic.kt#zeroLength, split
            MethodIntrinsic::RoundedPolygonConstructor {
                receiver_arg,
                features_arg,
                center_x_arg,
                center_y_arg,
                features_field_index,
                center_x_field_index,
                center_y_field_index,
                cubics_field_index,
                feature_cubics_field_index,
                cubic_points_field_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            } => self.apply_rounded_polygon_constructor(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                features_arg,
                center_x_arg,
                center_y_arg,
                features_field_index,
                center_x_field_index,
                center_y_field_index,
                cubics_field_index,
                feature_cubics_field_index,
                cubic_points_field_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            ),
            // cite: androidx RoundedPolygon.kt#transformed
            MethodIntrinsic::RoundedPolygonTransform {
                receiver_arg,
                transformer_arg,
                features_field_index,
                center_x_field_index,
                center_y_field_index,
                feature_transform_method_index,
                point_transform_method_index,
                constructor_method_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            } => self.apply_rounded_polygon_transform(
                resolved,
                invocation,
                receiver_arg,
                transformer_arg,
                features_field_index,
                center_x_field_index,
                center_y_field_index,
                feature_transform_method_index,
                point_transform_method_index,
                constructor_method_index,
                list_freeze_method_index,
                list_array_field_index,
                list_size_field_index,
                list_read_only_field_index,
                list_mod_count_field_index,
            ),
            // cite: androidx RoundedPolygon.kt#calculateBounds
            // cite: androidx Cubic.kt#calculateBounds, zeroLength
            MethodIntrinsic::RoundedPolygonBounds {
                polygon_arg,
                bounds_arg,
                mask_arg,
                cubics_field_index,
                cubic_points_field_index,
            } => self.apply_rounded_polygon_bounds(
                resolved.dex_file_index,
                invocation,
                polygon_arg,
                bounds_arg,
                mask_arg,
                cubics_field_index,
                cubic_points_field_index,
            ),
            // cite: androidx RoundedPolygon.kt#normalized
            MethodIntrinsic::RoundedPolygonNormalized {
                receiver_arg,
                bounds_method_index,
                transformer_type_index,
                transformer_constructor_method_index,
                transform_method_index,
            } => self.apply_rounded_polygon_normalized(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                bounds_method_index,
                transformer_type_index,
                transformer_constructor_method_index,
                transform_method_index,
            ),
            MethodIntrinsic::MeasuredCubicCut {
                receiver_arg,
                progress_arg,
                cubic_field_index,
                size_field_index,
                start_field_index,
                end_field_index,
                owner_field_index,
                owner_center_field_index,
                center_x_field_index,
                center_y_field_index,
                cubic_points_field_index,
                pair_first_field_index,
                pair_second_field_index,
            } => self.apply_measured_cubic_cut(
                resolved,
                invocation,
                receiver_arg,
                progress_arg,
                cubic_field_index,
                size_field_index,
                start_field_index,
                end_field_index,
                owner_field_index,
                owner_center_field_index,
                center_x_field_index,
                center_y_field_index,
                cubic_points_field_index,
                pair_first_field_index,
                pair_second_field_index,
            ),
            MethodIntrinsic::CircularFloatInterpolation {
                x_arg,
                y_arg,
                progress_arg,
                array_field_index,
                size_field_index,
            } => {
                let Some(x_values) = invocation.argument_value(x_arg)? else {
                    return Ok(None);
                };
                let Some(y_values) = invocation.argument_value(y_arg)? else {
                    return Ok(None);
                };
                let Some(progress) = invocation_float_argument(invocation, progress_arg)? else {
                    return Ok(None);
                };
                if !(0.0..=1.0).contains(&progress) {
                    return Ok(None);
                }
                let Some(size) = self
                    .read_intrinsic_instance_field(
                        resolved.dex_file_index,
                        size_field_index,
                        &x_values,
                    )?
                    .and_then(|value| read_int_argument(&value))
                else {
                    return Ok(None);
                };
                if size <= 0 {
                    return Ok(None);
                }

                for index in 0..size {
                    let Some(x_start) = self.read_intrinsic_float_vector_value(
                        resolved.dex_file_index,
                        array_field_index,
                        &x_values,
                        index,
                    )?
                    else {
                        return Ok(None);
                    };
                    let Some(next_index) = index.checked_add(1).map(|value| value % size) else {
                        return Ok(None);
                    };
                    let Some(x_end) = self.read_intrinsic_float_vector_value(
                        resolved.dex_file_index,
                        array_field_index,
                        &x_values,
                        next_index,
                    )?
                    else {
                        return Ok(None);
                    };
                    let contains = if x_end >= x_start {
                        x_start <= progress && progress <= x_end
                    } else {
                        progress >= x_start || progress <= x_end
                    };
                    if !contains {
                        continue;
                    }

                    let x_delta = ((x_end - x_start) % 1.0 + 1.0) % 1.0;
                    let Some(y_start) = self.read_intrinsic_float_vector_value(
                        resolved.dex_file_index,
                        array_field_index,
                        &y_values,
                        index,
                    )?
                    else {
                        return Ok(None);
                    };
                    let Some(y_end) = self.read_intrinsic_float_vector_value(
                        resolved.dex_file_index,
                        array_field_index,
                        &y_values,
                        next_index,
                    )?
                    else {
                        return Ok(None);
                    };
                    let y_delta = ((y_end - y_start) % 1.0 + 1.0) % 1.0;
                    let fraction = if x_delta < 0.001 {
                        0.5
                    } else {
                        ((progress - x_start) % 1.0 + 1.0) % 1.0 / x_delta
                    };
                    let interpolated = ((y_delta * fraction + y_start) % 1.0 + 1.0) % 1.0;
                    return Ok(Some(DexInvocationResult::Value(DexValue::Float(
                        interpolated,
                    ))));
                }

                Ok(None)
            }
            // cite: libcore Object.java#equals
            MethodIntrinsic::ObjectEquals {
                left_arg,
                right_arg,
            } => {
                let Some(left) = invocation.argument_value(left_arg)? else {
                    return Ok(None);
                };
                let Some(right) = invocation.argument_value(right_arg)? else {
                    return Ok(None);
                };
                let equals_invocation = DexInvocation {
                    kind: DexInvokeKind::Virtual,
                    range: false,
                    method_index: 0,
                    argument_registers: Default::default(),
                    arguments: smallvec::smallvec![left, right],
                };
                let value = self
                    .program
                    .runtime_object_equals(&equals_invocation, self.depth + 1)?;

                Ok(Some(DexInvocationResult::Value(DexValue::Int(value))))
            }
            // cite: libcore Arrays.java#copyOf
            // cite: libcore System.java#arraycopy
            MethodIntrinsic::ObjectArrayGapInsert {
                receiver_arg,
                index_arg,
                count_arg,
                array_field_index,
                size_field_index,
            } => self.apply_object_array_gap_insert(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                index_arg,
                count_arg,
                array_field_index,
                size_field_index,
            ),
            // cite: libcore AbstractList.java#modCount
            // cite: libcore Arrays.java#copyOf
            MethodIntrinsic::ObjectArrayAppend {
                receiver_arg,
                value_arg,
                array_field_index,
                size_field_index,
                read_only_field_index,
                mod_count_field_index,
            } => self.apply_object_array_append(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                value_arg,
                array_field_index,
                size_field_index,
                read_only_field_index,
                mod_count_field_index,
            ),
            // cite: androidx SlotTable.kt#insertSlots, moveSlotGapTo
            // cite: libcore System.java#arraycopy
            MethodIntrinsic::SlotObjectGapEnsure {
                receiver_arg,
                count_arg,
                anchor_arg,
                slot_index_field_index,
                slot_limit_field_index,
                object_array_field_index,
                object_gap_start_field_index,
                object_gap_size_field_index,
                object_anchor_field_index,
                group_array_field_index,
                group_gap_start_field_index,
                group_gap_size_field_index,
            } => self.apply_slot_object_gap_ensure(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                count_arg,
                anchor_arg,
                slot_index_field_index,
                slot_limit_field_index,
                object_array_field_index,
                object_gap_start_field_index,
                object_gap_size_field_index,
                object_anchor_field_index,
                group_array_field_index,
                group_gap_start_field_index,
                group_gap_size_field_index,
            ),
            // cite: androidx SlotTable.kt#update, rawUpdate, skip, set, insertSlots, moveSlotGapTo
            MethodIntrinsic::SlotObjectUpdate {
                receiver_arg,
                value_arg,
                insert_count_field_index,
                insert_anchor_field_index,
                slot_index_field_index,
                slot_limit_field_index,
                object_array_field_index,
                object_gap_start_field_index,
                object_gap_size_field_index,
                object_anchor_field_index,
                group_array_field_index,
                group_gap_start_field_index,
                group_gap_size_field_index,
            } => self.apply_slot_object_update(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                value_arg,
                insert_count_field_index,
                insert_anchor_field_index,
                slot_index_field_index,
                slot_limit_field_index,
                object_array_field_index,
                object_gap_start_field_index,
                object_gap_size_field_index,
                object_anchor_field_index,
                group_array_field_index,
                group_gap_start_field_index,
                group_gap_size_field_index,
            ),
            // cite: androidx SlotTable.kt#dataIndex, dataAnchorToDataIndex
            MethodIntrinsic::SlotGroupDataIndex {
                receiver_arg,
                groups_arg,
                address_arg,
                group_array_field_index,
                object_array_field_index,
                object_gap_size_field_index,
            } => self.apply_slot_group_data_index(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                groups_arg,
                address_arg,
                group_array_field_index,
                object_array_field_index,
                object_gap_size_field_index,
            ),
            // cite: androidx SlotTable.kt#size, capacity
            MethodIntrinsic::SlotGroupSize {
                receiver_arg,
                group_array_field_index,
                group_gap_size_field_index,
            } => self.apply_slot_group_size(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                group_array_field_index,
                group_gap_size_field_index,
            ),
            // cite: androidx SlotTable.kt#groupIndexToAddress
            MethodIntrinsic::SlotGroupAddress {
                receiver_arg,
                index_arg,
                group_gap_start_field_index,
                group_gap_size_field_index,
            } => self.apply_slot_group_address(
                resolved.dex_file_index,
                invocation,
                receiver_arg,
                index_arg,
                group_gap_start_field_index,
                group_gap_size_field_index,
            ),
        }
    }

    fn apply_measured_cubic_cut<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        resolved: &ResolvedMethod,
        invocation: &A,
        receiver_arg: usize,
        progress_arg: usize,
        cubic_field_index: u32,
        size_field_index: u32,
        start_field_index: u32,
        end_field_index: u32,
        owner_field_index: u32,
        owner_center_field_index: u32,
        center_x_field_index: u32,
        center_y_field_index: u32,
        cubic_points_field_index: u32,
        pair_first_field_index: u32,
        pair_second_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let Some(progress) = invocation_float_argument(invocation, progress_arg)? else {
            return Ok(None);
        };
        let Some(cubic) = self.read_intrinsic_instance_field(
            resolved.dex_file_index,
            cubic_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(owner) = self.read_intrinsic_instance_field(
            resolved.dex_file_index,
            owner_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(center) = self.read_intrinsic_instance_field(
            resolved.dex_file_index,
            owner_center_field_index,
            &owner,
        )?
        else {
            return Ok(None);
        };
        let Some(size) = self.read_intrinsic_instance_float(
            resolved.dex_file_index,
            size_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(start) = self.read_intrinsic_instance_float(
            resolved.dex_file_index,
            start_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(end) = self.read_intrinsic_instance_float(
            resolved.dex_file_index,
            end_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        if end < start {
            return Ok(None);
        }
        let Some(center_x) = self.read_intrinsic_instance_float(
            resolved.dex_file_index,
            center_x_field_index,
            &center,
        )?
        else {
            return Ok(None);
        };
        let Some(center_y) = self.read_intrinsic_instance_float(
            resolved.dex_file_index,
            center_y_field_index,
            &center,
        )?
        else {
            return Ok(None);
        };
        let Some(values) = self.read_intrinsic_cubic_values(
            resolved.dex_file_index,
            cubic_points_field_index,
            &cubic,
        )?
        else {
            return Ok(None);
        };

        let clamped = coerce_float_in(progress, start, end);
        let fraction = (clamped - start) / (end - start);
        let target = fraction * size;
        let start_angle = graphics_vector_angle(values[0] - center_x, values[1] - center_y);
        let cut = measured_cubic_cut_t(&values, center_x, center_y, start_angle, target);
        if 0.0 > cut || cut > 1.0 {
            return Ok(None);
        }

        let (left_values, right_values) = split_cubic_values(&values, cut);
        let Some(left_cubic) = self.allocate_intrinsic_cubic(
            resolved.dex_file_index,
            cubic_points_field_index,
            left_values,
        )?
        else {
            return Ok(None);
        };
        let Some(right_cubic) = self.allocate_intrinsic_cubic(
            resolved.dex_file_index,
            cubic_points_field_index,
            right_values,
        )?
        else {
            return Ok(None);
        };
        let Some(left) = self.allocate_intrinsic_measured_cubic(
            resolved,
            cubic_field_index,
            size_field_index,
            start_field_index,
            end_field_index,
            owner_field_index,
            cubic_points_field_index,
            &owner,
            left_cubic,
            left_values,
            center_x,
            center_y,
            start,
            clamped,
        )?
        else {
            return Ok(None);
        };
        let Some(right) = self.allocate_intrinsic_measured_cubic(
            resolved,
            cubic_field_index,
            size_field_index,
            start_field_index,
            end_field_index,
            owner_field_index,
            cubic_points_field_index,
            &owner,
            right_cubic,
            right_values,
            center_x,
            center_y,
            clamped,
            end,
        )?
        else {
            return Ok(None);
        };
        let first_field = self
            .program
            .field_id(resolved.dex_file_index, pair_first_field_index)?;
        let second_field = self
            .program
            .field_id(resolved.dex_file_index, pair_second_field_index)?;
        if first_field.class_descriptor != second_field.class_descriptor {
            return Ok(None);
        }
        let pair = self
            .program
            .allocate_object(first_field.class_descriptor.clone());
        if !self.write_intrinsic_instance_value(
            resolved.dex_file_index,
            pair_first_field_index,
            &pair,
            left,
        )? || !self.write_intrinsic_instance_value(
            resolved.dex_file_index,
            pair_second_field_index,
            &pair,
            right,
        )? {
            return Ok(None);
        }

        Ok(Some(DexInvocationResult::Value(pair)))
    }

    fn allocate_intrinsic_cubic(
        &self,
        dex_file_index: usize,
        cubic_points_field_index: u32,
        values: [f32; 8],
    ) -> Result<Option<DexValue>> {
        let points_field = self
            .program
            .field_id(dex_file_index, cubic_points_field_index)?;
        if points_field.type_descriptor != "[F" {
            return Ok(None);
        }
        let points = self.program.allocate_filled_array(
            "[F".to_owned(),
            values.into_iter().map(DexValue::Float).collect(),
        );
        let cubic = self
            .program
            .allocate_object(points_field.class_descriptor.clone());
        if !self.write_intrinsic_instance_value(
            dex_file_index,
            cubic_points_field_index,
            &cubic,
            points,
        )? {
            return Ok(None);
        }

        Ok(Some(cubic))
    }

    fn allocate_intrinsic_measured_cubic(
        &self,
        resolved: &ResolvedMethod,
        cubic_field_index: u32,
        size_field_index: u32,
        start_field_index: u32,
        end_field_index: u32,
        owner_field_index: u32,
        cubic_points_field_index: u32,
        owner: &DexValue,
        cubic: DexValue,
        values: [f32; 8],
        center_x: f32,
        center_y: f32,
        start: f32,
        end: f32,
    ) -> Result<Option<DexValue>> {
        if end < start {
            return Ok(None);
        }
        let receiver = self
            .program
            .allocate_object(resolved.class_descriptor.clone());
        let size = measured_cubic_size(&values, center_x, center_y);
        if !self.write_intrinsic_instance_value(
            resolved.dex_file_index,
            owner_field_index,
            &receiver,
            owner.clone(),
        )? || !self.write_intrinsic_instance_value(
            resolved.dex_file_index,
            cubic_field_index,
            &receiver,
            cubic,
        )? || !self.write_intrinsic_instance_value(
            resolved.dex_file_index,
            size_field_index,
            &receiver,
            DexValue::Float(size),
        )? || !self.write_intrinsic_instance_value(
            resolved.dex_file_index,
            start_field_index,
            &receiver,
            DexValue::Float(start),
        )? || !self.write_intrinsic_instance_value(
            resolved.dex_file_index,
            end_field_index,
            &receiver,
            DexValue::Float(end),
        )? {
            return Ok(None);
        }
        let Some(stored_cubic) = self.read_intrinsic_instance_field(
            resolved.dex_file_index,
            cubic_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        if self
            .read_intrinsic_cubic_values(
                resolved.dex_file_index,
                cubic_points_field_index,
                &stored_cubic,
            )?
            .is_none()
        {
            return Ok(None);
        }

        Ok(Some(receiver))
    }

    fn apply_matrix_point_transform<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        x_arg: usize,
        y_arg: usize,
        matrix_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let Some(x) = invocation_float_argument(invocation, x_arg)? else {
            return Ok(None);
        };
        let Some(y) = invocation_float_argument(invocation, y_arg)? else {
            return Ok(None);
        };
        let matrix_field = self.program.field_id(dex_file_index, matrix_field_index)?;
        if matrix_field.type_descriptor != "[F" {
            return Ok(None);
        }
        let Some(matrix) =
            self.read_intrinsic_instance_field(dex_file_index, matrix_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let matrix_key = map_identity_key(&matrix);
        let runtime = self.program.runtime.borrow();
        let Some(values) = runtime.array_entries.get(&matrix_key) else {
            return Ok(None);
        };
        if values.len() < 16 {
            return Ok(Some(DexInvocationResult::Value(DexValue::Long(
                pack_float_pair(x, y),
            ))));
        }
        let mut matrix = [0.0f32; 16];
        for (index, target) in matrix.iter_mut().enumerate() {
            let Some(value) = read_float_argument(&values[index]) else {
                return Ok(None);
            };
            *target = value;
        }
        let z = (matrix[3] * x) + (matrix[7] * y) + matrix[15];
        let inverse_z = 1.0f32 / z;
        let p_z = if inverse_z.is_finite() {
            inverse_z
        } else {
            0.0
        };
        let transformed_x = p_z * ((matrix[0] * x) + (matrix[4] * y) + matrix[12]);
        let transformed_y = p_z * ((matrix[1] * x) + (matrix[5] * y) + matrix[13]);

        Ok(Some(DexInvocationResult::Value(DexValue::Long(
            pack_float_pair(transformed_x, transformed_y),
        ))))
    }

    fn apply_cubic_transform<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        transformer_arg: usize,
        points_field_index: u32,
        mutable_cubic_type_index: u32,
        transform_method_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let Some(transformer) = invocation.argument_value(transformer_arg)? else {
            return Ok(None);
        };
        let Some(mut values) =
            self.read_intrinsic_cubic_values(dex_file_index, points_field_index, &receiver)?
        else {
            return Ok(None);
        };
        for index in [0usize, 2, 4, 6] {
            let Some(DexInvocationResult::Value(DexValue::Long(packed))) = self
                .invoke_receiver_intrinsic_only(
                    dex_file_index,
                    transform_method_index,
                    DexInvokeKind::Interface,
                    vec![
                        transformer.clone(),
                        DexValue::Float(values[index]),
                        DexValue::Float(values[index + 1]),
                    ],
                )?
            else {
                return Ok(None);
            };
            values[index] = f32::from_bits(((packed >> 32) as i32) as u32);
            values[index + 1] = f32::from_bits((packed as i32) as u32);
        }

        let points_field = self.program.field_id(dex_file_index, points_field_index)?;
        if points_field.type_descriptor != "[F" {
            return Ok(None);
        }
        let mutable_cubic_descriptor = self
            .program
            .type_descriptor(dex_file_index, mutable_cubic_type_index)?;
        if !mutable_cubic_descriptor.starts_with('L') {
            return Ok(None);
        }
        let points = self.program.allocate_filled_array(
            "[F".to_owned(),
            values.into_iter().map(DexValue::Float).collect(),
        );
        let cubic = self.program.allocate_object(mutable_cubic_descriptor);
        if !self.write_intrinsic_instance_value(
            dex_file_index,
            points_field_index,
            &cubic,
            points,
        )? {
            return Ok(None);
        }

        Ok(Some(DexInvocationResult::Value(cubic)))
    }

    fn apply_feature_edge_transform<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        transformer_arg: usize,
        cubics_field_index: u32,
        edge_type_index: u32,
        cubic_transform_method_index: u32,
        list_freeze_method_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
        list_read_only_field_index: u32,
        list_mod_count_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let Some(transformer) = invocation.argument_value(transformer_arg)? else {
            return Ok(None);
        };
        let Some(cubics_list) =
            self.read_intrinsic_instance_field(dex_file_index, cubics_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(transformed) = self.transform_intrinsic_cubic_list(
            dex_file_index,
            &cubics_list,
            &transformer,
            cubic_transform_method_index,
            list_array_field_index,
            list_size_field_index,
        )?
        else {
            return Ok(None);
        };

        let Some(transformed_list) = self.allocate_intrinsic_list_builder(
            dex_file_index,
            list_freeze_method_index,
            list_array_field_index,
            list_size_field_index,
            list_read_only_field_index,
            list_mod_count_field_index,
            transformed,
        )?
        else {
            return Ok(None);
        };

        let edge_descriptor = self
            .program
            .type_descriptor(dex_file_index, edge_type_index)?;
        if !edge_descriptor.starts_with('L') {
            return Ok(None);
        }
        let edge = self.program.allocate_object(edge_descriptor);
        if !self.write_intrinsic_instance_value(
            dex_file_index,
            cubics_field_index,
            &edge,
            transformed_list,
        )? {
            return Ok(None);
        }

        Ok(Some(DexInvocationResult::Value(edge)))
    }

    fn apply_feature_corner_transform<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        transformer_arg: usize,
        cubics_field_index: u32,
        vertex_field_index: u32,
        rounded_center_field_index: u32,
        convex_field_index: u32,
        corner_type_index: u32,
        cubic_transform_method_index: u32,
        point_transform_method_index: u32,
        list_freeze_method_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
        list_read_only_field_index: u32,
        list_mod_count_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let Some(transformer) = invocation.argument_value(transformer_arg)? else {
            return Ok(None);
        };
        let Some(cubics_list) =
            self.read_intrinsic_instance_field(dex_file_index, cubics_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(transformed) = self.transform_intrinsic_cubic_list(
            dex_file_index,
            &cubics_list,
            &transformer,
            cubic_transform_method_index,
            list_array_field_index,
            list_size_field_index,
        )?
        else {
            return Ok(None);
        };
        let Some(vertex) =
            self.read_intrinsic_instance_field(dex_file_index, vertex_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(rounded_center) = self.read_intrinsic_instance_field(
            dex_file_index,
            rounded_center_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(convex) =
            self.read_intrinsic_instance_bool(dex_file_index, convex_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(vertex) = read_long_argument(&vertex) else {
            return Ok(None);
        };
        let Some(rounded_center) = read_long_argument(&rounded_center) else {
            return Ok(None);
        };
        let Some(transformed_vertex) = self.transform_intrinsic_packed_point(
            dex_file_index,
            &transformer,
            vertex,
            point_transform_method_index,
        )?
        else {
            return Ok(None);
        };
        let Some(transformed_center) = self.transform_intrinsic_packed_point(
            dex_file_index,
            &transformer,
            rounded_center,
            point_transform_method_index,
        )?
        else {
            return Ok(None);
        };

        let Some(transformed_list) = self.allocate_intrinsic_list_builder(
            dex_file_index,
            list_freeze_method_index,
            list_array_field_index,
            list_size_field_index,
            list_read_only_field_index,
            list_mod_count_field_index,
            transformed,
        )?
        else {
            return Ok(None);
        };

        let corner_descriptor = self
            .program
            .type_descriptor(dex_file_index, corner_type_index)?;
        if !corner_descriptor.starts_with('L') {
            return Ok(None);
        }
        let corner = self.program.allocate_object(corner_descriptor);
        if !self.write_intrinsic_instance_value(
            dex_file_index,
            cubics_field_index,
            &corner,
            transformed_list,
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            vertex_field_index,
            &corner,
            DexValue::Long(transformed_vertex),
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            rounded_center_field_index,
            &corner,
            DexValue::Long(transformed_center),
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            convex_field_index,
            &corner,
            DexValue::Int(i32::from(convex)),
        )? {
            return Ok(None);
        }

        Ok(Some(DexInvocationResult::Value(corner)))
    }

    fn apply_feature_distance_squared<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        left_arg: usize,
        right_arg: usize,
        corner_type_index: u32,
        convex_field_index: u32,
        cubics_field_index: u32,
        cubic_points_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(left) = invocation.argument_value(left_arg)? else {
            return Ok(None);
        };
        let Some(right) = invocation.argument_value(right_arg)? else {
            return Ok(None);
        };
        let corner_descriptor = self
            .program
            .type_descriptor(dex_file_index, corner_type_index)?;
        let left_is_corner = self.program.value_matches_type(&left, &corner_descriptor);
        let right_is_corner = self.program.value_matches_type(&right, &corner_descriptor);
        if left_is_corner && right_is_corner {
            let Some(left_convex) =
                self.read_intrinsic_instance_bool(dex_file_index, convex_field_index, &left)?
            else {
                return Ok(None);
            };
            let Some(right_convex) =
                self.read_intrinsic_instance_bool(dex_file_index, convex_field_index, &right)?
            else {
                return Ok(None);
            };
            if left_convex != right_convex {
                return Ok(Some(DexInvocationResult::Value(DexValue::Float(f32::MAX))));
            }
        }
        let Some((left_x, left_y)) = self.intrinsic_feature_representative_point(
            dex_file_index,
            &left,
            cubics_field_index,
            cubic_points_field_index,
        )?
        else {
            return Ok(None);
        };
        let Some((right_x, right_y)) = self.intrinsic_feature_representative_point(
            dex_file_index,
            &right,
            cubics_field_index,
            cubic_points_field_index,
        )?
        else {
            return Ok(None);
        };
        let delta_x = left_x - right_x;
        let delta_y = left_y - right_y;
        let distance = delta_x * delta_x + delta_y * delta_y;

        Ok(Some(DexInvocationResult::Value(DexValue::Float(distance))))
    }

    fn apply_measured_polygon_measure<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        measurer_arg: usize,
        polygon_arg: usize,
        polygon_features_field_index: u32,
        feature_cubics_field_index: u32,
        cubic_points_field_index: u32,
        corner_type_index: u32,
        center_x_field_index: u32,
        center_y_field_index: u32,
        measured_polygon_measurer_field_index: u32,
        measured_polygon_cubics_field_index: u32,
        measured_polygon_features_field_index: u32,
        measured_cubic_owner_field_index: u32,
        measured_cubic_cubic_field_index: u32,
        measured_cubic_size_field_index: u32,
        measured_cubic_start_field_index: u32,
        measured_cubic_end_field_index: u32,
        progress_feature_progress_field_index: u32,
        progress_feature_feature_field_index: u32,
        list_freeze_method_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
        list_read_only_field_index: u32,
        list_mod_count_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(measurer) = invocation.argument_value(measurer_arg)? else {
            return Ok(None);
        };
        let Some(polygon) = invocation.argument_value(polygon_arg)? else {
            return Ok(None);
        };
        let Some(center_x) =
            self.read_intrinsic_instance_float(dex_file_index, center_x_field_index, &measurer)?
        else {
            return Ok(None);
        };
        let Some(center_y) =
            self.read_intrinsic_instance_float(dex_file_index, center_y_field_index, &measurer)?
        else {
            return Ok(None);
        };
        let Some(features_list) = self.read_intrinsic_instance_field(
            dex_file_index,
            polygon_features_field_index,
            &polygon,
        )?
        else {
            return Ok(None);
        };
        let Some(features) = self.read_intrinsic_list_values(
            dex_file_index,
            &features_list,
            list_array_field_index,
            list_size_field_index,
        )?
        else {
            return Ok(None);
        };

        let corner_descriptor = self
            .program
            .type_descriptor(dex_file_index, corner_type_index)?;
        let mut cubics = Vec::new();
        let mut feature_to_cubic = Vec::new();
        for feature in features {
            if !dex_value_is_object_like(&feature) {
                return Ok(None);
            }
            let is_corner = self
                .program
                .value_matches_type(&feature, &corner_descriptor);
            let Some(cubics_list) = self.read_intrinsic_instance_field(
                dex_file_index,
                feature_cubics_field_index,
                &feature,
            )?
            else {
                return Ok(None);
            };
            let Some(feature_cubics) = self.read_intrinsic_list_values(
                dex_file_index,
                &cubics_list,
                list_array_field_index,
                list_size_field_index,
            )?
            else {
                return Ok(None);
            };
            let representative_index = feature_cubics.len() / 2;
            for (index, cubic) in feature_cubics.into_iter().enumerate() {
                if !dex_value_is_object_like(&cubic) {
                    return Ok(None);
                }
                if is_corner && index == representative_index {
                    feature_to_cubic.push((cubics.len(), feature.clone()));
                }
                cubics.push(cubic);
            }
        }
        if cubics.is_empty() {
            return Ok(None);
        }

        let mut cubic_sizes = Vec::with_capacity(cubics.len());
        let mut cumulative = Vec::with_capacity(cubics.len() + 1);
        cumulative.push(0.0);
        let mut total = 0.0;
        for cubic in &cubics {
            let Some(values) =
                self.read_intrinsic_cubic_values(dex_file_index, cubic_points_field_index, cubic)?
            else {
                return Ok(None);
            };
            let size = measured_cubic_size(&values, center_x, center_y);
            if !size.is_finite() || size < 0.0 {
                return Ok(None);
            }
            total += size;
            if !total.is_finite() {
                return Ok(None);
            }
            cubic_sizes.push(size);
            cumulative.push(total);
        }
        if total <= 0.0 {
            return Ok(None);
        }
        let outline_progress = cumulative
            .into_iter()
            .map(|measure| measure / total)
            .collect::<Vec<_>>();

        let mut progress_features = Vec::with_capacity(feature_to_cubic.len());
        for (cubic_index, feature) in feature_to_cubic {
            let Some(start) = outline_progress.get(cubic_index).copied() else {
                return Ok(None);
            };
            let Some(end) = outline_progress.get(cubic_index + 1).copied() else {
                return Ok(None);
            };
            progress_features.push(IntrinsicProgressableFeature {
                progress: graphics_positive_remainder((start + end) / 2.0, 1.0),
                feature,
            });
        }

        let measured_polygon_descriptor = self
            .program
            .field_id(dex_file_index, measured_polygon_measurer_field_index)?
            .class_descriptor
            .clone();
        let measured_cubic_descriptor = self
            .program
            .field_id(dex_file_index, measured_cubic_cubic_field_index)?
            .class_descriptor
            .clone();
        let progress_feature_descriptor = self
            .program
            .field_id(dex_file_index, progress_feature_progress_field_index)?
            .class_descriptor
            .clone();
        let measured_polygon = self.program.allocate_object(measured_polygon_descriptor);

        let Some(progress_feature_values) = self.allocate_intrinsic_progress_features(
            dex_file_index,
            progress_feature_descriptor,
            progress_features,
            progress_feature_progress_field_index,
            progress_feature_feature_field_index,
        )?
        else {
            return Ok(None);
        };
        let Some(progress_feature_list) = self.allocate_intrinsic_list_builder(
            dex_file_index,
            list_freeze_method_index,
            list_array_field_index,
            list_size_field_index,
            list_read_only_field_index,
            list_mod_count_field_index,
            progress_feature_values,
        )?
        else {
            return Ok(None);
        };

        let Some(measured_cubics) =
            self.build_intrinsic_measured_cubics(&cubics, &cubic_sizes, &outline_progress)
        else {
            return Ok(None);
        };
        let Some(measured_cubic_values) = self.allocate_intrinsic_measured_cubics(
            dex_file_index,
            measured_cubic_descriptor,
            &measured_polygon,
            measured_cubics,
            measured_cubic_owner_field_index,
            measured_cubic_cubic_field_index,
            measured_cubic_size_field_index,
            measured_cubic_start_field_index,
            measured_cubic_end_field_index,
        )?
        else {
            return Ok(None);
        };
        let measured_cubics_list =
            self.allocate_intrinsic_array_list(measured_cubic_values.clone());

        if !self.write_intrinsic_instance_value(
            dex_file_index,
            measured_polygon_measurer_field_index,
            &measured_polygon,
            measurer,
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            measured_polygon_features_field_index,
            &measured_polygon,
            progress_feature_list,
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            measured_polygon_cubics_field_index,
            &measured_polygon,
            measured_cubics_list,
        )? {
            return Ok(None);
        }
        self.program
            .runtime
            .borrow_mut()
            .list_entries
            .insert(map_identity_key(&measured_polygon), measured_cubic_values);

        Ok(Some(DexInvocationResult::Value(measured_polygon)))
    }

    fn apply_rounded_polygon_constructor<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        features_arg: usize,
        center_x_arg: usize,
        center_y_arg: usize,
        features_field_index: u32,
        center_x_field_index: u32,
        center_y_field_index: u32,
        cubics_field_index: u32,
        feature_cubics_field_index: u32,
        cubic_points_field_index: u32,
        list_freeze_method_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
        list_read_only_field_index: u32,
        list_mod_count_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let Some(features_list) = invocation.argument_value(features_arg)? else {
            return Ok(None);
        };
        let Some(center_x) = invocation_float_argument(invocation, center_x_arg)? else {
            return Ok(None);
        };
        let Some(center_y) = invocation_float_argument(invocation, center_y_arg)? else {
            return Ok(None);
        };
        let Some(features) = self.read_intrinsic_list_values(
            dex_file_index,
            &features_list,
            list_array_field_index,
            list_size_field_index,
        )?
        else {
            return Ok(None);
        };
        let Some(cubics) = self.build_rounded_polygon_cubics(
            dex_file_index,
            &features,
            feature_cubics_field_index,
            cubic_points_field_index,
            list_array_field_index,
            list_size_field_index,
            center_x,
            center_y,
        )?
        else {
            return Ok(None);
        };
        if !rounded_polygon_cubics_are_contiguous(&cubics) {
            return Ok(None);
        }
        let cubic_values = cubics
            .iter()
            .map(|cubic| cubic.object.clone())
            .collect::<Vec<_>>();
        let Some(cubic_list) = self.allocate_intrinsic_list_builder(
            dex_file_index,
            list_freeze_method_index,
            list_array_field_index,
            list_size_field_index,
            list_read_only_field_index,
            list_mod_count_field_index,
            cubic_values,
        )?
        else {
            return Ok(None);
        };

        if !self.write_intrinsic_instance_value(
            dex_file_index,
            features_field_index,
            &receiver,
            features_list,
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            center_x_field_index,
            &receiver,
            DexValue::Float(center_x),
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            center_y_field_index,
            &receiver,
            DexValue::Float(center_y),
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            cubics_field_index,
            &receiver,
            cubic_list,
        )? {
            return Ok(None);
        }

        Ok(Some(DexInvocationResult::Void))
    }

    fn apply_rounded_polygon_transform<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        resolved: &ResolvedMethod,
        invocation: &A,
        receiver_arg: usize,
        transformer_arg: usize,
        features_field_index: u32,
        center_x_field_index: u32,
        center_y_field_index: u32,
        feature_transform_method_index: u32,
        point_transform_method_index: u32,
        constructor_method_index: u32,
        list_freeze_method_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
        list_read_only_field_index: u32,
        list_mod_count_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let Some(transformer) = invocation.argument_value(transformer_arg)? else {
            return Ok(None);
        };
        let Some(center_x) = self.read_intrinsic_instance_float(
            resolved.dex_file_index,
            center_x_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(center_y) = self.read_intrinsic_instance_float(
            resolved.dex_file_index,
            center_y_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(features_list) = self.read_intrinsic_instance_field(
            resolved.dex_file_index,
            features_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(features) = self.read_intrinsic_list_values(
            resolved.dex_file_index,
            &features_list,
            list_array_field_index,
            list_size_field_index,
        )?
        else {
            return Ok(None);
        };
        let Some(transformed_center) = self.transform_intrinsic_packed_point(
            resolved.dex_file_index,
            &transformer,
            pack_float_pair(center_x, center_y),
            point_transform_method_index,
        )?
        else {
            return Ok(None);
        };
        let transformed_center_x = f32::from_bits(((transformed_center >> 32) as i32) as u32);
        let transformed_center_y = f32::from_bits((transformed_center as i32) as u32);

        let mut transformed_features = Vec::with_capacity(features.len());
        for feature in features {
            let Some(DexInvocationResult::Value(value)) = self.invoke_receiver_intrinsic_only(
                resolved.dex_file_index,
                feature_transform_method_index,
                DexInvokeKind::Virtual,
                vec![feature, transformer.clone()],
            )?
            else {
                return Ok(None);
            };
            if !dex_value_is_object_like(&value) {
                return Ok(None);
            }
            transformed_features.push(value);
        }
        let Some(transformed_features_list) = self.allocate_intrinsic_list_builder(
            resolved.dex_file_index,
            list_freeze_method_index,
            list_array_field_index,
            list_size_field_index,
            list_read_only_field_index,
            list_mod_count_field_index,
            transformed_features,
        )?
        else {
            return Ok(None);
        };
        let polygon = self
            .program
            .allocate_object(resolved.method.proto.return_type.clone());
        let constructor = self
            .program
            .method_id(resolved.dex_file_index, constructor_method_index)?;
        let Some(constructor_resolved) = self.program.resolve_exact_method(&constructor)? else {
            return Ok(None);
        };
        let Some(constructor_intrinsic) = self.program.method_intrinsic(&constructor_resolved)?
        else {
            return Ok(None);
        };
        let constructor_invocation = DexInvocation {
            kind: DexInvokeKind::Direct,
            range: false,
            method_index: constructor_method_index,
            argument_registers: Default::default(),
            arguments: smallvec::smallvec![
                polygon.clone(),
                transformed_features_list,
                DexValue::Float(transformed_center_x),
                DexValue::Float(transformed_center_y),
            ],
        };
        let Some(DexInvocationResult::Void) = self.apply_method_intrinsic(
            &constructor_resolved,
            constructor_intrinsic,
            &constructor_invocation,
        )?
        else {
            return Ok(None);
        };

        Ok(Some(DexInvocationResult::Value(polygon)))
    }

    fn apply_rounded_polygon_bounds<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        polygon_arg: usize,
        bounds_arg: usize,
        mask_arg: usize,
        cubics_field_index: u32,
        cubic_points_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(polygon) = invocation.argument_value(polygon_arg)? else {
            return Ok(None);
        };
        let Some(mask) = invocation_int_argument(invocation, mask_arg)? else {
            return Ok(None);
        };
        let bounds = if mask & 1 != 0 {
            self.program
                .allocate_filled_array("[F".to_owned(), vec![DexValue::Float(0.0); 4])
        } else {
            let Some(bounds) = invocation.argument_value(bounds_arg)? else {
                return Ok(None);
            };
            bounds
        };
        if value_class_descriptor(&bounds) != Some("[F") {
            return Ok(None);
        }
        if !matches!(self.program.array_length(&bounds), Some(length) if length >= 4) {
            return Ok(None);
        }
        let Some(cubics_list) =
            self.read_intrinsic_instance_field(dex_file_index, cubics_field_index, &polygon)?
        else {
            return Ok(None);
        };
        let Some(cubics) = self.read_intrinsic_runtime_list_values(&cubics_list) else {
            return Ok(None);
        };

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN_POSITIVE;
        let mut max_y = f32::MIN_POSITIVE;
        for cubic in cubics {
            if !dex_value_is_object_like(&cubic) {
                return Ok(None);
            }
            let Some(values) =
                self.read_intrinsic_cubic_values(dex_file_index, cubic_points_field_index, &cubic)?
            else {
                return Ok(None);
            };
            let [left, top, right, bottom] = rounded_polygon_cubic_approximate_bounds(&values);
            min_x = intrinsic_math_min_float(min_x, left);
            min_y = intrinsic_math_min_float(min_y, top);
            max_x = intrinsic_math_max_float(max_x, right);
            max_y = intrinsic_math_max_float(max_y, bottom);
        }

        for (index, value) in [min_x, min_y, max_x, max_y].into_iter().enumerate() {
            let Some(index) = i32::try_from(index).ok() else {
                return Ok(None);
            };
            if self
                .program
                .write_array_value(&bounds, index, DexValue::Float(value))
                .is_none()
            {
                return Ok(None);
            }
        }

        Ok(Some(DexInvocationResult::Value(bounds)))
    }

    fn apply_rounded_polygon_normalized<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        bounds_method_index: u32,
        transformer_type_index: u32,
        transformer_constructor_method_index: u32,
        transform_method_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        let bounds_method = self
            .program
            .method_id(dex_file_index, bounds_method_index)?;
        let Some(bounds_resolved) = self.program.resolve_exact_method(&bounds_method)? else {
            return Ok(None);
        };
        let Some(bounds_intrinsic) = self.program.method_intrinsic(&bounds_resolved)? else {
            return Ok(None);
        };
        let bounds_invocation = DexInvocation {
            kind: DexInvokeKind::Static,
            range: false,
            method_index: bounds_method_index,
            argument_registers: Default::default(),
            arguments: smallvec::smallvec![receiver.clone(), DexValue::Null, DexValue::Int(1)],
        };
        let Some(DexInvocationResult::Value(bounds)) =
            self.apply_method_intrinsic(&bounds_resolved, bounds_intrinsic, &bounds_invocation)?
        else {
            return Ok(None);
        };
        let Some(left) = self
            .program
            .read_array_value(&bounds, 0)
            .and_then(|value| read_float_argument(&value))
        else {
            return Ok(None);
        };
        let Some(top) = self
            .program
            .read_array_value(&bounds, 1)
            .and_then(|value| read_float_argument(&value))
        else {
            return Ok(None);
        };
        let Some(right) = self
            .program
            .read_array_value(&bounds, 2)
            .and_then(|value| read_float_argument(&value))
        else {
            return Ok(None);
        };
        let Some(bottom) = self
            .program
            .read_array_value(&bounds, 3)
            .and_then(|value| read_float_argument(&value))
        else {
            return Ok(None);
        };
        let width = right - left;
        let height = bottom - top;
        let side = intrinsic_math_max_float(width, height);
        let offset_x = (side - width) / 2.0 - left;
        let offset_y = (side - height) / 2.0 - top;

        let transformer_descriptor = self
            .program
            .type_descriptor(dex_file_index, transformer_type_index)?;
        if !transformer_descriptor.starts_with('L') {
            return Ok(None);
        }
        let transformer = self.program.allocate_object(transformer_descriptor);
        let constructor = self
            .program
            .method_id(dex_file_index, transformer_constructor_method_index)?;
        let Some(constructor_resolved) = self.program.resolve_exact_method(&constructor)? else {
            return Ok(None);
        };
        let constructor_arguments = [
            transformer.clone(),
            DexValue::Float(offset_x),
            DexValue::Float(offset_y),
            DexValue::Float(side),
        ];
        let constructor_result = self.program.execute_method(
            None,
            &constructor_resolved,
            Some(&constructor_arguments),
            self.depth + 1,
        )?;
        match constructor_result {
            DexExecutionResult::Void => {}
            DexExecutionResult::Parked(execution) => {
                self.program.discard_parked_execution(&execution);
                return Err(WinedroidError::new(
                    "path transformer constructor parked without a guest-thread scheduler",
                ));
            }
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => return Ok(None),
        }

        let transform_method = self
            .program
            .method_id(dex_file_index, transform_method_index)?;
        let Some(transform_resolved) = self.program.resolve_exact_method(&transform_method)? else {
            return Ok(None);
        };
        let Some(transform_intrinsic) = self.program.method_intrinsic(&transform_resolved)? else {
            return Ok(None);
        };
        let transform_invocation = DexInvocation {
            kind: DexInvokeKind::Virtual,
            range: false,
            method_index: transform_method_index,
            argument_registers: Default::default(),
            arguments: smallvec::smallvec![receiver, transformer],
        };

        self.apply_method_intrinsic(
            &transform_resolved,
            transform_intrinsic,
            &transform_invocation,
        )
    }

    fn build_rounded_polygon_cubics(
        &self,
        dex_file_index: usize,
        features: &[DexValue],
        feature_cubics_field_index: u32,
        cubic_points_field_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
        center_x: f32,
        center_y: f32,
    ) -> Result<Option<Vec<IntrinsicCubicObject>>> {
        let mut first_feature_split_start = None;
        let mut first_feature_split_end = None;
        if let Some(first_feature) = features.first() {
            let Some(first_cubics) = self.read_intrinsic_feature_cubics(
                dex_file_index,
                first_feature,
                feature_cubics_field_index,
                cubic_points_field_index,
                list_array_field_index,
                list_size_field_index,
            )?
            else {
                return Ok(None);
            };
            if first_cubics.len() == 3 {
                let (start_values, end_values) = split_cubic_values(&first_cubics[1].values, 0.5);
                let Some(start_cubic) = self.allocate_intrinsic_cubic_object(
                    dex_file_index,
                    cubic_points_field_index,
                    start_values,
                )?
                else {
                    return Ok(None);
                };
                let Some(end_cubic) = self.allocate_intrinsic_cubic_object(
                    dex_file_index,
                    cubic_points_field_index,
                    end_values,
                )?
                else {
                    return Ok(None);
                };
                first_feature_split_start = Some(vec![first_cubics[0].clone(), start_cubic]);
                first_feature_split_end = Some(vec![end_cubic, first_cubics[2].clone()]);
            }
        }

        let mut first_cubic: Option<IntrinsicCubicObject> = None;
        let mut last_cubic: Option<IntrinsicCubicObject> = None;
        let mut cubics = Vec::new();
        for index in 0..=features.len() {
            let feature_cubics = if index == 0 && first_feature_split_end.is_some() {
                first_feature_split_end.clone().unwrap_or_default()
            } else if index == features.len() {
                let Some(split_start) = first_feature_split_start.clone() else {
                    break;
                };
                split_start
            } else {
                let Some(source_cubics) = self.read_intrinsic_feature_cubics(
                    dex_file_index,
                    &features[index],
                    feature_cubics_field_index,
                    cubic_points_field_index,
                    list_array_field_index,
                    list_size_field_index,
                )?
                else {
                    return Ok(None);
                };
                source_cubics
            };
            for cubic in feature_cubics {
                if rounded_polygon_cubic_zero_length(&cubic.values) {
                    if let Some(last) = last_cubic.take() {
                        let mut adjusted_values = last.values;
                        adjusted_values[6] = cubic.values[6];
                        adjusted_values[7] = cubic.values[7];
                        let Some(adjusted) = self.allocate_intrinsic_cubic_object(
                            dex_file_index,
                            cubic_points_field_index,
                            adjusted_values,
                        )?
                        else {
                            return Ok(None);
                        };
                        last_cubic = Some(adjusted);
                    }
                } else {
                    if let Some(last) = last_cubic.take() {
                        cubics.push(last);
                    }
                    if first_cubic.is_none() {
                        first_cubic = Some(cubic.clone());
                    }
                    last_cubic = Some(cubic);
                }
            }
        }

        if let (Some(last), Some(first)) = (last_cubic, first_cubic) {
            let closing_values = [
                last.values[0],
                last.values[1],
                last.values[2],
                last.values[3],
                last.values[4],
                last.values[5],
                first.values[0],
                first.values[1],
            ];
            let Some(closing) = self.allocate_intrinsic_cubic_object(
                dex_file_index,
                cubic_points_field_index,
                closing_values,
            )?
            else {
                return Ok(None);
            };
            cubics.push(closing);
        } else {
            let empty_values = [
                center_x, center_y, center_x, center_y, center_x, center_y, center_x, center_y,
            ];
            let Some(empty) = self.allocate_intrinsic_cubic_object(
                dex_file_index,
                cubic_points_field_index,
                empty_values,
            )?
            else {
                return Ok(None);
            };
            cubics.push(empty);
        }

        Ok(Some(cubics))
    }

    fn read_intrinsic_feature_cubics(
        &self,
        dex_file_index: usize,
        feature: &DexValue,
        feature_cubics_field_index: u32,
        cubic_points_field_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
    ) -> Result<Option<Vec<IntrinsicCubicObject>>> {
        if !dex_value_is_object_like(feature) {
            return Ok(None);
        }
        let Some(cubics_list) = self.read_intrinsic_instance_field(
            dex_file_index,
            feature_cubics_field_index,
            feature,
        )?
        else {
            return Ok(None);
        };
        let Some(cubics) = self.read_intrinsic_list_values(
            dex_file_index,
            &cubics_list,
            list_array_field_index,
            list_size_field_index,
        )?
        else {
            return Ok(None);
        };
        let mut values = Vec::with_capacity(cubics.len());
        for cubic in cubics {
            if !dex_value_is_object_like(&cubic) {
                return Ok(None);
            }
            let Some(cubic_values) =
                self.read_intrinsic_cubic_values(dex_file_index, cubic_points_field_index, &cubic)?
            else {
                return Ok(None);
            };
            values.push(IntrinsicCubicObject {
                object: cubic,
                values: cubic_values,
            });
        }

        Ok(Some(values))
    }

    fn intrinsic_feature_representative_point(
        &self,
        dex_file_index: usize,
        feature: &DexValue,
        feature_cubics_field_index: u32,
        cubic_points_field_index: u32,
    ) -> Result<Option<(f32, f32)>> {
        if !dex_value_is_object_like(feature) {
            return Ok(None);
        }
        let Some(cubics_list) = self.read_intrinsic_instance_field(
            dex_file_index,
            feature_cubics_field_index,
            feature,
        )?
        else {
            return Ok(None);
        };
        let Some(cubics) = self.read_intrinsic_runtime_list_values(&cubics_list) else {
            return Ok(None);
        };
        let Some(first) = cubics.first() else {
            return Ok(None);
        };
        let Some(last) = cubics.last() else {
            return Ok(None);
        };
        let Some(first_values) =
            self.read_intrinsic_cubic_values(dex_file_index, cubic_points_field_index, first)?
        else {
            return Ok(None);
        };
        let Some(last_values) =
            self.read_intrinsic_cubic_values(dex_file_index, cubic_points_field_index, last)?
        else {
            return Ok(None);
        };

        Ok(Some((
            (first_values[0] + last_values[6]) / 2.0,
            (first_values[1] + last_values[7]) / 2.0,
        )))
    }

    fn allocate_intrinsic_cubic_object(
        &self,
        dex_file_index: usize,
        cubic_points_field_index: u32,
        values: [f32; 8],
    ) -> Result<Option<IntrinsicCubicObject>> {
        let Some(object) =
            self.allocate_intrinsic_cubic(dex_file_index, cubic_points_field_index, values)?
        else {
            return Ok(None);
        };

        Ok(Some(IntrinsicCubicObject { object, values }))
    }

    fn build_intrinsic_measured_cubics(
        &self,
        cubics: &[DexValue],
        cubic_sizes: &[f32],
        outline_progress: &[f32],
    ) -> Option<Vec<IntrinsicMeasuredCubic>> {
        if cubics.len() != cubic_sizes.len() || outline_progress.len() != cubics.len() + 1 {
            return None;
        }
        let mut measured = Vec::new();
        let mut start_progress = 0.0;
        for index in 0..cubics.len() {
            let end_progress = outline_progress[index + 1];
            if end_progress - outline_progress[index] > 0.0001 {
                measured.push(IntrinsicMeasuredCubic {
                    cubic: cubics[index].clone(),
                    size: cubic_sizes[index],
                    start_progress,
                    end_progress,
                });
                start_progress = end_progress;
            }
        }
        let last = measured.last_mut()?;
        last.end_progress = 1.0;

        Some(measured)
    }

    fn allocate_intrinsic_progress_features(
        &self,
        dex_file_index: usize,
        progress_feature_descriptor: String,
        progress_features: Vec<IntrinsicProgressableFeature>,
        progress_field_index: u32,
        feature_field_index: u32,
    ) -> Result<Option<Vec<DexValue>>> {
        let mut values = Vec::with_capacity(progress_features.len());
        for progress_feature in progress_features {
            let object = self
                .program
                .allocate_object(progress_feature_descriptor.clone());
            if !self.write_intrinsic_instance_value(
                dex_file_index,
                progress_field_index,
                &object,
                DexValue::Float(progress_feature.progress),
            )? || !self.write_intrinsic_instance_value(
                dex_file_index,
                feature_field_index,
                &object,
                progress_feature.feature,
            )? {
                return Ok(None);
            }
            values.push(object);
        }

        Ok(Some(values))
    }

    fn allocate_intrinsic_measured_cubics(
        &self,
        dex_file_index: usize,
        measured_cubic_descriptor: String,
        owner: &DexValue,
        measured_cubics: Vec<IntrinsicMeasuredCubic>,
        owner_field_index: u32,
        cubic_field_index: u32,
        size_field_index: u32,
        start_field_index: u32,
        end_field_index: u32,
    ) -> Result<Option<Vec<DexValue>>> {
        let mut values = Vec::with_capacity(measured_cubics.len());
        for measured_cubic in measured_cubics {
            let object = self
                .program
                .allocate_object(measured_cubic_descriptor.clone());
            if !self.write_intrinsic_instance_value(
                dex_file_index,
                owner_field_index,
                &object,
                owner.clone(),
            )? || !self.write_intrinsic_instance_value(
                dex_file_index,
                cubic_field_index,
                &object,
                measured_cubic.cubic,
            )? || !self.write_intrinsic_instance_value(
                dex_file_index,
                size_field_index,
                &object,
                DexValue::Float(measured_cubic.size),
            )? || !self.write_intrinsic_instance_value(
                dex_file_index,
                start_field_index,
                &object,
                DexValue::Float(measured_cubic.start_progress),
            )? || !self.write_intrinsic_instance_value(
                dex_file_index,
                end_field_index,
                &object,
                DexValue::Float(measured_cubic.end_progress),
            )? {
                return Ok(None);
            }
            values.push(object);
        }

        Ok(Some(values))
    }

    fn allocate_intrinsic_array_list(&self, values: Vec<DexValue>) -> DexValue {
        let list = self
            .program
            .allocate_object("Ljava/util/ArrayList;".to_owned());
        self.program
            .runtime
            .borrow_mut()
            .list_entries
            .insert(map_identity_key(&list), values);

        list
    }

    fn transform_intrinsic_cubic_list(
        &self,
        dex_file_index: usize,
        cubics_list: &DexValue,
        transformer: &DexValue,
        cubic_transform_method_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
    ) -> Result<Option<Vec<DexValue>>> {
        let Some(cubics) = self.read_intrinsic_list_values(
            dex_file_index,
            cubics_list,
            list_array_field_index,
            list_size_field_index,
        )?
        else {
            return Ok(None);
        };

        let mut transformed = Vec::with_capacity(cubics.len());
        for cubic in cubics {
            let Some(DexInvocationResult::Value(value)) = self.invoke_receiver_intrinsic_only(
                dex_file_index,
                cubic_transform_method_index,
                DexInvokeKind::Virtual,
                vec![cubic, transformer.clone()],
            )?
            else {
                return Ok(None);
            };
            if !dex_value_is_object_like(&value) {
                return Ok(None);
            }
            transformed.push(value);
        }

        Ok(Some(transformed))
    }

    fn read_intrinsic_list_values(
        &self,
        dex_file_index: usize,
        list: &DexValue,
        list_array_field_index: u32,
        list_size_field_index: u32,
    ) -> Result<Option<Vec<DexValue>>> {
        let list_key = map_identity_key(list);
        {
            let runtime = self.program.runtime.borrow();
            if let Some(values) = runtime.list_entries.get(&list_key) {
                return Ok(Some(values.clone()));
            }
        }

        let Some(size) =
            self.read_intrinsic_instance_int(dex_file_index, list_size_field_index, list)?
        else {
            return Ok(None);
        };
        let Some(size) = usize::try_from(size).ok() else {
            return Ok(None);
        };
        let Some(array) =
            self.read_intrinsic_instance_field(dex_file_index, list_array_field_index, list)?
        else {
            return Ok(None);
        };
        let Some(array_descriptor) = value_class_descriptor(&array) else {
            return Ok(None);
        };
        if !array_descriptor.starts_with("[L") {
            return Ok(None);
        }
        let array_key = map_identity_key(&array);
        let runtime = self.program.runtime.borrow();
        let Some(values) = runtime.array_entries.get(&array_key) else {
            return Ok(None);
        };
        if size > values.len() {
            return Ok(None);
        }

        Ok(Some(values[..size].to_vec()))
    }

    fn read_intrinsic_runtime_list_values(&self, list: &DexValue) -> Option<Vec<DexValue>> {
        let list_key = map_identity_key(list);
        self.program
            .runtime
            .borrow()
            .list_entries
            .get(&list_key)
            .cloned()
    }

    fn allocate_intrinsic_list_builder(
        &self,
        dex_file_index: usize,
        list_freeze_method_index: u32,
        list_array_field_index: u32,
        list_size_field_index: u32,
        list_read_only_field_index: u32,
        list_mod_count_field_index: u32,
        values: Vec<DexValue>,
    ) -> Result<Option<DexValue>> {
        let list_descriptor = self
            .program
            .method_id(dex_file_index, list_freeze_method_index)?
            .proto
            .return_type
            .clone();
        if !list_descriptor.starts_with('L') {
            return Ok(None);
        }
        let array_field = self
            .program
            .field_id(dex_file_index, list_array_field_index)?;
        if !array_field.type_descriptor.starts_with("[L") {
            return Ok(None);
        }
        let Some(size) = i32::try_from(values.len()).ok() else {
            return Ok(None);
        };
        let array = self
            .program
            .allocate_filled_array(array_field.type_descriptor.clone(), values.clone());
        let list = self.program.allocate_object(list_descriptor);
        if !self.write_intrinsic_instance_value(
            dex_file_index,
            list_array_field_index,
            &list,
            array,
        )? || !self.write_intrinsic_instance_int(
            dex_file_index,
            list_size_field_index,
            &list,
            size,
        )? || !self.write_intrinsic_instance_value(
            dex_file_index,
            list_read_only_field_index,
            &list,
            DexValue::Int(1),
        )? || !self.write_intrinsic_instance_int(
            dex_file_index,
            list_mod_count_field_index,
            &list,
            0,
        )? {
            return Ok(None);
        }
        self.program
            .runtime
            .borrow_mut()
            .list_entries
            .insert(map_identity_key(&list), values);

        Ok(Some(list))
    }

    fn transform_intrinsic_packed_point(
        &self,
        dex_file_index: usize,
        transformer: &DexValue,
        packed: i64,
        transform_method_index: u32,
    ) -> Result<Option<i64>> {
        let x = f32::from_bits(((packed >> 32) as i32) as u32);
        let y = f32::from_bits((packed as i32) as u32);
        let Some(DexInvocationResult::Value(DexValue::Long(transformed))) = self
            .invoke_receiver_intrinsic_only(
                dex_file_index,
                transform_method_index,
                DexInvokeKind::Interface,
                vec![transformer.clone(), DexValue::Float(x), DexValue::Float(y)],
            )?
        else {
            return Ok(None);
        };

        Ok(Some(transformed))
    }

    fn read_intrinsic_cubic_values(
        &self,
        dex_file_index: usize,
        cubic_points_field_index: u32,
        cubic: &DexValue,
    ) -> Result<Option<[f32; 8]>> {
        let Some(values) = self.read_intrinsic_float_array_prefix(
            dex_file_index,
            cubic_points_field_index,
            cubic,
            8,
        )?
        else {
            return Ok(None);
        };
        let Ok(values) = <[f32; 8]>::try_from(values) else {
            return Ok(None);
        };

        Ok(Some(values))
    }

    fn write_intrinsic_instance_value(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
        value: DexValue,
    ) -> Result<bool> {
        let resolved = self
            .program
            .resolved_instance_field(dex_file_index, field_index)?;
        let (field, signature) = match &resolved {
            Some(resolved) => (Rc::clone(&resolved.field), Rc::clone(&resolved.signature)),
            None => self
                .program
                .field_id_and_signature(dex_file_index, field_index)?,
        };
        if !intrinsic_value_matches_field_descriptor(&field.type_descriptor, &value) {
            return Ok(false);
        }

        match &resolved {
            Some(resolved) => self
                .program
                .write_stored_instance_field_resolved(resolved, object, value),
            None => Ok(self.program.write_stored_instance_field_ic(
                dex_file_index,
                field_index,
                object,
                &signature,
                value,
            )),
        }
    }

    fn apply_object_array_gap_insert<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        index_arg: usize,
        count_arg: usize,
        array_field_index: u32,
        size_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        if !matches!(receiver, DexValue::Object(_) | DexValue::Reference(_)) {
            return Ok(None);
        }
        let Some(index) = invocation_int_argument(invocation, index_arg)? else {
            return Ok(None);
        };
        let Some(count) = invocation_int_argument(invocation, count_arg)? else {
            return Ok(None);
        };
        let Some(size) = self
            .read_intrinsic_instance_field(dex_file_index, size_field_index, &receiver)?
            .and_then(|value| read_int_argument(&value))
        else {
            return Ok(None);
        };
        let Some(new_size) = size.checked_add(count) else {
            return Ok(None);
        };
        if index < 0 || count < 0 || index > size || new_size < 0 {
            return Ok(None);
        }
        let Some(mut array) =
            self.read_intrinsic_instance_field(dex_file_index, array_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(array_descriptor) = value_class_descriptor(&array).map(str::to_owned) else {
            return Ok(None);
        };
        if !array_descriptor.starts_with("[L") {
            return Ok(None);
        }
        let array_field = self.program.field_id(dex_file_index, array_field_index)?;
        let size_field = self.program.field_id(dex_file_index, size_field_index)?;

        let index = usize::try_from(index).ok();
        let count = usize::try_from(count).ok();
        let size = usize::try_from(size).ok();
        let new_size = usize::try_from(new_size).ok();
        let (Some(index), Some(count), Some(size), Some(new_size)) = (index, count, size, new_size)
        else {
            return Ok(None);
        };

        let array_key = map_identity_key(&array);
        let capacity = {
            let runtime = self.program.runtime.borrow();
            runtime.array_entries.get(&array_key).map(Vec::len)
        };
        let Some(capacity) = capacity else {
            return Ok(None);
        };
        if size > capacity || new_size > i32::MAX as usize {
            return Ok(None);
        }
        if new_size > capacity {
            let Some(new_capacity) = grown_object_array_capacity(capacity, new_size) else {
                return Ok(None);
            };
            let copied = {
                let runtime = self.program.runtime.borrow();
                let Some(values) = runtime.array_entries.get(&array_key) else {
                    return Ok(None);
                };
                let mut copied = values.clone();
                copied.resize(new_capacity, DexValue::Null);
                copied
            };
            let new_array = self.program.allocate_filled_array(array_descriptor, copied);
            if !self
                .program
                .write_stored_instance_field(&receiver, &array_field, new_array.clone())
            {
                return Ok(None);
            }
            array = new_array;
        }

        let array_key = map_identity_key(&array);
        {
            let mut runtime = self.program.runtime.borrow_mut();
            let Some(values) = runtime.array_entries.get_mut(&array_key) else {
                return Ok(None);
            };
            if new_size > values.len() || size > values.len() || index > size {
                return Ok(None);
            }
            if count > 0 {
                for source in (index..size).rev() {
                    values[source + count] = values[source].clone();
                }
            }
        }

        if !self.program.write_stored_instance_field(
            &receiver,
            &size_field,
            DexValue::Int(i32::try_from(new_size).unwrap_or(i32::MAX)),
        ) {
            return Ok(None);
        }

        Ok(Some(DexInvocationResult::Void))
    }

    // cite: libcore AbstractList.java#modCount
    // cite: libcore Arrays.java#copyOf
    fn apply_object_array_append<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        value_arg: usize,
        array_field_index: u32,
        size_field_index: u32,
        read_only_field_index: u32,
        mod_count_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        if !matches!(receiver, DexValue::Object(_)) {
            return Ok(None);
        }
        let Some(value) = invocation.argument_value(value_arg)? else {
            return Ok(None);
        };
        let Some(read_only) =
            self.read_intrinsic_instance_bool(dex_file_index, read_only_field_index, &receiver)?
        else {
            return Ok(None);
        };
        if read_only {
            return Ok(None);
        }
        let Some(size) =
            self.read_intrinsic_instance_int(dex_file_index, size_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(mod_count) =
            self.read_intrinsic_instance_int(dex_file_index, mod_count_field_index, &receiver)?
        else {
            return Ok(None);
        };
        if size < 0 {
            return Ok(None);
        }
        let Some(new_size) = size.checked_add(1) else {
            return Ok(None);
        };
        let Some(size_usize) = usize::try_from(size).ok() else {
            return Ok(None);
        };
        let Some(new_size_usize) = usize::try_from(new_size).ok() else {
            return Ok(None);
        };
        let Some(mut array) =
            self.read_intrinsic_instance_field(dex_file_index, array_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(array_descriptor) = value_class_descriptor(&array).map(str::to_owned) else {
            return Ok(None);
        };
        if !array_descriptor.starts_with("[L") {
            return Ok(None);
        }
        let array_key = map_identity_key(&array);
        let capacity = {
            let runtime = self.program.runtime.borrow();
            runtime.array_entries.get(&array_key).map(Vec::len)
        };
        let Some(capacity) = capacity else {
            return Ok(None);
        };
        if size_usize > capacity || new_size_usize > i32::MAX as usize {
            return Ok(None);
        }
        if new_size_usize > capacity {
            let Some(new_capacity) = grown_object_array_capacity(capacity, new_size_usize) else {
                return Ok(None);
            };
            let copied = {
                let runtime = self.program.runtime.borrow();
                let Some(values) = runtime.array_entries.get(&array_key) else {
                    return Ok(None);
                };
                let mut copied = values.clone();
                copied.resize(new_capacity, DexValue::Null);
                copied
            };
            let new_array = self.program.allocate_filled_array(array_descriptor, copied);
            let array_field = self.program.field_id(dex_file_index, array_field_index)?;
            if !self
                .program
                .write_stored_instance_field(&receiver, &array_field, new_array.clone())
            {
                return Ok(None);
            }
            array = new_array;
        }
        if self
            .program
            .write_array_value(&array, size, value)
            .is_none()
        {
            return Ok(None);
        }
        let Some(new_mod_count) = mod_count.checked_add(1) else {
            return Ok(None);
        };
        if !self.write_intrinsic_instance_int(
            dex_file_index,
            mod_count_field_index,
            &receiver,
            new_mod_count,
        )? {
            return Ok(None);
        }
        if !self.write_intrinsic_instance_int(
            dex_file_index,
            size_field_index,
            &receiver,
            new_size,
        )? {
            return Ok(None);
        }

        Ok(Some(DexInvocationResult::Value(DexValue::Int(1))))
    }

    // cite: androidx SlotTable.kt#insertSlots, moveSlotGapTo
    // cite: libcore System.java#arraycopy
    fn apply_slot_object_gap_ensure<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        count_arg: usize,
        anchor_arg: usize,
        slot_index_field_index: u32,
        slot_limit_field_index: u32,
        object_array_field_index: u32,
        object_gap_start_field_index: u32,
        object_gap_size_field_index: u32,
        object_anchor_field_index: u32,
        group_array_field_index: u32,
        group_gap_start_field_index: u32,
        group_gap_size_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        if !matches!(receiver, DexValue::Object(_) | DexValue::Reference(_)) {
            return Ok(None);
        }
        let Some(count) = invocation_int_argument(invocation, count_arg)? else {
            return Ok(None);
        };
        let Some(anchor) = invocation_int_argument(invocation, anchor_arg)? else {
            return Ok(None);
        };
        let Some(()) = self.ensure_slot_object_gap_for_receiver(
            dex_file_index,
            &receiver,
            count,
            anchor,
            slot_index_field_index,
            slot_limit_field_index,
            object_array_field_index,
            object_gap_start_field_index,
            object_gap_size_field_index,
            object_anchor_field_index,
            group_array_field_index,
            group_gap_start_field_index,
            group_gap_size_field_index,
        )?
        else {
            return Ok(None);
        };

        Ok(Some(DexInvocationResult::Void))
    }

    fn ensure_slot_object_gap_for_receiver(
        &self,
        dex_file_index: usize,
        receiver: &DexValue,
        count: i32,
        anchor: i32,
        slot_index_field_index: u32,
        slot_limit_field_index: u32,
        object_array_field_index: u32,
        object_gap_start_field_index: u32,
        object_gap_size_field_index: u32,
        object_anchor_field_index: u32,
        group_array_field_index: u32,
        group_gap_start_field_index: u32,
        group_gap_size_field_index: u32,
    ) -> Result<Option<()>> {
        if count <= 0 {
            return Ok(Some(()));
        }
        let Some(slot_index) =
            self.read_intrinsic_instance_int(dex_file_index, slot_index_field_index, receiver)?
        else {
            return Ok(None);
        };

        let Some(()) = self.apply_slot_object_gap_move(
            dex_file_index,
            receiver,
            slot_index,
            anchor,
            object_array_field_index,
            object_gap_start_field_index,
            object_gap_size_field_index,
            object_anchor_field_index,
            group_array_field_index,
            group_gap_start_field_index,
            group_gap_size_field_index,
        )?
        else {
            return Ok(None);
        };

        let Some(object_gap_start) = self.read_intrinsic_instance_int(
            dex_file_index,
            object_gap_start_field_index,
            receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(mut object_gap_size) = self.read_intrinsic_instance_int(
            dex_file_index,
            object_gap_size_field_index,
            receiver,
        )?
        else {
            return Ok(None);
        };
        if object_gap_start < 0 || object_gap_size < 0 {
            return Ok(None);
        }

        if object_gap_size < count {
            let Some(object_array) = self.read_intrinsic_instance_field(
                dex_file_index,
                object_array_field_index,
                receiver,
            )?
            else {
                return Ok(None);
            };
            let Some(array_descriptor) = value_class_descriptor(&object_array).map(str::to_owned)
            else {
                return Ok(None);
            };
            if !array_descriptor.starts_with("[L") {
                return Ok(None);
            }
            let array_key = map_identity_key(&object_array);
            let copied = {
                let runtime = self.program.runtime.borrow();
                let Some(values) = runtime.array_entries.get(&array_key) else {
                    return Ok(None);
                };
                let capacity = values.len();
                let gap_start = usize::try_from(object_gap_start).ok();
                let gap_size = usize::try_from(object_gap_size).ok();
                let count = usize::try_from(count).ok();
                let (Some(gap_start), Some(gap_size), Some(count)) = (gap_start, gap_size, count)
                else {
                    return Ok(None);
                };
                if gap_start > capacity || gap_size > capacity || gap_start + gap_size > capacity {
                    return Ok(None);
                }
                let live = capacity - gap_size;
                let Some(double_capacity) = capacity.checked_mul(2) else {
                    return Ok(None);
                };
                let Some(needed) = live.checked_add(count) else {
                    return Ok(None);
                };
                let new_capacity = double_capacity.max(needed).max(32);
                let new_gap_size = new_capacity - live;
                let tail_start = gap_start + gap_size;
                let tail_target = gap_start + new_gap_size;
                let mut copied = vec![DexValue::Null; new_capacity];
                for index in 0..gap_start {
                    copied[index] = values[index].clone();
                }
                for source in tail_start..capacity {
                    copied[tail_target + (source - tail_start)] = values[source].clone();
                }
                let Some(new_gap_size) = i32::try_from(new_gap_size).ok() else {
                    return Ok(None);
                };
                object_gap_size = new_gap_size;
                copied
            };
            let new_array = self.program.allocate_filled_array(array_descriptor, copied);
            let object_array_field = self
                .program
                .field_id(dex_file_index, object_array_field_index)?;
            if !self
                .program
                .write_stored_instance_field(receiver, &object_array_field, new_array)
            {
                return Ok(None);
            }
        }

        let Some(slot_limit) =
            self.read_intrinsic_instance_int(dex_file_index, slot_limit_field_index, receiver)?
        else {
            return Ok(None);
        };
        if slot_limit >= object_gap_start {
            let Some(new_slot_limit) = slot_limit.checked_add(count) else {
                return Ok(None);
            };
            if !self.write_intrinsic_instance_int(
                dex_file_index,
                slot_limit_field_index,
                receiver,
                new_slot_limit,
            )? {
                return Ok(None);
            }
        }
        let Some(new_gap_start) = object_gap_start.checked_add(count) else {
            return Ok(None);
        };
        let Some(new_gap_size) = object_gap_size.checked_sub(count) else {
            return Ok(None);
        };
        if !self.write_intrinsic_instance_int(
            dex_file_index,
            object_gap_start_field_index,
            receiver,
            new_gap_start,
        )? {
            return Ok(None);
        }
        if !self.write_intrinsic_instance_int(
            dex_file_index,
            object_gap_size_field_index,
            receiver,
            new_gap_size,
        )? {
            return Ok(None);
        }

        Ok(Some(()))
    }

    fn apply_slot_object_update<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        value_arg: usize,
        insert_count_field_index: u32,
        insert_anchor_field_index: u32,
        slot_index_field_index: u32,
        slot_limit_field_index: u32,
        object_array_field_index: u32,
        object_gap_start_field_index: u32,
        object_gap_size_field_index: u32,
        object_anchor_field_index: u32,
        group_array_field_index: u32,
        group_gap_start_field_index: u32,
        group_gap_size_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        if !matches!(receiver, DexValue::Object(_) | DexValue::Reference(_)) {
            return Ok(None);
        }
        let Some(value) = invocation.argument_value(value_arg)? else {
            return Ok(None);
        };
        let Some(insert_count) =
            self.read_intrinsic_instance_int(dex_file_index, insert_count_field_index, &receiver)?
        else {
            return Ok(None);
        };

        if insert_count > 0 {
            let Some(insert_anchor) = self.read_intrinsic_instance_int(
                dex_file_index,
                insert_anchor_field_index,
                &receiver,
            )?
            else {
                return Ok(None);
            };
            let Some(()) = self.ensure_slot_object_gap_for_receiver(
                dex_file_index,
                &receiver,
                1,
                insert_anchor,
                slot_index_field_index,
                slot_limit_field_index,
                object_array_field_index,
                object_gap_start_field_index,
                object_gap_size_field_index,
                object_anchor_field_index,
                group_array_field_index,
                group_gap_start_field_index,
                group_gap_size_field_index,
            )?
            else {
                return Ok(None);
            };
        }

        let Some(current_slot) =
            self.read_intrinsic_instance_int(dex_file_index, slot_index_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(slot_limit) =
            self.read_intrinsic_instance_int(dex_file_index, slot_limit_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(new_current_slot) = current_slot.checked_add(1) else {
            return Ok(None);
        };
        if current_slot < 0 || new_current_slot > slot_limit {
            return Ok(None);
        }
        let Some(object_gap_start) = self.read_intrinsic_instance_int(
            dex_file_index,
            object_gap_start_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(object_gap_size) = self.read_intrinsic_instance_int(
            dex_file_index,
            object_gap_size_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        if object_gap_start < 0 || object_gap_size < 0 {
            return Ok(None);
        }
        let Some(object_array) = self.read_intrinsic_instance_field(
            dex_file_index,
            object_array_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(object_descriptor) = value_class_descriptor(&object_array) else {
            return Ok(None);
        };
        if !object_descriptor.starts_with("[L") {
            return Ok(None);
        }
        let gap_offset = if current_slot < object_gap_start {
            0
        } else {
            object_gap_size
        };
        let Some(slot_address) = current_slot
            .checked_add(gap_offset)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return Ok(None);
        };
        let object_key = map_identity_key(&object_array);
        let old_value = {
            let runtime = self.program.runtime.borrow();
            let Some(values) = runtime.array_entries.get(&object_key) else {
                return Ok(None);
            };
            let Some(old_value) = values.get(slot_address) else {
                return Ok(None);
            };
            old_value.clone()
        };

        if !self.write_intrinsic_instance_int(
            dex_file_index,
            slot_index_field_index,
            &receiver,
            new_current_slot,
        )? {
            return Ok(None);
        }
        {
            let mut runtime = self.program.runtime.borrow_mut();
            let Some(values) = runtime.array_entries.get_mut(&object_key) else {
                return Ok(None);
            };
            let Some(slot) = values.get_mut(slot_address) else {
                return Ok(None);
            };
            *slot = value;
        }

        Ok(Some(DexInvocationResult::Value(old_value)))
    }

    fn apply_slot_object_gap_move(
        &self,
        dex_file_index: usize,
        receiver: &DexValue,
        gap_start: i32,
        anchor: i32,
        object_array_field_index: u32,
        object_gap_start_field_index: u32,
        object_gap_size_field_index: u32,
        object_anchor_field_index: u32,
        group_array_field_index: u32,
        group_gap_start_field_index: u32,
        group_gap_size_field_index: u32,
    ) -> Result<Option<()>> {
        let Some(gap_size) = self.read_intrinsic_instance_int(
            dex_file_index,
            object_gap_size_field_index,
            receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(old_gap_start) = self.read_intrinsic_instance_int(
            dex_file_index,
            object_gap_start_field_index,
            receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(old_anchor) =
            self.read_intrinsic_instance_int(dex_file_index, object_anchor_field_index, receiver)?
        else {
            return Ok(None);
        };
        let Some(group_gap_start) = self.read_intrinsic_instance_int(
            dex_file_index,
            group_gap_start_field_index,
            receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(group_gap_size) =
            self.read_intrinsic_instance_int(dex_file_index, group_gap_size_field_index, receiver)?
        else {
            return Ok(None);
        };
        if gap_start < 0
            || gap_size < 0
            || old_gap_start < 0
            || group_gap_start < 0
            || group_gap_size < 0
        {
            return Ok(None);
        }

        let Some(object_array) =
            self.read_intrinsic_instance_field(dex_file_index, object_array_field_index, receiver)?
        else {
            return Ok(None);
        };
        let Some(group_array) =
            self.read_intrinsic_instance_field(dex_file_index, group_array_field_index, receiver)?
        else {
            return Ok(None);
        };
        let Some(object_descriptor) = value_class_descriptor(&object_array) else {
            return Ok(None);
        };
        let Some(group_descriptor) = value_class_descriptor(&group_array) else {
            return Ok(None);
        };
        if !object_descriptor.starts_with("[L") || group_descriptor != "[I" {
            return Ok(None);
        }
        let object_key = map_identity_key(&object_array);
        let group_key = map_identity_key(&group_array);

        let (object_capacity, group_capacity) = {
            let runtime = self.program.runtime.borrow();
            let Some(object_values) = runtime.array_entries.get(&object_key) else {
                return Ok(None);
            };
            let Some(group_values) = runtime.array_entries.get(&group_key) else {
                return Ok(None);
            };
            (object_values.len(), group_values.len())
        };
        if group_capacity % 5 != 0 {
            return Ok(None);
        }
        let gap_start_usize = usize::try_from(gap_start).ok();
        let gap_size_usize = usize::try_from(gap_size).ok();
        let old_gap_start_usize = usize::try_from(old_gap_start).ok();
        let (Some(gap_start_usize), Some(gap_size_usize), Some(old_gap_start_usize)) =
            (gap_start_usize, gap_size_usize, old_gap_start_usize)
        else {
            return Ok(None);
        };
        if gap_start_usize > object_capacity
            || old_gap_start_usize > object_capacity
            || gap_size_usize > object_capacity
            || gap_start_usize + gap_size_usize > object_capacity
            || old_gap_start_usize + gap_size_usize > object_capacity
        {
            return Ok(None);
        }

        let live_groups = (group_capacity / 5).saturating_sub(group_gap_size as usize);
        let new_anchor = (anchor + 1).min(i32::try_from(live_groups).unwrap_or(i32::MAX));
        let object_live = object_capacity - gap_size_usize;

        let q_index = |index: i32| -> Option<usize> {
            let gap = if index < group_gap_start {
                0
            } else {
                group_gap_size
            };
            usize::try_from(index.checked_add(gap)?).ok()
        };
        let Some(group_gap_start_usize) = usize::try_from(group_gap_start).ok() else {
            return Ok(None);
        };
        let Some(group_gap_size_usize) = usize::try_from(group_gap_size).ok() else {
            return Ok(None);
        };
        let Some(object_live_i32) = i32::try_from(object_live).ok() else {
            return Ok(None);
        };

        let mut anchor_updates: Vec<(usize, i32)> = Vec::new();
        if old_anchor != new_anchor {
            if new_anchor < old_anchor {
                let Some(mut cursor) = q_index(new_anchor) else {
                    return Ok(None);
                };
                let Some(end) = q_index(old_anchor) else {
                    return Ok(None);
                };
                while cursor < end {
                    let Some(entry) = cursor.checked_mul(5).and_then(|entry| entry.checked_add(4))
                    else {
                        return Ok(None);
                    };
                    let Some(value) = self.read_int_array_entry(&group_key, entry) else {
                        return Ok(None);
                    };
                    if value < 0 {
                        return Ok(None);
                    }
                    let Some(updated) = object_live_i32
                        .checked_sub(value)
                        .and_then(|value| value.checked_add(1))
                        .and_then(|value| value.checked_neg())
                    else {
                        return Ok(None);
                    };
                    anchor_updates.push((entry, updated));
                    cursor += 1;
                    if cursor == group_gap_start_usize {
                        cursor += group_gap_size_usize;
                    }
                }
            } else {
                let Some(mut cursor) = q_index(old_anchor) else {
                    return Ok(None);
                };
                let Some(end) = q_index(new_anchor) else {
                    return Ok(None);
                };
                while cursor < end {
                    let Some(entry) = cursor.checked_mul(5).and_then(|entry| entry.checked_add(4))
                    else {
                        return Ok(None);
                    };
                    let Some(value) = self.read_int_array_entry(&group_key, entry) else {
                        return Ok(None);
                    };
                    if value >= 0 {
                        return Ok(None);
                    }
                    let Some(updated) = value
                        .checked_add(object_live_i32)
                        .and_then(|value| value.checked_add(1))
                    else {
                        return Ok(None);
                    };
                    anchor_updates.push((entry, updated));
                    cursor += 1;
                    if cursor == group_gap_start_usize {
                        cursor += group_gap_size_usize;
                    }
                }
            }
        }

        if gap_start_usize != old_gap_start_usize {
            let mut runtime = self.program.runtime.borrow_mut();
            let Some(values) = runtime.array_entries.get_mut(&object_key) else {
                return Ok(None);
            };
            if gap_start_usize < old_gap_start_usize {
                for source in (gap_start_usize..old_gap_start_usize).rev() {
                    values[source + gap_size_usize] = values[source].clone();
                }
            } else {
                let source_start = old_gap_start_usize + gap_size_usize;
                let count = gap_start_usize - old_gap_start_usize;
                for offset in 0..count {
                    values[old_gap_start_usize + offset] = values[source_start + offset].clone();
                }
            }
        }
        if !anchor_updates.is_empty() {
            let mut runtime = self.program.runtime.borrow_mut();
            let Some(values) = runtime.array_entries.get_mut(&group_key) else {
                return Ok(None);
            };
            for (entry, updated) in anchor_updates {
                if entry >= values.len() {
                    return Ok(None);
                }
                values[entry] = DexValue::Int(updated);
            }
        }
        if old_anchor != new_anchor
            && !self.write_intrinsic_instance_int(
                dex_file_index,
                object_anchor_field_index,
                receiver,
                new_anchor,
            )?
        {
            return Ok(None);
        }
        if !self.write_intrinsic_instance_int(
            dex_file_index,
            object_gap_start_field_index,
            receiver,
            gap_start,
        )? {
            return Ok(None);
        }

        Ok(Some(()))
    }

    fn apply_slot_group_data_index<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        groups_arg: usize,
        address_arg: usize,
        group_array_field_index: u32,
        object_array_field_index: u32,
        object_gap_size_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        if !matches!(receiver, DexValue::Object(_) | DexValue::Reference(_)) {
            return Ok(None);
        }
        let Some(groups) = invocation.argument_value(groups_arg)? else {
            return Ok(None);
        };
        let Some(address) = invocation_int_argument(invocation, address_arg)? else {
            return Ok(None);
        };
        if address < 0 {
            return Ok(None);
        }

        let Some(receiver_groups) =
            self.read_intrinsic_instance_field(dex_file_index, group_array_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(receiver_groups_descriptor) = value_class_descriptor(&receiver_groups) else {
            return Ok(None);
        };
        let Some(groups_descriptor) = value_class_descriptor(&groups) else {
            return Ok(None);
        };
        if receiver_groups_descriptor != "[I" || groups_descriptor != "[I" {
            return Ok(None);
        }
        let Some(object_array) = self.read_intrinsic_instance_field(
            dex_file_index,
            object_array_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(object_descriptor) = value_class_descriptor(&object_array) else {
            return Ok(None);
        };
        if !object_descriptor.starts_with("[L") {
            return Ok(None);
        }
        let Some(object_gap_size) = self.read_intrinsic_instance_int(
            dex_file_index,
            object_gap_size_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        if object_gap_size < 0 {
            return Ok(None);
        }

        let receiver_group_key = map_identity_key(&receiver_groups);
        let groups_key = map_identity_key(&groups);
        let object_key = map_identity_key(&object_array);
        let (capacity, used_slots) = {
            let runtime = self.program.runtime.borrow();
            let Some(receiver_group_values) = runtime.array_entries.get(&receiver_group_key) else {
                return Ok(None);
            };
            let Some(object_values) = runtime.array_entries.get(&object_key) else {
                return Ok(None);
            };
            let Some(object_gap_size) = usize::try_from(object_gap_size).ok() else {
                return Ok(None);
            };
            if receiver_group_values.len() % 5 != 0 || object_gap_size > object_values.len() {
                return Ok(None);
            }
            (
                receiver_group_values.len() / 5,
                object_values.len() - object_gap_size,
            )
        };
        let Some(address) = usize::try_from(address).ok() else {
            return Ok(None);
        };
        if address >= capacity {
            let Some(used_slots) = i32::try_from(used_slots).ok() else {
                return Ok(None);
            };
            return Ok(Some(DexInvocationResult::Value(DexValue::Int(used_slots))));
        }

        let Some(entry) = address
            .checked_mul(5)
            .and_then(|entry| entry.checked_add(4))
        else {
            return Ok(None);
        };
        let Some(anchor) = self.read_int_array_entry(&groups_key, entry) else {
            return Ok(None);
        };
        let index = if anchor < 0 {
            let Some(used_slots) = i32::try_from(used_slots).ok() else {
                return Ok(None);
            };
            let Some(index) = used_slots
                .checked_add(anchor)
                .and_then(|value| value.checked_add(1))
            else {
                return Ok(None);
            };
            index
        } else {
            anchor
        };

        Ok(Some(DexInvocationResult::Value(DexValue::Int(index))))
    }

    fn apply_slot_group_size<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        group_array_field_index: u32,
        group_gap_size_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        if !matches!(receiver, DexValue::Object(_) | DexValue::Reference(_)) {
            return Ok(None);
        }
        let Some(group_array) =
            self.read_intrinsic_instance_field(dex_file_index, group_array_field_index, &receiver)?
        else {
            return Ok(None);
        };
        let Some(group_descriptor) = value_class_descriptor(&group_array) else {
            return Ok(None);
        };
        if group_descriptor != "[I" {
            return Ok(None);
        }
        let Some(group_gap_size) = self.read_intrinsic_instance_int(
            dex_file_index,
            group_gap_size_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        if group_gap_size < 0 {
            return Ok(None);
        }
        let group_key = map_identity_key(&group_array);
        let capacity = {
            let runtime = self.program.runtime.borrow();
            let Some(values) = runtime.array_entries.get(&group_key) else {
                return Ok(None);
            };
            if values.len() % 5 != 0 {
                return Ok(None);
            }
            values.len() / 5
        };
        let Some(group_gap_size) = usize::try_from(group_gap_size).ok() else {
            return Ok(None);
        };
        if group_gap_size > capacity {
            return Ok(None);
        }
        let Some(size) = i32::try_from(capacity - group_gap_size).ok() else {
            return Ok(None);
        };

        Ok(Some(DexInvocationResult::Value(DexValue::Int(size))))
    }

    fn apply_slot_group_address<A: MethodIntrinsicArguments + ?Sized>(
        &self,
        dex_file_index: usize,
        invocation: &A,
        receiver_arg: usize,
        index_arg: usize,
        group_gap_start_field_index: u32,
        group_gap_size_field_index: u32,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver) = invocation.argument_value(receiver_arg)? else {
            return Ok(None);
        };
        if !matches!(receiver, DexValue::Object(_) | DexValue::Reference(_)) {
            return Ok(None);
        }
        let Some(index) = invocation_int_argument(invocation, index_arg)? else {
            return Ok(None);
        };
        let Some(group_gap_start) = self.read_intrinsic_instance_int(
            dex_file_index,
            group_gap_start_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        let Some(group_gap_size) = self.read_intrinsic_instance_int(
            dex_file_index,
            group_gap_size_field_index,
            &receiver,
        )?
        else {
            return Ok(None);
        };
        if group_gap_start < 0 || group_gap_size < 0 {
            return Ok(None);
        }
        let gap_offset = if index < group_gap_start {
            0
        } else {
            group_gap_size
        };
        let Some(address) = index.checked_add(gap_offset) else {
            return Ok(None);
        };

        Ok(Some(DexInvocationResult::Value(DexValue::Int(address))))
    }

    fn read_int_array_entry(&self, array_key: &str, index: usize) -> Option<i32> {
        let runtime = self.program.runtime.borrow();
        match runtime.array_entries.get(array_key)?.get(index)? {
            DexValue::Int(value) => Some(*value),
            _ => None,
        }
    }

    fn read_intrinsic_instance_field(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
    ) -> Result<Option<DexValue>> {
        let resolved = self
            .program
            .resolved_instance_field(dex_file_index, field_index)?;
        let (field, signature) = match &resolved {
            Some(resolved) => (Rc::clone(&resolved.field), Rc::clone(&resolved.signature)),
            None => self
                .program
                .field_id_and_signature(dex_file_index, field_index)?,
        };

        let stored_value = match &resolved {
            Some(resolved) => self
                .program
                .read_stored_instance_field_resolved(resolved, object)?,
            None => self.program.read_stored_instance_field_ic(
                dex_file_index,
                field_index,
                object,
                &signature,
            ),
        };
        if let Some(value) = stored_value {
            return Ok(Some(value));
        }

        if let Some(value) = self.program.read_bootstrap_instance_field(object, &field) {
            return Ok(Some(value));
        }

        Ok(resolved
            .as_ref()
            .and_then(|resolved| resolved.default_value.clone())
            .or_else(|| default_field_value(&field)))
    }

    fn read_intrinsic_instance_int(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
    ) -> Result<Option<i32>> {
        let Some(value) =
            self.read_intrinsic_instance_field(dex_file_index, field_index, object)?
        else {
            return Ok(None);
        };

        Ok(read_int_argument(&value))
    }

    fn read_intrinsic_instance_bool(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
    ) -> Result<Option<bool>> {
        let field = self.program.field_id(dex_file_index, field_index)?;
        if field.type_descriptor != "Z" {
            return Ok(None);
        }
        let Some(value) =
            self.read_intrinsic_instance_field(dex_file_index, field_index, object)?
        else {
            return Ok(None);
        };

        Ok(read_int_argument(&value).map(|value| value != 0))
    }

    fn write_intrinsic_instance_int(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
        value: i32,
    ) -> Result<bool> {
        let resolved = self
            .program
            .resolved_instance_field(dex_file_index, field_index)?;
        let (field, signature) = match &resolved {
            Some(resolved) => (Rc::clone(&resolved.field), Rc::clone(&resolved.signature)),
            None => self
                .program
                .field_id_and_signature(dex_file_index, field_index)?,
        };
        if field.type_descriptor != "I" {
            return Ok(false);
        }

        match &resolved {
            Some(resolved) => self.program.write_stored_instance_field_resolved(
                resolved,
                object,
                DexValue::Int(value),
            ),
            None => Ok(self.program.write_stored_instance_field_ic(
                dex_file_index,
                field_index,
                object,
                &signature,
                DexValue::Int(value),
            )),
        }
    }

    fn read_intrinsic_instance_float(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
    ) -> Result<Option<f32>> {
        let Some(value) =
            self.read_intrinsic_instance_field(dex_file_index, field_index, object)?
        else {
            return Ok(None);
        };

        Ok(read_float_argument(&value))
    }

    fn read_intrinsic_float_vector_value(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
        index: i32,
    ) -> Result<Option<f32>> {
        let Some(array) =
            self.read_intrinsic_instance_field(dex_file_index, field_index, object)?
        else {
            return Ok(None);
        };
        let Some(value) = self.program.read_array_value(&array, index) else {
            return Ok(None);
        };

        Ok(read_float_argument(&value))
    }

    fn read_intrinsic_static_float(
        &self,
        dex_file_index: usize,
        field_index: u32,
    ) -> Result<Option<f32>> {
        let field = self.program.field_id(dex_file_index, field_index)?;
        self.program
            .ensure_class_initialized_synchronously(&field.class_descriptor, self.depth)?;

        if let Some(value) = self.program.read_stored_static_field(&field) {
            return Ok(read_float_argument(&value));
        }

        if let Some(value) = bootstrap_static_field(&field) {
            return Ok(read_float_argument(&value));
        }

        Ok(default_field_value(&field)
            .as_ref()
            .and_then(read_float_argument))
    }

    fn read_intrinsic_float_array_prefix(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
        count: i32,
    ) -> Result<Option<Vec<f32>>> {
        let Some(array) =
            self.read_intrinsic_instance_field(dex_file_index, field_index, object)?
        else {
            return Ok(None);
        };
        let mut values = Vec::with_capacity(count as usize);
        for index in 0..count {
            let Some(value) = self.program.read_array_value(&array, index) else {
                return Ok(None);
            };
            let Some(value) = read_float_argument(&value) else {
                return Ok(None);
            };
            values.push(value);
        }

        Ok(Some(values))
    }

    fn compute_cubic_float_array_point(
        &self,
        dex_file_index: usize,
        field_index: u32,
        object: &DexValue,
        t: f32,
    ) -> Result<Option<(f32, f32)>> {
        let Some(values) =
            self.read_intrinsic_float_array_prefix(dex_file_index, field_index, object, 8)?
        else {
            return Ok(None);
        };
        let inverse = 1.0 - t;
        let inverse_cubed = inverse * inverse * inverse;
        let three_t = 3.0 * t;
        let first_mid = three_t * inverse * inverse;
        let second_mid = three_t * t * inverse;
        let t_cubed = t * t * t;
        let x = (values[6] * t_cubed)
            + (values[4] * second_mid)
            + (values[2] * first_mid)
            + (values[0] * inverse_cubed);
        let y = (values[7] * t_cubed)
            + (values[5] * second_mid)
            + (values[3] * first_mid)
            + (values[1] * inverse_cubed);

        Ok(Some((x, y)))
    }

    fn materialize_adapter_view(&self, adapter_view: &DexValue, adapter: &DexValue) -> Result<()> {
        let proto = |return_type: &str, parameters: &[&str]| {
            let shorty: String = std::iter::once(type_shorty(return_type))
                .chain(parameters.iter().map(|parameter| type_shorty(parameter)))
                .collect();
            dex::DexProto {
                shorty,
                return_type: return_type.to_owned(),
                parameters: parameters
                    .iter()
                    .map(|parameter| (*parameter).to_owned())
                    .collect(),
            }
        };

        let count = match self.program.dispatch_object_value_method(
            adapter,
            "getCount",
            proto("I", &[]),
            Vec::new(),
            "AdapterView row count",
            self.depth,
        )? {
            DexValue::Int(count) => count.max(0),
            _ => 0,
        };

        self.program.clear_view_group_children(adapter_view);
        let adapter_view_id = self.program.view_id_from_value(adapter_view);
        if let Some(adapter_view_id) = adapter_view_id {
            self.program.clear_adapter_item_clicks(adapter_view_id);
        }
        let mut captured_rows = Vec::new();
        let mut item_layout_id = None;
        for position in 0..count {
            let view = self.program.dispatch_object_value_method(
                adapter,
                "getView",
                proto(
                    "Landroid/view/View;",
                    &["I", "Landroid/view/View;", "Landroid/view/ViewGroup;"],
                ),
                vec![
                    DexValue::Int(position),
                    DexValue::Null,
                    adapter_view.clone(),
                ],
                "AdapterView row view",
                self.depth,
            )?;
            if dex_value_is_object_like(&view) {
                if let Some(layout_id) = self.program.row_view_layout_id(&view) {
                    item_layout_id.get_or_insert(layout_id);
                    let mut row = self.program.capture_adapter_row(&view, layout_id)?;
                    if let Some(adapter_view_id) = adapter_view_id {
                        row.click_id = self.program.register_adapter_item_click(
                            adapter_view,
                            adapter_view_id,
                            position,
                        );
                    }
                    captured_rows.push(row);
                }
                self.program.append_view_child(adapter_view, &view);
            }
        }

        match (adapter_view_id, item_layout_id) {
            (Some(adapter_view_id), Some(item_layout_id)) if !captured_rows.is_empty() => {
                self.program.store_adapter_rows(DexAdapterRows {
                    adapter_view_id,
                    item_layout_id,
                    rows: captured_rows,
                });
            }
            (Some(adapter_view_id), _) => self.program.clear_adapter_rows(adapter_view_id),
            _ => {}
        }

        Ok(())
    }

    fn invoke_receiver_dispatched_method(
        &self,
        method: &DexMethodId,
        invocation: &DexInvocation,
        records_menu_inflate: bool,
    ) -> Result<Option<DexInvocationResult>> {
        let Some(receiver_descriptor) = receiver_dispatch_descriptor(invocation) else {
            return Ok(None);
        };

        let method_key = method_cache_key(self.current_dex_file_index, invocation.method_index);
        let cached = object_receiver_dispatch_descriptor(invocation).and_then(|receiver| {
            self.program
                .caches
                .borrow()
                .virtual_resolved_methods
                .get(&method_key)
                .and_then(|by_receiver| by_receiver.get(receiver))
                .cloned()
        });
        let resolved = match cached {
            Some(resolved) => Some(resolved),
            None => {
                let resolved = self
                    .program
                    .resolve_exact_method_in_class_hierarchy(receiver_descriptor, method)?;
                if let (Some(cache_receiver), Some(resolved)) =
                    (object_receiver_dispatch_descriptor(invocation), &resolved)
                {
                    self.program
                        .caches
                        .borrow_mut()
                        .virtual_resolved_methods
                        .entry(method_key)
                        .or_default()
                        .insert(cache_receiver.to_owned(), resolved.clone());
                }
                resolved
            }
        };

        if let Some(resolved) = resolved {
            if let Some(result) = self.invoke_resolved_intrinsic(&resolved, invocation)? {
                if records_menu_inflate {
                    self.program.sync_inflated_menu_items(invocation)?;
                }
                return Ok(Some(result));
            }
            let result = self.program.execute_method(
                None,
                &resolved,
                Some(&invocation.arguments),
                self.depth + 1,
            )?;
            if records_menu_inflate {
                self.program.sync_inflated_menu_items(invocation)?;
            }
            return Ok(Some(invocation_result_from_execution(result)));
        }

        if invocation.kind != DexInvokeKind::Interface {
            return Ok(None);
        }

        if let Some(resolved) = self
            .program
            .resolve_interface_method_in_class_hierarchy(receiver_descriptor, method)?
        {
            if let Some(result) = self.invoke_resolved_intrinsic(&resolved, invocation)? {
                if records_menu_inflate {
                    self.program.sync_inflated_menu_items(invocation)?;
                }
                return Ok(Some(result));
            }
            let result = self.program.execute_method(
                None,
                &resolved,
                Some(&invocation.arguments),
                self.depth + 1,
            )?;
            if records_menu_inflate {
                self.program.sync_inflated_menu_items(invocation)?;
            }
            return Ok(Some(invocation_result_from_execution(result)));
        }

        Ok(None)
    }

    fn invoke_receiver_intrinsic_only(
        &self,
        dex_file_index: usize,
        method_index: u32,
        kind: DexInvokeKind,
        arguments: Vec<DexValue>,
    ) -> Result<Option<DexInvocationResult>> {
        let method = self.program.method_id(dex_file_index, method_index)?;
        let invocation = DexInvocation {
            kind,
            range: false,
            method_index,
            argument_registers: Default::default(),
            arguments: arguments.into_iter().collect(),
        };
        let Some(receiver_descriptor) = receiver_dispatch_descriptor(&invocation) else {
            return Ok(None);
        };

        let method_key = method_cache_key(dex_file_index, method_index);
        let cached = self
            .program
            .caches
            .borrow()
            .virtual_resolved_methods
            .get(&method_key)
            .and_then(|by_receiver| by_receiver.get(receiver_descriptor))
            .cloned();
        let resolved = match cached {
            Some(resolved) => Some(resolved),
            None => {
                let resolved = self
                    .program
                    .resolve_exact_method_in_class_hierarchy(receiver_descriptor, &method)?;
                if let Some(resolved) = &resolved {
                    self.program
                        .caches
                        .borrow_mut()
                        .virtual_resolved_methods
                        .entry(method_key)
                        .or_default()
                        .insert(receiver_descriptor.to_owned(), resolved.clone());
                }
                resolved
            }
        };

        if let Some(resolved) = resolved {
            return self.invoke_resolved_intrinsic(&resolved, &invocation);
        }

        if kind != DexInvokeKind::Interface {
            return Ok(None);
        }

        let Some(resolved) = self
            .program
            .resolve_interface_method_in_class_hierarchy(receiver_descriptor, &method)?
        else {
            return Ok(None);
        };

        self.invoke_resolved_intrinsic(&resolved, &invocation)
    }

    fn append_string_builder_object(&self, invocation: &DexInvocation) -> Result<DexValue> {
        if invocation.arguments.len() != 2 {
            return Err(WinedroidError::new(format!(
                "StringBuilder.append(Object) expected 2 arguments, got {}",
                invocation.arguments.len()
            )));
        }

        let builder = invocation.arguments[0].clone();
        let appended = self.string_value_of_object(&invocation.arguments[1])?;

        self.program
            .append_string_builder_text(builder, &appended)
            .ok_or_else(|| {
                WinedroidError::new(
                    "StringBuilder.append(Object) could not append to builder state",
                )
            })
    }

    fn string_value_of_object(&self, value: &DexValue) -> Result<String> {
        match value {
            DexValue::String(value) => return Ok(value.clone().to_string()),
            DexValue::Null | DexValue::Int(0) => return Ok("null".to_owned()),
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized | DexValue::Int(_) | DexValue::Long(_) | DexValue::Float(_) => {
                return Err(WinedroidError::new(format!(
                    "StringBuilder.append(Object) requires an object-like value\nargument: {value}"
                )));
            }
        }

        let to_string = DexMethodId {
            class_descriptor: "Ljava/lang/Object;".to_owned(),
            name: "toString".to_owned(),
            proto: dex::DexProto {
                shorty: "L".to_owned(),
                return_type: "Ljava/lang/String;".to_owned(),
                parameters: Vec::new(),
            },
        };
        let to_string_invocation = DexInvocation {
            kind: DexInvokeKind::Virtual,
            range: false,
            method_index: 0,
            argument_registers: Default::default(),
            arguments: smallvec::smallvec![value.clone()],
        };

        if let Some(result) =
            self.invoke_receiver_dispatched_method(&to_string, &to_string_invocation, false)?
        {
            return self.string_from_to_string_result(value, result);
        }

        self.program.object_value_to_string(value).ok_or_else(|| {
            WinedroidError::new(format!(
                "StringBuilder.append(Object) could not stringify object\nargument: {value}"
            ))
        })
    }

    fn string_from_to_string_result(
        &self,
        receiver: &DexValue,
        result: DexInvocationResult,
    ) -> Result<String> {
        match result {
            DexInvocationResult::RawValue(value) => {
                self.string_from_to_string_result(receiver, DexInvocationResult::Value(value.into_value()?))
            }
            DexInvocationResult::Value(DexValue::String(value)) => Ok(value.to_string()),
            DexInvocationResult::Value(DexValue::Null) => Ok("null".to_owned()),
            DexInvocationResult::Value(value) => self
                .program
                .char_sequence_text(&value)
                .or_else(|| self.program.object_value_to_string(&value))
                .ok_or_else(|| {
                    WinedroidError::new(format!(
                        "Object.toString returned a non-string value\nreceiver: {receiver}\nvalue: {value}"
                    ))
                }),
            DexInvocationResult::Void => Err(WinedroidError::new(format!(
                "Object.toString returned void\nreceiver: {receiver}"
            ))),
            DexInvocationResult::Throw(exception) => Err(WinedroidError::new(format!(
                "Object.toString threw while appending to StringBuilder\nreceiver: {receiver}\nexception: {exception}"
            ))),
            DexInvocationResult::Parked(execution) => {
                self.program.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "Object.toString parked while appending to StringBuilder\nreceiver: {receiver}"
                )))
            }
        }
    }

    // cite: libcore Thread.java#start
    fn start_thread(&self, invocation: &DexInvocation) -> Result<()> {
        if invocation.arguments.len() != 1 {
            return Err(WinedroidError::new(format!(
                "Thread.start expected receiver only, got {} arguments",
                invocation.arguments.len()
            )));
        }

        self.program.thread_start(invocation).ok_or_else(|| {
            WinedroidError::new("Thread.start requires an object-backed Thread receiver")
        })?;

        let thread = invocation.arguments[0].clone();
        self.program
            .runtime
            .borrow_mut()
            .post_main_thread_task(MainThreadTask::StartThread { thread });

        Ok(())
    }

    // cite: frameworks-base Activity.java#runOnUiThread
    fn run_on_ui_thread(&self, invocation: &DexInvocation) -> Result<()> {
        if invocation.arguments.len() != 2 {
            return Err(WinedroidError::new(format!(
                "Activity.runOnUiThread expected 2 arguments, got {}",
                invocation.arguments.len()
            )));
        }

        let runnable = invocation.arguments[1].clone();
        if !self.program.current_guest_thread_is_main() {
            self.program.runtime.borrow_mut().post_handler_runnable(
                MAIN_THREAD_HANDLER_KEY.to_owned(),
                runnable,
                0,
                false,
            );
            return Ok(());
        }

        self.run_runnable(runnable, "Activity.runOnUiThread")
    }

    fn post_handler_runnable(&self, invocation: &DexInvocation) -> Result<()> {
        if invocation.arguments.len() != 2 {
            return Err(WinedroidError::new(format!(
                "Handler.post expected 2 arguments, got {}",
                invocation.arguments.len()
            )));
        }

        self.program
            .post_handler_runnable(invocation)
            .ok_or_else(|| WinedroidError::new("Handler.post received invalid arguments"))?;

        Ok(())
    }

    // cite: frameworks-base Dialog.java#show, dispatchOnCreate, onStart
    // cite: frameworks-base WindowManagerGlobal.java#addView
    fn dispatch_dialog_lifecycle_method(
        &self,
        dialog: &DexValue,
        method_name: &str,
        proto_descriptor: &str,
    ) -> Result<()> {
        let Some(class_descriptor) = value_class_descriptor(dialog) else {
            return Ok(());
        };
        let (shorty, return_type, parameters) = match proto_descriptor {
            "()V" => ("V".to_owned(), "V".to_owned(), Vec::new()),
            "(Landroid/os/Bundle;)V" => (
                "VL".to_owned(),
                "V".to_owned(),
                vec!["Landroid/os/Bundle;".to_owned()],
            ),
            _ => return Ok(()),
        };
        let method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: method_name.to_owned(),
            proto: crate::dex::DexProto {
                shorty,
                return_type,
                parameters,
            },
        };
        let Some(resolved) = self
            .program
            .resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
        else {
            return Ok(());
        };

        let arguments: Vec<DexValue> = match proto_descriptor {
            "(Landroid/os/Bundle;)V" => vec![dialog.clone(), DexValue::Null],
            _ => vec![dialog.clone()],
        };
        let result =
            self.program
                .execute_method(None, &resolved, Some(&arguments), self.depth + 1)?;

        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Int(_)
            | DexExecutionResult::Value(_)
            | DexExecutionResult::Throw(_) => Err(WinedroidError::new(format!(
                "Dialog lifecycle method did not return void\nmethod: {}\nresult: {result}",
                resolved.method.display_signature()
            ))),
            DexExecutionResult::Parked(execution) => {
                self.program.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "Dialog lifecycle method did not return void\nmethod: {}\nresult: parked",
                    resolved.method.display_signature()
                )))
            }
        }
    }

    fn dialog_show_runtime_view(&self, invocation: &DexInvocation) -> Result<()> {
        // cite: frameworks-base Dialog.java#show
        let dialog = invocation.arguments[0].clone();
        let lifecycle = self
            .dispatch_dialog_lifecycle_method(&dialog, "onCreate", "(Landroid/os/Bundle;)V")
            .and_then(|()| self.dispatch_dialog_lifecycle_method(&dialog, "onStart", "()V"));
        if lifecycle.is_err() {
            self.program
                .dialog_show_with_overlay(invocation, true)
                .ok_or_else(|| WinedroidError::new("Dialog.show() received invalid arguments"))?;
            return Ok(());
        }

        let content_view = self.program.dialog_content_view(invocation);

        self.program
            .dialog_show_with_overlay(invocation, content_view.is_none())
            .ok_or_else(|| WinedroidError::new("Dialog.show() received invalid arguments"))?;

        let Some(content_view) = content_view else {
            return Ok(());
        };

        let was_attached = self.program.view_value_is_attached_to_window(&content_view);
        self.program.register_runtime_view_root(&content_view);

        if !was_attached && self.program.view_value_is_attached_to_window(&content_view) {
            self.program
                .dispatch_view_attached_to_window(&content_view, self.depth)?;
        }

        Ok(())
    }

    // cite: frameworks-base Dialog.java#dismiss, dismissDialog
    fn dialog_dismiss_runtime_view(&self, invocation: &DexInvocation) -> Result<()> {
        if invocation.arguments.is_empty() {
            return Err(WinedroidError::new(
                "Dialog.dismiss() expected a receiver argument",
            ));
        }
        let dialog = invocation.arguments[0].clone();

        self.dispatch_dialog_lifecycle_method(&dialog, "onStop", "()V")?;

        let content_view = self.program.dialog_content_view(invocation);
        self.program
            .dialog_dismiss(invocation, content_view.as_ref())
            .ok_or_else(|| WinedroidError::new("Dialog.dismiss() received invalid arguments"))?;

        if let Some(listener) = self.program.dialog_on_dismiss_listener(&dialog) {
            if dex_value_is_object_like(&listener) {
                let Some(class_descriptor) = value_class_descriptor(&listener) else {
                    return Ok(());
                };
                let method = DexMethodId {
                    class_descriptor: class_descriptor.to_owned(),
                    name: "onDismiss".to_owned(),
                    proto: crate::dex::DexProto {
                        shorty: "VL".to_owned(),
                        return_type: "V".to_owned(),
                        parameters: vec!["Landroid/content/DialogInterface;".to_owned()],
                    },
                };
                if let Some(resolved) = self
                    .program
                    .resolve_exact_method_in_class_hierarchy(class_descriptor, &method)?
                {
                    let result = self.program.execute_method(
                        None,
                        &resolved,
                        Some(&[listener, dialog]),
                        self.depth + 1,
                    )?;
                    match result {
                        DexExecutionResult::Void => {}
                        DexExecutionResult::Parked(execution) => {
                            self.program.discard_parked_execution(&execution);
                            return Err(WinedroidError::new(
                                "Dialog.OnDismissListener parked without a guest-thread scheduler",
                            ));
                        }
                        result => {
                            return Err(WinedroidError::new(format!(
                                "Dialog.OnDismissListener did not complete normally\nresult: {result}"
                            )));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn activity_set_content_view_runtime_view(&self, invocation: &DexInvocation) -> Result<()> {
        let Some(view) = invocation.arguments.get(1).cloned() else {
            return Err(WinedroidError::new(format!(
                "Activity.setContentView(View) expected a view argument, got {} arguments",
                invocation.arguments.len()
            )));
        };
        let was_attached = self.program.view_value_is_attached_to_window(&view);

        self.program
            .activity_set_content_view(invocation)
            .ok_or_else(|| {
                WinedroidError::new("Activity.setContentView(View) received invalid arguments")
            })?;

        if !was_attached && self.program.view_value_is_attached_to_window(&view) {
            self.program
                .dispatch_view_attached_to_window(&view, self.depth)?;
        }

        Ok(())
    }

    fn view_group_add_view_runtime_child(
        &self,
        method: &DexMethodId,
        invocation: &DexInvocation,
    ) -> Result<()> {
        let Some(child) = invocation.arguments.get(1).cloned() else {
            return Err(WinedroidError::new(format!(
                "ViewGroup.addView expected a child view argument, got {} arguments",
                invocation.arguments.len()
            )));
        };
        let was_attached = self.program.view_value_is_attached_to_window(&child);
        let stored = match method.proto.descriptor().as_str() {
            "(Landroid/view/View;)V" => self.program.view_group_add_view(invocation),
            "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V" => self
                .program
                .view_group_add_view_with_layout_params(invocation),
            "(Landroid/view/View;I)V" => {
                self.program.view_group_add_view_at_index(invocation, false)
            }
            "(Landroid/view/View;ILandroid/view/ViewGroup$LayoutParams;)V" => {
                self.program.view_group_add_view_at_index(invocation, true)
            }
            descriptor => {
                return Err(WinedroidError::new(format!(
                    "unsupported ViewGroup.addView descriptor in attach dispatch: {descriptor}"
                )));
            }
        };

        stored
            .ok_or_else(|| WinedroidError::new("ViewGroup.addView received invalid arguments"))?;

        if !was_attached && self.program.view_value_is_attached_to_window(&child) {
            self.program
                .dispatch_view_attached_to_window(&child, self.depth)?;
        }

        Ok(())
    }

    fn view_group_remove_all_views(&self, invocation: &DexInvocation) -> Result<()> {
        self.program
            .view_group_remove_all_views(invocation)
            .ok_or_else(|| {
                WinedroidError::new("ViewGroup.removeAllViews received invalid arguments")
            })
    }

    fn view_group_draw_child(&self, invocation: &DexInvocation) -> Result<i32> {
        if !matches!(invocation.arguments.len(), 4 | 5) {
            return Err(WinedroidError::new(format!(
                "ViewGroup.drawChild expected receiver, Canvas, child View, and drawing time; got {} arguments",
                invocation.arguments.len()
            )));
        }

        let canvas = &invocation.arguments[1];
        let child = &invocation.arguments[2];
        if !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_object_like(canvas)
            || !dex_value_is_object_like(child)
        {
            return Err(WinedroidError::new(format!(
                "ViewGroup.drawChild requires object-backed receiver, Canvas, and child View\nreceiver: {}\ncanvas: {}\nchild: {}",
                invocation.arguments[0], canvas, child
            )));
        }

        self.program
            .dispatch_runtime_view_draw(child, canvas, self.depth + 1)?;

        Ok(1)
    }

    fn run_runnable(&self, runnable: DexValue, context: &str) -> Result<()> {
        let Some(class_descriptor) = value_class_descriptor(&runnable) else {
            return Err(WinedroidError::new(format!(
                "{context} requires an object-backed Runnable\nargument: {runnable}"
            )));
        };
        let run_method = DexMethodId {
            class_descriptor: class_descriptor.to_owned(),
            name: "run".to_owned(),
            proto: crate::dex::DexProto {
                shorty: "V".to_owned(),
                return_type: "V".to_owned(),
                parameters: Vec::new(),
            },
        };
        let Some(resolved) = self
            .program
            .resolve_exact_method_in_class_hierarchy(class_descriptor, &run_method)?
        else {
            return Err(WinedroidError::new(format!(
                "{context} Runnable.run() was not found\nrunnable class: {class_descriptor}"
            )));
        };
        let result =
            self.program
                .execute_method(None, &resolved, Some(&[runnable]), self.depth + 1)?;

        match result {
            DexExecutionResult::Void => Ok(()),
            DexExecutionResult::Throw(exception) => Err(WinedroidError::new(format!(
                "{context} Runnable.run() threw uncaught {}",
                self.program.describe_uncaught_throw(&exception)
            ))),
            DexExecutionResult::Int(_) | DexExecutionResult::Value(_) => Err(WinedroidError::new(
                format!("{context} Runnable.run() did not complete normally"),
            )),
            DexExecutionResult::Parked(execution) => {
                self.program.discard_parked_execution(&execution);
                Err(WinedroidError::new(format!(
                    "{context} Runnable.run() did not complete normally"
                )))
            }
        }
    }
}

// cite: art interpreter_common.h#DoInvoke, FindMethodToCall
// cite: art class_linker-inl.h#ResolveMethod
fn invoke_uncached_code_from_frame(
    handler: &mut DexRunInvocationHandler<'_>,
    invocation: &DexInvocationFrame<'_>,
    method_key: MethodCacheKey,
) -> Result<Option<DexInvocationResult>> {
    let method = handler
        .program
        .method_id(handler.current_dex_file_index, invocation.method_index())?;

    if is_menu_inflater_inflate_method(&method) || records_content_view_method(&method) {
        return Ok(None);
    }
    if let Some(result) = invoke_frame_runtime_contract(handler, invocation, &method)? {
        return Ok(Some(result));
    }
    if frame_invocation_requires_materialized_runtime_contract(&method.class_descriptor) {
        return Ok(None);
    }

    match invocation.kind() {
        DexInvokeKind::Static | DexInvokeKind::Direct => {
            let Some(resolved) = handler.program.resolve_exact_method(&method)? else {
                return Ok(None);
            };
            invoke_resolved_code_from_frame(handler, invocation, method_key, None, resolved)
        }
        DexInvokeKind::Super => {
            let Some(resolved) = handler
                .program
                .resolve_exact_method_in_class_hierarchy(&method.class_descriptor, &method)?
            else {
                return Ok(None);
            };
            invoke_resolved_code_from_frame(handler, invocation, method_key, None, resolved)
        }
        DexInvokeKind::Virtual | DexInvokeKind::Interface => {
            let Some(receiver_descriptor) = invocation.argument_class_descriptor(0)? else {
                return Ok(None);
            };
            let resolved = handler
                .program
                .resolve_exact_method_in_class_hierarchy(receiver_descriptor, &method)?;
            let resolved = if resolved.is_some() || invocation.kind() != DexInvokeKind::Interface {
                resolved
            } else {
                handler
                    .program
                    .resolve_interface_method_in_class_hierarchy(receiver_descriptor, &method)?
            };
            let Some(resolved) = resolved else {
                return Ok(None);
            };
            invoke_resolved_code_from_frame(
                handler,
                invocation,
                method_key,
                Some(receiver_descriptor),
                resolved,
            )
        }
    }
}

fn invoke_resolved_code_from_frame(
    handler: &mut DexRunInvocationHandler<'_>,
    invocation: &DexInvocationFrame<'_>,
    method_key: MethodCacheKey,
    receiver_descriptor: Option<&str>,
    resolved: Rc<ResolvedMethod>,
) -> Result<Option<DexInvocationResult>> {
    if resolved.encoded_method.code_offset == 0 {
        return Ok(None);
    }

    let direct_kind = matches!(
        invocation.kind(),
        DexInvokeKind::Static | DexInvokeKind::Direct
    );
    let null_assert_arg = if direct_kind {
        handler
            .program
            .code_item(&resolved)
            .ok()
            .and_then(|code| detect_null_assert_arg(handler.program, &resolved, &code))
    } else {
        None
    };
    if let Some(arg_index) = null_assert_arg {
        if null_assert_shortcut_applies(invocation, &resolved, arg_index)? {
            cache_resolved_frame_code_method(
                handler,
                method_key,
                receiver_descriptor,
                &resolved,
                Some(arg_index),
                direct_kind,
            );
            return Ok(Some(DexInvocationResult::Void));
        }
    }

    if let Some(intrinsic) = handler.program.method_intrinsic(&resolved)? {
        if let Some(result) =
            handler.invoke_resolved_intrinsic_from_frame(&resolved, intrinsic, invocation)?
        {
            cache_resolved_frame_code_method(
                handler,
                method_key,
                receiver_descriptor,
                &resolved,
                None,
                direct_kind,
            );
            return Ok(Some(result));
        }
    }

    cache_resolved_frame_code_method(
        handler,
        method_key,
        receiver_descriptor,
        &resolved,
        null_assert_arg,
        direct_kind,
    );
    let result = handler.program.execute_method_from_invocation_frame(
        &resolved,
        invocation,
        handler.depth + 1,
    )?;
    Ok(Some(invocation_result_from_execution(result)))
}

fn cache_resolved_frame_code_method(
    handler: &mut DexRunInvocationHandler<'_>,
    method_key: MethodCacheKey,
    receiver_descriptor: Option<&str>,
    resolved: &Rc<ResolvedMethod>,
    null_assert_arg: Option<usize>,
    direct_kind: bool,
) {
    if direct_kind {
        let mut caches = handler.program.caches.borrow_mut();
        caches.direct_code_methods.insert(method_key);
        caches
            .direct_resolved_methods
            .insert(method_key, resolved.clone());
        caches.null_assert_args.insert(method_key, null_assert_arg);
        return;
    }

    if let Some(receiver_descriptor) = receiver_descriptor {
        let mut caches = handler.program.caches.borrow_mut();
        caches
            .virtual_resolved_methods
            .entry(method_key)
            .or_default()
            .insert(receiver_descriptor.to_owned(), resolved.clone());
        caches
            .virtual_code_methods
            .entry(method_key)
            .or_default()
            .insert(receiver_descriptor.to_owned());
    }
}

fn invoke_frame_runtime_contract(
    handler: &mut DexRunInvocationHandler<'_>,
    invocation: &DexInvocationFrame<'_>,
    method: &DexMethodId,
) -> Result<Option<DexInvocationResult>> {
    if method.class_descriptor == "Ljava/util/Random;" && method.name == "nextInt" {
        if method.proto.descriptor_is("()I") {
            // cite: libcore Random.java#nextInt, next
            let Some(value) = handler.program.random_next_unbounded_int(invocation)? else {
                return Ok(None);
            };
            return Ok(Some(DexInvocationResult::RawValue(
                DexPendingResult::from_value(DexValue::Int(value)),
            )));
        }
        if method.proto.descriptor_is("(I)I") {
            // cite: libcore Random.java#nextInt, next
            let Some(value) = handler.program.random_next_int(invocation)? else {
                return Ok(None);
            };
            return Ok(Some(DexInvocationResult::RawValue(
                DexPendingResult::from_value(DexValue::Int(value)),
            )));
        }
    }

    if method.class_descriptor == "Ljava/util/ArrayDeque;" {
        if method.name == "offer" && method.proto.descriptor_is("(Ljava/lang/Object;)Z") {
            // cite: libcore ArrayDeque.java#offer, offerLast, addLast
            let Some(()) = handler.program.array_deque_add_last(invocation)? else {
                return Ok(None);
            };
            return Ok(Some(DexInvocationResult::RawValue(
                DexPendingResult::from_value(DexValue::Int(1)),
            )));
        }
        if method.name == "offerLast" && method.proto.descriptor_is("(Ljava/lang/Object;)Z") {
            // cite: libcore ArrayDeque.java#offerLast, addLast
            let Some(()) = handler.program.array_deque_add_last(invocation)? else {
                return Ok(None);
            };
            return Ok(Some(DexInvocationResult::RawValue(
                DexPendingResult::from_value(DexValue::Int(1)),
            )));
        }
        if method.name == "add" && method.proto.descriptor_is("(Ljava/lang/Object;)Z") {
            // cite: libcore ArrayDeque.java#add, addLast
            let Some(()) = handler.program.array_deque_add_last(invocation)? else {
                return Ok(None);
            };
            return Ok(Some(DexInvocationResult::RawValue(
                DexPendingResult::from_value(DexValue::Int(1)),
            )));
        }
        if method.name == "addLast" && method.proto.descriptor_is("(Ljava/lang/Object;)V") {
            // cite: libcore ArrayDeque.java#addLast
            let Some(()) = handler.program.array_deque_add_last(invocation)? else {
                return Ok(None);
            };
            return Ok(Some(DexInvocationResult::Void));
        }
    }

    Ok(None)
}

fn frame_invocation_requires_materialized_runtime_contract(class_descriptor: &str) -> bool {
    class_descriptor.starts_with("Ljava/")
        || class_descriptor.starts_with("Ljavax/")
        || class_descriptor.starts_with("Landroid/")
        || class_descriptor.starts_with("Ldalvik/")
        || class_descriptor.starts_with("Lorg/xml/")
        || class_descriptor.starts_with("Lorg/w3c/")
        || frame_invocation_androidx_runtime_contract(class_descriptor)
}

fn frame_invocation_androidx_runtime_contract(class_descriptor: &str) -> bool {
    let Some(rest) = class_descriptor.strip_prefix("Landroidx/") else {
        return false;
    };

    rest.starts_with("fragment/")
        || rest.starts_with("appcompat/")
        || rest.starts_with("lifecycle/")
        || rest.starts_with("preference/")
        || rest.starts_with("recyclerview/")
        || rest.starts_with("work/")
}

impl DexInvocationHandler for DexRunInvocationHandler<'_> {
    fn current_class_descriptor(&self) -> Option<&str> {
        self.current_class_descriptor.as_deref()
    }

    fn invoke_from_frame(
        &mut self,
        invocation: DexInvocationFrame<'_>,
    ) -> Result<DexInvocationResult> {
        let method_index = invocation.method_index();
        let result = self.invoke_supported_from_frame(invocation);
        self.finish_invocation(method_index, result)
    }

    fn invoke(&mut self, invocation: DexInvocation) -> Result<DexInvocationResult> {
        let method_index = invocation.method_index;
        let result = self.invoke_supported(invocation);
        self.finish_invocation(method_index, result)
    }

    fn exception_matches(&mut self, exception: &DexValue, type_descriptor: &str) -> Result<bool> {
        Ok(self.program.value_matches_type(exception, type_descriptor))
    }

    fn allocate_generated_exception(
        &mut self,
        exception: DexGeneratedException,
    ) -> Result<DexValue> {
        Ok(self.program.allocate_generated_exception(exception))
    }

    fn read_instance_field(&mut self, access: DexFieldAccess) -> Result<DexValue> {
        let resolved = self
            .program
            .resolved_instance_field(self.current_dex_file_index, access.field_index)?;
        let signature = match &resolved {
            Some(resolved) => Rc::clone(&resolved.signature),
            None => self
                .program
                .field_signature_by_index(self.current_dex_file_index, access.field_index)?,
        };
        let trace_field = if cached_env_flag(&DEX_TRACE, "WINEDROID_DEX_TRACE") {
            Some(
                self.program
                    .field_id(self.current_dex_file_index, access.field_index)?,
            )
        } else {
            None
        };

        let stored_value = match &resolved {
            Some(resolved) => self
                .program
                .read_stored_instance_field_resolved(resolved, &access.object)?,
            None => self.program.read_stored_instance_field_ic(
                self.current_dex_file_index,
                access.field_index,
                &access.object,
                &signature,
            ),
        };
        if let Some(value) = stored_value {
            if let Some(field) = &trace_field {
                trace_instance_field_read(self.depth, &access, field, &value, "stored");
            }
            return Ok(value);
        }

        let field = match trace_field {
            Some(field) => field,
            None => self
                .program
                .field_id(self.current_dex_file_index, access.field_index)?,
        };

        if let Some(value) = self
            .program
            .read_bootstrap_instance_field(&access.object, &field)
        {
            trace_instance_field_read(self.depth, &access, &field, &value, "bootstrap");
            return Ok(value);
        }

        if let Some(value) = resolved
            .as_ref()
            .and_then(|resolved| resolved.default_value.clone())
            .or_else(|| default_field_value(&field))
        {
            trace_instance_field_read(self.depth, &access, &field, &value, "default");
            return Ok(value);
        }

        Err(WinedroidError::new(format!(
            "DEX instance field read not implemented\nfield: {}\ndestination register: v{}\nobject register: v{}\nnext missing primitive: object field storage",
            field.display_signature(),
            access.destination_register,
            access.object_register
        )))
    }

    fn write_instance_field(&mut self, write: DexFieldWrite) -> Result<()> {
        let resolved = self
            .program
            .resolved_instance_field(self.current_dex_file_index, write.field_index)?;
        let signature = match &resolved {
            Some(resolved) => Rc::clone(&resolved.signature),
            None => self
                .program
                .field_signature_by_index(self.current_dex_file_index, write.field_index)?,
        };
        let trace_field = if cached_env_flag(&DEX_TRACE, "WINEDROID_DEX_TRACE") {
            Some(
                self.program
                    .field_id(self.current_dex_file_index, write.field_index)?,
            )
        } else {
            None
        };

        if let Some(field) = &trace_field {
            trace_instance_field_write(self.depth, &write, field);
        }
        let wrote = match &resolved {
            Some(resolved) => self.program.write_stored_instance_field_resolved(
                resolved,
                &write.object,
                write.value.clone(),
            )?,
            None => self.program.write_stored_instance_field_ic(
                self.current_dex_file_index,
                write.field_index,
                &write.object,
                &signature,
                write.value.clone(),
            ),
        };
        if wrote {
            return Ok(());
        }

        let field = match trace_field {
            Some(field) => field,
            None => self
                .program
                .field_id(self.current_dex_file_index, write.field_index)?,
        };

        if bootstrap_instance_field_write(&field) {
            return Ok(());
        }

        Err(WinedroidError::new(format!(
            "DEX instance field write not implemented\nfield: {}\nvalue register: v{}\nobject register: v{} (holds {})\nnext missing primitive: object field storage",
            field.display_signature(),
            write.value_register,
            write.object_register,
            write.object
        )))
    }

    fn read_static_field(
        &mut self,
        access: DexStaticFieldAccess,
    ) -> Result<DexStaticFieldReadResult> {
        let resolved = self
            .program
            .resolved_static_field(self.current_dex_file_index, access.field_index)?;
        let field = match &resolved {
            Some(resolved) => Rc::clone(&resolved.field),
            None => self
                .program
                .field_id(self.current_dex_file_index, access.field_index)?,
        };

        // cite: art nterp.cc#NterpGetStaticField
        match self
            .program
            .ensure_class_initialized(&field.class_descriptor, self.depth)?
        {
            ClassInitializationResult::Complete => {}
            ClassInitializationResult::Throw(exception) => {
                return Ok(DexStaticFieldReadResult::Throw(exception));
            }
            ClassInitializationResult::Parked(execution) => {
                return Ok(DexStaticFieldReadResult::Parked(execution));
            }
        }

        let stored_value = match &resolved {
            Some(resolved) => self.program.read_stored_static_field_resolved(resolved),
            None => self.program.read_stored_static_field(&field),
        };
        if let Some(value) = stored_value {
            trace_static_field_read(self.depth, &access, &field, &value, "stored");
            return Ok(DexStaticFieldReadResult::Value(value));
        }

        if let Some(value) = bootstrap_static_field(&field) {
            trace_static_field_read(self.depth, &access, &field, &value, "bootstrap");
            return Ok(DexStaticFieldReadResult::Value(value));
        }

        if let Some(value) = resolved
            .as_ref()
            .and_then(|resolved| resolved.default_value.clone())
            .or_else(|| default_field_value(&field))
        {
            trace_static_field_read(self.depth, &access, &field, &value, "default");
            return Ok(DexStaticFieldReadResult::Value(value));
        }

        Err(WinedroidError::new(format!(
            "DEX static field read not implemented\nfield: {}\ndestination register: v{}\nnext missing primitive: static field storage",
            field.display_signature(),
            access.destination_register
        )))
    }

    fn write_static_field(
        &mut self,
        write: DexStaticFieldWrite,
    ) -> Result<DexStaticFieldWriteResult> {
        let resolved = self
            .program
            .resolved_static_field(self.current_dex_file_index, write.field_index)?;
        let field = match &resolved {
            Some(resolved) => Rc::clone(&resolved.field),
            None => self
                .program
                .field_id(self.current_dex_file_index, write.field_index)?,
        };

        // cite: art nterp.cc#NterpGetStaticField
        match self
            .program
            .ensure_class_initialized(&field.class_descriptor, self.depth)?
        {
            ClassInitializationResult::Complete => {}
            ClassInitializationResult::Throw(exception) => {
                return Ok(DexStaticFieldWriteResult::Throw(exception));
            }
            ClassInitializationResult::Parked(execution) => {
                return Ok(DexStaticFieldWriteResult::Parked(execution));
            }
        }
        let wrote = match &resolved {
            Some(resolved) => self
                .program
                .write_stored_static_field_resolved(resolved, write.value.clone()),
            None => {
                self.program
                    .write_stored_static_field(&field, write.value.clone());
                true
            }
        };
        if !wrote {
            return Err(WinedroidError::new(format!(
                "DEX static field write not implemented\nfield: {}\nvalue register: v{}\nnext missing primitive: static field storage",
                field.display_signature(),
                write.value_register
            )));
        }

        Ok(DexStaticFieldWriteResult::Complete)
    }

    fn load_string(&mut self, access: DexStringAccess) -> Result<DexValue> {
        let value = self
            .program
            .string_value(self.current_dex_file_index, access.string_index)?;

        Ok(DexValue::String(std::rc::Rc::from(value)))
    }

    fn load_class(&mut self, access: DexClassAccess) -> Result<DexValue> {
        let descriptor = self
            .program
            .type_descriptor_value(self.current_dex_file_index, access.type_index)?;

        Ok(DexValue::Reference(std::rc::Rc::from(format!(
            "Ljava/lang/Class;:{descriptor}"
        ))))
    }

    fn allocate_object(&mut self, instance: DexNewInstance) -> Result<DexValue> {
        let descriptor = self
            .program
            .type_descriptor_value(self.current_dex_file_index, instance.type_index)?;

        Ok(self.program.allocate_object(descriptor))
    }

    fn allocate_array(&mut self, array: DexNewArray) -> Result<DexValue> {
        let descriptor = self
            .program
            .type_descriptor_value(self.current_dex_file_index, array.type_index)?;
        let size = array.size;

        self.program
            .allocate_array(descriptor, size)
            .ok_or_else(|| {
                WinedroidError::new(format!(
                    "DEX new-array allocation failed\nsize register: v{}\nsize: {}",
                    array.size_register, size
                ))
            })
    }

    fn array_length(&mut self, length: DexArrayLength) -> Result<i32> {
        self.program.array_length(&length.array).ok_or_else(|| {
            WinedroidError::new(format!(
                "DEX array-length failed\narray register: v{}\narray: {}\nnext missing primitive: array length storage",
                length.array_register, length.array
            ))
        })
    }

    fn read_array(&mut self, read: DexArrayRead) -> Result<DexValue> {
        self.program
            .read_array_value(&read.array, read.index)
            .ok_or_else(|| {
                WinedroidError::new(format!(
                    "DEX array read failed\narray register: v{}\nindex register: v{}\nindex: {}\narray: {}\nnext missing primitive: array value storage",
                    read.array_register, read.index_register, read.index, read.array
                ))
            })
    }

    fn write_array(&mut self, write: DexArrayWrite) -> Result<()> {
        self.program
            .write_array_value(&write.array, write.index, write.value.clone())
            .ok_or_else(|| {
                WinedroidError::new(format!(
                    "DEX array write failed\narray register: v{}\nindex register: v{}\nvalue register: v{}\nindex: {}\narray: {}\nvalue: {}\nnext missing primitive: array value storage",
                    write.array_register,
                    write.index_register,
                    write.value_register,
                    write.index,
                    write.array,
                    write.value
                ))
            })
    }

    fn allocate_filled_array(&mut self, array: DexFilledNewArray) -> Result<DexValue> {
        let descriptor = self
            .program
            .type_descriptor_value(self.current_dex_file_index, array.type_index)?;

        Ok(self.program.allocate_filled_array(descriptor, array.values))
    }

    fn check_cast(&mut self, check: DexTypeCheck) -> Result<DexCheckCastResult> {
        let target_descriptor = self
            .program
            .type_descriptor_value(self.current_dex_file_index, check.type_index)?;
        // cite: art method_verifier.cc#CodeFlowVerifyInstruction
        if reference_descriptor(&target_descriptor) != Some(target_descriptor.as_str()) {
            return Err(WinedroidError::new(format!(
                "DEX check-cast target is not a reference type\ntype index: {}\ntarget: {}",
                check.type_index, target_descriptor
            )));
        }

        // cite: art interpreter_switch_impl-inl.h#CHECK_CAST
        if matches!(&check.value, DexValue::Null | DexValue::Int(0))
            || self
                .program
                .value_matches_type(&check.value, &target_descriptor)
        {
            return Ok(DexCheckCastResult::Success);
        }
        if !matches!(
            &check.value,
            DexValue::Object(_) | DexValue::Reference(_) | DexValue::String(_)
        ) {
            // cite: art method_verifier.cc#CodeFlowVerifyInstruction
            return Err(WinedroidError::new(format!(
                "DEX check-cast expected a reference in v{}; found {}",
                check.register, check.value
            )));
        }

        let source_descriptor = value_class_descriptor(&check.value).ok_or_else(|| {
            WinedroidError::new(format!(
                "DEX check-cast source type is unavailable\nregister: v{}\nvalue: {}",
                check.register, check.value
            ))
        })?;
        trace_check_cast_failure(self.depth, source_descriptor, &target_descriptor);
        let source_name = descriptor_to_pretty_name(source_descriptor).ok_or_else(|| {
            WinedroidError::new(format!(
                "DEX check-cast source descriptor is invalid\ndescriptor: {source_descriptor}"
            ))
        })?;
        let target_name = descriptor_to_pretty_name(&target_descriptor).ok_or_else(|| {
            WinedroidError::new(format!(
                "DEX check-cast target descriptor is invalid\ndescriptor: {target_descriptor}"
            ))
        })?;

        // cite: art common_throws.cc#ThrowClassCastException
        Ok(DexCheckCastResult::Throw(DexGeneratedException {
            class_descriptor: "Ljava/lang/ClassCastException;".to_owned(),
            detail_message: Some(format!("{source_name} cannot be cast to {target_name}")),
        }))
    }

    fn instance_of(&mut self, check: DexInstanceOf) -> Result<i32> {
        let target_descriptor = self
            .program
            .type_descriptor_value(self.current_dex_file_index, check.type_index)?;

        Ok(i32::from(
            self.program
                .value_matches_type(&check.value, &target_descriptor),
        ))
    }
}

fn invocation_result_from_execution(result: DexExecutionResult) -> DexInvocationResult {
    match result {
        DexExecutionResult::Void => DexInvocationResult::Void,
        DexExecutionResult::Int(value) => {
            DexInvocationResult::RawValue(DexPendingResult::from_value(DexValue::Int(value)))
        }
        DexExecutionResult::Value(value) => {
            DexInvocationResult::RawValue(DexPendingResult::from_value(value))
        }
        DexExecutionResult::Throw(value) => DexInvocationResult::Throw(value),
        DexExecutionResult::Parked(execution) => DexInvocationResult::Parked(execution),
    }
}

fn tree_set_add_value_method(
    program: &DexProgram,
    method: &DexMethodId,
    invocation: &DexInvocation,
) -> bool {
    if method.name != "add" || !method.proto.descriptor_is("(Ljava/lang/Object;)Z") {
        return false;
    }

    invocation
        .arguments
        .first()
        .and_then(value_class_descriptor)
        .map(|descriptor| program.descriptor_matches_type(descriptor, "Ljava/util/TreeSet;"))
        .unwrap_or(false)
}

trait NullAssertResolver {
    fn resolve_null_assert_type(&self, dex_file_index: usize, type_index: u32) -> Option<String>;
    fn resolve_null_assert_method(
        &self,
        dex_file_index: usize,
        method_index: u32,
    ) -> Option<Rc<DexMethodId>>;
    fn null_assert_descriptor_matches(&self, candidate: &str, target: &str) -> bool;
}

impl NullAssertResolver for DexProgram {
    fn resolve_null_assert_type(&self, dex_file_index: usize, type_index: u32) -> Option<String> {
        self.type_descriptor_value(dex_file_index, type_index).ok()
    }

    fn resolve_null_assert_method(
        &self,
        dex_file_index: usize,
        method_index: u32,
    ) -> Option<Rc<DexMethodId>> {
        self.method_id(dex_file_index, method_index).ok()
    }

    fn null_assert_descriptor_matches(&self, candidate: &str, target: &str) -> bool {
        self.descriptor_matches_type(candidate, target)
    }
}

// cite: art method_verifier.cc#SetTypesFromSignature, CheckBranchTarget, CodeFlowVerifyInstruction, VerifyInvocationArgsFromIterator
// cite: art dex_instruction_list.h#IF_EQZ, RETURN_VOID, NEW_INSTANCE, INVOKE_DIRECT, INVOKE_DIRECT_RANGE, THROW
fn detect_null_assert_arg<R: NullAssertResolver + ?Sized>(
    resolver: &R,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Option<usize> {
    if resolved.encoded_method.access_flags & DEX_ACC_STATIC == 0
        || resolved.method.proto.return_type != "V"
        || code.tries_size != 0
        || !code.try_blocks.is_empty()
    {
        return None;
    }

    let instructions = &code.instructions;
    if instructions.len() < 9
        || instructions[0] & 0x00ff != 0x0038
        || instructions[1] as i16 != 3
        || instructions[2] != 0x000e
        || instructions[3] & 0x00ff != 0x0022
    {
        return None;
    }

    let guarded_register = instructions[0] >> 8;
    let argument_index = null_assert_reference_argument_index(resolved, code, guarded_register)?;
    let exception_register = instructions[3] >> 8;
    if exception_register >= code.registers_size {
        return None;
    }

    let exception_type =
        resolver.resolve_null_assert_type(resolved.dex_file_index, u32::from(instructions[4]))?;
    if !resolver.null_assert_descriptor_matches(&exception_type, "Ljava/lang/Throwable;") {
        return None;
    }

    let mut constructor_seen = false;
    let mut previous_was_invoke = false;
    let mut pc = 5;
    while pc < instructions.len() {
        let instruction = instructions[pc];
        let opcode = (instruction & 0x00ff) as u8;
        if opcode == 0x27 {
            if pc + 1 != instructions.len()
                || instruction >> 8 != exception_register
                || !constructor_seen
            {
                return None;
            }
            return Some(argument_index);
        }

        let width = null_assert_tail_instruction_width(opcode)?;
        pc.checked_add(width)
            .filter(|end| *end <= instructions.len())?;

        if matches!(opcode, 0x0a..=0x0c) && !previous_was_invoke {
            return None;
        }

        if null_assert_tail_overwrites_register(
            instructions,
            pc,
            opcode,
            code.registers_size,
            exception_register,
        )? {
            return None;
        }

        if let Some(source_register) = null_assert_tail_source_register(instructions, pc, opcode)? {
            if source_register >= code.registers_size
                || (!constructor_seen && source_register == exception_register)
            {
                return None;
            }
        }

        let invocation = if null_assert_tail_is_invoke(opcode) {
            Some(null_assert_invocation(instructions, pc, opcode)?)
        } else {
            None
        };
        if let Some((method_index, argument_registers)) = invocation.as_ref() {
            if argument_registers.len() > usize::from(code.outs_size)
                || argument_registers
                    .iter()
                    .any(|register| *register >= code.registers_size)
            {
                return None;
            }

            let first_argument = argument_registers.first().copied();
            if matches!(opcode, 0x70 | 0x76) && first_argument == Some(exception_register) {
                if constructor_seen {
                    return None;
                }
                let constructor =
                    resolver.resolve_null_assert_method(resolved.dex_file_index, *method_index)?;
                let constructor_words = method_parameter_word_count(&constructor.proto.parameters)?;
                if constructor.name != "<init>"
                    || constructor.proto.return_type != "V"
                    || argument_registers.len() != constructor_words.checked_add(1)?
                    || argument_registers
                        .iter()
                        .skip(1)
                        .any(|register| *register == exception_register)
                    || !resolver.null_assert_descriptor_matches(
                        &exception_type,
                        &constructor.class_descriptor,
                    )
                {
                    return None;
                }
                constructor_seen = true;
            } else if !constructor_seen
                && argument_registers
                    .iter()
                    .any(|register| *register == exception_register)
            {
                return None;
            }
        }

        previous_was_invoke = invocation.is_some();
        pc += width;
    }

    None
}

fn null_assert_reference_argument_index(
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
    register: u16,
) -> Option<usize> {
    let parameter_words = method_parameter_word_count(&resolved.method.proto.parameters)?;
    if parameter_words != usize::from(code.ins_size) {
        return None;
    }

    let first_parameter = code.registers_size.checked_sub(code.ins_size)?;
    let word_index = usize::from(register.checked_sub(first_parameter)?);
    let mut parameter_start = 0;
    for descriptor in &resolved.method.proto.parameters {
        let width = method_parameter_word_width(descriptor)?;
        if word_index == parameter_start {
            return valid_reference_parameter_descriptor(descriptor).then_some(word_index);
        }
        if word_index < parameter_start.checked_add(width)? {
            return None;
        }
        parameter_start = parameter_start.checked_add(width)?;
    }

    None
}

fn method_parameter_word_count(parameters: &[String]) -> Option<usize> {
    parameters.iter().try_fold(0usize, |words, descriptor| {
        words.checked_add(method_parameter_word_width(descriptor)?)
    })
}

fn method_parameter_word_width(descriptor: &str) -> Option<usize> {
    match descriptor.as_bytes().first().copied()? {
        b'J' | b'D' if descriptor.len() == 1 => Some(2),
        b'Z' | b'B' | b'S' | b'C' | b'I' | b'F' if descriptor.len() == 1 => Some(1),
        b'L' | b'[' if valid_reference_parameter_descriptor(descriptor) => Some(1),
        _ => None,
    }
}

fn valid_reference_parameter_descriptor(descriptor: &str) -> bool {
    if descriptor.starts_with('L') {
        return descriptor.len() > 2 && descriptor.ends_with(';');
    }

    let component = descriptor.trim_start_matches('[');
    if component.len() == descriptor.len() || component.is_empty() {
        return false;
    }
    if component.starts_with('L') {
        return component.len() > 2 && component.ends_with(';');
    }
    component.len() == 1
        && matches!(
            component.as_bytes()[0],
            b'Z' | b'B' | b'S' | b'C' | b'I' | b'J' | b'F' | b'D'
        )
}

// cite: art dex_instruction.h#SizeInCodeUnits
// cite: art dex_instruction_list.h#MOVE_OBJECT, MOVE_OBJECT_FROM16, MOVE_OBJECT_16, MOVE_RESULT, MOVE_RESULT_WIDE, MOVE_RESULT_OBJECT, CONST_STRING, CONST_STRING_JUMBO, CONST_CLASS, NEW_INSTANCE, IGET_OBJECT, SGET_OBJECT, INVOKE_VIRTUAL, INVOKE_DIRECT, INVOKE_STATIC, INVOKE_INTERFACE, INVOKE_VIRTUAL_RANGE, INVOKE_DIRECT_RANGE, INVOKE_STATIC_RANGE, INVOKE_INTERFACE_RANGE
fn null_assert_tail_instruction_width(opcode: u8) -> Option<usize> {
    match opcode {
        0x07 | 0x0a..=0x0c => Some(1),
        0x08 | 0x1a | 0x1c | 0x22 | 0x54 | 0x62 => Some(2),
        0x09 | 0x1b | 0x6e | 0x70..=0x72 | 0x74 | 0x76..=0x78 => Some(3),
        _ => None,
    }
}

// cite: art dex_instruction.h#VRegA
fn null_assert_tail_destination_register(
    instructions: &[u16],
    pc: usize,
    opcode: u8,
) -> Option<u16> {
    let instruction = *instructions.get(pc)?;
    match opcode {
        0x07 => Some((instruction >> 8) & 0x000f),
        0x08 | 0x0a..=0x0c | 0x1a..=0x1c | 0x22 | 0x62 => Some(instruction >> 8),
        0x09 => instructions.get(pc + 1).copied(),
        0x54 => Some((instruction >> 8) & 0x000f),
        _ => None,
    }
}

fn null_assert_tail_overwrites_register(
    instructions: &[u16],
    pc: usize,
    opcode: u8,
    register_count: u16,
    protected_register: u16,
) -> Option<bool> {
    let destination = null_assert_tail_destination_register(instructions, pc, opcode);
    let Some(destination) = destination else {
        return Some(false);
    };
    let destination_width = if opcode == 0x0b { 2 } else { 1 };
    let end = destination.checked_add(destination_width)?;
    if end > register_count {
        return None;
    }

    Some(destination <= protected_register && protected_register < end)
}

// cite: art dex_instruction.h#VRegB
fn null_assert_tail_source_register(
    instructions: &[u16],
    pc: usize,
    opcode: u8,
) -> Option<Option<u16>> {
    let instruction = *instructions.get(pc)?;
    match opcode {
        0x07 => Some(Some((instruction >> 12) & 0x000f)),
        0x08 => Some(Some(*instructions.get(pc + 1)?)),
        0x09 => Some(Some(*instructions.get(pc + 2)?)),
        0x54 => Some(Some((instruction >> 12) & 0x000f)),
        _ => Some(None),
    }
}

fn null_assert_tail_is_invoke(opcode: u8) -> bool {
    matches!(opcode, 0x6e | 0x70..=0x72 | 0x74 | 0x76..=0x78)
}

// cite: art dex_instruction.h#VRegA, VRegB, VRegC, GetVarArgs
// cite: art method_verifier.cc#VerifyInvocationArgsFromIterator
fn null_assert_invocation(
    instructions: &[u16],
    pc: usize,
    opcode: u8,
) -> Option<(u32, DexInvocationRegisters)> {
    let instruction = *instructions.get(pc)?;
    let method_index = u32::from(*instructions.get(pc + 1)?);
    let register_word = *instructions.get(pc + 2)?;
    let non_static = !matches!(opcode, 0x71 | 0x77);
    let mut argument_registers = DexInvocationRegisters::new();
    match opcode {
        0x6e | 0x70..=0x72 => {
            let argument_count = usize::from((instruction >> 12) & 0x000f);
            if argument_count > 5 || (non_static && argument_count == 0) {
                return None;
            }
            let candidates = [
                register_word & 0x000f,
                (register_word >> 4) & 0x000f,
                (register_word >> 8) & 0x000f,
                (register_word >> 12) & 0x000f,
                (instruction >> 8) & 0x000f,
            ];
            argument_registers.extend(candidates.into_iter().take(argument_count));
        }
        0x74 | 0x76..=0x78 => {
            let argument_count = usize::from(instruction >> 8);
            if non_static && argument_count == 0 {
                return None;
            }
            for offset in 0..argument_count {
                argument_registers.push(register_word.checked_add(u16::try_from(offset).ok()?)?);
            }
        }
        _ => return None,
    }

    Some((method_index, argument_registers))
}

fn method_intrinsic_key(resolved: &ResolvedMethod) -> MethodCacheKey {
    method_cache_key(resolved.dex_file_index, resolved.encoded_method.code_offset)
}

fn requested_compiled_runtime_flag() -> Option<&'static str> {
    if std::env::var_os("WD_NO_JIT").is_some() {
        return None;
    }
    [
        "WINEDROID_DEX3OAT_GRAPH",
        "WINEDROID_DEX3OAT_JIT",
        "WD_CGDIFF",
    ]
    .into_iter()
    .find(|name| std::env::var_os(name).is_some())
}

#[cfg(all(feature = "jit", not(target_family = "wasm")))]
fn jit_dispatch_enabled() -> bool {
    static JIT_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *JIT_ENABLED.get_or_init(|| requested_compiled_runtime_flag().is_some())
}

fn record_method_count_histogram(resolved: &ResolvedMethod) {
    if !method_count_histogram_enabled() {
        return;
    }

    thread_local! {
        static HIST: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
        static TOTAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }
    let sig = format!(
        "{}->{}{}",
        resolved.method.class_descriptor,
        resolved.method.name,
        resolved.method.proto.descriptor()
    );
    HIST.with(|hist| {
        *hist.borrow_mut().entry(sig).or_default() += 1;
    });
    let n = TOTAL.with(|total| {
        let n = total.get() + 1;
        total.set(n);
        n
    });
    if n % 250_000 != 0 {
        return;
    }

    HIST.with(|hist| {
        let hist = hist.borrow();
        let mut rows: Vec<_> = hist.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        eprintln!(
            "=== TOP METHODS BY CALL COUNT (n={n}, distinct={}) ===",
            hist.len()
        );
        for (sig, count) in rows.iter().take(30) {
            eprintln!("{count:>8}  {sig}");
        }
    });
}

fn method_count_histogram_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("WD_METHOD_HISTOGRAM").is_some())
}

fn method_wall_histogram_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("WD_METHOD_TIME_HISTOGRAM").is_some())
}

fn method_execution_observers_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        trace::enabled("perf")
            || method_count_histogram_enabled()
            || method_wall_histogram_enabled()
            || cached_env_flag(&DEX_TRACE_RETURNS, "WINEDROID_DEX_TRACE_RETURNS")
    })
}

#[derive(Clone, Copy, Default)]
struct MethodTimeStat {
    count: u64,
    inclusive: Duration,
    self_time: Duration,
}

struct MethodTimeFrame {
    signature: String,
    start: Instant,
    child_elapsed: Duration,
}

struct MethodTimeScope {
    active: bool,
}

thread_local! {
    static METHOD_TIME_STACK: RefCell<Vec<MethodTimeFrame>> = const { RefCell::new(Vec::new()) };
    static METHOD_TIME_HIST: RefCell<HashMap<String, MethodTimeStat>> =
        RefCell::new(HashMap::new());
    static METHOD_TIME_TOTAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl MethodTimeScope {
    fn enter(resolved: &ResolvedMethod) -> Self {
        let signature = format!(
            "{}->{}{}",
            resolved.method.class_descriptor,
            resolved.method.name,
            resolved.method.proto.descriptor()
        );
        METHOD_TIME_STACK.with(|stack| {
            stack.borrow_mut().push(MethodTimeFrame {
                signature,
                start: Instant::now(),
                child_elapsed: Duration::ZERO,
            });
        });

        Self { active: true }
    }
}

impl Drop for MethodTimeScope {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        let Some(frame) = METHOD_TIME_STACK.with(|stack| stack.borrow_mut().pop()) else {
            return;
        };
        let inclusive = frame.start.elapsed();
        let self_time = inclusive.saturating_sub(frame.child_elapsed);
        METHOD_TIME_STACK.with(|stack| {
            if let Some(parent) = stack.borrow_mut().last_mut() {
                parent.child_elapsed = parent.child_elapsed.saturating_add(inclusive);
            }
        });
        METHOD_TIME_HIST.with(|hist| {
            let mut hist = hist.borrow_mut();
            let stat = hist.entry(frame.signature).or_default();
            stat.count = stat.count.saturating_add(1);
            stat.inclusive = stat.inclusive.saturating_add(inclusive);
            stat.self_time = stat.self_time.saturating_add(self_time);
        });
        let n = METHOD_TIME_TOTAL.with(|total| {
            let n = total.get() + 1;
            total.set(n);
            n
        });
        if n % 1000 == 0 {
            METHOD_TIME_HIST.with(|hist| {
                let hist = hist.borrow();
                let mut rows: Vec<_> = hist.iter().collect();
                rows.sort_by(|a, b| {
                    (b.1)
                        .self_time
                        .cmp(&(a.1).self_time)
                        .then_with(|| (b.1).inclusive.cmp(&(a.1).inclusive))
                        .then_with(|| (b.1).count.cmp(&(a.1).count))
                });
                eprintln!(
                    "=== TOP METHODS BY SELF TIME (n={n}, distinct={}) ===",
                    hist.len()
                );
                for (sig, stat) in rows.iter().take(30) {
                    eprintln!(
                        "{:>8} self={:>8.2}ms incl={:>8.2}ms  {}",
                        stat.count,
                        stat.self_time.as_secs_f64() * 1000.0,
                        stat.inclusive.as_secs_f64() * 1000.0,
                        sig
                    );
                }
            });
        }
    }
}

fn detect_method_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    if resolved
        .method
        .proto
        .descriptor_is("(Ljava/lang/Object;Ljava/lang/Object;)Z")
    {
        if let Some(intrinsic) =
            detect_object_equals_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(II)V") {
        if let Some((array_field_index, size_field_index)) =
            detect_object_array_gap_insert_shape(code)
        {
            return Ok(Some(MethodIntrinsic::ObjectArrayGapInsert {
                receiver_arg: 0,
                index_arg: 1,
                count_arg: 2,
                array_field_index,
                size_field_index,
            }));
        }
        if let Some(intrinsic) = detect_slot_object_gap_ensure_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(Ljava/lang/Object;)Z") {
        if let Some(intrinsic) = detect_object_array_append_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved
        .method
        .proto
        .descriptor_is("(Ljava/util/AbstractList;FF)V")
    {
        if let Some(intrinsic) =
            detect_rounded_polygon_constructor_intrinsic(program, resolved, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.parameters.len() == 3
        && resolved.method.proto.parameters[0].starts_with('L')
        && resolved.method.proto.parameters[1] == "[F"
        && resolved.method.proto.parameters[2] == "I"
        && resolved.method.proto.return_type == "[F"
    {
        if let Some(intrinsic) = detect_rounded_polygon_bounds_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved
        .method
        .proto
        .descriptor_is("(Ljava/lang/Object;)Ljava/lang/Object;")
    {
        if let Some(intrinsic) = detect_slot_object_update_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("()I") {
        if let Some(intrinsic) = detect_slot_group_size_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(I)I") {
        if let Some(intrinsic) = detect_slot_group_address_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("([II)I") {
        if let Some(intrinsic) = detect_slot_group_data_index_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.parameters.len() == 1
        && resolved.method.proto.parameters[0].starts_with('L')
        && resolved.method.proto.return_type.starts_with('L')
    {
        if let Some(intrinsic) = detect_cubic_transform_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
        if let Some(intrinsic) = detect_feature_edge_transform_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
        if let Some(intrinsic) = detect_feature_corner_transform_intrinsic(program, resolved, code)?
        {
            return Ok(Some(intrinsic));
        }
        if let Some(intrinsic) =
            detect_rounded_polygon_transform_intrinsic(program, resolved, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.parameters.len() == 2
        && resolved
            .method
            .proto
            .parameters
            .iter()
            .all(|parameter| parameter.starts_with('L'))
        && resolved.method.proto.return_type.starts_with('L')
    {
        if let Some(intrinsic) = detect_measured_polygon_measure_intrinsic(program, resolved, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.parameters.len() == 2
        && resolved
            .method
            .proto
            .parameters
            .iter()
            .all(|parameter| parameter.starts_with('L'))
        && resolved.method.proto.return_type == "F"
    {
        if let Some(intrinsic) = detect_feature_distance_squared_intrinsic(program, resolved, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(FF)J") {
        if let Some(intrinsic) =
            detect_float_pack_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
        if let Some(intrinsic) =
            detect_affine_float_pack_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
        if let Some(intrinsic) =
            detect_matrix_point_transform_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(J)F") {
        if let Some(intrinsic) =
            detect_long_high_float_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
        if let Some(intrinsic) =
            detect_long_low_float_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.parameters.is_empty()
        && resolved.method.proto.return_type.starts_with('L')
    {
        if let Some(intrinsic) =
            detect_rounded_polygon_normalized_intrinsic(program, resolved, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.parameters.is_empty() {
        if let Some(intrinsic) = detect_instance_field_getter_intrinsic(
            program,
            resolved.dex_file_index,
            code,
            &resolved.method.proto.return_type,
        )? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("()F") {
        if let Some(intrinsic) = detect_float_array_getter_intrinsic(code) {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.return_type == "V"
        && resolved.method.proto.parameters.len() == 2
        && resolved.method.proto.parameters[1] == "I"
    {
        if let Some(intrinsic) =
            detect_float_array_pair_transform_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(FF)F") {
        if let Some(intrinsic) = detect_float_rem_add_rem_intrinsic(code) {
            return Ok(Some(intrinsic));
        }
        if let Some(intrinsic) =
            detect_float_atan2_normalize_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(FFF)F") {
        if let Some(intrinsic) = detect_float_lerp_intrinsic(code) {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(F)J") {
        if let Some(intrinsic) =
            detect_cubic_float_array_pack_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.parameters.len() == 1
        && resolved.method.proto.parameters[0] == "F"
        && resolved.method.proto.return_type.starts_with('L')
    {
        if let Some(intrinsic) = detect_measured_cubic_cut_intrinsic(program, resolved, code)? {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.descriptor_is("(F)F") {
        if let Some(intrinsic) =
            detect_path_angle_delta_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    if resolved.method.proto.return_type == "F"
        && resolved.method.proto.parameters.len() == 3
        && resolved.method.proto.parameters[0] == resolved.method.proto.parameters[1]
        && resolved.method.proto.parameters[2] == "F"
    {
        if let Some(intrinsic) =
            detect_circular_float_interpolation_intrinsic(program, resolved.dex_file_index, code)?
        {
            return Ok(Some(intrinsic));
        }
    }

    Ok(None)
}

fn detect_object_equals_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(method_index) = detect_object_equals_shape(code) else {
        return Ok(None);
    };
    if !method_ref_matches(
        program,
        dex_file_index,
        method_index,
        "Ljava/lang/Object;",
        "equals",
        "(Ljava/lang/Object;)Z",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::ObjectEquals {
        left_arg: 0,
        right_arg: 1,
    }))
}

// cite: art dex_instruction_list.h#IF_NEZ, CONST_4, INVOKE_VIRTUAL, MOVE_RESULT, RETURN
fn detect_object_equals_shape(code: &DexCodeItem) -> Option<u32> {
    let ins = &code.instructions;
    if code.registers_size != 2 || code.ins_size != 2 || ins.len() != 13 {
        return None;
    }
    if ins[0] != 0x0039
        || ins[1] != 0x0008
        || ins[2] != 0x0139
        || ins[3] != 0x0004
        || ins[4] != 0x1012
        || ins[5] != 0x000f
        || ins[6] != 0x0012
        || ins[7] != 0x000f
        || ins[8] != 0x206e
        || ins[10] != 0x0010
        || ins[11] != 0x000a
        || ins[12] != 0x000f
    {
        return None;
    }

    Some(u32::from(ins[9]))
}

fn detect_object_array_gap_insert_shape(code: &DexCodeItem) -> Option<(u32, u32)> {
    let ins = &code.instructions;
    if code.registers_size != 8 || code.ins_size != 3 || ins.len() != 65 {
        return None;
    }
    if ins[0] != 0x5052
        || ins[2] != 0x70b0
        || ins[3] != 0x003a
        || ins[4] != 0x0038
        || ins[5] != 0x5154
        || ins[7] != 0x1221
        || ins[8] != 0x2037
        || ins[16] != 0x033b
        || ins[42] != 0x505b
        || ins[44] != 0x5054
        || ins[46] != 0x5152
        || ins[48] != 0x0290
        || ins[50] != 0x5171
        || ins[51] != 0x0d61
        || ins[52] != 0x6200
        || ins[53] != 0x5652
        || ins[55] != 0x76b0
        || ins[56] != 0x5659
        || ins[58] != 0x000e
        || ins[64] != 0x0627
    {
        return None;
    }

    Some((u32::from(ins[6]), u32::from(ins[1])))
}

fn detect_object_array_append_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(fields) = detect_object_array_append_fields(program, resolved, code)? else {
        return Ok(None);
    };

    Ok(Some(MethodIntrinsic::ObjectArrayAppend {
        receiver_arg: 0,
        value_arg: 1,
        array_field_index: fields.array_field_index,
        size_field_index: fields.size_field_index,
        read_only_field_index: fields.read_only_field_index,
        mod_count_field_index: fields.mod_count_field_index,
    }))
}

#[derive(Clone, Copy)]
struct ObjectArrayAppendFields {
    array_field_index: u32,
    size_field_index: u32,
    read_only_field_index: u32,
    mod_count_field_index: u32,
}

fn detect_object_array_append_fields(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<ObjectArrayAppendFields>> {
    let Some(shape) = detect_object_array_append_shape(code) else {
        return Ok(None);
    };
    let mutability_method =
        program.method_id(resolved.dex_file_index, shape.mutability_method_index)?;
    if mutability_method.class_descriptor != resolved.method.class_descriptor
        || !mutability_method.proto.descriptor_is("()V")
    {
        return Ok(None);
    }
    let Some(mutability_resolved) = program.resolve_exact_method(&mutability_method)? else {
        return Ok(None);
    };
    let mutability_code = program.code_item(&mutability_resolved)?;
    let Some(read_only_field_index) = detect_object_array_mutability_check_shape(&mutability_code)
    else {
        return Ok(None);
    };
    let grow_method = program.method_id(resolved.dex_file_index, shape.grow_method_index)?;
    if grow_method.class_descriptor != resolved.method.class_descriptor
        || !grow_method.proto.descriptor_is("(II)V")
    {
        return Ok(None);
    }
    let Some(grow_resolved) = program.resolve_exact_method(&grow_method)? else {
        return Ok(None);
    };
    let grow_code = program.code_item(&grow_resolved)?;
    let Some((array_field_index, size_field_index)) =
        detect_object_array_gap_insert_shape(&grow_code)
    else {
        return Ok(None);
    };
    if array_field_index != shape.array_field_index || size_field_index != shape.size_field_index {
        return Ok(None);
    }
    if !field_ref_type_matches(program, resolved.dex_file_index, size_field_index, "I")?
        || !field_ref_type_matches(program, resolved.dex_file_index, read_only_field_index, "Z")?
    {
        return Ok(None);
    }
    let array_field = program.field_id(resolved.dex_file_index, array_field_index)?;
    if !array_field.type_descriptor.starts_with("[L") {
        return Ok(None);
    }
    let mod_count_field = program.field_id(resolved.dex_file_index, shape.mod_count_field_index)?;
    if mod_count_field.class_descriptor != "Ljava/util/AbstractList;"
        || mod_count_field.name != "modCount"
        || mod_count_field.type_descriptor != "I"
    {
        return Ok(None);
    }

    Ok(Some(ObjectArrayAppendFields {
        array_field_index,
        size_field_index,
        read_only_field_index,
        mod_count_field_index: shape.mod_count_field_index,
    }))
}

fn detect_object_array_append_fields_from_method_ref(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
) -> Result<Option<ObjectArrayAppendFields>> {
    let method = program.method_id(dex_file_index, method_index)?;
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };
    let code = program.code_item(&resolved)?;
    detect_object_array_append_fields(program, &resolved, &code)
}

#[derive(Clone, Copy)]
struct ObjectArrayAppendShape {
    mutability_method_index: u32,
    grow_method_index: u32,
    array_field_index: u32,
    size_field_index: u32,
    mod_count_field_index: u32,
}

fn detect_object_array_append_shape(code: &DexCodeItem) -> Option<ObjectArrayAppendShape> {
    let ins = &code.instructions;
    if code.registers_size != 5 || code.ins_size != 2 || code.outs_size != 3 || ins.len() != 19 {
        return None;
    }
    if ins[0] != 0x106e
        || ins[3] != 0x3052
        || ins[5] != 0x3152
        || ins[7] != 0x1212
        || ins[8] != 0x21b0
        || ins[9] != 0x3159
        || ins[11] != 0x306e
        || ins[14] != 0x3154
        || ins[16] != 0x044d
        || ins[17] != 0x0001
        || ins[18] != 0x020f
        || ins[6] != ins[10]
    {
        return None;
    }

    Some(ObjectArrayAppendShape {
        mutability_method_index: u32::from(ins[1]),
        grow_method_index: u32::from(ins[12]),
        array_field_index: u32::from(ins[15]),
        size_field_index: u32::from(ins[4]),
        mod_count_field_index: u32::from(ins[6]),
    })
}

fn detect_object_array_mutability_check_shape(code: &DexCodeItem) -> Option<u32> {
    let ins = &code.instructions;
    if code.registers_size != 2 || code.ins_size != 1 || code.outs_size != 1 || ins.len() != 11 {
        return None;
    }
    if ins[0] != 0x1055
        || ins[2] != 0x0039
        || ins[3] != 0x0003
        || ins[4] != 0x000e
        || ins[5] != 0x0022
        || ins[7] != 0x1070
        || ins[9] != 0x0000
        || ins[10] != 0x0027
    {
        return None;
    }

    Some(u32::from(ins[1]))
}

fn detect_slot_object_gap_ensure_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some((
        slot_index_field_index,
        object_gap_start_field_index,
        object_gap_size_field_index,
        object_array_field_index,
        slot_limit_field_index,
        move_method_index,
    )) = detect_slot_object_gap_ensure_shape(code)
    else {
        return Ok(None);
    };
    let move_method = program.method_id(resolved.dex_file_index, move_method_index)?;
    if move_method.class_descriptor != resolved.method.class_descriptor
        || !move_method.proto.descriptor_is("(II)V")
    {
        return Ok(None);
    }
    let Some(move_resolved) = program.resolve_exact_method(&move_method)? else {
        return Ok(None);
    };
    let move_code = program.code_item(&move_resolved)?;
    let Some(move_shape) = detect_slot_object_gap_move_shape(&move_code) else {
        return Ok(None);
    };
    if move_shape.object_array_field_index != object_array_field_index
        || move_shape.object_gap_start_field_index != object_gap_start_field_index
        || move_shape.object_gap_size_field_index != object_gap_size_field_index
    {
        return Ok(None);
    }
    if !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        slot_index_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        slot_limit_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        object_gap_start_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        object_gap_size_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        move_shape.object_anchor_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        move_shape.group_array_field_index,
        "[I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        move_shape.group_gap_start_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        move_shape.group_gap_size_field_index,
        "I",
    )? {
        return Ok(None);
    }
    let object_array_field = program.field_id(resolved.dex_file_index, object_array_field_index)?;
    if !object_array_field.type_descriptor.starts_with("[L") {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::SlotObjectGapEnsure {
        receiver_arg: 0,
        count_arg: 1,
        anchor_arg: 2,
        slot_index_field_index,
        slot_limit_field_index,
        object_array_field_index,
        object_gap_start_field_index,
        object_gap_size_field_index,
        object_anchor_field_index: move_shape.object_anchor_field_index,
        group_array_field_index: move_shape.group_array_field_index,
        group_gap_start_field_index: move_shape.group_gap_start_field_index,
        group_gap_size_field_index: move_shape.group_gap_size_field_index,
    }))
}

fn detect_slot_object_gap_ensure_shape(
    code: &DexCodeItem,
) -> Option<(u32, u32, u32, u32, u32, u32)> {
    let ins = &code.instructions;
    if code.registers_size != 12 || code.ins_size != 3 || code.outs_size != 5 || ins.len() != 72 {
        return None;
    }
    if ins[0] != 0x0a3d
        || ins[1] != 0x0047
        || ins[2] != 0x9052
        || ins[4] != 0x306e
        || ins[7] != 0x9b52
        || ins[9] != 0x9052
        || ins[11] != 0xa035
        || ins[13] != 0x9154
        || ins[15] != 0x1221
        || ins[20] != 0x0590
        || ins[28] != 0x2071
        || ins[48] != 0x5b71
        || ins[52] != 0x5271
        || ins[55] != 0x955b
        || ins[58] != 0x9152
        || ins[63] != 0x9159
        || ins[66] != 0x9b59
        || ins[69] != 0x9059
        || ins[71] != 0x000e
    {
        return None;
    }

    Some((
        u32::from(ins[3]),
        u32::from(ins[8]),
        u32::from(ins[10]),
        u32::from(ins[14]),
        u32::from(ins[59]),
        u32::from(ins[5]),
    ))
}

fn detect_slot_object_update_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_slot_object_update_shape(code) else {
        return Ok(None);
    };
    let slot_gap_method =
        program.method_id(resolved.dex_file_index, shape.slot_gap_method_index)?;
    if slot_gap_method.class_descriptor != resolved.method.class_descriptor
        || !slot_gap_method.proto.descriptor_is("(II)V")
    {
        return Ok(None);
    }
    let Some(MethodIntrinsic::SlotObjectGapEnsure {
        receiver_arg: 0,
        count_arg: 1,
        anchor_arg: 2,
        slot_index_field_index,
        slot_limit_field_index,
        object_array_field_index,
        object_gap_start_field_index,
        object_gap_size_field_index,
        object_anchor_field_index,
        group_array_field_index,
        group_gap_start_field_index,
        group_gap_size_field_index,
    }) = method_ref_intrinsic(
        program,
        resolved.dex_file_index,
        shape.slot_gap_method_index,
    )?
    else {
        return Ok(None);
    };
    if shape.slot_index_field_index != slot_index_field_index
        || shape.slot_limit_field_index != slot_limit_field_index
        || shape.object_array_field_index != object_array_field_index
    {
        return Ok(None);
    }

    let data_address_method =
        program.method_id(resolved.dex_file_index, shape.data_address_method_index)?;
    if data_address_method.class_descriptor != resolved.method.class_descriptor
        || !data_address_method.proto.descriptor_is("(I)I")
    {
        return Ok(None);
    }
    let Some(data_address_resolved) = program.resolve_exact_method(&data_address_method)? else {
        return Ok(None);
    };
    let data_address_code = program.code_item(&data_address_resolved)?;
    let Some((data_gap_size_field_index, data_gap_start_field_index)) =
        detect_slot_object_data_address_shape(&data_address_code)
    else {
        return Ok(None);
    };
    if data_gap_size_field_index != object_gap_size_field_index
        || data_gap_start_field_index != object_gap_start_field_index
    {
        return Ok(None);
    }
    if !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        shape.insert_count_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        shape.insert_anchor_field_index,
        "I",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::SlotObjectUpdate {
        receiver_arg: 0,
        value_arg: 1,
        insert_count_field_index: shape.insert_count_field_index,
        insert_anchor_field_index: shape.insert_anchor_field_index,
        slot_index_field_index,
        slot_limit_field_index,
        object_array_field_index,
        object_gap_start_field_index,
        object_gap_size_field_index,
        object_anchor_field_index,
        group_array_field_index,
        group_gap_start_field_index,
        group_gap_size_field_index,
    }))
}

#[derive(Clone, Copy)]
struct SlotObjectUpdateShape {
    insert_count_field_index: u32,
    insert_anchor_field_index: u32,
    object_array_field_index: u32,
    slot_index_field_index: u32,
    slot_limit_field_index: u32,
    slot_gap_method_index: u32,
    data_address_method_index: u32,
}

// cite: art dex_instruction_list.h#IGET, CONST_4, IF_LEZ, INVOKE_VIRTUAL, IGET_OBJECT, ADD_INT_LIT8, IPUT, MOVE_RESULT, AGET_OBJECT, IF_LE, GOTO, CONST_STRING, INVOKE_STATIC, SUB_INT_2ADDR, APUT_OBJECT, RETURN_OBJECT
fn detect_slot_object_update_shape(code: &DexCodeItem) -> Option<SlotObjectUpdateShape> {
    let ins = &code.instructions;
    if code.registers_size != 6 || code.ins_size != 2 || code.outs_size != 3 || ins.len() != 48 {
        return None;
    }
    let fixed = [
        (0, 0x4052),
        (2, 0x1112),
        (3, 0x003d),
        (4, 0x0007),
        (5, 0x4052),
        (7, 0x306e),
        (9, 0x0014),
        (10, 0x4054),
        (12, 0x4252),
        (14, 0x03d8),
        (15, 0x0102),
        (16, 0x4359),
        (18, 0x206e),
        (20, 0x0024),
        (21, 0x020a),
        (22, 0x0046),
        (23, 0x0200),
        (24, 0x4252),
        (26, 0x4352),
        (28, 0x3236),
        (29, 0x0003),
        (30, 0x0628),
        (33, 0x1071),
        (35, 0x0002),
        (36, 0x4254),
        (38, 0x4352),
        (40, 0x13b1),
        (41, 0x206e),
        (43, 0x0034),
        (44, 0x010a),
        (45, 0x054d),
        (46, 0x0102),
        (47, 0x0011),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[13] != ins[17] || ins[13] != ins[25] || ins[13] != ins[39] {
        return None;
    }
    if ins[11] != ins[37] {
        return None;
    }
    if ins[19] != ins[42] {
        return None;
    }

    Some(SlotObjectUpdateShape {
        insert_count_field_index: u32::from(ins[1]),
        insert_anchor_field_index: u32::from(ins[6]),
        object_array_field_index: u32::from(ins[11]),
        slot_index_field_index: u32::from(ins[13]),
        slot_limit_field_index: u32::from(ins[27]),
        slot_gap_method_index: u32::from(ins[8]),
        data_address_method_index: u32::from(ins[19]),
    })
}

// cite: androidx SlotTable.kt#dataIndexToDataAddress
fn detect_slot_object_data_address_shape(code: &DexCodeItem) -> Option<(u32, u32)> {
    let ins = &code.instructions;
    if code.registers_size != 4 || code.ins_size != 2 || code.outs_size != 0 || ins.len() != 12 {
        return None;
    }
    let fixed = [
        (0, 0x2052),
        (2, 0x2152),
        (4, 0x1335),
        (5, 0x0004),
        (6, 0x0112),
        (7, 0x0228),
        (8, 0x1112),
        (9, 0x10b2),
        (10, 0x30b0),
        (11, 0x000f),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }

    Some((u32::from(ins[1]), u32::from(ins[3])))
}

fn detect_slot_group_data_index_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_slot_group_data_index_shape(code) else {
        return Ok(None);
    };
    let capacity_method =
        program.method_id(resolved.dex_file_index, shape.capacity_method_index)?;
    if capacity_method.class_descriptor != resolved.method.class_descriptor
        || !capacity_method.proto.descriptor_is("()I")
    {
        return Ok(None);
    }
    let Some(capacity_resolved) = program.resolve_exact_method(&capacity_method)? else {
        return Ok(None);
    };
    let capacity_code = program.code_item(&capacity_resolved)?;
    let Some(group_array_field_index) = detect_slot_group_capacity_shape(&capacity_code) else {
        return Ok(None);
    };
    if !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        group_array_field_index,
        "[I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        shape.object_gap_size_field_index,
        "I",
    )? {
        return Ok(None);
    }
    let object_array_field =
        program.field_id(resolved.dex_file_index, shape.object_array_field_index)?;
    if !object_array_field.type_descriptor.starts_with("[L") {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::SlotGroupDataIndex {
        receiver_arg: 0,
        groups_arg: 1,
        address_arg: 2,
        group_array_field_index,
        object_array_field_index: shape.object_array_field_index,
        object_gap_size_field_index: shape.object_gap_size_field_index,
    }))
}

#[derive(Clone, Copy)]
struct SlotGroupDataIndexShape {
    capacity_method_index: u32,
    object_array_field_index: u32,
    object_gap_size_field_index: u32,
}

// cite: androidx SlotTable.kt#dataIndex, dataAnchorToDataIndex
fn detect_slot_group_data_index_shape(code: &DexCodeItem) -> Option<SlotGroupDataIndexShape> {
    let ins = &code.instructions;
    if code.registers_size != 4 || code.ins_size != 3 || code.outs_size != 1 || ins.len() != 32 {
        return None;
    }
    let fixed = [
        (0, 0x106e),
        (2, 0x0001),
        (3, 0x000a),
        (4, 0x0334),
        (5, 0x0009),
        (6, 0x1254),
        (8, 0x2221),
        (9, 0x1352),
        (11, 0x32b1),
        (12, 0x020f),
        (13, 0x03da),
        (14, 0x0503),
        (15, 0x03d8),
        (16, 0x0403),
        (17, 0x0244),
        (18, 0x0302),
        (19, 0x1352),
        (21, 0x1054),
        (23, 0x0021),
        (24, 0x023b),
        (25, 0x0007),
        (26, 0x30b1),
        (27, 0x20b0),
        (28, 0x00d8),
        (29, 0x0100),
        (30, 0x000f),
        (31, 0x020f),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[7] != ins[22] || ins[10] != ins[20] {
        return None;
    }

    Some(SlotGroupDataIndexShape {
        capacity_method_index: u32::from(ins[1]),
        object_array_field_index: u32::from(ins[7]),
        object_gap_size_field_index: u32::from(ins[10]),
    })
}

// cite: androidx SlotTable.kt#capacity
fn detect_slot_group_capacity_shape(code: &DexCodeItem) -> Option<u32> {
    let ins = &code.instructions;
    if code.registers_size != 2 || code.ins_size != 1 || code.outs_size != 0 || ins.len() != 6 {
        return None;
    }
    if ins[0] != 0x1054
        || ins[2] != 0x0021
        || ins[3] != 0x00db
        || ins[4] != 0x0500
        || ins[5] != 0x000f
    {
        return None;
    }

    Some(u32::from(ins[1]))
}

fn detect_slot_group_size_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some((capacity_method_index, group_gap_size_field_index)) =
        detect_slot_group_size_shape(code)
    else {
        return Ok(None);
    };
    let capacity_method = program.method_id(resolved.dex_file_index, capacity_method_index)?;
    if capacity_method.class_descriptor != resolved.method.class_descriptor
        || !capacity_method.proto.descriptor_is("()I")
    {
        return Ok(None);
    }
    let Some(capacity_resolved) = program.resolve_exact_method(&capacity_method)? else {
        return Ok(None);
    };
    let capacity_code = program.code_item(&capacity_resolved)?;
    let Some(group_array_field_index) = detect_slot_group_capacity_shape(&capacity_code) else {
        return Ok(None);
    };
    if !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        group_array_field_index,
        "[I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        group_gap_size_field_index,
        "I",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::SlotGroupSize {
        receiver_arg: 0,
        group_array_field_index,
        group_gap_size_field_index,
    }))
}

// cite: androidx SlotTable.kt#size, capacity
fn detect_slot_group_size_shape(code: &DexCodeItem) -> Option<(u32, u32)> {
    let ins = &code.instructions;
    if code.registers_size != 3 || code.ins_size != 1 || code.outs_size != 1 || ins.len() != 8 {
        return None;
    }
    if ins[0] != 0x106e
        || ins[2] != 0x0002
        || ins[3] != 0x000a
        || ins[4] != 0x2152
        || ins[6] != 0x10b1
        || ins[7] != 0x000f
    {
        return None;
    }

    Some((u32::from(ins[1]), u32::from(ins[5])))
}

fn detect_slot_group_address_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some((group_gap_size_field_index, group_gap_start_field_index)) =
        detect_slot_group_address_shape(code)
    else {
        return Ok(None);
    };
    if !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        group_gap_size_field_index,
        "I",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        group_gap_start_field_index,
        "I",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::SlotGroupAddress {
        receiver_arg: 0,
        index_arg: 1,
        group_gap_start_field_index,
        group_gap_size_field_index,
    }))
}

// cite: androidx SlotTable.kt#groupIndexToAddress
fn detect_slot_group_address_shape(code: &DexCodeItem) -> Option<(u32, u32)> {
    let ins = &code.instructions;
    if code.registers_size != 4 || code.ins_size != 2 || code.outs_size != 0 || ins.len() != 12 {
        return None;
    }
    let fixed = [
        (0, 0x2052),
        (2, 0x2152),
        (4, 0x1335),
        (5, 0x0004),
        (6, 0x0112),
        (7, 0x0228),
        (8, 0x1112),
        (9, 0x10b2),
        (10, 0x30b0),
        (11, 0x000f),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }

    Some((u32::from(ins[1]), u32::from(ins[3])))
}

#[derive(Clone, Copy)]
struct SlotObjectGapMoveShape {
    object_array_field_index: u32,
    object_gap_start_field_index: u32,
    object_gap_size_field_index: u32,
    object_anchor_field_index: u32,
    group_array_field_index: u32,
    group_gap_start_field_index: u32,
    group_gap_size_field_index: u32,
}

fn detect_slot_object_gap_move_shape(code: &DexCodeItem) -> Option<SlotObjectGapMoveShape> {
    let ins = &code.instructions;
    if code.registers_size != 12 || code.ins_size != 3 || code.outs_size != 5 || ins.len() != 145 {
        return None;
    }
    if ins[0] != 0x9052
        || ins[2] != 0x9152
        || ins[4] != 0x9252
        || ins[8] != 0x9354
        || ins[15] != 0x5171
        || ins[24] != 0x5571
        || ins[29] != 0x106e
        || ins[33] != 0x2071
        || ins[39] != 0x9354
        || ins[46] != 0x206e
        || ins[50] != 0x206e
        || ins[54] != 0x9552
        || ins[58] != 0x9654
        || ins[75] != 0x1071
        || ins[78] != 0x9854
        || ins[90] != 0x9652
        || ins[94] != 0x206e
        || ins[98] != 0x206e
        || ins[104] != 0x9554
        || ins[121] != 0x1071
        || ins[124] != 0x9754
        || ins[132] != 0x9552
        || ins[136] != 0x9552
        || ins[140] != 0x9b59
        || ins[142] != 0x9a59
        || ins[144] != 0x000e
    {
        return None;
    }

    Some(SlotObjectGapMoveShape {
        object_gap_size_field_index: u32::from(ins[1]),
        object_gap_start_field_index: u32::from(ins[3]),
        object_anchor_field_index: u32::from(ins[5]),
        object_array_field_index: u32::from(ins[9]),
        group_gap_start_field_index: u32::from(ins[55]),
        group_array_field_index: u32::from(ins[59]),
        group_gap_size_field_index: u32::from(ins[91]),
    })
}

fn grown_object_array_capacity(current: usize, needed: usize) -> Option<usize> {
    const MAX_ARRAY_SIZE: usize = 2_147_483_639;
    if needed > MAX_ARRAY_SIZE {
        return None;
    }
    let mut capacity = current.checked_add(current >> 1)?;
    if capacity < needed {
        capacity = needed;
    }
    if capacity > MAX_ARRAY_SIZE {
        capacity = MAX_ARRAY_SIZE;
    }
    Some(capacity)
}

fn detect_float_pack_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 21 {
        return Ok(None);
    }

    let Some(high_register) = invoke_static_one_arg(
        program,
        dex_file_index,
        ins,
        0,
        "Ljava/lang/Float;",
        "floatToRawIntBits",
        "(F)I",
    )?
    else {
        return Ok(None);
    };
    let Some(high_arg) = parameter_word_index(code, high_register) else {
        return Ok(None);
    };
    let high_bits = dex_high_byte(ins[3]);
    let Some((packed_high, high_source)) = dex_12x(ins[4], 0x81) else {
        return Ok(None);
    };
    if dex_opcode(ins[3]) != 0x0a || high_source != high_bits {
        return Ok(None);
    }

    let Some(low_register) = invoke_static_one_arg(
        program,
        dex_file_index,
        ins,
        5,
        "Ljava/lang/Float;",
        "floatToRawIntBits",
        "(F)I",
    )?
    else {
        return Ok(None);
    };
    let Some(low_arg) = parameter_word_index(code, low_register) else {
        return Ok(None);
    };
    let low_bits = dex_high_byte(ins[8]);
    let Some((packed_low, low_source)) = dex_12x(ins[9], 0x81) else {
        return Ok(None);
    };
    if dex_opcode(ins[8]) != 0x0a || low_source != low_bits {
        return Ok(None);
    }

    if dex_opcode(ins[10]) != 0x13 || ins[11] as i16 != 32 {
        return Ok(None);
    }
    let shift_register = dex_high_byte(ins[10]);
    if dex_12x(ins[12], 0xc3) != Some((packed_high, shift_register)) {
        return Ok(None);
    }
    if dex_opcode(ins[13]) != 0x18
        || ins[14] != 0xffff
        || ins[15] != 0xffff
        || ins[16] != 0
        || ins[17] != 0
    {
        return Ok(None);
    }
    let mask_register = dex_high_byte(ins[13]);
    if dex_12x(ins[18], 0xc0) != Some((packed_low, mask_register)) {
        return Ok(None);
    }
    if dex_12x(ins[19], 0xc1) != Some((packed_low, packed_high)) {
        return Ok(None);
    }
    if dex_opcode(ins[20]) != 0x10 || dex_high_byte(ins[20]) != packed_low {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::FloatPack { high_arg, low_arg }))
}

fn detect_affine_float_pack_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 15 {
        return Ok(None);
    }

    let Some((x_offset_register, receiver_register)) = dex_12x(ins[0], 0x52) else {
        return Ok(None);
    };
    let x_offset_field_index = u32::from(ins[1]);
    let Some((x_register, x_offset_source)) = dex_12x(ins[2], 0xc6) else {
        return Ok(None);
    };
    if x_offset_source != x_offset_register {
        return Ok(None);
    }

    let Some((scale_register, scale_receiver_register)) = dex_12x(ins[3], 0x52) else {
        return Ok(None);
    };
    if scale_receiver_register != receiver_register {
        return Ok(None);
    }
    let scale_field_index = u32::from(ins[4]);
    if dex_12x(ins[5], 0xc9) != Some((x_register, scale_register)) {
        return Ok(None);
    }

    let Some((y_offset_register, y_receiver_register)) = dex_12x(ins[6], 0x52) else {
        return Ok(None);
    };
    if y_receiver_register != receiver_register {
        return Ok(None);
    }
    let y_offset_field_index = u32::from(ins[7]);
    let Some((y_register, y_offset_source)) = dex_12x(ins[8], 0xc6) else {
        return Ok(None);
    };
    if y_offset_source != y_offset_register {
        return Ok(None);
    }
    if dex_12x(ins[9], 0xc9) != Some((y_register, scale_register)) {
        return Ok(None);
    }

    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 10, 0x71, "(FF)J")?
        != Some(vec![x_register, y_register])
    {
        return Ok(None);
    }
    if !matches!(
        method_ref_intrinsic(program, dex_file_index, u32::from(ins[11]))?,
        Some(MethodIntrinsic::FloatPack {
            high_arg: 0,
            low_arg: 1
        })
    ) {
        return Ok(None);
    }

    if dex_opcode(ins[13]) != 0x0b {
        return Ok(None);
    }
    let result_register = dex_high_byte(ins[13]);
    if dex_opcode(ins[14]) != 0x10 || dex_high_byte(ins[14]) != result_register {
        return Ok(None);
    }

    let Some(receiver_arg) = parameter_word_index(code, receiver_register) else {
        return Ok(None);
    };
    let Some(x_arg) = parameter_word_index(code, x_register) else {
        return Ok(None);
    };
    let Some(y_arg) = parameter_word_index(code, y_register) else {
        return Ok(None);
    };

    if !field_ref_type_matches(program, dex_file_index, x_offset_field_index, "F")?
        || !field_ref_type_matches(program, dex_file_index, scale_field_index, "F")?
        || !field_ref_type_matches(program, dex_file_index, y_offset_field_index, "F")?
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::AffineFloatPack {
        receiver_arg,
        x_arg,
        y_arg,
        x_offset_field_index,
        scale_field_index,
        y_offset_field_index,
    }))
}

fn detect_matrix_point_transform_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_matrix_point_transform_shape(code) else {
        return Ok(None);
    };
    let ins = &code.instructions;
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 2, 0x71, "(FF)J")?
        != Some(vec![2, 3])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 6, 0x71, "(J[F)J")?
        != Some(vec![2, 3, shape.matrix_register])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 10, 0x71, "(J)F")?
        != Some(vec![2, 3])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 14, 0x71, "(J)F")?
        != Some(vec![2, 3])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 18, 0x71, "(FF)J")?
        != Some(vec![0, 2])
    {
        return Ok(None);
    }
    if !field_ref_type_matches(program, dex_file_index, shape.matrix_field_index, "[F")? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::MatrixPointTransform {
        receiver_arg: shape.receiver_arg,
        x_arg: shape.x_arg,
        y_arg: shape.y_arg,
        matrix_field_index: shape.matrix_field_index,
    }))
}

#[derive(Clone, Copy)]
struct MatrixPointTransformShape {
    receiver_arg: usize,
    x_arg: usize,
    y_arg: usize,
    matrix_register: u16,
    matrix_field_index: u32,
}

// cite: androidx Matrix.kt#map
fn detect_matrix_point_transform_shape(code: &DexCodeItem) -> Option<MatrixPointTransformShape> {
    let ins = &code.instructions;
    if code.registers_size != 4 || code.ins_size != 3 || code.outs_size != 3 || ins.len() != 23 {
        return None;
    }
    let fixed = [
        (0, 0x1054),
        (2, 0x2071),
        (4, 0x0032),
        (5, 0x020b),
        (6, 0x3071),
        (8, 0x0032),
        (9, 0x020b),
        (10, 0x2071),
        (12, 0x0032),
        (13, 0x000a),
        (14, 0x2071),
        (16, 0x0032),
        (17, 0x020a),
        (18, 0x2071),
        (20, 0x0020),
        (21, 0x020b),
        (22, 0x0210),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    let Some((matrix_register, receiver_register)) = dex_12x(ins[0], 0x54) else {
        return None;
    };
    let receiver_arg = parameter_word_index(code, receiver_register)?;
    let x_arg = parameter_word_index(code, 2)?;
    let y_arg = parameter_word_index(code, 3)?;

    Some(MatrixPointTransformShape {
        receiver_arg,
        x_arg,
        y_arg,
        matrix_register,
        matrix_field_index: u32::from(ins[1]),
    })
}

fn detect_long_high_float_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 9 {
        return Ok(None);
    }
    if dex_opcode(ins[0]) != 0x13 || ins[1] as i16 != 32 {
        return Ok(None);
    }
    let shift_register = dex_high_byte(ins[0]);
    let Some((value_register, shift_source)) = dex_12x(ins[2], 0xc4) else {
        return Ok(None);
    };
    if shift_source != shift_register {
        return Ok(None);
    }
    let Some(value_arg) = parameter_word_index(code, value_register) else {
        return Ok(None);
    };
    if dex_12x(ins[3], 0x84) != Some((value_register, value_register)) {
        return Ok(None);
    }
    let Some(bits_register) = invoke_static_one_arg(
        program,
        dex_file_index,
        ins,
        4,
        "Ljava/lang/Float;",
        "intBitsToFloat",
        "(I)F",
    )?
    else {
        return Ok(None);
    };
    if bits_register != value_register {
        return Ok(None);
    }
    if dex_opcode(ins[7]) != 0x0a || dex_high_byte(ins[7]) != value_register {
        return Ok(None);
    }
    if dex_opcode(ins[8]) != 0x0f || dex_high_byte(ins[8]) != value_register {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::LongHighFloat { value_arg }))
}

fn detect_long_low_float_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 12 {
        return Ok(None);
    }
    if dex_opcode(ins[0]) != 0x18
        || ins[1] != 0xffff
        || ins[2] != 0xffff
        || ins[3] != 0
        || ins[4] != 0
    {
        return Ok(None);
    }
    let mask_register = dex_high_byte(ins[0]);
    let Some((value_register, mask_source)) = dex_12x(ins[5], 0xc0) else {
        return Ok(None);
    };
    if mask_source != mask_register {
        return Ok(None);
    }
    let Some(value_arg) = parameter_word_index(code, value_register) else {
        return Ok(None);
    };
    if dex_12x(ins[6], 0x84) != Some((value_register, value_register)) {
        return Ok(None);
    }
    let Some(bits_register) = invoke_static_one_arg(
        program,
        dex_file_index,
        ins,
        7,
        "Ljava/lang/Float;",
        "intBitsToFloat",
        "(I)F",
    )?
    else {
        return Ok(None);
    };
    if bits_register != value_register {
        return Ok(None);
    }
    if dex_opcode(ins[10]) != 0x0a || dex_high_byte(ins[10]) != value_register {
        return Ok(None);
    }
    if dex_opcode(ins[11]) != 0x0f || dex_high_byte(ins[11]) != value_register {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::LongLowFloat { value_arg }))
}

fn detect_instance_field_getter_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
    return_type: &str,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 3 || code.ins_size != 1 {
        return Ok(None);
    }
    let Some(first_parameter) = code.registers_size.checked_sub(code.ins_size) else {
        return Ok(None);
    };

    let (iget_opcode, return_opcode, value_type) = match return_type {
        "Z" | "B" | "C" | "S" | "I" => (0x52, 0x0f, IntrinsicFieldValueType::Int),
        "J" => (0x53, 0x10, IntrinsicFieldValueType::Long),
        "F" => (0x52, 0x0f, IntrinsicFieldValueType::Float),
        _ if return_type.starts_with('L') || return_type.starts_with('[') => {
            (0x54, 0x11, IntrinsicFieldValueType::Object)
        }
        _ => return Ok(None),
    };
    let Some((value_register, receiver_register)) = dex_12x(ins[0], iget_opcode) else {
        return Ok(None);
    };
    if receiver_register != first_parameter {
        return Ok(None);
    }
    let field_index = u32::from(ins[1]);
    if dex_opcode(ins[2]) != return_opcode || dex_high_byte(ins[2]) != value_register {
        return Ok(None);
    }
    if !field_ref_type_matches(program, dex_file_index, field_index, return_type)? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::InstanceFieldGetter {
        receiver_arg: 0,
        field_index,
        value_type,
    }))
}

fn detect_float_array_getter_intrinsic(code: &DexCodeItem) -> Option<MethodIntrinsic> {
    let ins = &code.instructions;
    if ins.len() != 6 {
        return None;
    }
    let (array_register, object_register) = dex_12x(ins[0], 0x54)?;
    let receiver_arg = parameter_word_index(code, object_register)?;
    let field_index = u32::from(ins[1]);
    if dex_opcode(ins[2]) != 0x12 {
        return None;
    }
    let index_register = dex_nibble_a(ins[2]);
    let element_index = sign_extend_dex_nibble(dex_nibble_b(ins[2]));
    if dex_opcode(ins[3]) != 0x44 || dex_high_byte(ins[3]) != array_register {
        return None;
    }
    let array_and_index = ins[4];
    if (array_and_index & 0x00ff) != array_register
        || ((array_and_index >> 8) & 0x00ff) != index_register
    {
        return None;
    }
    if dex_opcode(ins[5]) != 0x0f || dex_high_byte(ins[5]) != array_register {
        return None;
    }

    Some(MethodIntrinsic::FloatArrayGetter {
        receiver_arg,
        field_index,
        element_index,
    })
}

fn detect_circular_float_interpolation_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 172 || code.ins_size != 3 {
        return Ok(None);
    }
    let Some(first_parameter) = code.registers_size.checked_sub(code.ins_size) else {
        return Ok(None);
    };
    let x_register = first_parameter;
    let y_register = first_parameter + 1;
    let progress_register = first_parameter + 2;

    let Some((_size_register, size_receiver)) = dex_12x(ins[22], 0x52) else {
        return Ok(None);
    };
    if size_receiver != x_register {
        return Ok(None);
    }
    let size_field_index = u32::from(ins[23]);
    for pc in [51usize, 80] {
        let Some((_, receiver)) = dex_12x(ins[pc], 0x52) else {
            return Ok(None);
        };
        if receiver != x_register || u32::from(ins[pc + 1]) != size_field_index {
            return Ok(None);
        }
    }

    let Some((array_field_index, getter_size_field_index)) =
        method_ref_checked_float_vector_getter_fields(program, dex_file_index, u32::from(ins[46]))?
    else {
        return Ok(None);
    };
    if getter_size_field_index != size_field_index {
        return Ok(None);
    }
    for pc in [45usize, 55, 83, 87, 96, 100, 119, 129] {
        let Some((array_field, size_field)) = method_ref_checked_float_vector_getter_fields(
            program,
            dex_file_index,
            u32::from(ins[pc + 1]),
        )?
        else {
            return Ok(None);
        };
        if array_field != array_field_index || size_field != size_field_index {
            return Ok(None);
        }
    }

    for pc in [92usize, 105, 124, 135] {
        if !matches!(
            method_ref_intrinsic(program, dex_file_index, u32::from(ins[pc + 1]))?,
            Some(MethodIntrinsic::FloatRemAddRem { .. })
        ) {
            return Ok(None);
        }
    }

    if dex_opcode(ins[10]) != 0x12 || dex_high_byte(ins[10]) != 0 {
        return Ok(None);
    }
    if dex_opcode(ins[15]) != 0x15 || f32::from_bits(u32::from(ins[16]) << 16) != 1.0 {
        return Ok(None);
    }
    if !field_ref_type_matches(program, dex_file_index, size_field_index, "I")?
        || !field_ref_type_matches(program, dex_file_index, array_field_index, "[F")?
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::CircularFloatInterpolation {
        x_arg: parameter_word_index(code, x_register).unwrap_or(0),
        y_arg: parameter_word_index(code, y_register).unwrap_or(1),
        progress_arg: parameter_word_index(code, progress_register).unwrap_or(2),
        array_field_index,
        size_field_index,
    }))
}

fn method_ref_checked_float_vector_getter_fields(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
) -> Result<Option<(u32, u32)>> {
    let method = program.method_id(dex_file_index, method_index)?;
    if !method.proto.descriptor_is("(I)F") {
        return Ok(None);
    }
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };
    let code = program.code_item(&resolved)?;
    let ins = &code.instructions;
    if ins.len() != 18 || code.ins_size != 2 {
        return Ok(None);
    }
    let Some(first_parameter) = code.registers_size.checked_sub(code.ins_size) else {
        return Ok(None);
    };
    let receiver_register = first_parameter;
    let index_register = first_parameter + 1;
    let Some((_size_register, size_receiver)) = dex_12x(ins[2], 0x52) else {
        return Ok(None);
    };
    if size_receiver != receiver_register {
        return Ok(None);
    }
    let size_field_index = u32::from(ins[3]);
    let Some((array_register, array_receiver)) = dex_12x(ins[6], 0x54) else {
        return Ok(None);
    };
    if array_receiver != receiver_register {
        return Ok(None);
    }
    let array_field_index = u32::from(ins[7]);
    let Some((value_register, array_source, index_source)) = dex_23x(ins[8], ins[9], 0x44) else {
        return Ok(None);
    };
    if array_source != array_register || index_source != index_register {
        return Ok(None);
    }
    if dex_opcode(ins[10]) != 0x0f || dex_high_byte(ins[10]) != value_register {
        return Ok(None);
    }
    if !field_ref_type_matches(program, dex_file_index, size_field_index, "I")?
        || !field_ref_type_matches(program, dex_file_index, array_field_index, "[F")?
    {
        return Ok(None);
    }

    Ok(Some((array_field_index, size_field_index)))
}

fn detect_float_array_pair_transform_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 37 {
        return Ok(None);
    }

    let Some((array_register, receiver_register)) = dex_12x(ins[0], 0x54) else {
        return Ok(None);
    };
    let array_field_index = u32::from(ins[1]);
    let Some((x_register, array_source, index_register)) = dex_23x(ins[2], ins[3], 0x44) else {
        return Ok(None);
    };
    if array_source != array_register {
        return Ok(None);
    }
    let Some((next_index_register, index_source, offset)) = dex_22b(ins[4], ins[5], 0xd8) else {
        return Ok(None);
    };
    if index_source != index_register || offset != 1 {
        return Ok(None);
    }
    let Some((y_register, second_array_source, second_index_source)) =
        dex_23x(ins[6], ins[7], 0x44)
    else {
        return Ok(None);
    };
    if second_array_source != array_register || second_index_source != next_index_register {
        return Ok(None);
    }

    let invoke_opcode = dex_opcode(ins[8]);
    let transform_is_interface = match invoke_opcode {
        0x6e => false,
        0x72 => true,
        _ => return Ok(None),
    };
    let Some(invoke_args) =
        invoke_35c_args_matching_proto(program, dex_file_index, ins, 8, invoke_opcode, "(FF)J")?
    else {
        return Ok(None);
    };
    if invoke_args.len() != 3 || invoke_args[1] != x_register || invoke_args[2] != y_register {
        return Ok(None);
    }
    let transformer_register = invoke_args[0];
    let transform_method_index = u32::from(ins[9]);

    if dex_opcode(ins[11]) != 0x0b {
        return Ok(None);
    }
    let packed_register = dex_high_byte(ins[11]);
    if dex_opcode(ins[12]) != 0x13 || ins[13] as i16 != 32 {
        return Ok(None);
    }
    let shift_register = dex_high_byte(ins[12]);
    let Some((high_long_register, packed_source, shift_source)) = dex_23x(ins[14], ins[15], 0xa4)
    else {
        return Ok(None);
    };
    if packed_source != packed_register || shift_source != shift_register {
        return Ok(None);
    }
    let Some((high_int_register, high_long_source)) = dex_12x(ins[16], 0x84) else {
        return Ok(None);
    };
    if high_long_source != high_long_register {
        return Ok(None);
    }
    if invoke_static_one_arg(
        program,
        dex_file_index,
        ins,
        17,
        "Ljava/lang/Float;",
        "intBitsToFloat",
        "(I)F",
    )? != Some(high_int_register)
    {
        return Ok(None);
    }
    if dex_opcode(ins[20]) != 0x0a || dex_high_byte(ins[20]) != high_int_register {
        return Ok(None);
    }
    if dex_23x(ins[21], ins[22], 0x4b) != Some((high_int_register, array_register, index_register))
    {
        return Ok(None);
    }

    if dex_opcode(ins[23]) != 0x18
        || ins[24] != 0xffff
        || ins[25] != 0xffff
        || ins[26] != 0
        || ins[27] != 0
    {
        return Ok(None);
    }
    let mask_register = dex_high_byte(ins[23]);
    if dex_12x(ins[28], 0xc0) != Some((mask_register, packed_register)) {
        return Ok(None);
    }
    let Some((low_int_register, low_long_source)) = dex_12x(ins[29], 0x84) else {
        return Ok(None);
    };
    if low_long_source != mask_register {
        return Ok(None);
    }
    if invoke_static_one_arg(
        program,
        dex_file_index,
        ins,
        30,
        "Ljava/lang/Float;",
        "intBitsToFloat",
        "(I)F",
    )? != Some(low_int_register)
    {
        return Ok(None);
    }
    if dex_opcode(ins[33]) != 0x0a || dex_high_byte(ins[33]) != low_int_register {
        return Ok(None);
    }
    if dex_23x(ins[34], ins[35], 0x4b)
        != Some((low_int_register, array_register, next_index_register))
    {
        return Ok(None);
    }
    if ins[36] != 0x000e {
        return Ok(None);
    }

    let Some(receiver_arg) = parameter_word_index(code, receiver_register) else {
        return Ok(None);
    };
    let Some(transformer_arg) = parameter_word_index(code, transformer_register) else {
        return Ok(None);
    };
    let Some(index_arg) = parameter_word_index(code, index_register) else {
        return Ok(None);
    };
    if !field_ref_type_matches(program, dex_file_index, array_field_index, "[F")? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::FloatArrayPairTransform {
        receiver_arg,
        transformer_arg,
        index_arg,
        array_field_index,
        transform_method_index,
        transform_is_interface,
    }))
}

fn detect_float_rem_add_rem_intrinsic(code: &DexCodeItem) -> Option<MethodIntrinsic> {
    let ins = &code.instructions;
    if ins.len() != 4 {
        return None;
    }
    let (left_register, right_register) = dex_12x(ins[0], 0xca)?;
    if dex_12x(ins[1], 0xc6) != Some((left_register, right_register)) {
        return None;
    }
    if dex_12x(ins[2], 0xca) != Some((left_register, right_register)) {
        return None;
    }
    if dex_opcode(ins[3]) != 0x0f || dex_high_byte(ins[3]) != left_register {
        return None;
    }

    Some(MethodIntrinsic::FloatRemAddRem {
        left_arg: parameter_word_index(code, left_register)?,
        right_arg: parameter_word_index(code, right_register)?,
    })
}

fn detect_float_lerp_intrinsic(code: &DexCodeItem) -> Option<MethodIntrinsic> {
    let ins = &code.instructions;
    if ins.len() != 7 {
        return None;
    }
    if dex_opcode(ins[0]) != 0x12 || dex_nibble_b(ins[0]) != 1 {
        return None;
    }
    let temp_register = dex_nibble_a(ins[0]);
    if dex_12x(ins[1], 0x82) != Some((temp_register, temp_register)) {
        return None;
    }
    let Some((temp_sub_register, fraction_register)) = dex_12x(ins[2], 0xc7) else {
        return None;
    };
    if temp_sub_register != temp_register {
        return None;
    }
    let Some((temp_mul_register, start_register)) = dex_12x(ins[3], 0xc8) else {
        return None;
    };
    if temp_mul_register != temp_register {
        return None;
    }
    let Some((fraction_mul_register, end_register)) = dex_12x(ins[4], 0xc8) else {
        return None;
    };
    if fraction_mul_register != fraction_register {
        return None;
    }
    if dex_12x(ins[5], 0xc6) != Some((fraction_register, temp_register)) {
        return None;
    }
    if dex_opcode(ins[6]) != 0x0f || dex_high_byte(ins[6]) != fraction_register {
        return None;
    }

    Some(MethodIntrinsic::FloatLerp {
        start_arg: parameter_word_index(code, start_register)?,
        end_arg: parameter_word_index(code, end_register)?,
        fraction_arg: parameter_word_index(code, fraction_register)?,
    })
}

fn detect_float_atan2_normalize_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 12 {
        return Ok(None);
    }
    let Some((wide_y, y_register)) = dex_12x(ins[0], 0x89) else {
        return Ok(None);
    };
    let Some((wide_x, x_register)) = dex_12x(ins[1], 0x89) else {
        return Ok(None);
    };
    let Some(y_arg) = parameter_word_index(code, y_register) else {
        return Ok(None);
    };
    let Some(x_arg) = parameter_word_index(code, x_register) else {
        return Ok(None);
    };
    let Some(args) = invoke_static_args(
        program,
        dex_file_index,
        ins,
        2,
        "Ljava/lang/Math;",
        "atan2",
        "(DD)D",
    )?
    else {
        return Ok(None);
    };
    if args != [wide_y, wide_y + 1, wide_x, wide_x + 1] {
        return Ok(None);
    }
    if dex_opcode(ins[5]) != 0x0b {
        return Ok(None);
    }
    let result_wide = dex_high_byte(ins[5]);
    if dex_12x(ins[6], 0x8c) != Some((result_wide, result_wide)) {
        return Ok(None);
    }
    if dex_opcode(ins[7]) != 0x60 {
        return Ok(None);
    }
    let limit_register = dex_high_byte(ins[7]);
    let limit_field_index = u32::from(ins[8]);
    if dex_12x(ins[9], 0xc6) != Some((result_wide, limit_register)) {
        return Ok(None);
    }
    if dex_12x(ins[10], 0xca) != Some((result_wide, limit_register)) {
        return Ok(None);
    }
    if dex_opcode(ins[11]) != 0x0f || dex_high_byte(ins[11]) != result_wide {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::FloatAtan2Normalize {
        x_arg,
        y_arg,
        limit_field_index,
    }))
}

fn detect_cubic_float_array_pack_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 62 {
        return Ok(None);
    }
    let fixed = [
        (0, 0x1012),
        (1, 0x0182),
        (2, 0xa1c7),
        (3, 0x0212),
        (4, 0x9354),
        (6, 0x0244),
        (7, 0x0203),
        (8, 0x04a8),
        (9, 0x0101),
        (10, 0x14c8),
        (11, 0x42c8),
        (12, 0x2512),
        (13, 0x0544),
        (14, 0x0503),
        (15, 0x3612),
        (16, 0x6782),
        (17, 0xa7c8),
        (18, 0x08a8),
        (19, 0x0107),
        (20, 0x18c8),
        (21, 0x85c8),
        (22, 0x25c6),
        (23, 0x4212),
        (24, 0x0244),
        (25, 0x0203),
        (26, 0xa7c8),
        (27, 0x17c8),
        (28, 0x72c8),
        (29, 0x52c6),
        (30, 0x106e),
        (32, 0x0009),
        (33, 0x010a),
        (34, 0x05a8),
        (35, 0x0a0a),
        (36, 0xa5c8),
        (37, 0x51c8),
        (38, 0x21c6),
        (39, 0x0a44),
        (40, 0x0003),
        (41, 0x4ac8),
        (42, 0x0044),
        (43, 0x0603),
        (44, 0x80c8),
        (45, 0xa0c6),
        (46, 0x5a12),
        (47, 0x0a44),
        (48, 0x0a03),
        (49, 0x7ac8),
        (50, 0x0ac6),
        (51, 0x106e),
        (53, 0x0009),
        (54, 0x000a),
        (55, 0x50c8),
        (56, 0xa0c6),
        (57, 0x2071),
        (59, 0x0001),
        (60, 0x000b),
        (61, 0x0010),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return Ok(None);
        }
    }
    let Some((_, receiver_register)) = dex_12x(ins[4], 0x54) else {
        return Ok(None);
    };
    let Some((_, t_register)) = dex_12x(ins[2], 0xc7) else {
        return Ok(None);
    };
    let Some(receiver_arg) = parameter_word_index(code, receiver_register) else {
        return Ok(None);
    };
    let Some(t_arg) = parameter_word_index(code, t_register) else {
        return Ok(None);
    };
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 30, 0x6e, "()F")?
        != Some(vec![receiver_register])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 51, 0x6e, "()F")?
        != Some(vec![receiver_register])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 57, 0x71, "(FF)J")?
        != Some(vec![1, 0])
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::CubicFloatArrayPack {
        receiver_arg,
        t_arg,
        field_index: u32::from(ins[5]),
    }))
}

fn detect_path_angle_delta_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let ins = &code.instructions;
    if ins.len() != 53 {
        return Ok(None);
    }
    let fixed = [
        (0, 0x001a),
        (2, 0x4154),
        (4, 0x2071),
        (6, 0x0001),
        (7, 0x001a),
        (9, 0x4254),
        (11, 0x2071),
        (13, 0x0002),
        (14, 0x206e),
        (16, 0x0051),
        (17, 0x000b),
        (18, 0x2071),
        (20, 0x0010),
        (21, 0x050a),
        (22, 0x2352),
        (24, 0x35c7),
        (25, 0x2071),
        (27, 0x0010),
        (28, 0x000a),
        (29, 0x2152),
        (31, 0x10c7),
        (32, 0x2071),
        (34, 0x0005),
        (35, 0x050a),
        (36, 0x4052),
        (38, 0x05c7),
        (39, 0x0060),
        (41, 0x2071),
        (43, 0x0005),
        (44, 0x050a),
        (45, 0x4052),
        (47, 0x05c7),
        (48, 0x1071),
        (50, 0x0005),
        (51, 0x050a),
        (52, 0x050f),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return Ok(None);
        }
    }
    let Some((curve_register, receiver_register)) = dex_12x(ins[2], 0x54) else {
        return Ok(None);
    };
    let Some((anchor_register, receiver_register_2)) = dex_12x(ins[9], 0x54) else {
        return Ok(None);
    };
    if receiver_register != receiver_register_2 {
        return Ok(None);
    }
    let t_register = (ins[16] >> 4) & 0x000f;
    let Some(receiver_arg) = parameter_word_index(code, receiver_register) else {
        return Ok(None);
    };
    let Some(t_arg) = parameter_word_index(code, t_register) else {
        return Ok(None);
    };
    if invoke_35c_args_matching_proto(
        program,
        dex_file_index,
        ins,
        4,
        0x71,
        "(Ljava/lang/Object;Ljava/lang/String;)V",
    )? != Some(vec![curve_register, 0])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(
        program,
        dex_file_index,
        ins,
        11,
        0x71,
        "(Ljava/lang/Object;Ljava/lang/String;)V",
    )? != Some(vec![anchor_register, 0])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 14, 0x6e, "(F)J")?
        != Some(vec![curve_register, t_register])
    {
        return Ok(None);
    }
    let curve_array_field_index =
        match method_ref_intrinsic(program, dex_file_index, u32::from(ins[15]))? {
            Some(MethodIntrinsic::CubicFloatArrayPack { field_index, .. }) => field_index,
            _ => return Ok(None),
        };
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 18, 0x71, "(J)F")?
        != Some(vec![0, 1])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 25, 0x71, "(J)F")?
        != Some(vec![0, 1])
    {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 32, 0x71, "(FF)F")?
        != Some(vec![5, 0])
    {
        return Ok(None);
    }
    if !matches!(
        method_ref_intrinsic(program, dex_file_index, u32::from(ins[33]))?,
        Some(MethodIntrinsic::FloatAtan2Normalize { .. })
    ) {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 41, 0x71, "(FF)F")?
        != Some(vec![5, 0])
    {
        return Ok(None);
    }
    if !matches!(
        method_ref_intrinsic(program, dex_file_index, u32::from(ins[42]))?,
        Some(MethodIntrinsic::FloatRemAddRem { .. })
    ) {
        return Ok(None);
    }
    if invoke_35c_args_matching_proto(program, dex_file_index, ins, 48, 0x71, "(F)F")?
        != Some(vec![5])
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        dex_file_index,
        u32::from(ins[49]),
        "Ljava/lang/Math;",
        "abs",
        "(F)F",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::PathAngleDelta {
        receiver_arg,
        t_arg,
        curve_field_index: u32::from(ins[3]),
        curve_array_field_index,
        anchor_field_index: u32::from(ins[10]),
        anchor_x_field_index: u32::from(ins[23]),
        anchor_y_field_index: u32::from(ins[30]),
        start_angle_field_index: u32::from(ins[37]),
        offset_field_index: u32::from(ins[46]),
        limit_field_index: u32::from(ins[40]),
    }))
}

fn detect_cubic_transform_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_cubic_transform_shape(code) else {
        return Ok(None);
    };
    let mutable_cubic_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.mutable_cubic_type_index)?;
    if mutable_cubic_descriptor != resolved.method.proto.return_type {
        return Ok(None);
    }
    let float_array_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.float_array_type_index)?;
    if float_array_descriptor != "[F" {
        return Ok(None);
    }
    let points_field = program.field_id(resolved.dex_file_index, shape.points_field_index)?;
    if points_field.type_descriptor != "[F" {
        return Ok(None);
    }
    let constructor = program.method_id(resolved.dex_file_index, shape.constructor_method_index)?;
    if constructor.class_descriptor != points_field.class_descriptor
        || constructor.name != "<init>"
        || !constructor.proto.descriptor_is("([F)V")
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.arraycopy_method_index,
        "Ljava/lang/System;",
        "arraycopy",
        "(Ljava/lang/Object;ILjava/lang/Object;II)V",
    )? {
        return Ok(None);
    }
    let transform_point =
        program.method_id(resolved.dex_file_index, shape.transform_point_method_index)?;
    if transform_point.class_descriptor != resolved.method.proto.return_type
        || !transform_point
            .proto
            .descriptor_is(&format!("({}I)V", resolved.method.proto.parameters[0]))
    {
        return Ok(None);
    }
    let Some(transform_point_resolved) = program.resolve_exact_method(&transform_point)? else {
        return Ok(None);
    };
    let transform_point_code = program.code_item(&transform_point_resolved)?;
    let Some(transform_point_shape) =
        detect_mutable_cubic_transform_point_shape(&transform_point_code)
    else {
        return Ok(None);
    };
    if transform_point_shape.points_field_index != shape.points_field_index {
        return Ok(None);
    }
    if !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        transform_point_shape.transform_method_index,
        "(FF)J",
    )? {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        transform_point_shape.int_bits_to_float_method_index,
        "Ljava/lang/Float;",
        "intBitsToFloat",
        "(I)F",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::CubicTransform {
        receiver_arg: 0,
        transformer_arg: 1,
        points_field_index: shape.points_field_index,
        mutable_cubic_type_index: shape.mutable_cubic_type_index,
        transform_method_index: transform_point_shape.transform_method_index,
    }))
}

#[derive(Clone, Copy)]
struct CubicTransformShape {
    mutable_cubic_type_index: u32,
    float_array_type_index: u32,
    constructor_method_index: u32,
    points_field_index: u32,
    arraycopy_method_index: u32,
    transform_point_method_index: u32,
}

// cite: androidx Cubic.kt#transformed
fn detect_cubic_transform_shape(code: &DexCodeItem) -> Option<CubicTransformShape> {
    let ins = &code.instructions;
    if code.registers_size != 7 || code.ins_size != 2 || code.outs_size != 5 || ins.len() != 32 {
        return None;
    }
    let fixed = [
        (0, 0x0022),
        (2, 0x0113),
        (3, 0x0008),
        (4, 0x1123),
        (6, 0x2070),
        (8, 0x0010),
        (9, 0x5254),
        (11, 0x2321),
        (12, 0x0412),
        (13, 0x5371),
        (15, 0x4142),
        (16, 0x306e),
        (18, 0x0460),
        (19, 0x2112),
        (20, 0x306e),
        (22, 0x0160),
        (23, 0x4112),
        (24, 0x306e),
        (26, 0x0160),
        (27, 0x6112),
        (28, 0x306e),
        (30, 0x0160),
        (31, 0x0011),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[17] != ins[21] || ins[17] != ins[25] || ins[17] != ins[29] {
        return None;
    }

    Some(CubicTransformShape {
        mutable_cubic_type_index: u32::from(ins[1]),
        float_array_type_index: u32::from(ins[5]),
        constructor_method_index: u32::from(ins[7]),
        points_field_index: u32::from(ins[10]),
        arraycopy_method_index: u32::from(ins[14]),
        transform_point_method_index: u32::from(ins[17]),
    })
}

#[derive(Clone, Copy)]
struct MutableCubicTransformPointShape {
    points_field_index: u32,
    transform_method_index: u32,
    int_bits_to_float_method_index: u32,
}

// cite: androidx Cubic.kt#transformOnePoint
fn detect_mutable_cubic_transform_point_shape(
    code: &DexCodeItem,
) -> Option<MutableCubicTransformPointShape> {
    let ins = &code.instructions;
    if code.registers_size != 10 || code.ins_size != 3 || code.outs_size != 3 || ins.len() != 37 {
        return None;
    }
    let fixed = [
        (0, 0x7054),
        (2, 0x0144),
        (3, 0x0900),
        (4, 0x02d8),
        (5, 0x0109),
        (6, 0x0344),
        (7, 0x0200),
        (8, 0x3072),
        (10, 0x0318),
        (11, 0x030b),
        (12, 0x0813),
        (13, 0x0020),
        (14, 0x05a4),
        (15, 0x0803),
        (16, 0x5884),
        (17, 0x1071),
        (19, 0x0008),
        (20, 0x080a),
        (21, 0x084b),
        (22, 0x0900),
        (23, 0x0818),
        (24, 0xffff),
        (25, 0xffff),
        (26, 0x0000),
        (27, 0x0000),
        (28, 0x38c0),
        (29, 0x8884),
        (30, 0x1071),
        (32, 0x0008),
        (33, 0x080a),
        (34, 0x084b),
        (35, 0x0200),
        (36, 0x000e),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[18] != ins[31] {
        return None;
    }

    Some(MutableCubicTransformPointShape {
        points_field_index: u32::from(ins[1]),
        transform_method_index: u32::from(ins[9]),
        int_bits_to_float_method_index: u32::from(ins[18]),
    })
}

fn detect_feature_edge_transform_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_feature_edge_transform_shape(code) else {
        return Ok(None);
    };

    let cubics_field = program.field_id(resolved.dex_file_index, shape.cubics_field_index)?;
    if cubics_field.type_descriptor != "Ljava/util/List;" {
        return Ok(None);
    }
    let edge_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.edge_type_index)?;
    if !program.descriptor_matches_type(&edge_descriptor, &resolved.method.proto.return_type) {
        return Ok(None);
    }
    let cubic_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.cubic_type_index)?;
    let cubic_transform =
        program.method_id(resolved.dex_file_index, shape.cubic_transform_method_index)?;
    if cubic_transform.class_descriptor != cubic_descriptor
        || cubic_transform.proto.parameters != resolved.method.proto.parameters
        || !cubic_transform.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let Some(cubic_transform_resolved) = program.resolve_exact_method(&cubic_transform)? else {
        return Ok(None);
    };
    let cubic_transform_code = program.code_item(&cubic_transform_resolved)?;
    if detect_cubic_transform_intrinsic(program, &cubic_transform_resolved, &cubic_transform_code)?
        .is_none()
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_size_method_index,
        "Ljava/util/List;",
        "size",
        "()I",
    )? {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_get_method_index,
        "Ljava/util/List;",
        "get",
        "(I)Ljava/lang/Object;",
    )? {
        return Ok(None);
    }
    let Some(list_fields) = detect_object_array_append_fields_from_method_ref(
        program,
        resolved.dex_file_index,
        shape.list_add_method_index,
    )?
    else {
        return Ok(None);
    };
    let list_factory =
        program.method_id(resolved.dex_file_index, shape.list_factory_method_index)?;
    if !list_factory.proto.parameters.is_empty() || !list_factory.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let list_freeze = program.method_id(resolved.dex_file_index, shape.list_freeze_method_index)?;
    if list_freeze.proto.parameters.len() != 1
        || list_freeze.proto.parameters[0] != list_factory.proto.return_type
        || list_freeze.proto.return_type != list_factory.proto.return_type
    {
        return Ok(None);
    }
    let constructor = program.method_id(resolved.dex_file_index, shape.constructor_method_index)?;
    if constructor.class_descriptor != cubics_field.class_descriptor
        || constructor.name != "<init>"
        || !constructor.proto.descriptor_is("(Ljava/util/List;)V")
    {
        return Ok(None);
    }
    if program.string_value(resolved.dex_file_index, shape.cubics_string_index)? != "cubics" {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::FeatureEdgeTransform {
        receiver_arg: 0,
        transformer_arg: 1,
        cubics_field_index: shape.cubics_field_index,
        edge_type_index: shape.edge_type_index,
        cubic_transform_method_index: shape.cubic_transform_method_index,
        list_freeze_method_index: shape.list_freeze_method_index,
        list_array_field_index: list_fields.array_field_index,
        list_size_field_index: list_fields.size_field_index,
        list_read_only_field_index: list_fields.read_only_field_index,
        list_mod_count_field_index: list_fields.mod_count_field_index,
    }))
}

#[derive(Clone, Copy)]
struct FeatureEdgeTransformShape {
    list_factory_method_index: u32,
    cubics_field_index: u32,
    list_size_method_index: u32,
    list_get_method_index: u32,
    cubic_type_index: u32,
    cubic_transform_method_index: u32,
    list_add_method_index: u32,
    list_freeze_method_index: u32,
    edge_type_index: u32,
    cubics_string_index: u32,
    constructor_method_index: u32,
}

// cite: androidx Features.kt#Edge, transformed
fn detect_feature_edge_transform_shape(code: &DexCodeItem) -> Option<FeatureEdgeTransformShape> {
    let ins = &code.instructions;
    if code.registers_size != 7 || code.ins_size != 2 || code.outs_size != 2 || ins.len() != 44 {
        return None;
    }
    let fixed = [
        (0, 0x0071),
        (2, 0x0000),
        (3, 0x000c),
        (4, 0x5154),
        (6, 0x1072),
        (8, 0x0001),
        (9, 0x020a),
        (10, 0x0312),
        (11, 0x2335),
        (12, 0x0012),
        (13, 0x2072),
        (15, 0x0031),
        (16, 0x040c),
        (17, 0x041f),
        (19, 0x206e),
        (21, 0x0064),
        (22, 0x040c),
        (23, 0x206e),
        (25, 0x0040),
        (26, 0x03d8),
        (27, 0x0103),
        (28, 0xef28),
        (29, 0x1071),
        (31, 0x0000),
        (32, 0x060c),
        (33, 0x0022),
        (35, 0x011a),
        (37, 0x2071),
        (39, 0x0016),
        (40, 0x2070),
        (42, 0x0060),
        (43, 0x0011),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }

    Some(FeatureEdgeTransformShape {
        list_factory_method_index: u32::from(ins[1]),
        cubics_field_index: u32::from(ins[5]),
        list_size_method_index: u32::from(ins[7]),
        list_get_method_index: u32::from(ins[14]),
        cubic_type_index: u32::from(ins[18]),
        cubic_transform_method_index: u32::from(ins[20]),
        list_add_method_index: u32::from(ins[24]),
        list_freeze_method_index: u32::from(ins[30]),
        edge_type_index: u32::from(ins[34]),
        cubics_string_index: u32::from(ins[36]),
        constructor_method_index: u32::from(ins[41]),
    })
}

fn detect_feature_corner_transform_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_feature_corner_transform_shape(code) else {
        return Ok(None);
    };

    let cubics_field = program.field_id(resolved.dex_file_index, shape.cubics_field_index)?;
    if cubics_field.type_descriptor != "Ljava/util/List;" {
        return Ok(None);
    }
    let corner_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.corner_type_index)?;
    if !program.descriptor_matches_type(&corner_descriptor, &resolved.method.proto.return_type) {
        return Ok(None);
    }
    let vertex_field = program.field_id(resolved.dex_file_index, shape.vertex_field_index)?;
    let rounded_center_field =
        program.field_id(resolved.dex_file_index, shape.rounded_center_field_index)?;
    let convex_field = program.field_id(resolved.dex_file_index, shape.convex_field_index)?;
    if vertex_field.class_descriptor != corner_descriptor
        || rounded_center_field.class_descriptor != corner_descriptor
        || convex_field.class_descriptor != corner_descriptor
        || vertex_field.type_descriptor != "J"
        || rounded_center_field.type_descriptor != "J"
        || convex_field.type_descriptor != "Z"
    {
        return Ok(None);
    }
    let cubic_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.cubic_type_index)?;
    let cubic_transform =
        program.method_id(resolved.dex_file_index, shape.cubic_transform_method_index)?;
    if cubic_transform.class_descriptor != cubic_descriptor
        || cubic_transform.proto.parameters != resolved.method.proto.parameters
        || !cubic_transform.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let Some(cubic_transform_resolved) = program.resolve_exact_method(&cubic_transform)? else {
        return Ok(None);
    };
    let cubic_transform_code = program.code_item(&cubic_transform_resolved)?;
    if detect_cubic_transform_intrinsic(program, &cubic_transform_resolved, &cubic_transform_code)?
        .is_none()
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_size_method_index,
        "Ljava/util/List;",
        "size",
        "()I",
    )? {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_get_method_index,
        "Ljava/util/List;",
        "get",
        "(I)Ljava/lang/Object;",
    )? {
        return Ok(None);
    }
    let Some(list_fields) = detect_object_array_append_fields_from_method_ref(
        program,
        resolved.dex_file_index,
        shape.list_add_method_index,
    )?
    else {
        return Ok(None);
    };
    let list_factory =
        program.method_id(resolved.dex_file_index, shape.list_factory_method_index)?;
    if !list_factory.proto.parameters.is_empty() || !list_factory.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let list_freeze = program.method_id(resolved.dex_file_index, shape.list_freeze_method_index)?;
    if list_freeze.proto.parameters.len() != 1
        || list_freeze.proto.parameters[0] != list_factory.proto.return_type
        || list_freeze.proto.return_type != list_factory.proto.return_type
    {
        return Ok(None);
    }
    let point_transform =
        program.method_id(resolved.dex_file_index, shape.point_transform_method_index)?;
    if point_transform.proto.parameters.len() != 2
        || point_transform.proto.parameters[0] != "J"
        || point_transform.proto.parameters[1] != resolved.method.proto.parameters[0]
        || point_transform.proto.return_type != "J"
    {
        return Ok(None);
    }
    let Some(point_transform_resolved) = program.resolve_exact_method(&point_transform)? else {
        return Ok(None);
    };
    let point_transform_code = program.code_item(&point_transform_resolved)?;
    let Some(point_transform_shape) = detect_packed_point_transform_shape(&point_transform_code)
    else {
        return Ok(None);
    };
    if !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        point_transform_shape.x_method_index,
        "(J)F",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        point_transform_shape.y_method_index,
        "(J)F",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        point_transform_shape.transform_method_index,
        "(FF)J",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        point_transform_shape.int_bits_to_float_method_index,
        "Ljava/lang/Float;",
        "intBitsToFloat",
        "(I)F",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        point_transform_shape.pack_method_index,
        "(FF)J",
    )? {
        return Ok(None);
    }
    let constructor = program.method_id(resolved.dex_file_index, shape.constructor_method_index)?;
    if constructor.class_descriptor != corner_descriptor
        || constructor.name != "<init>"
        || !constructor.proto.descriptor_is("(Ljava/util/List;JJZ)V")
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::FeatureCornerTransform {
        receiver_arg: 0,
        transformer_arg: 1,
        cubics_field_index: shape.cubics_field_index,
        vertex_field_index: shape.vertex_field_index,
        rounded_center_field_index: shape.rounded_center_field_index,
        convex_field_index: shape.convex_field_index,
        corner_type_index: shape.corner_type_index,
        cubic_transform_method_index: shape.cubic_transform_method_index,
        point_transform_method_index: point_transform_shape.transform_method_index,
        list_freeze_method_index: shape.list_freeze_method_index,
        list_array_field_index: list_fields.array_field_index,
        list_size_field_index: list_fields.size_field_index,
        list_read_only_field_index: list_fields.read_only_field_index,
        list_mod_count_field_index: list_fields.mod_count_field_index,
    }))
}

#[derive(Clone, Copy)]
struct FeatureCornerTransformShape {
    list_factory_method_index: u32,
    cubics_field_index: u32,
    list_size_method_index: u32,
    list_get_method_index: u32,
    cubic_type_index: u32,
    cubic_transform_method_index: u32,
    list_add_method_index: u32,
    list_freeze_method_index: u32,
    vertex_field_index: u32,
    point_transform_method_index: u32,
    rounded_center_field_index: u32,
    corner_type_index: u32,
    convex_field_index: u32,
    constructor_method_index: u32,
}

// cite: androidx Features.kt#Corner, transformed
fn detect_feature_corner_transform_shape(
    code: &DexCodeItem,
) -> Option<FeatureCornerTransformShape> {
    let ins = &code.instructions;
    if code.registers_size != 13 || code.ins_size != 2 || code.outs_size != 7 || ins.len() != 53 {
        return None;
    }
    let fixed = [
        (0, 0x0071),
        (2, 0x0000),
        (3, 0x000c),
        (4, 0xb154),
        (6, 0x1072),
        (8, 0x0001),
        (9, 0x020a),
        (10, 0x0312),
        (11, 0x2335),
        (12, 0x0012),
        (13, 0x2072),
        (15, 0x0031),
        (16, 0x040c),
        (17, 0x041f),
        (19, 0x206e),
        (21, 0x00c4),
        (22, 0x040c),
        (23, 0x206e),
        (25, 0x0040),
        (26, 0x03d8),
        (27, 0x0103),
        (28, 0xef28),
        (29, 0x1071),
        (31, 0x0000),
        (32, 0x050c),
        (33, 0xb053),
        (35, 0x3071),
        (37, 0x0c10),
        (38, 0x060b),
        (39, 0xb053),
        (41, 0x3071),
        (43, 0x0c10),
        (44, 0x080b),
        (45, 0x0422),
        (47, 0xba55),
        (49, 0x0776),
        (51, 0x0004),
        (52, 0x0411),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[36] != ins[42] {
        return None;
    }

    Some(FeatureCornerTransformShape {
        list_factory_method_index: u32::from(ins[1]),
        cubics_field_index: u32::from(ins[5]),
        list_size_method_index: u32::from(ins[7]),
        list_get_method_index: u32::from(ins[14]),
        cubic_type_index: u32::from(ins[18]),
        cubic_transform_method_index: u32::from(ins[20]),
        list_add_method_index: u32::from(ins[24]),
        list_freeze_method_index: u32::from(ins[30]),
        vertex_field_index: u32::from(ins[34]),
        point_transform_method_index: u32::from(ins[36]),
        rounded_center_field_index: u32::from(ins[40]),
        corner_type_index: u32::from(ins[46]),
        convex_field_index: u32::from(ins[48]),
        constructor_method_index: u32::from(ins[50]),
    })
}

fn detect_feature_distance_squared_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_feature_distance_squared_shape(code) else {
        return Ok(None);
    };
    if resolved.method.proto.parameters.len() != 2
        || resolved.method.proto.parameters[0] != resolved.method.proto.parameters[1]
        || resolved.method.proto.return_type != "F"
    {
        return Ok(None);
    }
    if program.string_value(resolved.dex_file_index, shape.left_string_index)? != "f1"
        || program.string_value(resolved.dex_file_index, shape.right_string_index)? != "f2"
    {
        return Ok(None);
    }
    let feature_descriptor = &resolved.method.proto.parameters[0];
    let corner_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.corner_type_index)?;
    let cubic_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.cubic_type_index)?;
    if !program.descriptor_matches_type(&corner_descriptor, feature_descriptor) {
        return Ok(None);
    }
    let convex_field = program.field_id(resolved.dex_file_index, shape.convex_field_index)?;
    let cubics_field = program.field_id(resolved.dex_file_index, shape.cubics_field_index)?;
    let cubic_points_field =
        program.field_id(resolved.dex_file_index, shape.cubic_points_field_index)?;
    if convex_field.class_descriptor != corner_descriptor
        || convex_field.type_descriptor != "Z"
        || cubics_field.class_descriptor.as_str() != feature_descriptor.as_str()
        || cubics_field.type_descriptor != "Ljava/util/List;"
        || cubic_points_field.class_descriptor != cubic_descriptor
        || cubic_points_field.type_descriptor != "[F"
    {
        return Ok(None);
    }
    if !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        shape.null_check_method_index,
        "(Ljava/lang/Object;Ljava/lang/String;)V",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        shape.list_first_method_index,
        "(Ljava/util/List;)Ljava/lang/Object;",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        shape.list_last_method_index,
        "(Ljava/util/List;)Ljava/lang/Object;",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_anchor_x_method_index,
        &cubic_descriptor,
        "a",
        "()F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_anchor_y_method_index,
        &cubic_descriptor,
        "b",
        "()F",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::FeatureDistanceSquared {
        left_arg: 0,
        right_arg: 1,
        corner_type_index: shape.corner_type_index,
        convex_field_index: shape.convex_field_index,
        cubics_field_index: shape.cubics_field_index,
        cubic_points_field_index: shape.cubic_points_field_index,
    }))
}

#[derive(Clone, Copy)]
struct FeatureDistanceSquaredShape {
    left_string_index: u32,
    right_string_index: u32,
    null_check_method_index: u32,
    corner_type_index: u32,
    convex_field_index: u32,
    cubics_field_index: u32,
    cubic_type_index: u32,
    cubic_points_field_index: u32,
    list_first_method_index: u32,
    list_last_method_index: u32,
    cubic_anchor_x_method_index: u32,
    cubic_anchor_y_method_index: u32,
}

// cite: androidx FeatureMapping.kt#featureDistSquared, featureRepresentativePoint
// cite: androidx Point.kt#getDistanceSquared
fn detect_feature_distance_squared_shape(
    code: &DexCodeItem,
) -> Option<FeatureDistanceSquaredShape> {
    let ins = &code.instructions;
    if code.registers_size != 7 || code.ins_size != 2 || code.outs_size != 2 || ins.len() != 136 {
        return None;
    }
    let fixed = [
        (0, 0x001a),
        (2, 0x2071),
        (4, 0x0005),
        (5, 0x001a),
        (7, 0x2071),
        (9, 0x0006),
        (10, 0x5020),
        (12, 0x0038),
        (13, 0x0016),
        (14, 0x6020),
        (16, 0x0038),
        (17, 0x0012),
        (18, 0x5007),
        (19, 0x001f),
        (21, 0x6107),
        (22, 0x011f),
        (24, 0x0055),
        (26, 0x1155),
        (28, 0x1032),
        (29, 0x0006),
        (30, 0x0514),
        (31, 0xffff),
        (32, 0x7f7f),
        (33, 0x050f),
        (34, 0x5554),
        (36, 0x1071),
        (38, 0x0005),
        (39, 0x000c),
        (40, 0x001f),
        (42, 0x0054),
        (44, 0x0112),
        (45, 0x0044),
        (46, 0x0100),
        (47, 0x1071),
        (49, 0x0005),
        (50, 0x020c),
        (51, 0x021f),
        (53, 0x106e),
        (55, 0x0002),
        (56, 0x020a),
        (57, 0x02c6),
        (58, 0x0015),
        (59, 0x4000),
        (60, 0x02c9),
        (61, 0x1071),
        (63, 0x0005),
        (64, 0x030c),
        (65, 0x031f),
        (67, 0x3354),
        (69, 0x1412),
        (70, 0x0344),
        (71, 0x0403),
        (72, 0x1071),
        (74, 0x0005),
        (75, 0x050c),
        (76, 0x051f),
        (78, 0x106e),
        (80, 0x0005),
        (81, 0x050a),
        (82, 0x35c6),
        (83, 0x05c9),
        (84, 0x6654),
        (86, 0x1071),
        (88, 0x0006),
        (89, 0x030c),
        (90, 0x031f),
        (92, 0x3354),
        (94, 0x0144),
        (95, 0x0103),
        (96, 0x1071),
        (98, 0x0006),
        (99, 0x030c),
        (100, 0x031f),
        (102, 0x106e),
        (104, 0x0003),
        (105, 0x030a),
        (106, 0x13c6),
        (107, 0x03c9),
        (108, 0x1071),
        (110, 0x0006),
        (111, 0x010c),
        (112, 0x011f),
        (114, 0x1154),
        (116, 0x0144),
        (117, 0x0401),
        (118, 0x1071),
        (120, 0x0006),
        (121, 0x060c),
        (122, 0x061f),
        (124, 0x106e),
        (126, 0x0006),
        (127, 0x060a),
        (128, 0x16c6),
        (129, 0x06c9),
        (130, 0x32c7),
        (131, 0x65c7),
        (132, 0x22c8),
        (133, 0x55c8),
        (134, 0x25c6),
        (135, 0x050f),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[3] != ins[8] {
        return None;
    }
    for index in [15, 20, 23] {
        if ins[index] != ins[11] {
            return None;
        }
    }
    if ins[27] != ins[25] || ins[85] != ins[35] {
        return None;
    }
    for index in [62, 87, 109] {
        if ins[index] != ins[37] {
            return None;
        }
    }
    for index in [52, 66, 77, 91, 101, 113, 123] {
        if ins[index] != ins[41] {
            return None;
        }
    }
    for index in [68, 93, 115] {
        if ins[index] != ins[43] {
            return None;
        }
    }
    for index in [73, 97, 119] {
        if ins[index] != ins[48] {
            return None;
        }
    }
    if ins[103] != ins[54] || ins[125] != ins[79] {
        return None;
    }

    Some(FeatureDistanceSquaredShape {
        left_string_index: u32::from(ins[1]),
        right_string_index: u32::from(ins[6]),
        null_check_method_index: u32::from(ins[3]),
        corner_type_index: u32::from(ins[11]),
        convex_field_index: u32::from(ins[25]),
        cubics_field_index: u32::from(ins[35]),
        cubic_type_index: u32::from(ins[41]),
        cubic_points_field_index: u32::from(ins[43]),
        list_first_method_index: u32::from(ins[37]),
        list_last_method_index: u32::from(ins[48]),
        cubic_anchor_x_method_index: u32::from(ins[54]),
        cubic_anchor_y_method_index: u32::from(ins[79]),
    })
}

#[derive(Clone, Copy)]
struct PackedPointTransformShape {
    x_method_index: u32,
    y_method_index: u32,
    transform_method_index: u32,
    int_bits_to_float_method_index: u32,
    pack_method_index: u32,
}

// cite: androidx Features.kt#Corner, transformed
fn detect_packed_point_transform_shape(code: &DexCodeItem) -> Option<PackedPointTransformShape> {
    let ins = &code.instructions;
    if code.registers_size != 5 || code.ins_size != 3 || code.outs_size != 3 || ins.len() != 37 {
        return None;
    }
    let fixed = [
        (0, 0x2071),
        (2, 0x0032),
        (3, 0x000a),
        (4, 0x2071),
        (6, 0x0032),
        (7, 0x020a),
        (8, 0x3072),
        (10, 0x0204),
        (11, 0x020b),
        (12, 0x0413),
        (13, 0x0020),
        (14, 0x00a4),
        (15, 0x0402),
        (16, 0x0484),
        (17, 0x1071),
        (19, 0x0004),
        (20, 0x040a),
        (21, 0x0018),
        (22, 0xffff),
        (23, 0xffff),
        (24, 0x0000),
        (25, 0x0000),
        (26, 0x02c0),
        (27, 0x2284),
        (28, 0x1071),
        (30, 0x0002),
        (31, 0x020a),
        (32, 0x2071),
        (34, 0x0024),
        (35, 0x020b),
        (36, 0x0210),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[18] != ins[29] {
        return None;
    }

    Some(PackedPointTransformShape {
        x_method_index: u32::from(ins[1]),
        y_method_index: u32::from(ins[5]),
        transform_method_index: u32::from(ins[9]),
        int_bits_to_float_method_index: u32::from(ins[18]),
        pack_method_index: u32::from(ins[33]),
    })
}

fn detect_rounded_polygon_transform_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_rounded_polygon_transform_shape(code) else {
        return Ok(None);
    };
    let features_field = program.field_id(resolved.dex_file_index, shape.features_field_index)?;
    let center_x_field = program.field_id(resolved.dex_file_index, shape.center_x_field_index)?;
    let center_y_field = program.field_id(resolved.dex_file_index, shape.center_y_field_index)?;
    if features_field.class_descriptor != resolved.class_descriptor
        || center_x_field.class_descriptor != resolved.class_descriptor
        || center_y_field.class_descriptor != resolved.class_descriptor
        || features_field.type_descriptor != "Ljava/util/AbstractList;"
        || center_x_field.type_descriptor != "F"
        || center_y_field.type_descriptor != "F"
    {
        return Ok(None);
    }
    let feature_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.feature_type_index)?;
    let feature_transform = program.method_id(
        resolved.dex_file_index,
        shape.feature_transform_method_index,
    )?;
    if feature_transform.class_descriptor != feature_descriptor
        || feature_transform.proto.parameters != resolved.method.proto.parameters
        || !program
            .descriptor_matches_type(&feature_transform.proto.return_type, &feature_descriptor)
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_size_method_index,
        "Ljava/util/List;",
        "size",
        "()I",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_get_method_index,
        "Ljava/util/List;",
        "get",
        "(I)Ljava/lang/Object;",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        shape.float_pack_method_index,
        "(FF)J",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        shape.center_x_method_index,
        "(J)F",
    )? || !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        shape.center_y_method_index,
        "(J)F",
    )? {
        return Ok(None);
    }
    let packed_point_transform = program.method_id(
        resolved.dex_file_index,
        shape.packed_point_transform_method_index,
    )?;
    let Some(point_transform_resolved) = program.resolve_exact_method(&packed_point_transform)?
    else {
        return Ok(None);
    };
    let point_transform_code = program.code_item(&point_transform_resolved)?;
    let Some(point_transform_shape) = detect_packed_point_transform_shape(&point_transform_code)
    else {
        return Ok(None);
    };
    if !method_ref_proto_matches(
        program,
        resolved.dex_file_index,
        point_transform_shape.transform_method_index,
        "(FF)J",
    )? {
        return Ok(None);
    }
    let Some(list_fields) = detect_object_array_append_fields_from_method_ref(
        program,
        resolved.dex_file_index,
        shape.list_add_method_index,
    )?
    else {
        return Ok(None);
    };
    let list_factory =
        program.method_id(resolved.dex_file_index, shape.list_factory_method_index)?;
    if !list_factory.proto.parameters.is_empty() || !list_factory.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let list_freeze = program.method_id(resolved.dex_file_index, shape.list_freeze_method_index)?;
    if list_freeze.proto.parameters.len() != 1
        || list_freeze.proto.parameters[0] != list_factory.proto.return_type
        || list_freeze.proto.return_type != list_factory.proto.return_type
    {
        return Ok(None);
    }
    let constructor = program.method_id(resolved.dex_file_index, shape.constructor_method_index)?;
    let polygon_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.polygon_type_index)?;
    if !program.descriptor_matches_type(&polygon_descriptor, &resolved.method.proto.return_type) {
        return Ok(None);
    }
    if constructor.class_descriptor != resolved.method.proto.return_type
        || constructor.name != "<init>"
        || !constructor
            .proto
            .descriptor_is("(Ljava/util/AbstractList;FF)V")
    {
        return Ok(None);
    }
    let Some(constructor_resolved) = program.resolve_exact_method(&constructor)? else {
        return Ok(None);
    };
    let constructor_code = program.code_item(&constructor_resolved)?;
    if detect_rounded_polygon_constructor_intrinsic(
        program,
        &constructor_resolved,
        &constructor_code,
    )?
    .is_none()
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::RoundedPolygonTransform {
        receiver_arg: 0,
        transformer_arg: 1,
        features_field_index: shape.features_field_index,
        center_x_field_index: shape.center_x_field_index,
        center_y_field_index: shape.center_y_field_index,
        feature_transform_method_index: shape.feature_transform_method_index,
        point_transform_method_index: point_transform_shape.transform_method_index,
        constructor_method_index: shape.constructor_method_index,
        list_freeze_method_index: shape.list_freeze_method_index,
        list_array_field_index: list_fields.array_field_index,
        list_size_field_index: list_fields.size_field_index,
        list_read_only_field_index: list_fields.read_only_field_index,
        list_mod_count_field_index: list_fields.mod_count_field_index,
    }))
}

#[derive(Clone, Copy)]
struct RoundedPolygonTransformShape {
    center_x_field_index: u32,
    center_y_field_index: u32,
    float_pack_method_index: u32,
    packed_point_transform_method_index: u32,
    list_factory_method_index: u32,
    features_field_index: u32,
    list_size_method_index: u32,
    list_get_method_index: u32,
    feature_type_index: u32,
    feature_transform_method_index: u32,
    list_add_method_index: u32,
    list_freeze_method_index: u32,
    center_x_method_index: u32,
    center_y_method_index: u32,
    polygon_type_index: u32,
    constructor_method_index: u32,
}

// cite: androidx RoundedPolygon.kt#transformed
fn detect_rounded_polygon_transform_shape(
    code: &DexCodeItem,
) -> Option<RoundedPolygonTransformShape> {
    let ins = &code.instructions;
    if code.registers_size != 9 || code.ins_size != 2 || code.outs_size != 4 || ins.len() != 59 {
        return None;
    }
    let fixed = [
        (0, 0x7052),
        (2, 0x7152),
        (4, 0x2071),
        (8, 0x3071),
        (12, 0x0071),
        (16, 0x7354),
        (18, 0x1072),
        (21, 0x040a),
        (22, 0x0512),
        (23, 0x4535),
        (24, 0x0012),
        (25, 0x2072),
        (29, 0x061f),
        (31, 0x206e),
        (34, 0x060c),
        (35, 0x206e),
        (37, 0x0062),
        (38, 0x05d8),
        (39, 0x0105),
        (40, 0xef28),
        (41, 0x1071),
        (45, 0x2071),
        (49, 0x2071),
        (53, 0x0122),
        (55, 0x4070),
        (58, 0x0111),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    Some(RoundedPolygonTransformShape {
        center_x_field_index: u32::from(ins[1]),
        center_y_field_index: u32::from(ins[3]),
        float_pack_method_index: u32::from(ins[5]),
        packed_point_transform_method_index: u32::from(ins[9]),
        list_factory_method_index: u32::from(ins[13]),
        features_field_index: u32::from(ins[17]),
        list_size_method_index: u32::from(ins[19]),
        list_get_method_index: u32::from(ins[26]),
        feature_type_index: u32::from(ins[30]),
        feature_transform_method_index: u32::from(ins[32]),
        list_add_method_index: u32::from(ins[36]),
        list_freeze_method_index: u32::from(ins[42]),
        center_x_method_index: u32::from(ins[46]),
        center_y_method_index: u32::from(ins[50]),
        polygon_type_index: u32::from(ins[54]),
        constructor_method_index: u32::from(ins[56]),
    })
}

fn detect_rounded_polygon_bounds_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_rounded_polygon_bounds_shape(code) else {
        return Ok(None);
    };
    if program.type_descriptor(resolved.dex_file_index, shape.float_array_type_index)? != "[F" {
        return Ok(None);
    }
    let cubics_field = program.field_id(resolved.dex_file_index, shape.cubics_field_index)?;
    if cubics_field.class_descriptor != resolved.method.proto.parameters[0]
        || !cubics_field.type_descriptor.starts_with('L')
    {
        return Ok(None);
    }
    let cubic_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.cubic_type_index)?;
    let cubic_points_field =
        program.field_id(resolved.dex_file_index, shape.cubic_points_field_index)?;
    if cubic_points_field.class_descriptor != cubic_descriptor
        || cubic_points_field.type_descriptor != "[F"
    {
        return Ok(None);
    }
    let builder_size =
        program.method_id(resolved.dex_file_index, shape.builder_size_method_index)?;
    let builder_get = program.method_id(resolved.dex_file_index, shape.builder_get_method_index)?;
    if !builder_size.class_descriptor.starts_with('L')
        || builder_size.name != "a"
        || !builder_size.proto.descriptor_is("()I")
        || !builder_get.class_descriptor.starts_with('L')
        || builder_get.name != "get"
        || !builder_get.proto.descriptor_is("(I)Ljava/lang/Object;")
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.get_class_method_index,
        "Ljava/lang/Object;",
        "getClass",
        "()Ljava/lang/Class;",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_zero_length_method_index,
        &cubic_descriptor,
        "f",
        "()Z",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_anchor_x_method_index,
        &cubic_descriptor,
        "a",
        "()F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_anchor_y_method_index,
        &cubic_descriptor,
        "b",
        "()F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.math_min_method_index,
        "Ljava/lang/Math;",
        "min",
        "(FF)F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.math_max_method_index,
        "Ljava/lang/Math;",
        "max",
        "(FF)F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.exception_constructor_method_index,
        "Ljava/lang/IllegalArgumentException;",
        "<init>",
        "(Ljava/lang/String;)V",
    )? {
        return Ok(None);
    }
    if program.type_descriptor(resolved.dex_file_index, shape.exception_type_index)?
        != "Ljava/lang/IllegalArgumentException;"
        || program.string_value(resolved.dex_file_index, shape.required_bounds_string_index)?
            != "Required bounds size of 4"
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::RoundedPolygonBounds {
        polygon_arg: 0,
        bounds_arg: 1,
        mask_arg: 2,
        cubics_field_index: shape.cubics_field_index,
        cubic_points_field_index: shape.cubic_points_field_index,
    }))
}

#[derive(Clone, Copy)]
struct RoundedPolygonBoundsShape {
    float_array_type_index: u32,
    get_class_method_index: u32,
    cubics_field_index: u32,
    builder_size_method_index: u32,
    builder_get_method_index: u32,
    cubic_type_index: u32,
    cubic_zero_length_method_index: u32,
    cubic_points_field_index: u32,
    cubic_anchor_x_method_index: u32,
    cubic_anchor_y_method_index: u32,
    math_min_method_index: u32,
    math_max_method_index: u32,
    exception_type_index: u32,
    required_bounds_string_index: u32,
    exception_constructor_method_index: u32,
}

// cite: androidx RoundedPolygon.kt#calculateBounds
// cite: androidx Cubic.kt#calculateBounds, zeroLength
fn detect_rounded_polygon_bounds_shape(code: &DexCodeItem) -> Option<RoundedPolygonBoundsShape> {
    let ins = &code.instructions;
    if code.registers_size != 20 || code.ins_size != 3 || code.outs_size != 2 || ins.len() != 242 {
        return None;
    }
    let fixed = [
        (0, 0x1012),
        (2, 0x0113),
        (3, 0x4212),
        (4, 0x0138),
        (5, 0x0005),
        (6, 0x2123),
        (8, 0x0328),
        (9, 0x0108),
        (10, 0x0012),
        (11, 0x0174),
        (13, 0x0011),
        (14, 0x1321),
        (15, 0x2334),
        (16, 0x00db),
        (17, 0x0308),
        (18, 0x0011),
        (19, 0x3354),
        (21, 0x106e),
        (23, 0x0003),
        (24, 0x040a),
        (25, 0x1512),
        (26, 0x0614),
        (27, 0xffff),
        (28, 0x7f7f),
        (29, 0x0712),
        (34, 0x2b12),
        (35, 0x3c12),
        (36, 0x4a35),
        (37, 0x00b7),
        (38, 0x206e),
        (40, 0x00a3),
        (41, 0x0d0c),
        (42, 0x0d1f),
        (44, 0x106e),
        (46, 0x000d),
        (47, 0x106e),
        (49, 0x000d),
        (50, 0x0e0a),
        (51, 0xdf54),
        (53, 0x0e38),
        (54, 0x001b),
        (71, 0x1002),
        (72, 0x0000),
        (73, 0x1302),
        (74, 0x0002),
        (75, 0x1102),
        (76, 0x0007),
        (77, 0x1202),
        (78, 0x000b),
        (79, 0x6a28),
        (84, 0x106e),
        (86, 0x000d),
        (88, 0x2071),
        (90, 0x000e),
        (96, 0x106e),
        (98, 0x000d),
        (100, 0x2071),
        (102, 0x002e),
        (108, 0x106e),
        (110, 0x000d),
        (112, 0x2071),
        (114, 0x007e),
        (118, 0x106e),
        (120, 0x000d),
        (122, 0x2071),
        (124, 0x00de),
        (209, 0x0ad8),
        (210, 0x010a),
        (211, 0x0702),
        (212, 0x0011),
        (213, 0x0202),
        (214, 0x0013),
        (215, 0x0002),
        (216, 0x0010),
        (217, 0x0029),
        (218, 0xff49),
        (219, 0x1002),
        (220, 0x0000),
        (221, 0x1102),
        (222, 0x0007),
        (223, 0x1202),
        (224, 0x000b),
        (233, 0x0111),
        (234, 0x0022),
        (236, 0x011a),
        (238, 0x2070),
        (240, 0x0010),
        (241, 0x0027),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[12] != ins[45] || ins[85] != ins[109] || ins[97] != ins[119] {
        return None;
    }
    for index in [101, 133, 137, 148, 152, 188, 194] {
        if ins[index] != ins[89] {
            return None;
        }
    }
    for index in [123, 162, 166, 176, 180, 200, 206] {
        if ins[index] != ins[113] {
            return None;
        }
    }

    Some(RoundedPolygonBoundsShape {
        float_array_type_index: u32::from(ins[7]),
        get_class_method_index: u32::from(ins[12]),
        cubics_field_index: u32::from(ins[20]),
        builder_size_method_index: u32::from(ins[22]),
        builder_get_method_index: u32::from(ins[39]),
        cubic_type_index: u32::from(ins[43]),
        cubic_zero_length_method_index: u32::from(ins[48]),
        cubic_points_field_index: u32::from(ins[52]),
        cubic_anchor_x_method_index: u32::from(ins[85]),
        cubic_anchor_y_method_index: u32::from(ins[97]),
        math_min_method_index: u32::from(ins[89]),
        math_max_method_index: u32::from(ins[113]),
        exception_type_index: u32::from(ins[235]),
        required_bounds_string_index: u32::from(ins[237]),
        exception_constructor_method_index: u32::from(ins[239]),
    })
}

fn detect_rounded_polygon_normalized_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    if resolved.method.proto.return_type != resolved.class_descriptor {
        return Ok(None);
    }
    let Some(shape) = detect_rounded_polygon_normalized_shape(code) else {
        return Ok(None);
    };
    let bounds_method = program.method_id(resolved.dex_file_index, shape.bounds_method_index)?;
    if bounds_method.class_descriptor != resolved.class_descriptor
        || bounds_method.proto.parameters
            != [
                resolved.class_descriptor.clone(),
                "[F".to_owned(),
                "I".to_owned(),
            ]
        || bounds_method.proto.return_type != "[F"
    {
        return Ok(None);
    }
    let Some(MethodIntrinsic::RoundedPolygonBounds { .. }) =
        method_ref_intrinsic(program, resolved.dex_file_index, shape.bounds_method_index)?
    else {
        return Ok(None);
    };
    let transformer_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.transformer_type_index)?;
    if !transformer_descriptor.starts_with('L') {
        return Ok(None);
    }
    let constructor = program.method_id(
        resolved.dex_file_index,
        shape.transformer_constructor_method_index,
    )?;
    if constructor.class_descriptor != transformer_descriptor
        || constructor.name != "<init>"
        || !constructor.proto.descriptor_is("(FFF)V")
    {
        return Ok(None);
    }
    let transform = program.method_id(resolved.dex_file_index, shape.transform_method_index)?;
    if transform.class_descriptor != resolved.class_descriptor
        || transform.proto.parameters.len() != 1
        || !transform.proto.parameters[0].starts_with('L')
        || transform.proto.return_type != resolved.class_descriptor
    {
        return Ok(None);
    }
    let Some(MethodIntrinsic::RoundedPolygonTransform { .. }) = method_ref_intrinsic(
        program,
        resolved.dex_file_index,
        shape.transform_method_index,
    )?
    else {
        return Ok(None);
    };
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.math_max_method_index,
        "Ljava/lang/Math;",
        "max",
        "(FF)F",
    )? {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::RoundedPolygonNormalized {
        receiver_arg: 0,
        bounds_method_index: shape.bounds_method_index,
        transformer_type_index: shape.transformer_type_index,
        transformer_constructor_method_index: shape.transformer_constructor_method_index,
        transform_method_index: shape.transform_method_index,
    }))
}

#[derive(Clone, Copy)]
struct RoundedPolygonNormalizedShape {
    bounds_method_index: u32,
    math_max_method_index: u32,
    transformer_type_index: u32,
    transformer_constructor_method_index: u32,
    transform_method_index: u32,
}

// cite: androidx RoundedPolygon.kt#normalized
fn detect_rounded_polygon_normalized_shape(
    code: &DexCodeItem,
) -> Option<RoundedPolygonNormalizedShape> {
    let ins = &code.instructions;
    if code.registers_size != 8 || code.ins_size != 1 || code.outs_size != 4 || ins.len() != 46 {
        return None;
    }
    let fixed = [
        (0, 0x0012),
        (1, 0x3112),
        (2, 0x3071),
        (4, 0x0107),
        (5, 0x000c),
        (6, 0x2212),
        (7, 0x0344),
        (8, 0x0200),
        (9, 0x0412),
        (10, 0x0544),
        (11, 0x0400),
        (12, 0x53c7),
        (13, 0x0144),
        (14, 0x0100),
        (15, 0x1512),
        (16, 0x0644),
        (17, 0x0500),
        (18, 0x61c7),
        (19, 0x2071),
        (21, 0x0013),
        (22, 0x060a),
        (23, 0x03a7),
        (24, 0x0306),
        (25, 0x2282),
        (26, 0x23c9),
        (27, 0x0444),
        (28, 0x0400),
        (29, 0x43c7),
        (30, 0x01a7),
        (31, 0x0106),
        (32, 0x21c9),
        (33, 0x0044),
        (34, 0x0500),
        (35, 0x01c7),
        (36, 0x0022),
        (38, 0x4070),
        (40, 0x1630),
        (41, 0x206e),
        (43, 0x0007),
        (44, 0x000c),
        (45, 0x0011),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }

    Some(RoundedPolygonNormalizedShape {
        bounds_method_index: u32::from(ins[3]),
        math_max_method_index: u32::from(ins[20]),
        transformer_type_index: u32::from(ins[37]),
        transformer_constructor_method_index: u32::from(ins[39]),
        transform_method_index: u32::from(ins[42]),
    })
}

fn detect_rounded_polygon_constructor_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    if resolved.method.name != "<init>" || resolved.method.proto.return_type != "V" {
        return Ok(None);
    }
    let Some(shape) = detect_rounded_polygon_constructor_shape(code) else {
        return Ok(None);
    };
    if program.string_value(resolved.dex_file_index, shape.features_string_index)? != "features" {
        return Ok(None);
    }
    let features_field = program.field_id(resolved.dex_file_index, shape.features_field_index)?;
    let center_x_field = program.field_id(resolved.dex_file_index, shape.center_x_field_index)?;
    let center_y_field = program.field_id(resolved.dex_file_index, shape.center_y_field_index)?;
    let cubics_field = program.field_id(resolved.dex_file_index, shape.cubics_field_index)?;
    if features_field.class_descriptor != resolved.class_descriptor
        || center_x_field.class_descriptor != resolved.class_descriptor
        || center_y_field.class_descriptor != resolved.class_descriptor
        || cubics_field.class_descriptor != resolved.class_descriptor
        || features_field.type_descriptor != "Ljava/util/AbstractList;"
        || center_x_field.type_descriptor != "F"
        || center_y_field.type_descriptor != "F"
        || !cubics_field.type_descriptor.starts_with('L')
    {
        return Ok(None);
    }
    let feature_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.feature_type_index)?;
    let feature_cubics_field =
        program.field_id(resolved.dex_file_index, shape.feature_cubics_field_index)?;
    if feature_cubics_field.class_descriptor != feature_descriptor
        || feature_cubics_field.type_descriptor != "Ljava/util/List;"
    {
        return Ok(None);
    }
    let cubic_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.cubic_type_index)?;
    let cubic_points_field =
        program.field_id(resolved.dex_file_index, shape.cubic_points_field_index)?;
    if cubic_points_field.class_descriptor != cubic_descriptor
        || cubic_points_field.type_descriptor != "[F"
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_size_method_index,
        "Ljava/util/List;",
        "size",
        "()I",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_get_method_index,
        "Ljava/util/List;",
        "get",
        "(I)Ljava/lang/Object;",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_anchor_x_method_index,
        &cubic_descriptor,
        "a",
        "()F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_anchor_y_method_index,
        &cubic_descriptor,
        "b",
        "()F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.cubic_zero_length_method_index,
        &cubic_descriptor,
        "f",
        "()Z",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.math_abs_method_index,
        "Ljava/lang/Math;",
        "abs",
        "(F)F",
    )? {
        return Ok(None);
    }
    let cubic_split = program.method_id(resolved.dex_file_index, shape.cubic_split_method_index)?;
    if cubic_split.class_descriptor != cubic_descriptor
        || cubic_split.proto.parameters != ["F".to_owned()]
        || !cubic_split.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let cubic_factory =
        program.method_id(resolved.dex_file_index, shape.cubic_factory_method_index)?;
    if cubic_factory.proto.parameters
        != [
            "F".to_owned(),
            "F".to_owned(),
            "F".to_owned(),
            "F".to_owned(),
            "F".to_owned(),
            "F".to_owned(),
            "F".to_owned(),
            "F".to_owned(),
        ]
        || cubic_factory.proto.return_type != cubic_descriptor
    {
        return Ok(None);
    }
    let Some(list_fields) = detect_object_array_append_fields_from_method_ref(
        program,
        resolved.dex_file_index,
        shape.list_add_method_index,
    )?
    else {
        return Ok(None);
    };
    let list_factory =
        program.method_id(resolved.dex_file_index, shape.list_factory_method_index)?;
    if !list_factory.proto.parameters.is_empty() || !list_factory.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let list_freeze = program.method_id(resolved.dex_file_index, shape.list_freeze_method_index)?;
    if list_freeze.proto.parameters.len() != 1
        || list_freeze.proto.parameters[0] != list_factory.proto.return_type
        || list_freeze.proto.return_type != list_factory.proto.return_type
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.builder_size_method_index,
        &list_factory.proto.return_type,
        "a",
        "()I",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.builder_get_method_index,
        &list_factory.proto.return_type,
        "get",
        "(I)Ljava/lang/Object;",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.exception_constructor_method_index,
        "Ljava/lang/IllegalArgumentException;",
        "<init>",
        "(Ljava/lang/String;)V",
    )? {
        return Ok(None);
    }
    if program.string_value(resolved.dex_file_index, shape.contiguity_string_index)?
        != "RoundedPolygon must be contiguous, with the anchor points of all curves matching the anchor points of the preceding and succeeding cubics"
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::RoundedPolygonConstructor {
        receiver_arg: 0,
        features_arg: 1,
        center_x_arg: 2,
        center_y_arg: 3,
        features_field_index: shape.features_field_index,
        center_x_field_index: shape.center_x_field_index,
        center_y_field_index: shape.center_y_field_index,
        cubics_field_index: shape.cubics_field_index,
        feature_cubics_field_index: shape.feature_cubics_field_index,
        cubic_points_field_index: shape.cubic_points_field_index,
        list_freeze_method_index: shape.list_freeze_method_index,
        list_array_field_index: list_fields.array_field_index,
        list_size_field_index: list_fields.size_field_index,
        list_read_only_field_index: list_fields.read_only_field_index,
        list_mod_count_field_index: list_fields.mod_count_field_index,
    }))
}

#[derive(Clone, Copy)]
struct RoundedPolygonConstructorShape {
    features_string_index: u32,
    features_field_index: u32,
    center_x_field_index: u32,
    center_y_field_index: u32,
    cubics_field_index: u32,
    feature_type_index: u32,
    feature_cubics_field_index: u32,
    cubic_type_index: u32,
    cubic_split_method_index: u32,
    cubic_zero_length_method_index: u32,
    cubic_anchor_x_method_index: u32,
    cubic_anchor_y_method_index: u32,
    cubic_points_field_index: u32,
    cubic_factory_method_index: u32,
    list_factory_method_index: u32,
    list_size_method_index: u32,
    list_get_method_index: u32,
    list_add_method_index: u32,
    list_freeze_method_index: u32,
    builder_size_method_index: u32,
    builder_get_method_index: u32,
    math_abs_method_index: u32,
    contiguity_string_index: u32,
    exception_constructor_method_index: u32,
}

// cite: androidx RoundedPolygon.kt#RoundedPolygon, cubics
// cite: androidx Cubic.kt#zeroLength, split
fn detect_rounded_polygon_constructor_shape(
    code: &DexCodeItem,
) -> Option<RoundedPolygonConstructorShape> {
    let ins = &code.instructions;
    if code.registers_size != 22 || code.ins_size != 4 || code.outs_size != 8 || ins.len() != 357 {
        return None;
    }
    let fixed = [
        (4, 0x021a),
        (6, 0x2071),
        (9, 0x1070),
        (12, 0x015b),
        (16, 0x0259),
        (20, 0x0259),
        (22, 0x0071),
        (26, 0x1072),
        (37, 0x2072),
        (41, 0x031f),
        (43, 0x3354),
        (45, 0x1072),
        (51, 0x2072),
        (55, 0x031f),
        (57, 0x3354),
        (59, 0x2072),
        (63, 0x031f),
        (67, 0x206e),
        (73, 0x091f),
        (75, 0x3354),
        (77, 0x031f),
        (81, 0x2072),
        (85, 0x0b1f),
        (87, 0xbb54),
        (89, 0x2072),
        (97, 0x1071),
        (105, 0x2072),
        (109, 0x031f),
        (111, 0x3354),
        (113, 0x2072),
        (119, 0x1071),
        (126, 0x1072),
        (140, 0x0c54),
        (142, 0x1072),
        (157, 0x0c54),
        (159, 0x2072),
        (163, 0x0c1f),
        (165, 0xcc54),
        (167, 0x1072),
        (174, 0x2072),
        (178, 0x0f1f),
        (180, 0x106e),
        (188, 0x206e),
        (202, 0x106e),
        (208, 0xa454),
        (216, 0x106e),
        (246, 0x8354),
        (262, 0x1154),
        (268, 0x0877),
        (272, 0x206e),
        (275, 0x1071),
        (279, 0x015b),
        (281, 0x106e),
        (286, 0x206e),
        (290, 0x106e),
        (297, 0x0454),
        (299, 0x206e),
        (303, 0x041f),
        (305, 0x4554),
        (309, 0x021f),
        (311, 0x106e),
        (316, 0x1071),
        (327, 0x4554),
        (331, 0x106e),
        (336, 0x1071),
        (348, 0x0122),
        (350, 0x021a),
        (352, 0x2070),
        (356, 0x000e),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[27] != ins[46] || ins[27] != ins[127] || ins[27] != ins[143] || ins[27] != ins[168] {
        return None;
    }
    if ins[38] != ins[52]
        || ins[38] != ins[60]
        || ins[38] != ins[82]
        || ins[38] != ins[90]
        || ins[38] != ins[106]
        || ins[38] != ins[114]
        || ins[38] != ins[160]
        || ins[38] != ins[175]
    {
        return None;
    }
    if ins[42] != ins[56] || ins[42] != ins[86] || ins[42] != ins[110] || ins[42] != ins[164] {
        return None;
    }
    if ins[64] != ins[74]
        || ins[64] != ins[78]
        || ins[64] != ins[179]
        || ins[64] != ins[304]
        || ins[64] != ins[310]
    {
        return None;
    }
    if ins[13] != ins[141] || ins[13] != ins[158] {
        return None;
    }
    if ins[44] != ins[58] || ins[44] != ins[88] || ins[44] != ins[112] || ins[44] != ins[166] {
        return None;
    }
    if ins[189] != ins[273] {
        return None;
    }
    if ins[203] != ins[312] || ins[217] != ins[332] {
        return None;
    }
    if ins[209] != ins[247] || ins[209] != ins[263] || ins[209] != ins[306] || ins[209] != ins[328]
    {
        return None;
    }
    if ins[282] != ins[291] || ins[287] != ins[300] || ins[317] != ins[337] {
        return None;
    }

    Some(RoundedPolygonConstructorShape {
        features_string_index: u32::from(ins[5]),
        features_field_index: u32::from(ins[13]),
        center_x_field_index: u32::from(ins[17]),
        center_y_field_index: u32::from(ins[21]),
        cubics_field_index: u32::from(ins[280]),
        feature_type_index: u32::from(ins[42]),
        feature_cubics_field_index: u32::from(ins[44]),
        cubic_type_index: u32::from(ins[64]),
        cubic_split_method_index: u32::from(ins[68]),
        cubic_zero_length_method_index: u32::from(ins[181]),
        cubic_anchor_x_method_index: u32::from(ins[203]),
        cubic_anchor_y_method_index: u32::from(ins[217]),
        cubic_points_field_index: u32::from(ins[209]),
        cubic_factory_method_index: u32::from(ins[269]),
        list_factory_method_index: u32::from(ins[23]),
        list_size_method_index: u32::from(ins[27]),
        list_get_method_index: u32::from(ins[38]),
        list_add_method_index: u32::from(ins[189]),
        list_freeze_method_index: u32::from(ins[276]),
        builder_size_method_index: u32::from(ins[282]),
        builder_get_method_index: u32::from(ins[287]),
        math_abs_method_index: u32::from(ins[317]),
        contiguity_string_index: u32::from(ins[351]),
        exception_constructor_method_index: u32::from(ins[353]),
    })
}

fn detect_measured_polygon_measure_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_measured_polygon_measure_shape(code) else {
        return Ok(None);
    };
    let polygon_features_field =
        program.field_id(resolved.dex_file_index, shape.polygon_features_field_index)?;
    if polygon_features_field.class_descriptor != resolved.method.proto.parameters[1]
        || polygon_features_field.type_descriptor != "Ljava/util/AbstractList;"
    {
        return Ok(None);
    }
    let feature_cubics_field =
        program.field_id(resolved.dex_file_index, shape.feature_cubics_field_index)?;
    if feature_cubics_field.type_descriptor != "Ljava/util/List;" {
        return Ok(None);
    }
    let corner_descriptor =
        program.type_descriptor(resolved.dex_file_index, shape.corner_type_index)?;
    if !program.descriptor_matches_type(&corner_descriptor, &feature_cubics_field.class_descriptor)
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_size_method_index,
        "Ljava/util/List;",
        "size",
        "()I",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.list_get_method_index,
        "Ljava/util/List;",
        "get",
        "(I)Ljava/lang/Object;",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.array_list_add_method_index,
        "Ljava/util/ArrayList;",
        "add",
        "(Ljava/lang/Object;)Z",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.array_list_size_method_index,
        "Ljava/util/ArrayList;",
        "size",
        "()I",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.array_list_get_method_index,
        "Ljava/util/ArrayList;",
        "get",
        "(I)Ljava/lang/Object;",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.float_value_of_method_index,
        "Ljava/lang/Float;",
        "valueOf",
        "(F)Ljava/lang/Float;",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.number_float_value_method_index,
        "Ljava/lang/Number;",
        "floatValue",
        "()F",
    )? || !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.number_int_value_method_index,
        "Ljava/lang/Number;",
        "intValue",
        "()I",
    )? {
        return Ok(None);
    }

    let Some(list_fields) = detect_object_array_append_fields_from_method_ref(
        program,
        resolved.dex_file_index,
        shape.list_add_method_index,
    )?
    else {
        return Ok(None);
    };
    let list_factory =
        program.method_id(resolved.dex_file_index, shape.list_factory_method_index)?;
    if !list_factory.proto.parameters.is_empty() || !list_factory.proto.return_type.starts_with('L')
    {
        return Ok(None);
    }
    let list_freeze = program.method_id(resolved.dex_file_index, shape.list_freeze_method_index)?;
    if list_freeze.proto.parameters.len() != 1
        || list_freeze.proto.parameters[0] != list_factory.proto.return_type
        || list_freeze.proto.return_type != list_factory.proto.return_type
    {
        return Ok(None);
    }

    let measure_method =
        program.method_id(resolved.dex_file_index, shape.measure_cubic_method_index)?;
    if measure_method.class_descriptor != resolved.method.proto.parameters[0] {
        return Ok(None);
    }
    let Some(measure_shape) = method_ref_angular_measure_cubic_shape(
        program,
        resolved.dex_file_index,
        shape.measure_cubic_method_index,
    )?
    else {
        return Ok(None);
    };
    if measure_method.proto.parameters.len() != 1
        || measure_method.proto.parameters[0].as_str() != measure_shape.cubic_descriptor.as_str()
        || measure_method.proto.return_type != "F"
    {
        return Ok(None);
    }
    let Some(progress_fields) = method_ref_progress_feature_constructor_fields(
        program,
        resolved.dex_file_index,
        shape.progress_feature_constructor_method_index,
        &feature_cubics_field.class_descriptor,
    )?
    else {
        return Ok(None);
    };
    let Some(polygon_fields) = method_ref_measured_polygon_constructor_fields(
        program,
        resolved.dex_file_index,
        shape.measured_polygon_constructor_method_index,
        &resolved.method.proto.parameters[0],
        &measure_shape.cubic_descriptor,
    )?
    else {
        return Ok(None);
    };
    let constructor = program.method_id(
        resolved.dex_file_index,
        shape.measured_polygon_constructor_method_index,
    )?;
    if constructor.class_descriptor != resolved.method.proto.return_type {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::MeasuredPolygonMeasure {
        measurer_arg: 0,
        polygon_arg: 1,
        polygon_features_field_index: shape.polygon_features_field_index,
        feature_cubics_field_index: shape.feature_cubics_field_index,
        cubic_points_field_index: measure_shape.cubic_points_field_index,
        corner_type_index: shape.corner_type_index,
        center_x_field_index: measure_shape.center_x_field_index,
        center_y_field_index: measure_shape.center_y_field_index,
        measured_polygon_measurer_field_index: polygon_fields.measurer_field_index,
        measured_polygon_cubics_field_index: polygon_fields.cubics_field_index,
        measured_polygon_features_field_index: polygon_fields.features_field_index,
        measured_cubic_owner_field_index: polygon_fields.measured_cubic_fields.owner_field_index,
        measured_cubic_cubic_field_index: polygon_fields.measured_cubic_fields.cubic_field_index,
        measured_cubic_size_field_index: polygon_fields.measured_cubic_fields.size_field_index,
        measured_cubic_start_field_index: polygon_fields.measured_cubic_fields.start_field_index,
        measured_cubic_end_field_index: polygon_fields.measured_cubic_fields.end_field_index,
        progress_feature_progress_field_index: progress_fields.progress_field_index,
        progress_feature_feature_field_index: progress_fields.feature_field_index,
        list_freeze_method_index: shape.list_freeze_method_index,
        list_array_field_index: list_fields.array_field_index,
        list_size_field_index: list_fields.size_field_index,
        list_read_only_field_index: list_fields.read_only_field_index,
        list_mod_count_field_index: list_fields.mod_count_field_index,
    }))
}

#[derive(Clone, Copy)]
struct MeasuredPolygonMeasureShape {
    polygon_features_field_index: u32,
    feature_cubics_field_index: u32,
    corner_type_index: u32,
    measure_cubic_method_index: u32,
    list_size_method_index: u32,
    list_get_method_index: u32,
    array_list_add_method_index: u32,
    array_list_size_method_index: u32,
    array_list_get_method_index: u32,
    float_value_of_method_index: u32,
    number_float_value_method_index: u32,
    number_int_value_method_index: u32,
    list_factory_method_index: u32,
    list_add_method_index: u32,
    list_freeze_method_index: u32,
    progress_feature_constructor_method_index: u32,
    measured_polygon_constructor_method_index: u32,
}

// cite: androidx PolygonMeasure.kt#measurePolygon
fn detect_measured_polygon_measure_shape(
    code: &DexCodeItem,
) -> Option<MeasuredPolygonMeasureShape> {
    let ins = &code.instructions;
    if code.registers_size != 14 || code.ins_size != 2 || code.outs_size != 5 || ins.len() != 263 {
        return None;
    }
    let fixed = [
        (0, 0x0022),
        (2, 0x1070),
        (5, 0x0122),
        (7, 0x1070),
        (10, 0xdd54),
        (12, 0x1072),
        (21, 0x2072),
        (25, 0x061f),
        (27, 0x6754),
        (36, 0x6920),
        (38, 0x6a54),
        (42, 0x1072),
        (49, 0x106e),
        (57, 0x0b22),
        (59, 0x3070),
        (62, 0x206e),
        (65, 0x2072),
        (69, 0x206e),
        (79, 0x1071),
        (91, 0x1071),
        (96, 0x0622),
        (100, 0x2070),
        (103, 0x206e),
        (106, 0x106e),
        (110, 0x1072),
        (116, 0x1072),
        (120, 0x071f),
        (126, 0x206e),
        (135, 0x1071),
        (139, 0x206e),
        (152, 0x1071),
        (158, 0x106e),
        (162, 0x0422),
        (168, 0x2070),
        (178, 0x2072),
        (184, 0x106e),
        (189, 0x206e),
        (195, 0x0071),
        (199, 0x106e),
        (205, 0x206e),
        (211, 0x6654),
        (215, 0x106e),
        (219, 0x0722),
        (221, 0x206e),
        (227, 0x206e),
        (234, 0x206e),
        (240, 0x8854),
        (244, 0x3070),
        (247, 0x206e),
        (253, 0x1071),
        (257, 0x0122),
        (259, 0x5470),
        (262, 0x0111),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[1] != ins[6] || ins[3] != ins[8] {
        return None;
    }
    if ins[13] != ins[43] || ins[13] != ins[172] {
        return None;
    }
    if ins[22] != ins[66] || ins[22] != ins[179] {
        return None;
    }
    if ins[28] != ins[39] {
        return None;
    }
    if ins[50] != ins[200] {
        return None;
    }
    if ins[63] != ins[70] || ins[63] != ins[104] || ins[63] != ins[140] {
        return None;
    }
    if ins[80] != ins[136] {
        return None;
    }
    if ins[159] != ins[185] {
        return None;
    }
    if ins[206] != ins[235] {
        return None;
    }
    if ins[222] != ins[228] {
        return None;
    }

    Some(MeasuredPolygonMeasureShape {
        polygon_features_field_index: u32::from(ins[11]),
        feature_cubics_field_index: u32::from(ins[28]),
        corner_type_index: u32::from(ins[37]),
        measure_cubic_method_index: u32::from(ins[127]),
        list_size_method_index: u32::from(ins[13]),
        list_get_method_index: u32::from(ins[22]),
        array_list_add_method_index: u32::from(ins[63]),
        array_list_size_method_index: u32::from(ins[50]),
        array_list_get_method_index: u32::from(ins[206]),
        float_value_of_method_index: u32::from(ins[80]),
        number_float_value_method_index: u32::from(ins[159]),
        number_int_value_method_index: u32::from(ins[216]),
        list_factory_method_index: u32::from(ins[196]),
        list_add_method_index: u32::from(ins[248]),
        list_freeze_method_index: u32::from(ins[254]),
        progress_feature_constructor_method_index: u32::from(ins[245]),
        measured_polygon_constructor_method_index: u32::from(ins[260]),
    })
}

struct AngularMeasureCubicShape {
    cubic_descriptor: String,
    cubic_points_field_index: u32,
    center_x_field_index: u32,
    center_y_field_index: u32,
}

fn method_ref_angular_measure_cubic_shape(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
) -> Result<Option<AngularMeasureCubicShape>> {
    let method = program.method_id(dex_file_index, method_index)?;
    if method.proto.parameters.len() != 1 || method.proto.return_type != "F" {
        return Ok(None);
    }
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };
    let code = program.code_item(&resolved)?;
    let Some(shape) = detect_angular_measure_cubic_shape(&code) else {
        return Ok(None);
    };
    let cubic_points_field = program.field_id(dex_file_index, shape.cubic_points_field_index)?;
    if cubic_points_field.class_descriptor != method.proto.parameters[0]
        || cubic_points_field.type_descriptor != "[F"
        || !field_ref_type_matches(program, dex_file_index, shape.center_x_field_index, "F")?
        || !field_ref_type_matches(program, dex_file_index, shape.center_y_field_index, "F")?
    {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        dex_file_index,
        shape.anchor_x_method_index,
        &method.proto.parameters[0],
        "a",
        "()F",
    )? || !method_ref_matches(
        program,
        dex_file_index,
        shape.anchor_y_method_index,
        &method.proto.parameters[0],
        "b",
        "()F",
    )? || !method_ref_proto_matches(program, dex_file_index, shape.angle_method_index, "(FF)F")?
        || !method_ref_proto_matches(
            program,
            dex_file_index,
            shape.positive_modulo_method_index,
            "(FF)F",
        )?
    {
        return Ok(None);
    }

    Ok(Some(AngularMeasureCubicShape {
        cubic_descriptor: method.proto.parameters[0].clone(),
        cubic_points_field_index: shape.cubic_points_field_index,
        center_x_field_index: shape.center_x_field_index,
        center_y_field_index: shape.center_y_field_index,
    }))
}

#[derive(Clone, Copy)]
struct AngularMeasureCubicShapeFields {
    anchor_x_method_index: u32,
    anchor_y_method_index: u32,
    angle_method_index: u32,
    positive_modulo_method_index: u32,
    cubic_points_field_index: u32,
    center_x_field_index: u32,
    center_y_field_index: u32,
}

fn detect_angular_measure_cubic_shape(
    code: &DexCodeItem,
) -> Option<AngularMeasureCubicShapeFields> {
    let ins = &code.instructions;
    if code.registers_size != 6 || code.ins_size != 2 || code.outs_size != 2 || ins.len() != 55 {
        return None;
    }
    let fixed = [
        (0, 0x001a),
        (2, 0x2071),
        (5, 0x106e),
        (9, 0x4152),
        (12, 0x106e),
        (16, 0x4352),
        (19, 0x2071),
        (24, 0x5554),
        (33, 0x2071),
        (40, 0x2071),
        (53, 0x050f),
        (54, 0x000f),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[20] != ins[34] {
        return None;
    }

    Some(AngularMeasureCubicShapeFields {
        anchor_x_method_index: u32::from(ins[6]),
        anchor_y_method_index: u32::from(ins[13]),
        angle_method_index: u32::from(ins[20]),
        positive_modulo_method_index: u32::from(ins[41]),
        cubic_points_field_index: u32::from(ins[25]),
        center_x_field_index: u32::from(ins[10]),
        center_y_field_index: u32::from(ins[17]),
    })
}

struct ProgressFeatureConstructorFields {
    progress_field_index: u32,
    feature_field_index: u32,
}

fn method_ref_progress_feature_constructor_fields(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
    feature_descriptor: &str,
) -> Result<Option<ProgressFeatureConstructorFields>> {
    let method = program.method_id(dex_file_index, method_index)?;
    if method.name != "<init>"
        || method.proto.return_type != "V"
        || method.proto.parameters != ["F".to_owned(), feature_descriptor.to_owned()]
    {
        return Ok(None);
    }
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };
    let code = program.code_item(&resolved)?;
    let ins = &code.instructions;
    if code.registers_size != 4
        || code.ins_size != 3
        || code.outs_size != 2
        || ins.len() != 13
        || ins[2] != 0x2071
        || ins[5] != 0x1070
        || ins[8] != 0x1259
        || ins[10] != 0x135b
        || ins[12] != 0x000e
    {
        return Ok(None);
    }
    let progress_field = program.field_id(dex_file_index, u32::from(ins[9]))?;
    let feature_field = program.field_id(dex_file_index, u32::from(ins[11]))?;
    if progress_field.class_descriptor != method.class_descriptor
        || feature_field.class_descriptor != method.class_descriptor
        || progress_field.type_descriptor != "F"
        || feature_field.type_descriptor != feature_descriptor
    {
        return Ok(None);
    }

    Ok(Some(ProgressFeatureConstructorFields {
        progress_field_index: u32::from(ins[9]),
        feature_field_index: u32::from(ins[11]),
    }))
}

struct MeasuredPolygonConstructorFields {
    measurer_field_index: u32,
    cubics_field_index: u32,
    features_field_index: u32,
    measured_cubic_fields: MeasuredCubicConstructorFields,
}

fn method_ref_measured_polygon_constructor_fields(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
    measurer_descriptor: &str,
    cubic_descriptor: &str,
) -> Result<Option<MeasuredPolygonConstructorFields>> {
    let method = program.method_id(dex_file_index, method_index)?;
    if method.name != "<init>"
        || method.proto.return_type != "V"
        || method.proto.parameters.len() != 4
        || method.proto.parameters[0] != measurer_descriptor
        || method.proto.parameters[2] != "Ljava/util/ArrayList;"
    {
        return Ok(None);
    }
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };
    let code = program.code_item(&resolved)?;
    let ins = &code.instructions;
    if code.registers_size != 12
        || code.ins_size != 5
        || code.outs_size != 5
        || ins.len() != 163
        || ins[42] != 0x785b
        || ins[44] != 0x795b
        || ins[46] != 0x0822
        || ins[87] != 0x5470
        || ins[120] != 0x785b
    {
        return Ok(None);
    }
    let measurer_field = program.field_id(dex_file_index, u32::from(ins[43]))?;
    let features_field = program.field_id(dex_file_index, u32::from(ins[45]))?;
    let cubics_field = program.field_id(dex_file_index, u32::from(ins[121]))?;
    if measurer_field.class_descriptor != method.class_descriptor
        || features_field.class_descriptor != method.class_descriptor
        || cubics_field.class_descriptor != method.class_descriptor
        || measurer_field.type_descriptor != measurer_descriptor
        || features_field.type_descriptor != method.proto.parameters[1]
        || cubics_field.type_descriptor != "Ljava/util/ArrayList;"
    {
        return Ok(None);
    }
    let Some(measured_cubic_fields) = method_ref_measured_cubic_constructor_fields(
        program,
        dex_file_index,
        u32::from(ins[88]),
        &method.class_descriptor,
        cubic_descriptor,
    )?
    else {
        return Ok(None);
    };

    Ok(Some(MeasuredPolygonConstructorFields {
        measurer_field_index: u32::from(ins[43]),
        cubics_field_index: u32::from(ins[121]),
        features_field_index: u32::from(ins[45]),
        measured_cubic_fields,
    }))
}

#[derive(Clone, Copy)]
struct MeasuredCubicConstructorFields {
    owner_field_index: u32,
    cubic_field_index: u32,
    size_field_index: u32,
    start_field_index: u32,
    end_field_index: u32,
}

fn method_ref_measured_cubic_constructor_fields(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
    measured_polygon_descriptor: &str,
    cubic_descriptor: &str,
) -> Result<Option<MeasuredCubicConstructorFields>> {
    let method = program.method_id(dex_file_index, method_index)?;
    if method.name != "<init>"
        || method.proto.return_type != "V"
        || method.proto.parameters
            != [
                measured_polygon_descriptor.to_owned(),
                cubic_descriptor.to_owned(),
                "F".to_owned(),
                "F".to_owned(),
            ]
    {
        return Ok(None);
    }
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };
    let code = program.code_item(&resolved)?;
    let ins = &code.instructions;
    if code.registers_size != 6
        || code.ins_size != 5
        || code.outs_size != 2
        || ins.len() != 37
        || ins[0] != 0x1070
        || ins[5] != 0x2071
        || ins[8] != 0x125b
        || ins[10] != 0x135b
        || ins[16] != 0x2254
        || ins[18] != 0x206e
        || ins[22] != 0x1259
        || ins[24] != 0x1459
        || ins[26] != 0x1559
    {
        return Ok(None);
    }
    let owner_field = program.field_id(dex_file_index, u32::from(ins[9]))?;
    let cubic_field = program.field_id(dex_file_index, u32::from(ins[11]))?;
    let size_field = program.field_id(dex_file_index, u32::from(ins[23]))?;
    let start_field = program.field_id(dex_file_index, u32::from(ins[25]))?;
    let end_field = program.field_id(dex_file_index, u32::from(ins[27]))?;
    if owner_field.class_descriptor != method.class_descriptor
        || cubic_field.class_descriptor != method.class_descriptor
        || size_field.class_descriptor != method.class_descriptor
        || start_field.class_descriptor != method.class_descriptor
        || end_field.class_descriptor != method.class_descriptor
        || owner_field.type_descriptor != measured_polygon_descriptor
        || cubic_field.type_descriptor != cubic_descriptor
        || size_field.type_descriptor != "F"
        || start_field.type_descriptor != "F"
        || end_field.type_descriptor != "F"
    {
        return Ok(None);
    }

    Ok(Some(MeasuredCubicConstructorFields {
        owner_field_index: u32::from(ins[9]),
        cubic_field_index: u32::from(ins[11]),
        size_field_index: u32::from(ins[23]),
        start_field_index: u32::from(ins[25]),
        end_field_index: u32::from(ins[27]),
    }))
}

fn detect_measured_cubic_cut_intrinsic(
    program: &DexProgram,
    resolved: &ResolvedMethod,
    code: &DexCodeItem,
) -> Result<Option<MethodIntrinsic>> {
    let Some(shape) = detect_measured_cubic_cut_shape(code) else {
        return Ok(None);
    };
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.object_get_class_method_index,
        "Ljava/lang/Object;",
        "getClass",
        "()Ljava/lang/Class;",
    )? {
        return Ok(None);
    }
    if !method_ref_matches(
        program,
        resolved.dex_file_index,
        shape.coerce_method_index,
        "Landroidx/glance/appwidget/protobuf/a0;",
        "r",
        "(FFF)F",
    )? {
        return Ok(None);
    }
    if !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        shape.size_field_index,
        "F",
    )? || !field_ref_type_matches(
        program,
        resolved.dex_file_index,
        shape.start_field_index,
        "F",
    )? || !field_ref_type_matches(program, resolved.dex_file_index, shape.end_field_index, "F")?
        || !field_ref_type_matches(
            program,
            resolved.dex_file_index,
            shape.center_x_field_index,
            "F",
        )?
        || !field_ref_type_matches(
            program,
            resolved.dex_file_index,
            shape.center_y_field_index,
            "F",
        )?
        || !field_ref_type_matches(
            program,
            resolved.dex_file_index,
            shape.cubic_points_field_index,
            "[F",
        )?
    {
        return Ok(None);
    }

    let cubic_field = program.field_id(resolved.dex_file_index, shape.cubic_field_index)?;
    let owner_field = program.field_id(resolved.dex_file_index, shape.owner_field_index)?;
    let owner_center_field =
        program.field_id(resolved.dex_file_index, shape.owner_center_field_index)?;
    if !cubic_field.type_descriptor.starts_with('L')
        || !owner_field.type_descriptor.starts_with('L')
        || !owner_center_field.type_descriptor.starts_with('L')
    {
        return Ok(None);
    }
    let split_method = program.method_id(resolved.dex_file_index, shape.split_method_index)?;
    if split_method.class_descriptor != cubic_field.type_descriptor
        || !split_method.proto.descriptor_is("(F)LG4/k;")
    {
        return Ok(None);
    }
    if !method_ref_measured_cubic_constructor_matches(
        program,
        resolved.dex_file_index,
        shape.measured_constructor_method_index,
        &resolved.class_descriptor,
        &owner_field.type_descriptor,
        &cubic_field.type_descriptor,
    )? {
        return Ok(None);
    }

    let Some((pair_first_field_index, pair_second_field_index)) =
        method_ref_pair_constructor_fields(
            program,
            resolved.dex_file_index,
            shape.pair_constructor_method_index,
        )?
    else {
        return Ok(None);
    };
    let pair_first = program.field_id(resolved.dex_file_index, pair_first_field_index)?;
    let pair_second = program.field_id(resolved.dex_file_index, pair_second_field_index)?;
    if pair_first.class_descriptor != resolved.method.proto.return_type
        || pair_second.class_descriptor != resolved.method.proto.return_type
        || pair_first.type_descriptor != "Ljava/lang/Object;"
        || pair_second.type_descriptor != "Ljava/lang/Object;"
    {
        return Ok(None);
    }

    let angle_intrinsic =
        method_ref_intrinsic(program, resolved.dex_file_index, shape.angle_method_index)?;
    if !matches!(
        angle_intrinsic,
        Some(MethodIntrinsic::FloatAtan2Normalize { .. })
    ) {
        return Ok(None);
    }
    let path_delta_intrinsic = method_ref_intrinsic(
        program,
        resolved.dex_file_index,
        shape.path_delta_method_index,
    )?;
    let Some(MethodIntrinsic::PathAngleDelta {
        curve_array_field_index,
        anchor_x_field_index,
        anchor_y_field_index,
        ..
    }) = path_delta_intrinsic
    else {
        return Ok(None);
    };
    if curve_array_field_index != shape.cubic_points_field_index
        || anchor_x_field_index != shape.center_x_field_index
        || anchor_y_field_index != shape.center_y_field_index
    {
        return Ok(None);
    }

    Ok(Some(MethodIntrinsic::MeasuredCubicCut {
        receiver_arg: 0,
        progress_arg: 1,
        cubic_field_index: shape.cubic_field_index,
        size_field_index: shape.size_field_index,
        start_field_index: shape.start_field_index,
        end_field_index: shape.end_field_index,
        owner_field_index: shape.owner_field_index,
        owner_center_field_index: shape.owner_center_field_index,
        center_x_field_index: shape.center_x_field_index,
        center_y_field_index: shape.center_y_field_index,
        cubic_points_field_index: shape.cubic_points_field_index,
        pair_first_field_index,
        pair_second_field_index,
    }))
}

#[derive(Clone, Copy)]
struct MeasuredCubicCutShape {
    coerce_method_index: u32,
    object_get_class_method_index: u32,
    angle_method_index: u32,
    path_delta_method_index: u32,
    split_method_index: u32,
    measured_constructor_method_index: u32,
    pair_constructor_method_index: u32,
    cubic_field_index: u32,
    size_field_index: u32,
    start_field_index: u32,
    end_field_index: u32,
    owner_field_index: u32,
    owner_center_field_index: u32,
    center_x_field_index: u32,
    center_y_field_index: u32,
    cubic_points_field_index: u32,
}

fn detect_measured_cubic_cut_shape(code: &DexCodeItem) -> Option<MeasuredCubicCutShape> {
    let ins = &code.instructions;
    if code.registers_size != 13 || code.ins_size != 2 || code.outs_size != 5 || ins.len() != 148 {
        return None;
    }
    let fixed = [
        (0, 0xb052),
        (2, 0xb152),
        (4, 0x3071),
        (6, 0x010c),
        (7, 0x0c0a),
        (8, 0xb052),
        (10, 0xb152),
        (12, 0x10c7),
        (13, 0x01a7),
        (14, 0x010c),
        (15, 0x01c9),
        (16, 0xb054),
        (18, 0x0254),
        (20, 0xb352),
        (22, 0x31c8),
        (23, 0x106e),
        (25, 0x0002),
        (26, 0xb354),
        (28, 0x041a),
        (30, 0x2071),
        (32, 0x0043),
        (33, 0x3454),
        (35, 0x0512),
        (36, 0x0544),
        (37, 0x0504),
        (38, 0x2652),
        (40, 0x65c7),
        (41, 0x1612),
        (42, 0x0444),
        (43, 0x0604),
        (44, 0x2652),
        (46, 0x64c7),
        (47, 0x2071),
        (49, 0x0045),
        (50, 0x040a),
        (51, 0x0522),
        (53, 0x5170),
        (55, 0x4235),
        (56, 0x0115),
        (57, 0x3f80),
        (58, 0x0212),
        (59, 0x1401),
        (60, 0x2601),
        (61, 0x07a7),
        (62, 0x0604),
        (63, 0x0814),
        (64, 0xc5ac),
        (65, 0x3727),
        (66, 0x072d),
        (67, 0x0807),
        (68, 0x2812),
        (69, 0x073d),
        (70, 0x001c),
        (71, 0x8782),
        (72, 0x08a8),
        (73, 0x0607),
        (74, 0x48c6),
        (75, 0x3912),
        (76, 0x9982),
        (77, 0x98c9),
        (78, 0x47c8),
        (79, 0x67c6),
        (80, 0x97c9),
        (81, 0x206e),
        (83, 0x0085),
        (84, 0x090a),
        (85, 0x206e),
        (87, 0x0075),
        (88, 0x0a0a),
        (89, 0x092e),
        (90, 0x0a09),
        (91, 0x093b),
        (92, 0x0004),
        (93, 0x7401),
        (94, 0xdf28),
        (95, 0x8601),
        (96, 0xdd28),
        (97, 0x46c6),
        (98, 0x8482),
        (99, 0x46c9),
        (100, 0x022e),
        (101, 0x0602),
        (102, 0x023c),
        (103, 0x0026),
        (104, 0x012e),
        (105, 0x0106),
        (106, 0x013c),
        (107, 0x0022),
        (108, 0x206e),
        (110, 0x0063),
        (111, 0x010c),
        (112, 0x1254),
        (114, 0x021f),
        (116, 0x1154),
        (118, 0x011f),
        (120, 0x0322),
        (122, 0xb452),
        (124, 0x5c70),
        (126, 0x4203),
        (127, 0x0222),
        (129, 0xb452),
        (131, 0x5470),
        (133, 0xc102),
        (134, 0x0c22),
        (136, 0x3070),
        (138, 0x023c),
        (139, 0x0c11),
        (140, 0x0c22),
        (142, 0x001a),
        (144, 0x2070),
        (146, 0x000c),
        (147, 0x0c27),
    ];
    for (index, expected) in fixed {
        if ins[index] != expected {
            return None;
        }
    }
    if ins[1] != ins[11] || ins[1] != ins[123] {
        return None;
    }
    if ins[3] != ins[9] || ins[3] != ins[130] {
        return None;
    }
    if ins[125] != ins[132] {
        return None;
    }

    Some(MeasuredCubicCutShape {
        coerce_method_index: u32::from(ins[5]),
        object_get_class_method_index: u32::from(ins[24]),
        angle_method_index: u32::from(ins[48]),
        path_delta_method_index: u32::from(ins[82]),
        split_method_index: u32::from(ins[109]),
        measured_constructor_method_index: u32::from(ins[125]),
        pair_constructor_method_index: u32::from(ins[137]),
        cubic_field_index: u32::from(ins[27]),
        size_field_index: u32::from(ins[21]),
        start_field_index: u32::from(ins[1]),
        end_field_index: u32::from(ins[3]),
        owner_field_index: u32::from(ins[17]),
        owner_center_field_index: u32::from(ins[19]),
        center_x_field_index: u32::from(ins[39]),
        center_y_field_index: u32::from(ins[45]),
        cubic_points_field_index: u32::from(ins[34]),
    })
}

fn method_ref_measured_cubic_constructor_matches(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
    measured_class_descriptor: &str,
    owner_class_descriptor: &str,
    cubic_class_descriptor: &str,
) -> Result<bool> {
    let method = program.method_id(dex_file_index, method_index)?;
    Ok(method.class_descriptor == measured_class_descriptor
        && method.name == "<init>"
        && method.proto.return_type == "V"
        && method.proto.parameters
            == [
                owner_class_descriptor.to_owned(),
                cubic_class_descriptor.to_owned(),
                "F".to_owned(),
                "F".to_owned(),
            ])
}

fn method_ref_pair_constructor_fields(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
) -> Result<Option<(u32, u32)>> {
    let method = program.method_id(dex_file_index, method_index)?;
    if method.name != "<init>"
        || !method
            .proto
            .descriptor_is("(Ljava/lang/Object;Ljava/lang/Object;)V")
    {
        return Ok(None);
    }
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };
    let code = program.code_item(&resolved)?;
    let ins = &code.instructions;
    if code.registers_size != 3
        || code.ins_size != 3
        || code.outs_size != 1
        || ins.len() != 8
        || ins[0] != 0x1070
        || ins[3] != 0x015b
        || ins[5] != 0x025b
        || ins[7] != 0x000e
    {
        return Ok(None);
    }
    let first_field_index = u32::from(ins[4]);
    let second_field_index = u32::from(ins[6]);
    let first_field = program.field_id(dex_file_index, first_field_index)?;
    let second_field = program.field_id(dex_file_index, second_field_index)?;
    if first_field.class_descriptor != method.class_descriptor
        || second_field.class_descriptor != method.class_descriptor
        || first_field.type_descriptor != "Ljava/lang/Object;"
        || second_field.type_descriptor != "Ljava/lang/Object;"
    {
        return Ok(None);
    }

    Ok(Some((first_field_index, second_field_index)))
}

fn invoke_static_one_arg(
    program: &DexProgram,
    dex_file_index: usize,
    instructions: &[u16],
    pc: usize,
    class_descriptor: &str,
    name: &str,
    proto_descriptor: &str,
) -> Result<Option<u16>> {
    let Some(instruction) = instructions.get(pc).copied() else {
        return Ok(None);
    };
    if dex_opcode(instruction) != 0x71 || ((instruction >> 12) & 0x0f) != 1 {
        return Ok(None);
    }
    let Some(method_index) = instructions.get(pc + 1).copied().map(u32::from) else {
        return Ok(None);
    };
    let Some(register_word) = instructions.get(pc + 2).copied() else {
        return Ok(None);
    };
    if !method_ref_matches(
        program,
        dex_file_index,
        method_index,
        class_descriptor,
        name,
        proto_descriptor,
    )? {
        return Ok(None);
    }

    Ok(Some(register_word & 0x000f))
}

fn invoke_static_args(
    program: &DexProgram,
    dex_file_index: usize,
    instructions: &[u16],
    pc: usize,
    class_descriptor: &str,
    name: &str,
    proto_descriptor: &str,
) -> Result<Option<Vec<u16>>> {
    let Some(instruction) = instructions.get(pc).copied() else {
        return Ok(None);
    };
    if dex_opcode(instruction) != 0x71 {
        return Ok(None);
    }
    let argument_count = ((instruction >> 12) & 0x0f) as usize;
    let Some(method_index) = instructions.get(pc + 1).copied().map(u32::from) else {
        return Ok(None);
    };
    let Some(register_word) = instructions.get(pc + 2).copied() else {
        return Ok(None);
    };
    if !method_ref_matches(
        program,
        dex_file_index,
        method_index,
        class_descriptor,
        name,
        proto_descriptor,
    )? {
        return Ok(None);
    }

    let candidates = [
        register_word & 0x000f,
        (register_word >> 4) & 0x000f,
        (register_word >> 8) & 0x000f,
        (register_word >> 12) & 0x000f,
        (instruction >> 8) & 0x000f,
    ];

    Ok(Some(candidates.into_iter().take(argument_count).collect()))
}

fn invoke_35c_args_matching_proto(
    program: &DexProgram,
    dex_file_index: usize,
    instructions: &[u16],
    pc: usize,
    opcode: u8,
    proto_descriptor: &str,
) -> Result<Option<Vec<u16>>> {
    let Some(instruction) = instructions.get(pc).copied() else {
        return Ok(None);
    };
    if dex_opcode(instruction) != opcode {
        return Ok(None);
    }
    let argument_count = ((instruction >> 12) & 0x0f) as usize;
    let Some(method_index) = instructions.get(pc + 1).copied().map(u32::from) else {
        return Ok(None);
    };
    let Some(register_word) = instructions.get(pc + 2).copied() else {
        return Ok(None);
    };
    if !method_ref_proto_matches(program, dex_file_index, method_index, proto_descriptor)? {
        return Ok(None);
    }

    let candidates = [
        register_word & 0x000f,
        (register_word >> 4) & 0x000f,
        (register_word >> 8) & 0x000f,
        (register_word >> 12) & 0x000f,
        (instruction >> 8) & 0x000f,
    ];

    Ok(Some(candidates.into_iter().take(argument_count).collect()))
}

fn method_ref_matches(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
    class_descriptor: &str,
    name: &str,
    proto_descriptor: &str,
) -> Result<bool> {
    let method = program.method_id(dex_file_index, method_index)?;
    Ok(method.class_descriptor == class_descriptor
        && method.name == name
        && method.proto.descriptor_is(proto_descriptor))
}

fn method_ref_proto_matches(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
    proto_descriptor: &str,
) -> Result<bool> {
    let method = program.method_id(dex_file_index, method_index)?;
    Ok(method.proto.descriptor_is(proto_descriptor))
}

fn field_ref_type_matches(
    program: &DexProgram,
    dex_file_index: usize,
    field_index: u32,
    type_descriptor: &str,
) -> Result<bool> {
    let field = program.field_id(dex_file_index, field_index)?;
    Ok(field.type_descriptor == type_descriptor)
}

fn method_ref_intrinsic(
    program: &DexProgram,
    dex_file_index: usize,
    method_index: u32,
) -> Result<Option<MethodIntrinsic>> {
    let method = program.method_id(dex_file_index, method_index)?;
    let Some(resolved) = program.resolve_exact_method(&method)? else {
        return Ok(None);
    };

    program.method_intrinsic(&resolved)
}

fn pack_float_pair(high: f32, low: f32) -> i64 {
    let high = i64::from(high.to_bits() as i32).wrapping_shl(32);
    let low = i64::from(low.to_bits() as i32) & 0xffff_ffff;
    high | low
}

fn coerce_float_in(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn graphics_vector_angle(x: f32, y: f32) -> f32 {
    let angle = (f64::from(y)).atan2(f64::from(x)) as f32;
    (angle + GRAPHICS_TWO_PI) % GRAPHICS_TWO_PI
}

fn graphics_positive_remainder(value: f32, modulus: f32) -> f32 {
    ((value % modulus) + modulus) % modulus
}

fn measured_cubic_size(values: &[f32; 8], center_x: f32, center_y: f32) -> f32 {
    let end_angle = graphics_vector_angle(values[6] - center_x, values[7] - center_y);
    let start_angle = graphics_vector_angle(values[0] - center_x, values[1] - center_y);
    let delta = graphics_positive_remainder(end_angle - start_angle, GRAPHICS_TWO_PI);
    if delta > GRAPHICS_TWO_PI - 0.0001 {
        0.0
    } else {
        delta
    }
}

fn measured_cubic_cut_t(
    values: &[f32; 8],
    center_x: f32,
    center_y: f32,
    start_angle: f32,
    target: f32,
) -> f32 {
    let mut upper = 1.0;
    let mut lower = 0.0;
    while upper - lower > 0.00001 {
        let left = ((2.0 * lower) + upper) / 3.0;
        let right = ((2.0 * upper) + lower) / 3.0;
        if measured_cubic_cut_error(values, center_x, center_y, start_angle, target, left)
            < measured_cubic_cut_error(values, center_x, center_y, start_angle, target, right)
        {
            upper = right;
        } else {
            lower = left;
        }
    }

    (lower + upper) / 2.0
}

fn measured_cubic_cut_error(
    values: &[f32; 8],
    center_x: f32,
    center_y: f32,
    start_angle: f32,
    target: f32,
    t: f32,
) -> f32 {
    let (x, y) = cubic_point(values, t);
    let angle = graphics_vector_angle(x - center_x, y - center_y);
    let delta = graphics_positive_remainder(angle - start_angle, GRAPHICS_TWO_PI);
    (delta - target).abs()
}

fn cubic_point(values: &[f32; 8], t: f32) -> (f32, f32) {
    let inverse = 1.0 - t;
    let inverse_cubed = inverse * inverse * inverse;
    let three_t = 3.0 * t;
    let first_mid = three_t * inverse * inverse;
    let second_mid = three_t * t * inverse;
    let t_cubed = t * t * t;
    let x = (values[6] * t_cubed)
        + (values[4] * second_mid)
        + (values[2] * first_mid)
        + (values[0] * inverse_cubed);
    let y = (values[7] * t_cubed)
        + (values[5] * second_mid)
        + (values[3] * first_mid)
        + (values[1] * inverse_cubed);

    (x, y)
}

fn split_cubic_values(values: &[f32; 8], t: f32) -> ([f32; 8], [f32; 8]) {
    let inverse = 1.0 - t;
    let (point_x, point_y) = cubic_point(values, t);
    let inverse_squared = inverse * inverse;
    let double_inverse_t = 2.0 * inverse * t;
    let t_squared = t * t;
    let left = [
        values[0],
        values[1],
        (values[2] * t) + (values[0] * inverse),
        (values[3] * t) + (values[1] * inverse),
        (values[4] * t_squared) + (values[2] * double_inverse_t) + (values[0] * inverse_squared),
        (values[5] * t_squared) + (values[3] * double_inverse_t) + (values[1] * inverse_squared),
        point_x,
        point_y,
    ];
    let right = [
        point_x,
        point_y,
        (values[6] * t_squared) + (values[4] * double_inverse_t) + (values[2] * inverse_squared),
        (values[7] * t_squared) + (values[5] * double_inverse_t) + (values[3] * inverse_squared),
        (values[6] * t) + (values[4] * inverse),
        (values[7] * t) + (values[5] * inverse),
        values[6],
        values[7],
    ];

    (left, right)
}

fn rounded_polygon_cubic_zero_length(values: &[f32; 8]) -> bool {
    (values[0] - values[6]).abs() < GRAPHICS_DISTANCE_EPSILON
        && (values[1] - values[7]).abs() < GRAPHICS_DISTANCE_EPSILON
}

fn rounded_polygon_cubic_approximate_bounds(values: &[f32; 8]) -> [f32; 4] {
    if rounded_polygon_cubic_zero_length(values) {
        return [values[0], values[1], values[0], values[1]];
    }
    [
        intrinsic_math_min_float(
            intrinsic_math_min_float(values[0], values[6]),
            intrinsic_math_min_float(values[2], values[4]),
        ),
        intrinsic_math_min_float(
            intrinsic_math_min_float(values[1], values[7]),
            intrinsic_math_min_float(values[3], values[5]),
        ),
        intrinsic_math_max_float(
            intrinsic_math_max_float(values[0], values[6]),
            intrinsic_math_max_float(values[2], values[4]),
        ),
        intrinsic_math_max_float(
            intrinsic_math_max_float(values[1], values[7]),
            intrinsic_math_max_float(values[3], values[5]),
        ),
    ]
}

fn intrinsic_math_min_float(left: f32, right: f32) -> f32 {
    if left.is_nan() || right.is_nan() {
        return f32::NAN;
    }
    if left == right {
        if left == 0.0 && (left.is_sign_negative() || right.is_sign_negative()) {
            return -0.0;
        }
        return left;
    }
    if left < right {
        left
    } else {
        right
    }
}

fn intrinsic_math_max_float(left: f32, right: f32) -> f32 {
    if left.is_nan() || right.is_nan() {
        return f32::NAN;
    }
    if left == right {
        if left == 0.0 && (left.is_sign_positive() || right.is_sign_positive()) {
            return 0.0;
        }
        return left;
    }
    if left > right {
        left
    } else {
        right
    }
}

fn rounded_polygon_cubics_are_contiguous(cubics: &[IntrinsicCubicObject]) -> bool {
    let Some(mut previous) = cubics.last() else {
        return false;
    };
    for cubic in cubics {
        if (cubic.values[0] - previous.values[6]).abs() > GRAPHICS_DISTANCE_EPSILON
            || (cubic.values[1] - previous.values[7]).abs() > GRAPHICS_DISTANCE_EPSILON
        {
            return false;
        }
        previous = cubic;
    }

    true
}

const GRAPHICS_TWO_PI: f32 = 6.2831855;
const GRAPHICS_DISTANCE_EPSILON: f32 = 0.0001;

fn intrinsic_value_matches_field_descriptor(type_descriptor: &str, value: &DexValue) -> bool {
    match type_descriptor {
        "Z" | "B" | "C" | "S" | "I" => matches!(value, DexValue::Int(_)),
        "J" => matches!(value, DexValue::Long(_)),
        "F" => matches!(value, DexValue::Float(_)),
        _ if type_descriptor.starts_with('L') || type_descriptor.starts_with('[') => matches!(
            value,
            DexValue::Null | DexValue::String(_) | DexValue::Object(_) | DexValue::Reference(_)
        ),
        _ => false,
    }
}

fn intrinsic_value_matches_type(value_type: IntrinsicFieldValueType, value: &DexValue) -> bool {
    match (value_type, value) {
        (IntrinsicFieldValueType::Int, DexValue::Int(_))
        | (IntrinsicFieldValueType::Long, DexValue::Long(_))
        | (IntrinsicFieldValueType::Float, DexValue::Float(_))
        | (IntrinsicFieldValueType::Object, DexValue::Null)
        | (IntrinsicFieldValueType::Object, DexValue::String(_))
        | (IntrinsicFieldValueType::Object, DexValue::Object(_))
        | (IntrinsicFieldValueType::Object, DexValue::Reference(_)) => true,
        _ => false,
    }
}

fn read_long_argument(value: &DexValue) -> Option<i64> {
    match value {
        DexValue::Long(value) => Some(*value),
        _ => None,
    }
}

fn read_int_argument(value: &DexValue) -> Option<i32> {
    match value {
        DexValue::Int(value) => Some(*value),
        _ => None,
    }
}

fn parameter_word_index(code: &DexCodeItem, register: u16) -> Option<usize> {
    let first_parameter = code.registers_size.checked_sub(code.ins_size)?;
    if register < first_parameter {
        return None;
    }
    let index = register - first_parameter;
    if index >= code.ins_size {
        return None;
    }
    Some(index as usize)
}

fn dex_opcode(instruction: u16) -> u8 {
    (instruction & 0x00ff) as u8
}

fn dex_high_byte(instruction: u16) -> u16 {
    instruction >> 8
}

fn dex_nibble_a(instruction: u16) -> u16 {
    (instruction >> 8) & 0x000f
}

fn dex_nibble_b(instruction: u16) -> u16 {
    (instruction >> 12) & 0x000f
}

fn dex_12x(instruction: u16, opcode: u8) -> Option<(u16, u16)> {
    if dex_opcode(instruction) != opcode {
        return None;
    }

    Some((dex_nibble_a(instruction), dex_nibble_b(instruction)))
}

fn dex_23x(instruction: u16, next: u16, opcode: u8) -> Option<(u16, u16, u16)> {
    if dex_opcode(instruction) != opcode {
        return None;
    }

    Some((
        dex_high_byte(instruction),
        next & 0x00ff,
        (next >> 8) & 0x00ff,
    ))
}

fn dex_22b(instruction: u16, next: u16, opcode: u8) -> Option<(u16, u16, i8)> {
    if dex_opcode(instruction) != opcode {
        return None;
    }

    Some((dex_high_byte(instruction), next & 0x00ff, (next >> 8) as i8))
}

fn sign_extend_dex_nibble(value: u16) -> i32 {
    let value = value as i32;
    if value & 0x08 == 0 {
        value
    } else {
        value | !0x0f
    }
}

fn receiver_dispatch_descriptor(invocation: &DexInvocation) -> Option<&str> {
    if !matches!(
        invocation.kind,
        DexInvokeKind::Virtual | DexInvokeKind::Interface
    ) {
        return None;
    }

    value_class_descriptor(invocation.arguments.first()?)
}

fn object_receiver_dispatch_descriptor(invocation: &DexInvocation) -> Option<&str> {
    if !matches!(
        invocation.kind,
        DexInvokeKind::Virtual | DexInvokeKind::Interface
    ) {
        return None;
    }

    let Some(DexValue::Object(receiver)) = invocation.arguments.first() else {
        return None;
    };

    Some(receiver.class_descriptor.as_ref())
}

fn window_add_flags(invocation: &DexInvocation) -> Option<()> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    match (&invocation.arguments[0], &invocation.arguments[1]) {
        (DexValue::Object(_) | DexValue::Reference(_), DexValue::Int(_)) => Some(()),
        _ => None,
    }
}

fn layout_transition_type(invocation: &DexInvocation) -> Option<()> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    match (&invocation.arguments[0], &invocation.arguments[1]) {
        (DexValue::Object(_) | DexValue::Reference(_), DexValue::Int(_)) => Some(()),
        _ => None,
    }
}

fn push_default_entrypoint_argument(arguments: &mut Vec<DexValue>, descriptor: &str) -> Option<()> {
    if descriptor.starts_with('L') || descriptor.starts_with('[') {
        arguments.push(DexValue::Null);
        return Some(());
    }

    match descriptor {
        "Z" | "B" | "C" | "S" | "I" => arguments.push(DexValue::Int(0)),
        "F" => arguments.push(DexValue::Float(0.0)),
        "J" | "D" => {
            arguments.push(DexValue::Long(0));
            arguments.push(DexValue::Long(0));
        }
        _ => return None,
    }

    Some(())
}

fn uuid_to_string(invocation: &DexInvocation) -> Option<String> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    let DexValue::Reference(reference) = &invocation.arguments[0] else {
        return None;
    };

    reference
        .strip_prefix("Ljava/util/UUID;:")
        .map(str::to_owned)
}

fn time_unit_to_millis(invocation: &DexInvocation) -> Option<i64> {
    let (unit, value) = time_unit_argument(invocation)?;

    match unit {
        "NANOSECONDS" => Some(value / 1_000_000),
        "MICROSECONDS" => Some(value / 1_000),
        "MILLISECONDS" => Some(value),
        "SECONDS" => Some(value.saturating_mul(1_000)),
        "MINUTES" => Some(value.saturating_mul(60_000)),
        "HOURS" => Some(value.saturating_mul(3_600_000)),
        "DAYS" => Some(value.saturating_mul(86_400_000)),
        _ => None,
    }
}

fn time_unit_convert(invocation: &DexInvocation) -> Option<i64> {
    if invocation.arguments.len() != 4 {
        return None;
    }

    let target_unit = time_unit_reference_name(&invocation.arguments[0])?;
    let DexValue::Long(duration) = invocation.arguments[1] else {
        return None;
    };
    let DexValue::Long(high_slot_duration) = invocation.arguments[2] else {
        return None;
    };
    let source_unit = time_unit_reference_name(&invocation.arguments[3])?;

    if duration != high_slot_duration {
        return None;
    }

    let source_nanos = i128::from(time_unit_nanos(source_unit)?);
    let target_nanos = i128::from(time_unit_nanos(target_unit)?);
    let converted = i128::from(duration) * source_nanos / target_nanos;

    Some(saturating_i128_to_i64(converted))
}

fn time_unit_to_nanos(invocation: &DexInvocation) -> Option<i64> {
    let (unit, value) = time_unit_argument(invocation)?;

    match unit {
        "NANOSECONDS" => Some(value),
        "MICROSECONDS" => Some(value.saturating_mul(1_000)),
        "MILLISECONDS" => Some(value.saturating_mul(1_000_000)),
        "SECONDS" => Some(value.saturating_mul(1_000_000_000)),
        "MINUTES" => Some(value.saturating_mul(60_000_000_000)),
        "HOURS" => Some(value.saturating_mul(3_600_000_000_000)),
        "DAYS" => Some(value.saturating_mul(86_400_000_000_000)),
        _ => None,
    }
}

fn time_unit_nanos(unit: &str) -> Option<i64> {
    match unit {
        "NANOSECONDS" => Some(1),
        "MICROSECONDS" => Some(1_000),
        "MILLISECONDS" => Some(1_000_000),
        "SECONDS" => Some(1_000_000_000),
        "MINUTES" => Some(60_000_000_000),
        "HOURS" => Some(3_600_000_000_000),
        "DAYS" => Some(86_400_000_000_000),
        _ => None,
    }
}

fn saturating_i128_to_i64(value: i128) -> i64 {
    if value > i128::from(i64::MAX) {
        return i64::MAX;
    }

    if value < i128::from(i64::MIN) {
        return i64::MIN;
    }

    value as i64
}

fn time_unit_argument(invocation: &DexInvocation) -> Option<(&str, i64)> {
    if invocation.arguments.len() != 3 {
        return None;
    }

    let unit = time_unit_reference_name(&invocation.arguments[0])?;
    let DexValue::Long(value) = invocation.arguments[1] else {
        return None;
    };
    let DexValue::Long(high_slot_value) = invocation.arguments[2] else {
        return None;
    };

    if value != high_slot_value {
        return None;
    }

    Some((unit, value))
}

fn time_unit_reference_name(value: &DexValue) -> Option<&str> {
    let DexValue::Reference(reference) = value else {
        return None;
    };

    reference.strip_prefix("Ljava/util/concurrent/TimeUnit;.")
}

fn work_infos_by_tag_future(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    let DexValue::String(tag) = &invocation.arguments[1] else {
        return None;
    };

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Lcom/google/common/util/concurrent/ListenableFuture;:workInfosByTag:{tag}"
    ))))
}

fn future_get(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    let DexValue::Reference(reference) = &invocation.arguments[0] else {
        return None;
    };
    let tag = reference
        .strip_prefix("Lcom/google/common/util/concurrent/ListenableFuture;:workInfosByTag:")?;

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Ljava/util/List;:workInfosByTag:{tag}:empty"
    ))))
}

fn work_manager_enqueue_unique_work(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 4 {
        return None;
    }

    let DexValue::String(name) = &invocation.arguments[1] else {
        return None;
    };

    match &invocation.arguments[2] {
        DexValue::Reference(reference)
            if reference.starts_with("Landroidx/work/ExistingWorkPolicy;.") => {}
        _ => return None,
    }

    match &invocation.arguments[3] {
        DexValue::Reference(_) | DexValue::Object(_) => {}
        _ => return None,
    }

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Landroidx/work/Operation;:enqueueUniqueWork:{}:{}",
        name,
        map_identity_key(&invocation.arguments[3])
    ))))
}

fn work_request_builder_build_internal(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Landroidx/work/WorkRequest;:built:{}",
        map_identity_key(&invocation.arguments[0])
    ))))
}

fn lifecycle_state_ordinal(value: &DexValue) -> Option<i32> {
    let DexValue::Reference(reference) = value else {
        return None;
    };

    if let Some(name) = reference
        .strip_prefix("Landroidx/lifecycle/l;.")
        .and_then(|value| value.split(':').next())
    {
        return match name {
            "a" => Some(0),
            "b" => Some(1),
            "c" => Some(2),
            "d" => Some(3),
            "e" => Some(4),
            _ => None,
        };
    }

    if reference.ends_with(".DESTROYED") {
        return Some(0);
    }

    if reference.ends_with(".INITIALIZED") {
        return Some(1);
    }

    if reference.ends_with(".CREATED") {
        return Some(2);
    }

    if reference.ends_with(".STARTED") {
        return Some(3);
    }

    if reference.ends_with(".RESUMED") {
        return Some(4);
    }

    None
}

fn lifecycle_event_ordinal(value: &DexValue) -> Option<i32> {
    let DexValue::Reference(reference) = value else {
        return None;
    };

    let name = if let Some(name) = reference.strip_prefix("Landroidx/lifecycle/k;.") {
        name
    } else if let Some(name) = reference.strip_prefix("Landroidx/lifecycle/Lifecycle$Event;.") {
        name
    } else {
        return None;
    };
    let name = name.split(':').next().unwrap_or(name);

    match name {
        "ON_CREATE" => Some(0),
        "ON_START" => Some(1),
        "ON_RESUME" => Some(2),
        "ON_PAUSE" => Some(3),
        "ON_STOP" => Some(4),
        "ON_DESTROY" => Some(5),
        "ON_ANY" => Some(6),
        _ => None,
    }
}

fn enum_reference_name(value: &DexValue) -> Option<String> {
    let DexValue::Reference(reference) = value else {
        return None;
    };

    let field_start = reference.find(";.")? + 2;
    let field_end = reference[field_start..]
        .find(':')
        .map(|offset| field_start + offset)
        .unwrap_or(reference.len());

    reference.get(field_start..field_end).map(str::to_owned)
}

fn lifecycle_event_target_state(arguments: &[DexValue]) -> Option<DexValue> {
    let event = arguments.first()?;
    let DexValue::Reference(reference) = event else {
        return None;
    };

    if reference.ends_with(".ON_CREATE") || reference.ends_with(".ON_STOP") {
        return Some(DexValue::Reference(std::rc::Rc::from(
            "Landroidx/lifecycle/Lifecycle$State;.CREATED",
        )));
    }

    if reference.ends_with(".ON_START") || reference.ends_with(".ON_PAUSE") {
        return Some(DexValue::Reference(std::rc::Rc::from(
            "Landroidx/lifecycle/Lifecycle$State;.STARTED",
        )));
    }

    if reference.ends_with(".ON_RESUME") {
        return Some(DexValue::Reference(std::rc::Rc::from(
            "Landroidx/lifecycle/Lifecycle$State;.RESUMED",
        )));
    }

    if reference.ends_with(".ON_DESTROY") {
        return Some(DexValue::Reference(std::rc::Rc::from(
            "Landroidx/lifecycle/Lifecycle$State;.DESTROYED",
        )));
    }

    None
}

fn obfuscated_lifecycle_event_target_state(arguments: &[DexValue]) -> Option<DexValue> {
    let event = arguments.first()?;
    let DexValue::Reference(reference) = event else {
        return None;
    };
    let name = reference
        .strip_prefix("Landroidx/lifecycle/k;.")
        .and_then(|value| value.split(':').next())?;

    let state = match name {
        "ON_CREATE" | "ON_STOP" => "c",
        "ON_START" | "ON_PAUSE" => "d",
        "ON_RESUME" => "e",
        "ON_DESTROY" => "a",
        _ => return None,
    };

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Landroidx/lifecycle/l;.{state}:Landroidx/lifecycle/l;"
    ))))
}

// cite: frameworks-base ActivityThread.java#main
fn main_guest_thread_reference() -> DexValue {
    DexValue::Reference(std::rc::Rc::from("Ljava/lang/Thread;:main"))
}

// cite: libcore ThreadPoolExecutor.java#runWorker
fn background_pool_thread_reference() -> DexValue {
    DexValue::Reference(std::rc::Rc::from("Ljava/lang/Thread;:pool"))
}

// cite: frameworks-base Activity.java#runOnUiThread
const MAIN_THREAD_HANDLER_KEY: &str = "Landroid/app/Activity;.runOnUiThread";

fn future_task_get_cause_field() -> DexFieldId {
    // cite: libcore Throwable.java#Throwable
    DexFieldId {
        class_descriptor: "Ljava/lang/Throwable;".to_owned(),
        type_descriptor: "Ljava/lang/Throwable;".to_owned(),
        name: "cause".to_owned(),
    }
}

fn argument_registers_display(registers: &[u16]) -> String {
    if registers.is_empty() {
        return "none".to_owned();
    }

    registers
        .iter()
        .map(|register| format!("v{register}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn cached_env_flag(cell: &'static std::sync::OnceLock<bool>, name: &str) -> bool {
    *cell.get_or_init(|| env::var_os(name).is_some())
}

static DEX_TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static DEX_TRACE_RETURNS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static DEX_TRACE_FIELD_WRITES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn method_trace_prefixes() -> Option<&'static [String]> {
    use std::sync::OnceLock;
    static PREFIXES: OnceLock<Option<Vec<String>>> = OnceLock::new();
    PREFIXES
        .get_or_init(|| {
            env::var("WINEDROID_METHOD_TRACE").ok().map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|prefix| !prefix.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
        })
        .as_deref()
}

fn trace_invocation(depth: usize, invocation: &DexInvocation, method: &DexMethodId) {
    if let Some(prefixes) = method_trace_prefixes() {
        if prefixes
            .iter()
            .any(|prefix| method.class_descriptor.starts_with(prefix.as_str()))
        {
            eprintln!(
                "[method-trace] d{depth} {}.{} args: {}",
                method.class_descriptor,
                method.display_signature(),
                invocation_arguments_display(&invocation.arguments)
            );
        }
    }
    if !cached_env_flag(&DEX_TRACE, "WINEDROID_DEX_TRACE") {
        return;
    }

    eprintln!(
        "{}{} {}.{} args: {}",
        "  ".repeat(depth),
        invocation.instruction_name(),
        method.class_descriptor,
        method.display_signature(),
        invocation_arguments_display(&invocation.arguments)
    );
}

fn trace_method_result(depth: usize, resolved: &ResolvedMethod, result: &DexExecutionResult) {
    if !cached_env_flag(&DEX_TRACE_RETURNS, "WINEDROID_DEX_TRACE_RETURNS") {
        return;
    }

    eprintln!(
        "{}return {}.{} -> {}",
        "  ".repeat(depth),
        resolved.class_descriptor,
        resolved.method.display_signature(),
        dex_execution_result_display(result)
    );
}

fn trace_check_cast_failure(depth: usize, source_descriptor: &str, target_descriptor: &str) {
    if !cached_env_flag(&DEX_TRACE, "WINEDROID_DEX_TRACE") {
        return;
    }

    eprintln!(
        "{}check-cast {} -> {} failed",
        "  ".repeat(depth),
        source_descriptor,
        target_descriptor
    );
}

fn trace_instance_field_read(
    depth: usize,
    access: &DexFieldAccess,
    field: &DexFieldId,
    value: &DexValue,
    source: &str,
) {
    if !cached_env_flag(&DEX_TRACE, "WINEDROID_DEX_TRACE") {
        return;
    }

    eprintln!(
        "{}iget {} -> {} ({source}; object v{}, destination v{})",
        "  ".repeat(depth),
        field.display_signature(),
        value,
        access.object_register,
        access.destination_register
    );
}

fn trace_instance_field_write(depth: usize, write: &DexFieldWrite, field: &DexFieldId) {
    if !cached_env_flag(&DEX_TRACE_FIELD_WRITES, "WINEDROID_DEX_TRACE_FIELD_WRITES") {
        return;
    }

    eprintln!(
        "{}iput {} <- {} (object {}, value register v{})",
        "  ".repeat(depth),
        field.display_signature(),
        write.value,
        write.object,
        write.value_register
    );
}

fn dex_execution_result_display(result: &DexExecutionResult) -> String {
    match result {
        DexExecutionResult::Void => "void".to_owned(),
        DexExecutionResult::Int(value) => format!("int {value}"),
        DexExecutionResult::Value(value) => value.to_string(),
        DexExecutionResult::Throw(value) => format!("throw {value}"),
        DexExecutionResult::Parked(_) => "parked".to_owned(),
    }
}

fn trace_static_field_read(
    depth: usize,
    access: &DexStaticFieldAccess,
    field: &DexFieldId,
    value: &DexValue,
    source: &str,
) {
    if !cached_env_flag(&DEX_TRACE, "WINEDROID_DEX_TRACE") {
        return;
    }

    eprintln!(
        "{}sget {} -> {} ({source}; destination v{})",
        "  ".repeat(depth),
        field.display_signature(),
        value,
        access.destination_register
    );
}

fn invocation_arguments_display(arguments: &[DexValue]) -> String {
    if arguments.is_empty() {
        return "none".to_owned();
    }

    arguments
        .iter()
        .map(|argument| argument.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

struct ApkManifestContext {
    package_name: Option<String>,
    min_sdk_version: i32,
    target_sdk_version: i32,
    request_legacy_external_storage: bool,
    default_private_storage: crate::guest_filesystem::PrivateStorageKind,
    requested_permissions: Vec<String>,
    application_class_descriptor: Option<String>,
    activity_themes: HashMap<String, u32>,
    application_theme: Option<u32>,
    activity_labels: HashMap<String, u32>,
    application_label: Option<u32>,
}

impl ApkManifestContext {
    fn from_manifest(manifest: manifest::AndroidManifest) -> Result<Self> {
        let min_sdk_version = manifest.effective_min_sdk_version()?;
        let target_sdk_version = manifest.effective_target_sdk_version()?;
        let request_legacy_external_storage = manifest.requests_legacy_external_storage()?;
        // cite: frameworks-base ApplicationInfo.java#initForUser, PRIVATE_FLAG_DEFAULT_TO_DEVICE_PROTECTED_STORAGE
        let default_private_storage = if manifest.defaults_to_device_protected_storage(false) {
            crate::guest_filesystem::PrivateStorageKind::DeviceProtected
        } else {
            crate::guest_filesystem::PrivateStorageKind::CredentialProtected
        };
        let requested_permissions =
            manifest.active_permission_names(BOOTSTRAP_ANDROID_SDK_INT, &HashSet::new());
        let application_class_descriptor = manifest
            .application_name
            .as_deref()
            .map(normalize_class_name);
        let activity_themes = manifest
            .activities
            .iter()
            .filter_map(|activity| {
                let theme = activity.theme?;
                Some((normalize_class_name(&activity.name), theme))
            })
            .collect();
        let activity_labels = manifest
            .activities
            .iter()
            .filter_map(|activity| {
                let label = activity.label?;
                Some((normalize_class_name(&activity.name), label))
            })
            .collect();

        Ok(Self {
            package_name: manifest.package_name,
            min_sdk_version,
            target_sdk_version,
            request_legacy_external_storage,
            default_private_storage,
            requested_permissions,
            application_class_descriptor,
            activity_themes,
            application_theme: manifest.application_theme,
            activity_labels,
            application_label: manifest.application_label,
        })
    }
}

fn load_apk_manifest_context(apk_path: &Path) -> Result<ApkManifestContext> {
    let manifest_data = if let Some(path) =
        installed_runtime_dir(apk_path).map(|dir| dir.join("AndroidManifest.xml"))
    {
        if path.exists() {
            fs::read(&path).map_err(|error| {
                WinedroidError::with_source(
                    format!(
                        "failed to read installed AndroidManifest.xml `{}`",
                        path.display()
                    ),
                    error,
                )
            })?
        } else {
            let mut apk = ApkArchive::open(apk_path)?;
            apk.read_required_file("AndroidManifest.xml")?
        }
    } else {
        let mut apk = ApkArchive::open(apk_path)?;
        apk.read_required_file("AndroidManifest.xml")?
    };
    ApkManifestContext::from_manifest(manifest::parse_manifest(&manifest_data)?)
}

fn load_dex_files(apk_path: &Path) -> Result<Vec<DexFile>> {
    if let Some(files) = load_installed_dex_files(apk_path)? {
        return Ok(files);
    }

    load_dex_files_from_apk(apk_path)
}

fn load_installed_dex_files(apk_path: &Path) -> Result<Option<Vec<DexFile>>> {
    let Some(dex_dir) = installed_runtime_dir(apk_path).map(|dir| dir.join("dex")) else {
        return Ok(None);
    };
    if !dex_dir.exists() {
        return Ok(None);
    }

    let entries = fs::read_dir(&dex_dir).map_err(|error| {
        WinedroidError::with_source(
            format!(
                "failed to read installed DEX directory `{}`",
                dex_dir.display()
            ),
            error,
        )
    })?;
    let mut dex_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to read installed DEX directory `{}`",
                    dex_dir.display()
                ),
                error,
            )
        })?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if dex::is_dex_file(name) {
            dex_paths.push(path);
        }
    }
    dex_paths.sort();
    if dex_paths.is_empty() {
        return Err(WinedroidError::new(format!(
            "installed DEX directory `{}` does not contain any DEX files",
            dex_dir.display()
        )));
    }

    let mut files = Vec::with_capacity(dex_paths.len());
    for path in dex_paths {
        let source_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                WinedroidError::new(format!("invalid installed DEX path `{}`", path.display()))
            })?
            .to_owned();
        let data = {
            let _span = trace::stat_span("perf", "runtime read installed dex file");
            fs::read(&path).map_err(|error| {
                WinedroidError::with_source(
                    format!("failed to read installed DEX file `{}`", path.display()),
                    error,
                )
            })?
        };
        let file = {
            let cache_path = installed_dex_metadata_cache_path(&path)?;
            if cache_path.exists() {
                let cache = {
                    let _span =
                        trace::stat_span("perf", "runtime read installed dex metadata cache");
                    fs::read(&cache_path).map_err(|error| {
                        WinedroidError::with_source(
                            format!(
                                "failed to read installed DEX metadata cache `{}`",
                                cache_path.display()
                            ),
                            error,
                        )
                    })?
                };
                let _span = trace::stat_span("perf", "runtime load installed dex metadata cache");
                dex::parse_dex_file_with_metadata_cache(source_name, data, &cache)?
            } else {
                let _span = trace::stat_span("perf", "runtime parse installed dex file");
                dex::parse_dex_file(source_name, data)?
            }
        };
        files.push(file);
    }

    Ok(Some(files))
}

fn installed_dex_metadata_cache_path(dex_path: &Path) -> Result<PathBuf> {
    let file_name = dex_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| WinedroidError::new(format!("invalid DEX path `{}`", dex_path.display())))?;
    Ok(dex_path.with_file_name(format!("{file_name}.wdidx")))
}

fn load_dex_files_from_apk(apk_path: &Path) -> Result<Vec<DexFile>> {
    let mut apk = {
        let _span = trace::span("runtime", "open APK for full DEX load");
        ApkArchive::open(apk_path)?
    };
    let mut dex_entries = apk
        .entries()?
        .into_iter()
        .filter(|entry| dex::is_dex_file(&entry.name))
        .collect::<Vec<_>>();

    dex_entries.sort_by(|left, right| left.name.cmp(&right.name));

    if dex_entries.is_empty() {
        return Err(WinedroidError::new("APK does not contain any DEX files"));
    }

    let mut files = Vec::with_capacity(dex_entries.len());

    for entry in dex_entries {
        let data = {
            let _span = trace::stat_span("perf", "runtime read full dex entry");
            apk.read_required_file(&entry.name)?
        };
        let file = {
            let _span = trace::stat_span("perf", "runtime parse full dex file");
            dex::parse_dex_file(entry.name, data)?
        };
        files.push(file);
    }

    Ok(files)
}

fn load_apk_resources(apk_path: &Path) -> Result<Option<Vec<u8>>> {
    if let Some(path) = installed_runtime_dir(apk_path).map(|dir| dir.join("resources.arsc")) {
        if path.exists() {
            return fs::read(&path).map(Some).map_err(|error| {
                WinedroidError::with_source(
                    format!(
                        "failed to read installed resources.arsc `{}`",
                        path.display()
                    ),
                    error,
                )
            });
        }
    }

    let mut apk = ApkArchive::open(apk_path)?;

    apk.read_optional_file("resources.arsc")
}

fn installed_runtime_dir(apk_path: &Path) -> Option<PathBuf> {
    if apk_path.file_name().and_then(|name| name.to_str()) != Some("source.apk") {
        return None;
    }

    let parent = apk_path.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }

    Some(parent.join("runtime"))
}

fn collect_method_matches(
    program: &DexProgram,
    loaded_class: &LoadedClass,
    encoded_methods: &[DexEncodedMethod],
    method_name: &str,
    matches: &mut Vec<ResolvedMethod>,
) -> Result<()> {
    let dex_file = &program.dex_files[loaded_class.dex_file_index];

    for encoded_method in encoded_methods {
        let method = program.method_id(loaded_class.dex_file_index, encoded_method.method_index)?;

        if method.name == method_name {
            matches.push(resolved_method(
                loaded_class.dex_file_index,
                dex_file,
                &loaded_class.class_def,
                encoded_method.clone(),
                &method,
            ));
        }
    }

    Ok(())
}

fn collect_exact_method_matches(
    program: &DexProgram,
    loaded_class: &LoadedClass,
    encoded_methods: &[DexEncodedMethod],
    expected_method: &DexMethodId,
    expected_proto_descriptor: &str,
    matches: &mut Vec<ResolvedMethod>,
) -> Result<()> {
    let dex_file = &program.dex_files[loaded_class.dex_file_index];

    for encoded_method in encoded_methods {
        let method = program.method_id(loaded_class.dex_file_index, encoded_method.method_index)?;

        if method.name != expected_method.name {
            continue;
        }

        if method.proto.descriptor() != expected_proto_descriptor {
            continue;
        }

        matches.push(resolved_method(
            loaded_class.dex_file_index,
            dex_file,
            &loaded_class.class_def,
            encoded_method.clone(),
            &method,
        ));
    }

    Ok(())
}

fn resolved_method(
    dex_file_index: usize,
    dex_file: &DexFile,
    class_def: &DexClassDef,
    encoded_method: DexEncodedMethod,
    method: &DexMethodId,
) -> ResolvedMethod {
    ResolvedMethod {
        dex_file_index,
        dex_file_name: dex_file.source_name().to_owned(),
        class_descriptor: class_def.descriptor.clone(),
        encoded_method,
        method: method.clone(),
    }
}

fn matching_prototypes_display(matches: &[ResolvedMethod]) -> String {
    matches
        .iter()
        .map(|resolved| resolved.method.proto.descriptor())
        .collect::<Vec<_>>()
        .join(", ")
}

// cite: art class_linker.cc#ResolveField
// cite: frameworks-base ViewGroup.java#LayoutParams
fn framework_field_declaring_class(field: &DexFieldId) -> Option<&'static str> {
    if field.class_descriptor == "Landroid/view/WindowManager$LayoutParams;"
        && matches!(field.name.as_str(), "width" | "height")
        && field.type_descriptor == "I"
    {
        return Some("Landroid/view/ViewGroup$LayoutParams;");
    }

    None
}

fn is_host_browser_url(value: &str) -> bool {
    let Some((scheme, _rest)) = value.split_once(':') else {
        return false;
    };

    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

fn print_dex_run_summary(apk_path: &Path, resolved: &ResolvedMethod, code: &DexCodeItem) {
    println!("DEX run target");
    println!("  apk: {}", apk_path.display());
    println!("  dex file: {}", resolved.dex_file_name);
    println!("  class: {}", resolved.class_descriptor);
    println!("  method: {}", resolved.method.display_signature());
    println!("  method kind: {}", resolved.encoded_method.kind);
    println!("  code offset: 0x{:x}", resolved.encoded_method.code_offset);
    println!("  registers: {}", code.registers_size);
    println!("  ins: {}", code.ins_size);
    println!("  outs: {}", code.outs_size);
    println!("  tries: {}", code.tries_size);
    println!("  instructions: {}", code.instructions.len());
    println!();
}

fn method_execution_error(resolved: &ResolvedMethod, error: WinedroidError) -> WinedroidError {
    WinedroidError::with_source(
        format!(
            "while executing DEX method\n  dex file: {}\n  class: {}\n  method: {}",
            resolved.dex_file_name,
            resolved.class_descriptor,
            resolved.method.display_signature()
        ),
        error,
    )
}

#[cfg(test)]
mod tests;
