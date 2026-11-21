use crate::activity_runtime::{settle_runtime_view_frames, ActivityRuntimeWorker};
use crate::config::{DisplaySize, WinedroidConfig};
use crate::dex_run;
use crate::error::{Result, WinedroidError};
use crate::host_window;
use crate::inspect;
use crate::install::{self, InstalledApp, InstalledPackageArtifact};
use crate::layout_render;
use crate::native_deps::NativeDependencyPlan;
use crate::native_runtime::{NativeExecutionStrategy, NativeRuntimePlan};
use crate::prefix::WinedroidPrefix;
use crate::trace;
use std::path::Path;

mod diagnostics;
mod projection;

use diagnostics::*;

use projection::*;

pub fn launch_apk(apk_path: &Path) -> Result<()> {
    launch_apk_with_display_size(apk_path, None)
}

pub(crate) fn launch_apk_with_display_size(
    apk_path: &Path,
    display_size: Option<DisplaySize>,
) -> Result<()> {
    let _span = trace::span("launch", "launch APK");
    let report = {
        let _span = trace::span("inspect", "inspect APK");
        inspect::inspect_apk(apk_path)?
    };
    let prefix = {
        let _span = trace::span("launch", "resolve prefix");
        WinedroidPrefix::default()?
    };
    let mut config = {
        let _span = trace::span("launch", "load Winedroid config");
        WinedroidConfig::load_or_create(&prefix)?
    };
    if let Some(display_size) = display_size {
        config = config.override_display_size(display_size);
    }
    let installed_app = {
        let _span = trace::span("install", "install APK");
        install::install_apk(&prefix, apk_path, &report)?
    };

    run_installed(
        &prefix,
        installed_app,
        LaunchMetadata::from_report(&report)?,
        config,
    )
}

pub fn install(apk_path: &Path) -> Result<()> {
    let _span = trace::span("install", "install APK");
    let report = {
        let _span = trace::span("inspect", "inspect APK");
        inspect::inspect_apk(apk_path)?
    };
    let prefix = {
        let _span = trace::span("launch", "resolve prefix");
        WinedroidPrefix::default()?
    };
    let installed_app = install::install_apk(&prefix, apk_path, &report)?;
    let config = WinedroidConfig::load_or_create(&prefix)?;
    let native_plan = report.native_runtime_plan();
    let native_dependency_plan = report.native_dependency_plan(&native_plan);
    println!("Installed {}", installed_app.package_name);
    print_launch_summary(
        &prefix,
        &installed_app,
        &installed_app.launch_activity,
        &native_plan,
        &native_dependency_plan,
        &config,
    );

    Ok(())
}

pub fn launch_installed(package_name: &str) -> Result<()> {
    let _span = trace::span("launch", "launch installed package");
    let prefix = {
        let _span = trace::span("launch", "resolve prefix");
        WinedroidPrefix::default()?
    };
    let installed_app = {
        let _span = trace::span("install", "load installed package");
        InstalledApp::load(&prefix, package_name)?
    };
    let artifact = {
        let _span = trace::span("install", "load package artifact");
        installed_app.artifact()?
    };
    let config = {
        let _span = trace::span("launch", "load Winedroid config");
        WinedroidConfig::load_or_create(&prefix)?
    };

    run_installed(
        &prefix,
        installed_app,
        LaunchMetadata::from_artifact(&artifact),
        config,
    )
}

struct LaunchMetadata {
    version_name: Option<String>,
    version_code: Option<String>,
    launch_activity: String,
    native_plan: NativeRuntimePlan,
    native_dependency_plan: NativeDependencyPlan,
}

impl LaunchMetadata {
    fn from_report(report: &inspect::InspectionReport) -> Result<Self> {
        let launch_activity = {
            let _span = trace::span("launch", "resolve launch activity");
            report.single_launch_activity()?
        };
        let native_plan = report.native_runtime_plan();
        let native_dependency_plan = report.native_dependency_plan(&native_plan);

        Ok(Self {
            version_name: report.version_name().map(str::to_owned),
            version_code: report.version_code().map(str::to_owned),
            launch_activity,
            native_plan,
            native_dependency_plan,
        })
    }

    fn from_artifact(artifact: &InstalledPackageArtifact) -> Self {
        let native_plan = artifact.native_runtime_plan();
        let native_dependency_plan = artifact.native_dependency_plan(&native_plan);

        Self {
            version_name: artifact.version_name.clone(),
            version_code: artifact.version_code.clone(),
            launch_activity: artifact.launch_activity.clone(),
            native_plan,
            native_dependency_plan,
        }
    }
}

fn run_installed(
    prefix: &WinedroidPrefix,
    installed_app: InstalledApp,
    metadata: LaunchMetadata,
    config: WinedroidConfig,
) -> Result<()> {
    if installed_app.launch_activity != metadata.launch_activity {
        return Err(WinedroidError::new(format!(
            "installed package launch activity metadata does not match app layout\nmetadata: {launch_activity}\nlayout: {}",
            installed_app.launch_activity,
            launch_activity = metadata.launch_activity
        )));
    }

    print_session_header(
        &installed_app.package_name,
        metadata.version_name.as_deref(),
        metadata.version_code.as_deref(),
        &metadata.native_plan,
    );
    print_launch_summary(
        prefix,
        &installed_app,
        &installed_app.launch_activity,
        &metadata.native_plan,
        &metadata.native_dependency_plan,
        &config,
    );
    print_native_execution_status(&metadata.native_plan);

    let frame_interval_nanos = configured_frame_interval_nanos(&config);
    let display_size = config.display_size();
    let display_round = config.display_shape().is_round();
    let shared_storage_dir = prefix.storage_root();
    let activity_runtime = {
        let _span = trace::span("runtime", "start activity runtime worker");
        ActivityRuntimeWorker::start(
            &installed_app.source_apk,
            &installed_app.launch_activity,
            Some(&installed_app.data_dir),
            Some(&installed_app.device_data_dir),
            Some(&shared_storage_dir),
            config,
        )?
    };
    let mut window_request = host_window::ActivityWindowRequest::new(
        &installed_app.package_name,
        &installed_app.launch_activity,
        display_size,
    )
    .with_display_round(display_round)
    .with_frame_interval_nanos(frame_interval_nanos)
    .with_screenshots_dir(Some(crate::external_storage::screenshots_dir(prefix)?));
    {
        let _span = trace::span("ui", "settle runtime view frames");
        settle_runtime_view_frames(
            &activity_runtime,
            window_request.width,
            window_request.height,
        )?;
        activity_runtime.dispatch_view_tree_draw_listeners()?;
    }
    let activity_start = {
        let _span = trace::span("runtime", "read activity start snapshot");
        activity_runtime.activity_start()?
    };
    trace_runtime_canvas_frames(&activity_start.view_snapshot);
    let layout_summary = {
        let _span = trace::span("render", "summarize activity layouts");
        layout_render::summarize_activity_layouts(
            &installed_app.source_apk,
            &activity_start.view_snapshot,
            window_request.width,
            window_request.height,
        )?
    };
    {
        let _span = trace::span("ui", "register layout state into runtime");
        register_layout_text_values(&activity_runtime, &layout_summary)?;
        register_layout_view_visibilities(&activity_runtime, &layout_summary)?;
        register_declared_on_click_handlers(&activity_runtime, &layout_summary)?;
    }
    let activity_start = {
        let _span = trace::span("runtime", "read registered activity snapshot");
        activity_runtime.activity_start()?
    };
    print_managed_activity_start(&activity_start);
    print_layout_render_summary(&layout_summary);

    window_request = window_request.with_detail_lines(activity_window_detail_lines(
        &activity_start.view_snapshot,
        &layout_summary,
    ));
    let mut text_elements =
        activity_text_elements(&layout_summary, window_request.width, window_request.height);
    refresh_activity_text_elements(&activity_runtime, &mut text_elements)?;
    apply_activity_image_resources(
        &installed_app.source_apk,
        &activity_start.view_snapshot,
        &mut text_elements,
    )?;
    apply_activity_text_cursors(&activity_start.view_snapshot, &mut text_elements);
    normalize_activity_text_projection(
        &mut text_elements,
        window_request.width,
        window_request.height,
    );
    expand_scrollable_text_elements(
        &mut text_elements,
        &layout_summary.scrollable_view_ids,
        window_request.height,
    );
    let preference_snapshot = {
        let _span = trace::span("ui", "read preference snapshot");
        activity_runtime.preference_snapshot()?
    };
    let activity_theme = layout_render::layout_theme_from_snapshot(&activity_start.view_snapshot);
    append_preference_text_elements(
        &mut text_elements,
        &preference_snapshot,
        &installed_app.source_apk,
        &activity_theme,
        window_request.width,
        window_request.height,
    )?;
    append_runtime_view_root_elements(
        &mut text_elements,
        &activity_start.view_snapshot,
        window_request.width,
        window_request.height,
    );
    let action_bar = build_activity_action_bar(
        &installed_app.source_apk,
        activity_start.resources.as_deref(),
        &activity_start.view_snapshot,
    )?;
    if action_bar.as_ref().is_some_and(|bar| bar.is_decor) {
        inset_elements_below_action_bar(&mut text_elements, host_window::ACTION_BAR_HEIGHT);
    }
    let runtime_canvas_frames = activity_runtime_canvas_frames(&activity_start.view_snapshot);
    window_request = window_request
        .with_window_background(
            activity_start
                .resources
                .as_deref()
                .and_then(|resources| {
                    layout_render::activity_window_background_color_from_resources(
                        resources,
                        &activity_start.view_snapshot,
                    )
                })
                .or_else(|| {
                    layout_render::activity_window_background_color(
                        &installed_app.source_apk,
                        &activity_start.view_snapshot,
                    )
                }),
        )
        .with_text_elements(text_elements)
        .with_runtime_canvas_frames(runtime_canvas_frames)
        .with_action_bar(action_bar)
        .with_resource_id_names(apk_resource_id_names(&installed_app.source_apk)?);
    let host_context = Box::new(ActivityHostContext {
        activity_runtime,
        apk_path: installed_app.source_apk.clone(),
        resources: activity_start.resources.clone(),
        package_name: installed_app.package_name.clone(),
        width: window_request.width,
        height: window_request.height,
    });
    #[cfg(target_os = "emscripten")]
    let host_context_pointer = Box::into_raw(host_context);
    #[cfg(not(target_os = "emscripten"))]
    let host_context_pointer =
        Box::as_ref(&host_context) as *const ActivityHostContext as *mut ActivityHostContext;
    let click_handler = unsafe {
        host_window::ActivityClickHandler::new(
            host_context_pointer.cast(),
            click_activity_runtime,
            click_activity_preference,
            click_activity_touch,
            touch_activity_hold,
            touch_activity_swipe,
            click_activity_dialog_button,
            click_activity_dialog_item,
            focus_activity_view,
            key_activity_input,
            read_activity_view_text,
            read_activity_surface,
            set_activity_frame_interval,
            drive_activity_frame,
        )
    };
    trace::event(
        "window",
        format!(
            "opening host Activity window {}x{}",
            window_request.width, window_request.height
        ),
    );
    trace::report_stats();
    let result = host_window::open_activity_window(&window_request, Some(click_handler));
    #[cfg(target_os = "emscripten")]
    unsafe {
        drop(Box::from_raw(host_context_pointer));
    }
    #[cfg(not(target_os = "emscripten"))]
    drop(host_context);
    result
}

fn apk_resource_id_names(apk_path: &Path) -> Result<std::collections::BTreeMap<i32, String>> {
    Ok(layout_render::cached_resource_id_names(apk_path)?
        .as_ref()
        .clone())
}

fn configured_frame_interval_nanos(config: &WinedroidConfig) -> Option<i64> {
    match config.frame_rate() {
        crate::config::WinedroidFrameRate::Host => None,
        crate::config::WinedroidFrameRate::Fixed { frames_per_second } => Some(
            crate::frame_clock::frame_interval_nanos_for_fps(frames_per_second),
        ),
    }
}

fn print_launch_summary(
    prefix: &WinedroidPrefix,
    installed_app: &InstalledApp,
    launch_activity: &str,
    native_plan: &NativeRuntimePlan,
    native_dependency_plan: &NativeDependencyPlan,
    config: &WinedroidConfig,
) {
    if !trace::enabled("launch") {
        return;
    }
    println!("Installed APK");
    println!("  prefix: {}", prefix.root().display());
    println!("  config: {}", prefix.config_path().display());
    println!("  package: {}", installed_app.package_name);
    println!("  app dir: {}", installed_app.app_dir.display());
    println!("  source: {}", installed_app.source_apk.display());
    println!(
        "  package artifact: {}",
        installed_app.package_artifact.display()
    );
    println!("  data: {}", installed_app.data_dir.display());
    println!("  device data: {}", installed_app.device_data_dir.display());
    println!("  cache: {}", installed_app.cache_dir.display());
    println!("  files: {}", installed_app.files_dir.display());
    println!("  native: {}", installed_app.native_dir.display());
    println!(
        "  native libraries: {}",
        installed_app.native_libraries.len()
    );
    println!();
    println!("Launch target");
    println!("  activity: {launch_activity}");
    println!();
    println!("ABI support");
    println!("  host: {}", native_plan.host);
    println!(
        "  APK native ABIs: {}",
        native_plan.inventory.known_display()
    );
    println!(
        "  unknown ABI dirs: {}",
        native_plan.inventory.unknown_display()
    );
    println!(
        "  selected guest ABI: {}",
        native_plan.selected_guest_abi_display()
    );
    println!("  native backend branch: {}", native_plan.branch);
    println!();
    println!("Native dependency plan");
    println!(
        "  app libraries: {}",
        native_dependency_plan.app_libraries_display()
    );
    println!(
        "  Android system libraries: {}",
        native_dependency_plan.android_system_libraries_display()
    );
    println!(
        "  unresolved libraries: {}",
        native_dependency_plan.missing_libraries_display()
    );
    println!();
    println!("Runtime config");
    println!("  uiMode: 0x{:02x}", config.android_configuration_ui_mode());
    println!(
        "  display: {}x{}",
        config.display_size().width(),
        config.display_size().height()
    );
    println!();
}

fn print_native_execution_status(native_plan: &NativeRuntimePlan) {
    if !trace::enabled("launch") {
        return;
    }
    println!("Native execution");
    println!("  status: {}", native_execution_status(native_plan));
    println!();
}

fn native_execution_status(native_plan: &NativeRuntimePlan) -> String {
    match native_plan.execution_strategy() {
        NativeExecutionStrategy::NotRequired => "not required for this APK".to_owned(),
        strategy => strategy.status(),
    }
}

fn print_managed_activity_start(activity_start: &dex_run::DexActivityStart) {
    if !trace::enabled("runtime") {
        return;
    }
    println!("Managed Activity bootstrap");
    println!("  class: {}", activity_start.class_descriptor);
    println!("  method: {}", activity_start.method_signature);
    println!("  result: {}", activity_start.result);
    println!(
        "  content layouts: {}",
        display_i32_list(&activity_start.view_snapshot.content_layouts)
    );
    println!(
        "  inflated layouts: {}",
        display_i32_list(&activity_start.view_snapshot.inflated_layouts)
    );
    println!(
        "  runtime view roots: {}",
        display_runtime_view_roots(&activity_start.view_snapshot.runtime_view_roots)
    );
    println!(
        "  runtime canvas frames: {}",
        display_runtime_canvas_frames(&activity_start.view_snapshot.runtime_canvas_frames)
    );
    println!(
        "  view lookups: {}",
        display_i32_list(&activity_start.view_snapshot.view_lookup_ids)
    );
    println!(
        "  view texts: {}",
        display_view_texts(&activity_start.view_snapshot.view_texts)
    );
    println!(
        "  view images: {}",
        display_view_images(&activity_start.view_snapshot.view_images)
    );
    println!(
        "  view visibilities: {}",
        display_view_visibilities(&activity_start.view_snapshot.view_visibilities)
    );
    println!(
        "  view text cursors: {}",
        display_view_text_cursors(&activity_start.view_snapshot.view_text_cursors)
    );
    println!(
        "  click listeners: {}",
        display_click_listeners(&activity_start.view_snapshot.click_listeners)
    );
    println!(
        "  XML onClick handlers: {}",
        display_declared_on_click_handlers(
            &activity_start.view_snapshot.declared_on_click_handlers
        )
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Activity, AndroidManifest};

    #[test]
    fn rejects_manifest_without_launchable_activity() {
        let report = report_with_activities(Vec::new());
        let error = report
            .single_launch_activity()
            .expect_err("missing launch activity should fail");

        assert!(
            error.to_string().contains("launchable activity"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_manifest_with_multiple_launchable_activities() {
        let report = report_with_activities(vec![activity("one.Main"), activity("two.Main")]);
        let error = report
            .single_launch_activity()
            .expect_err("multiple launch activities should fail");

        assert!(
            error.to_string().contains("multiple launchable activities"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn launch_selection_swaps_alias_for_target_activity() {
        // cite: frameworks-base ActivityThread.java#performLaunchActivity
        let mut target = activity("com.example.Main");
        target.has_main_action = false;
        target.has_launcher_category = false;
        let mut alias = activity("com.example.AliasDefault");
        alias.is_alias = true;
        alias.target_activity = Some("com.example.Main".to_owned());
        let mut disabled_alias = activity("com.example.AliasRed");
        disabled_alias.is_alias = true;
        disabled_alias.target_activity = Some("com.example.Main".to_owned());
        disabled_alias.enabled = Some(false);

        let report = report_with_activities(vec![target, alias, disabled_alias]);

        assert_eq!(report.single_launch_activity().unwrap(), "com.example.Main");
    }

    #[test]
    fn rejects_alias_without_target_activity() {
        // cite: frameworks-base PackageParser.java#parseActivityAlias
        let mut alias = activity("com.example.Alias");
        alias.is_alias = true;

        let report = report_with_activities(vec![alias]);
        let error = report
            .single_launch_activity()
            .expect_err("alias without targetActivity should fail");

        assert!(
            error.to_string().contains("targetActivity"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn appends_runtime_view_root_when_no_text_elements_exist() {
        let snapshot = dex_run::DexViewSnapshot {
            content_layouts: Vec::new(),
            inflated_layouts: Vec::new(),
            theme_styles: Vec::new(),
            resource_sdk_version: 35,
            resource_ui_mode: 0,
            resource_display_width: 420,
            resource_display_height: 760,
            resource_screen_round: false,
            runtime_view_roots: vec![dex_run::DexRuntimeViewRoot {
                view_key: "object:12".to_owned(),
                class_descriptor: "Landroidx/compose/ui/platform/ComposeView;".to_owned(),
            }],
            runtime_canvas_frames: Vec::new(),
            view_lookup_ids: Vec::new(),
            view_texts: Vec::new(),
            view_text_cursors: Vec::new(),
            view_images: Vec::new(),
            view_visibilities: Vec::new(),
            click_listeners: Vec::new(),
            declared_on_click_handlers: Vec::new(),
            adapter_rows: Vec::new(),
            dynamic_children: Vec::new(),
            activity_overlays: Vec::new(),
            dialog_overlays: Vec::new(),
            action_bar_title: None,
            action_bar_nav_icon: None,
            action_bar_overflow_open: false,
            action_bar_present: false,
            action_bar_home_as_up: false,
            options_menu: Vec::new(),
        };
        let mut elements = Vec::new();

        append_runtime_view_root_elements(&mut elements, &snapshot, 420, 760);

        assert_eq!(elements.len(), 1);
        assert_eq!(
            elements[0].kind,
            host_window::ActivityTextKind::RuntimeViewRoot
        );
        assert_eq!(elements[0].width, 420);
        assert_eq!(elements[0].height, 760);
    }

    #[test]
    fn reports_native_backend_as_unsupported_until_native_code_is_reached() {
        let host = crate::abi::HostPlatform {
            os: crate::abi::HostOs::Macos,
            arch: crate::abi::HostArch::Arm64,
        };
        let abi_names = vec!["arm64-v8a".to_owned()];
        let plan = NativeRuntimePlan::new(host, &abi_names);

        assert_eq!(
            native_execution_status(&plan),
            "unsupported until native code is reached; missing primitive: Android ELF loader with Bionic runtime on macOS"
        );
    }

    #[test]
    fn reports_native_backend_as_not_required_for_dex_only_apps() {
        let host = crate::abi::HostPlatform {
            os: crate::abi::HostOs::Linux,
            arch: crate::abi::HostArch::X86_64,
        };
        let plan = NativeRuntimePlan::new(host, &[]);

        assert_eq!(native_execution_status(&plan), "not required for this APK");
    }

    #[test]
    fn maps_layout_text_elements_to_host_window_controls() {
        let summary = layout_render::LayoutRenderSummary {
            lines: Vec::new(),
            view_visibilities: Vec::new(),
            scrollable_view_ids: std::collections::BTreeSet::new(),
            text_elements: vec![
                layout_render::LayoutTextElement {
                    text: "Welcome".to_owned(),
                    kind: layout_render::LayoutTextKind::Label,
                    view_id: None,
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: layout_render::LayoutTextAlignment::Start,
                    editable: true,
                    depth: 2,
                    frame: Some(layout_render::LayoutFrame {
                        x: 24,
                        y: 100,
                        width: 200,
                        height: 30,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                layout_render::LayoutTextElement {
                    text: "Accept".to_owned(),
                    kind: layout_render::LayoutTextKind::Button,
                    view_id: Some(2131296342),
                    on_click: Some("acceptClick".to_owned()),
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: layout_render::LayoutTextAlignment::Start,
                    editable: false,
                    depth: 1,
                    frame: None,
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
            ],
        };

        let elements = activity_text_elements(&summary, 420, 760);

        assert_eq!(
            elements,
            vec![
                host_window::ActivityTextElement {
                    text: "Welcome".to_owned(),
                    kind: host_window::ActivityTextKind::Label,
                    view_id: None,
                    preference_key: None,
                    accessory: None,
                    image: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: host_window::ActivityTextAlignment::Start,
                    editable: true,
                    cursor: None,
                    x: 24,
                    y: 630,
                    width: 200,
                    height: 30,
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                host_window::ActivityTextElement {
                    text: "Accept".to_owned(),
                    kind: host_window::ActivityTextKind::Button,
                    view_id: Some(2131296342),
                    preference_key: None,
                    accessory: None,
                    image: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: host_window::ActivityTextAlignment::Start,
                    editable: false,
                    cursor: None,
                    x: 32,
                    y: 648,
                    width: 364,
                    height: 26,
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
            ]
        );
    }

    #[test]
    fn maps_zero_frame_text_elements_with_fallback_frame() {
        let summary = layout_render::LayoutRenderSummary {
            lines: Vec::new(),
            view_visibilities: Vec::new(),
            scrollable_view_ids: std::collections::BTreeSet::new(),
            text_elements: vec![layout_render::LayoutTextElement {
                text: String::new(),
                kind: layout_render::LayoutTextKind::Label,
                view_id: Some(2131296539),
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: layout_render::LayoutTextAlignment::Start,
                editable: true,
                depth: 2,
                frame: Some(layout_render::LayoutFrame {
                    x: 8,
                    y: 0,
                    width: 8,
                    height: 0,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }],
        };

        let elements = activity_text_elements(&summary, 420, 760);

        assert_eq!(
            elements,
            vec![host_window::ActivityTextElement {
                text: String::new(),
                kind: host_window::ActivityTextKind::Label,
                view_id: Some(2131296539),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: host_window::ActivityTextAlignment::Start,
                editable: true,
                cursor: None,
                x: 40,
                y: 682,
                width: 356,
                height: 26,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn normalizes_navigation_toolbar_title_and_up_icon() {
        let mut elements = vec![
            host_window::ActivityTextElement {
                text: String::new(),
                kind: host_window::ActivityTextKind::Label,
                view_id: Some(2131296272),
                preference_key: None,
                accessory: None,
                image: Some(test_activity_image()),
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: host_window::ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 30,
                y: 636,
                width: 44,
                height: 44,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
            host_window::ActivityTextElement {
                text: "About".to_owned(),
                kind: host_window::ActivityTextKind::Label,
                view_id: Some(2131296274),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: host_window::ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 88,
                y: 636,
                width: 120,
                height: 44,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
        ];

        normalize_navigation_toolbar(&mut elements, 420, 760);

        assert_eq!(elements[0].x, 0);
        assert_eq!(elements[0].y, 704);
        assert_eq!(elements[0].width, 48);
        assert_eq!(elements[0].height, 56);
        assert_eq!(elements[1].x, 52);
        assert_eq!(elements[1].y, 704);
        assert_eq!(elements[1].width, 360);
        assert_eq!(elements[1].height, 56);
    }

    #[test]
    fn expands_overflowing_scrollable_text_to_full_height_with_clip() {
        let long_text = "Wrap me. ".repeat(400);
        let mut elements = vec![host_window::ActivityTextElement {
            text: long_text,
            kind: host_window::ActivityTextKind::Label,
            view_id: Some(0x7f05_0021),
            preference_key: None,
            accessory: None,
            image: None,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: None,
            alignment: host_window::ActivityTextAlignment::Start,
            editable: false,
            cursor: None,
            x: 8,
            y: 732,
            width: 404,
            height: 28,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        }];
        let mut scrollable_view_ids = std::collections::BTreeSet::new();
        scrollable_view_ids.insert(0x7f05_0021);

        expand_scrollable_text_elements(&mut elements, &scrollable_view_ids, 760);

        assert!(elements[0].height > 760);
        let clip = elements[0].clip.expect("scrollable text should be clipped");
        assert_eq!(clip.y, 0);
        assert_eq!(clip.height, 760);
    }

    #[test]
    fn does_not_expand_text_views_outside_a_scroll_container() {
        let mut elements = vec![host_window::ActivityTextElement {
            text: "Wrap me. ".repeat(400),
            kind: host_window::ActivityTextKind::Label,
            view_id: Some(0x7f05_0021),
            preference_key: None,
            accessory: None,
            image: None,
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: None,
            alignment: host_window::ActivityTextAlignment::Start,
            editable: false,
            cursor: None,
            x: 8,
            y: 732,
            width: 404,
            height: 28,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        }];

        expand_scrollable_text_elements(&mut elements, &std::collections::BTreeSet::new(), 760);

        assert_eq!(elements[0].height, 28);
        assert!(elements[0].clip.is_none());
    }

    #[test]
    fn promotes_icon_text_labels_to_view_backed_list_rows() {
        let mut elements = vec![host_window::ActivityTextElement {
            text: "Donate".to_owned(),
            kind: host_window::ActivityTextKind::Label,
            view_id: Some(2131296277),
            preference_key: None,
            accessory: None,
            image: Some(test_activity_image()),
            text_size: None,
            text_padding_left: 0,
            text_padding_right: 0,
            text_color: None,
            background_color: None,
            alignment: host_window::ActivityTextAlignment::Start,
            editable: false,
            cursor: None,
            x: 30,
            y: 586,
            width: 220,
            height: 26,
            clip: None,
            background_corner_radius: 0,
            background_border_color: None,
            background_border_width: 0,
            elevation_px: 0,
        }];

        promote_compound_label_rows(&mut elements, 420);

        assert_eq!(elements[0].kind, host_window::ActivityTextKind::ListItem);
        assert_eq!(elements[0].view_id, Some(2131296277));
        assert_eq!(elements[0].preference_key, None);
        assert_eq!(elements[0].x, 24);
        assert_eq!(elements[0].y, 575);
        assert_eq!(elements[0].width, 372);
        assert_eq!(elements[0].height, 48);
    }

    #[test]
    fn appends_preference_rows_below_layout_elements() {
        let mut elements = Vec::new();
        let preferences = vec![
            dex_run::DexPreferenceSnapshot {
                runtime_key: "object:1".to_owned(),
                key: None,
                title: "General".to_owned(),
                summary: None,
                icon_resource_id: None,
                class_descriptor: "Landroidx/preference/PreferenceCategory;".to_owned(),
                kind: dex_run::DexPreferenceKind::Category,
                depth: 0,
            },
            dex_run::DexPreferenceSnapshot {
                runtime_key: "object:2".to_owned(),
                key: Some("com.example.LANGUAGE".to_owned()),
                title: "Language".to_owned(),
                summary: Some("English".to_owned()),
                icon_resource_id: None,
                class_descriptor: "Landroidx/preference/Preference;".to_owned(),
                kind: dex_run::DexPreferenceKind::Plain,
                depth: 1,
            },
        ];

        append_preference_text_elements(
            &mut elements,
            &preferences,
            Path::new("/unused.apk"),
            &test_layout_theme(),
            420,
            760,
        )
        .expect("preference rows without icons should append");

        assert_eq!(
            elements,
            vec![
                host_window::ActivityTextElement {
                    text: "General".to_owned(),
                    kind: host_window::ActivityTextKind::ListItem,
                    view_id: None,
                    preference_key: None,
                    accessory: None,
                    image: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: host_window::ActivityTextAlignment::Start,
                    editable: false,
                    cursor: None,
                    x: 0,
                    y: 710,
                    width: 420,
                    height: 42,
                    clip: Some(host_window::ActivityRect {
                        x: 0,
                        y: 28,
                        width: 420,
                        height: 724,
                    }),
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                host_window::ActivityTextElement {
                    text: "Language\nEnglish".to_owned(),
                    kind: host_window::ActivityTextKind::ListItem,
                    view_id: None,
                    preference_key: Some("object:2".to_owned()),
                    accessory: None,
                    image: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: host_window::ActivityTextAlignment::Start,
                    editable: false,
                    cursor: None,
                    x: 0,
                    y: 652,
                    width: 420,
                    height: 58,
                    clip: Some(host_window::ActivityRect {
                        x: 0,
                        y: 28,
                        width: 420,
                        height: 724,
                    }),
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
            ]
        );
    }

    #[test]
    fn appends_preference_rows_with_projected_theme_colors() {
        let mut elements = vec![
            host_window::ActivityTextElement {
                text: String::new(),
                kind: host_window::ActivityTextKind::Label,
                view_id: Some(0x7f09_0107),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: Some(0xff15_1b2c),
                alignment: host_window::ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 0,
                y: 0,
                width: 420,
                height: 620,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
            host_window::ActivityTextElement {
                text: "Settings".to_owned(),
                kind: host_window::ActivityTextKind::Label,
                view_id: Some(0x7f09_0200),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: Some(0xffef_efef),
                background_color: None,
                alignment: host_window::ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 52,
                y: 704,
                width: 320,
                height: 56,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
        ];
        let preferences = vec![dex_run::DexPreferenceSnapshot {
            runtime_key: "object:1".to_owned(),
            key: Some("theme".to_owned()),
            title: "Theme".to_owned(),
            summary: None,
            icon_resource_id: None,
            class_descriptor: "Landroidx/preference/Preference;".to_owned(),
            kind: dex_run::DexPreferenceKind::Plain,
            depth: 0,
        }];

        append_preference_text_elements(
            &mut elements,
            &preferences,
            Path::new("/unused.apk"),
            &test_layout_theme(),
            420,
            760,
        )
        .expect("preference rows without icons should append");

        let row = elements.last().expect("preference row should append");
        assert_eq!(row.text, "Theme");
        assert_eq!(row.text_color, Some(0xffef_efef));
        assert_eq!(row.background_color, Some(0xff15_1b2c));
    }

    #[test]
    fn preference_rows_ignore_blank_background_placeholders_when_choosing_start_top() {
        let mut elements = vec![
            host_window::ActivityTextElement {
                text: "Settings".to_owned(),
                kind: host_window::ActivityTextKind::Label,
                view_id: Some(0x7f09_0200),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: Some(0xffef_efef),
                background_color: None,
                alignment: host_window::ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 52,
                y: 704,
                width: 320,
                height: 56,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
            host_window::ActivityTextElement {
                text: String::new(),
                kind: host_window::ActivityTextKind::Label,
                view_id: Some(0x7f09_053b),
                preference_key: None,
                accessory: None,
                image: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: Some(0xff12_1212),
                alignment: host_window::ActivityTextAlignment::Start,
                editable: false,
                cursor: None,
                x: 48,
                y: 580,
                width: 348,
                height: 26,
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            },
        ];
        let preferences = vec![dex_run::DexPreferenceSnapshot {
            runtime_key: "object:1".to_owned(),
            key: None,
            title: "General".to_owned(),
            summary: None,
            icon_resource_id: None,
            class_descriptor: "Landroidx/preference/PreferenceCategory;".to_owned(),
            kind: dex_run::DexPreferenceKind::Category,
            depth: 0,
        }];

        append_preference_text_elements(
            &mut elements,
            &preferences,
            Path::new("/unused.apk"),
            &test_layout_theme(),
            420,
            760,
        )
        .expect("preference rows without icons should append");

        let row = elements.last().expect("preference row should append");
        assert_eq!(row.text, "General");
        assert_eq!(row.y, 654);
        assert_eq!(
            row.clip,
            Some(host_window::ActivityRect {
                x: 0,
                y: 28,
                width: 420,
                height: 668,
            })
        );
    }

    #[test]
    fn appends_preference_rows_below_viewport_for_scrolling() {
        let mut elements = Vec::new();
        let preferences = (0..20)
            .map(|index| dex_run::DexPreferenceSnapshot {
                runtime_key: format!("object:{index}"),
                key: Some(format!("key.{index}")),
                title: format!("Setting {index}"),
                summary: Some(format!("Summary {index}")),
                icon_resource_id: None,
                class_descriptor: "Landroidx/preference/Preference;".to_owned(),
                kind: dex_run::DexPreferenceKind::Plain,
                depth: 1,
            })
            .collect::<Vec<_>>();

        append_preference_text_elements(
            &mut elements,
            &preferences,
            Path::new("/unused.apk"),
            &test_layout_theme(),
            420,
            760,
        )
        .expect("preference rows without icons should append");

        assert_eq!(elements.len(), 20);
        assert!(
            elements
                .last()
                .expect("last row should exist")
                .y
                .is_negative(),
            "last row should be below the unscrolled viewport"
        );
        assert!(elements.iter().all(|element| element.clip
            == Some(host_window::ActivityRect {
                x: 0,
                y: 28,
                width: 420,
                height: 724,
            })));
    }

    fn report_with_activities(activities: Vec<Activity>) -> inspect::InspectionReport {
        inspect::InspectionReport::for_test(AndroidManifest {
            package_name: Some("com.example".to_owned()),
            activities,
            ..AndroidManifest::default()
        })
    }

    fn activity(name: &str) -> Activity {
        Activity {
            name: name.to_owned(),
            exported: Some(true),
            enabled: None,
            is_alias: false,
            target_activity: None,
            has_main_action: true,
            has_launcher_category: true,
            label: None,
            theme: None,
        }
    }

    fn test_activity_image() -> host_window::ActivityImage {
        host_window::ActivityImage {
            source: "res/test.xml".to_owned(),
            data: Vec::new(),
            resources: Vec::new(),
            theme: test_layout_theme(),
            tint_color: None,
            padding_after: 0,
        }
    }

    fn test_layout_theme() -> layout_render::LayoutTheme {
        layout_render::LayoutTheme {
            styles: Vec::new(),
            config: crate::resource_table::ResourceResolutionConfig::default(),
        }
    }
}
