use std::path::Path;
use std::sync::mpsc;
use std::thread;

use crate::config::WinedroidConfig;
use crate::dex_run;
use crate::error::{Result, WinedroidError};
use crate::input::InputKey;
use crate::trace;

const DEX_RUNTIME_STACK_BYTES: usize = 64 * 1024 * 1024;
const HOST_REFRESH_UI_WORK_ITEMS_PER_PASS: usize = 4;
pub(crate) struct ActivityRuntimeWorker {
    commands: mpsc::Sender<ActivityRuntimeCommand>,
    thread: Option<thread::JoinHandle<()>>,
}

enum ActivityRuntimeCommand {
    ActivityStart {
        response: mpsc::Sender<Result<dex_run::DexActivityStart>>,
    },
    PumpUiThread {
        width: u32,
        height: u32,
        max_passes: Option<usize>,
        max_work_items_per_pass: Option<usize>,
        response: mpsc::Sender<Result<dex_run::DexUiPumpResult>>,
    },
    DispatchViewTreeDrawListeners {
        response: mpsc::Sender<Result<usize>>,
    },
    SetFrameInterval {
        frame_interval_nanos: i64,
        response: mpsc::Sender<Result<()>>,
    },
    AdvanceFrameClock {
        skipped_intervals: i64,
        response: mpsc::Sender<Result<()>>,
    },
    FinishDrivenFrame {
        response: mpsc::Sender<Result<()>>,
    },
    WantsFrames {
        response: mpsc::Sender<Result<bool>>,
    },
    MainThreadIdle {
        response: mpsc::Sender<Result<bool>>,
    },
    NextWakeDelay {
        response: mpsc::Sender<Result<Option<i64>>>,
    },
    ClickViewId {
        view_id: i32,
        response: mpsc::Sender<Result<i32>>,
    },
    ClickPreference {
        preference_key: String,
        response: mpsc::Sender<Result<i32>>,
    },
    ClickTouch {
        x: u32,
        y: u32,
        response: mpsc::Sender<Result<i32>>,
    },
    TouchHold {
        x: u32,
        y: u32,
        hold_ms: u64,
        response: mpsc::Sender<Result<i32>>,
    },
    TouchSwipe {
        start: (u32, u32),
        end: (u32, u32),
        duration_ms: u64,
        steps: Option<u32>,
        response: mpsc::Sender<Result<i32>>,
    },
    RotaryScroll {
        axis_scroll: f32,
        response: mpsc::Sender<Result<i32>>,
    },
    FocusView {
        view_id: i32,
        response: mpsc::Sender<Result<i32>>,
    },
    KeyInput {
        characters: String,
        key: InputKey,
        response: mpsc::Sender<Result<i32>>,
    },
    ClickDialogButton {
        dialog_index: usize,
        button_index: usize,
        response: mpsc::Sender<Result<i32>>,
    },
    ClickDialogItem {
        dialog_index: usize,
        item_index: usize,
        response: mpsc::Sender<Result<i32>>,
    },
    TakeHostActions {
        response: mpsc::Sender<Result<Vec<dex_run::DexHostAction>>>,
    },
    RegisterViewText {
        view_id: i32,
        text: String,
        response: mpsc::Sender<Result<()>>,
    },
    RegisterViewVisibility {
        view_id: i32,
        visibility: i32,
        response: mpsc::Sender<Result<()>>,
    },
    RegisterDeclaredOnClickHandler {
        view_id: i32,
        method_name: String,
        response: mpsc::Sender<Result<()>>,
    },
    ViewText {
        view_id: i32,
        response: mpsc::Sender<Result<Option<String>>>,
    },
    ActivityOverlays {
        response: mpsc::Sender<Result<Vec<dex_run::DexActivityOverlay>>>,
    },
    DialogOverlays {
        response: mpsc::Sender<Result<Vec<dex_run::DexDialogOverlay>>>,
    },
    DialogContentLayouts {
        response: mpsc::Sender<Result<Vec<i32>>>,
    },
    PreferenceSnapshot {
        response: mpsc::Sender<Result<Vec<dex_run::DexPreferenceSnapshot>>>,
    },
    Shutdown,
}

impl ActivityRuntimeWorker {
    pub(crate) fn start(
        apk_path: &Path,
        activity_name: &str,
        app_data_dir: Option<&Path>,
        app_device_data_dir: Option<&Path>,
        shared_storage_dir: Option<&Path>,
        config: WinedroidConfig,
    ) -> Result<Self> {
        let apk_path = apk_path.to_path_buf();
        let activity_name = activity_name.to_owned();
        let app_data_dir = app_data_dir.map(Path::to_path_buf);
        let app_device_data_dir = app_device_data_dir.map(Path::to_path_buf);
        let shared_storage_dir = shared_storage_dir.map(Path::to_path_buf);
        let (command_sender, command_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::channel();

        let thread = thread::Builder::new()
            .name("winedroid-dex-runtime".to_owned())
            .stack_size(DEX_RUNTIME_STACK_BYTES)
            .spawn(move || {
                let runtime = {
                    let _span = trace::span("runtime", "execute Activity.onCreate bootstrap");
                    dex_run::start_activity_on_create_runtime_with_app_data_and_config(
                        &apk_path,
                        &activity_name,
                        app_data_dir.as_deref(),
                        app_device_data_dir.as_deref(),
                        shared_storage_dir.as_deref(),
                        config,
                    )
                };

                match runtime {
                    Ok(runtime) => {
                        let _ = ready_sender.send(Ok(()));
                        run_activity_runtime_worker(runtime, command_receiver);
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                    }
                }
            })
            .map_err(|error| {
                WinedroidError::with_source("failed to start DEX runtime worker", error)
            })?;

        let _span = trace::span("runtime", "wait for Activity runtime ready");
        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                commands: command_sender,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(WinedroidError::with_source(
                    "DEX runtime worker exited during startup",
                    error,
                ))
            }
        }
    }

    pub(crate) fn activity_start(&self) -> Result<dex_run::DexActivityStart> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ActivityStart { response })?;
        receive_activity_runtime_response(receiver)
    }

    fn pump_ui_thread(
        &self,
        width: u32,
        height: u32,
        max_passes: Option<usize>,
        max_work_items_per_pass: Option<usize>,
    ) -> Result<dex_run::DexUiPumpResult> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::PumpUiThread {
            width,
            height,
            max_passes,
            max_work_items_per_pass,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn dispatch_view_tree_draw_listeners(&self) -> Result<usize> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::DispatchViewTreeDrawListeners { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn set_frame_interval_nanos(&self, frame_interval_nanos: i64) -> Result<()> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::SetFrameInterval {
            frame_interval_nanos,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn advance_frame_clock(&self, skipped_intervals: i64) -> Result<()> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::AdvanceFrameClock {
            skipped_intervals,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn finish_driven_frame(&self) -> Result<()> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::FinishDrivenFrame { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn wants_frames(&self) -> Result<bool> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::WantsFrames { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn main_thread_idle(&self) -> Result<bool> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::MainThreadIdle { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn next_wake_delay_millis(&self) -> Result<Option<i64>> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::NextWakeDelay { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn click_view_id(&self, view_id: i32) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ClickViewId { view_id, response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn click_preference(&self, preference_key: &str) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ClickPreference {
            preference_key: preference_key.to_owned(),
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn click_touch(&self, x: u32, y: u32) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ClickTouch { x, y, response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn touch_hold(&self, x: u32, y: u32, hold_ms: u64) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::TouchHold {
            x,
            y,
            hold_ms,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn touch_swipe(
        &self,
        start: (u32, u32),
        end: (u32, u32),
        duration_ms: u64,
        steps: Option<u32>,
    ) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::TouchSwipe {
            start,
            end,
            duration_ms,
            steps,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn rotary_scroll(&self, axis_scroll: f32) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::RotaryScroll {
            axis_scroll,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn focus_view(&self, view_id: i32) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::FocusView { view_id, response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn key_input(&self, characters: &str, key: InputKey) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::KeyInput {
            characters: characters.to_owned(),
            key,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn click_dialog_button(
        &self,
        dialog_index: usize,
        button_index: usize,
    ) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ClickDialogButton {
            dialog_index,
            button_index,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn click_dialog_item(&self, dialog_index: usize, item_index: usize) -> Result<i32> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ClickDialogItem {
            dialog_index,
            item_index,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn take_host_actions(&self) -> Result<Vec<dex_run::DexHostAction>> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::TakeHostActions { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn register_view_text(&self, view_id: i32, text: &str) -> Result<()> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::RegisterViewText {
            view_id,
            text: text.to_owned(),
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn register_view_visibility(&self, view_id: i32, visibility: i32) -> Result<()> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::RegisterViewVisibility {
            view_id,
            visibility,
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn register_declared_on_click_handler(
        &self,
        view_id: i32,
        method_name: &str,
    ) -> Result<()> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::RegisterDeclaredOnClickHandler {
            view_id,
            method_name: method_name.to_owned(),
            response,
        })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn view_text(&self, view_id: i32) -> Result<Option<String>> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ViewText { view_id, response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn activity_overlays(&self) -> Result<Vec<dex_run::DexActivityOverlay>> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::ActivityOverlays { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn dialog_overlays(&self) -> Result<Vec<dex_run::DexDialogOverlay>> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::DialogOverlays { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn dialog_content_layouts(&self) -> Result<Vec<i32>> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::DialogContentLayouts { response })?;
        receive_activity_runtime_response(receiver)
    }

    pub(crate) fn preference_snapshot(&self) -> Result<Vec<dex_run::DexPreferenceSnapshot>> {
        let (response, receiver) = mpsc::channel();
        self.send(ActivityRuntimeCommand::PreferenceSnapshot { response })?;
        receive_activity_runtime_response(receiver)
    }

    fn send(&self, command: ActivityRuntimeCommand) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| WinedroidError::new("DEX runtime worker is not running"))
    }
}

pub(crate) fn settle_runtime_view_frames(
    activity_runtime: &ActivityRuntimeWorker,
    width: u32,
    height: u32,
) -> Result<()> {
    settle_runtime_view_frames_with_limit(activity_runtime, width, height, 4, false).map(|_| ())
}

pub(crate) fn settle_runtime_view_frames_extended(
    activity_runtime: &ActivityRuntimeWorker,
    width: u32,
    height: u32,
) -> Result<bool> {
    settle_runtime_view_frames_with_limit(activity_runtime, width, height, 24, true)
}

fn settle_runtime_view_frames_with_limit(
    activity_runtime: &ActivityRuntimeWorker,
    width: u32,
    height: u32,
    max_passes: usize,
    stop_after_first_change: bool,
) -> Result<bool> {
    let mut total_pumped = 0;
    let mut saw_changed_snapshot = false;

    for pass in 0..max_passes {
        trace::event("ui", format!("settle runtime view pass {pass} start"));
        let pump = {
            let _span = trace::span("ui", "pump UI thread pass");
            let max_passes = if stop_after_first_change {
                Some(1)
            } else {
                None
            };
            let max_work_items_per_pass = if stop_after_first_change {
                Some(HOST_REFRESH_UI_WORK_ITEMS_PER_PASS)
            } else {
                None
            };
            activity_runtime.pump_ui_thread(width, height, max_passes, max_work_items_per_pass)?
        };
        let dispatched = pump.dispatched;
        total_pumped += dispatched;
        trace::event(
            "ui",
            format!(
                "settle runtime view pass {pass} pumped {dispatched} callbacks view_changed={} pending_non_frame_work={} pending_main_work={} work_budget_exhausted={}",
                pump.view_snapshot_changed,
                pump.pending_non_frame_work,
                pump.pending_main_work,
                pump.work_budget_exhausted
            ),
        );

        if pass > 0 && dispatched == 0 {
            trace::event(
                "ui",
                format!("settle runtime view pass {pass} stopped after empty pump"),
            );
            break;
        }
        if stop_after_first_change && pass > 0 && saw_changed_snapshot {
            trace::event(
                "ui",
                format!("settle runtime view pass {pass} stopped after changed snapshot follow-up"),
            );
            break;
        }
        if stop_after_first_change && !pump.view_snapshot_changed && pump.pending_non_frame_work {
            trace::event(
                "ui",
                format!(
                    "settle runtime view pass {pass} stopped after stable snapshot with pending work"
                ),
            );
            break;
        }
        if pump.view_snapshot_changed {
            saw_changed_snapshot = true;
        }
        if pass > 0 && !pump.view_snapshot_changed && !pump.pending_non_frame_work {
            trace::event(
                "ui",
                format!("settle runtime view pass {pass} stopped after stable view snapshot"),
            );
            break;
        }
    }

    trace::event(
        "ui",
        format!("settle runtime view totals pumped={total_pumped}"),
    );

    Ok(saw_changed_snapshot)
}

impl Drop for ActivityRuntimeWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(ActivityRuntimeCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_activity_runtime_worker(
    runtime: dex_run::DexActivityRuntime,
    commands: mpsc::Receiver<ActivityRuntimeCommand>,
) {
    while let Ok(command) = commands.recv() {
        match command {
            ActivityRuntimeCommand::ActivityStart { response } => {
                send_activity_runtime_response(response, Ok(runtime.activity_start()));
            }
            ActivityRuntimeCommand::PumpUiThread {
                width,
                height,
                max_passes,
                max_work_items_per_pass,
                response,
            } => {
                let result = match max_passes {
                    Some(max_passes) => runtime.pump_ui_thread_with_budget(
                        width,
                        height,
                        max_passes,
                        max_work_items_per_pass,
                    ),
                    None => runtime.pump_ui_thread(width, height),
                };
                send_activity_runtime_response(response, result);
            }
            ActivityRuntimeCommand::DispatchViewTreeDrawListeners { response } => {
                send_activity_runtime_response(
                    response,
                    runtime.dispatch_view_tree_draw_listeners(),
                );
            }
            ActivityRuntimeCommand::SetFrameInterval {
                frame_interval_nanos,
                response,
            } => {
                runtime.set_frame_interval_nanos(frame_interval_nanos);
                send_activity_runtime_response(response, Ok(()));
            }
            ActivityRuntimeCommand::AdvanceFrameClock {
                skipped_intervals,
                response,
            } => {
                runtime.advance_frame_clock(skipped_intervals);
                send_activity_runtime_response(response, Ok(()));
            }
            ActivityRuntimeCommand::FinishDrivenFrame { response } => {
                runtime.finish_driven_frame();
                send_activity_runtime_response(response, Ok(()));
            }
            ActivityRuntimeCommand::WantsFrames { response } => {
                send_activity_runtime_response(response, Ok(runtime.wants_frames()));
            }
            ActivityRuntimeCommand::MainThreadIdle { response } => {
                send_activity_runtime_response(response, Ok(runtime.main_thread_idle()));
            }
            ActivityRuntimeCommand::NextWakeDelay { response } => {
                send_activity_runtime_response(response, Ok(runtime.next_wake_delay_millis()));
            }
            ActivityRuntimeCommand::ClickViewId { view_id, response } => {
                send_activity_runtime_response(response, runtime.click_view_id(view_id));
            }
            ActivityRuntimeCommand::ClickPreference {
                preference_key,
                response,
            } => {
                send_activity_runtime_response(response, runtime.click_preference(&preference_key));
            }
            ActivityRuntimeCommand::ClickTouch { x, y, response } => {
                send_activity_runtime_response(response, runtime.click_touch(x, y));
            }
            ActivityRuntimeCommand::TouchHold {
                x,
                y,
                hold_ms,
                response,
            } => {
                send_activity_runtime_response(response, runtime.touch_hold(x, y, hold_ms));
            }
            ActivityRuntimeCommand::TouchSwipe {
                start,
                end,
                duration_ms,
                steps,
                response,
            } => {
                send_activity_runtime_response(
                    response,
                    runtime.touch_swipe(start, end, duration_ms, steps),
                );
            }
            ActivityRuntimeCommand::RotaryScroll {
                axis_scroll,
                response,
            } => {
                send_activity_runtime_response(response, runtime.rotary_scroll(axis_scroll));
            }
            ActivityRuntimeCommand::FocusView { view_id, response } => {
                send_activity_runtime_response(response, runtime.focus_view(view_id));
            }
            ActivityRuntimeCommand::KeyInput {
                characters,
                key,
                response,
            } => {
                send_activity_runtime_response(response, runtime.key_input(&characters, key));
            }
            ActivityRuntimeCommand::ClickDialogButton {
                dialog_index,
                button_index,
                response,
            } => {
                send_activity_runtime_response(
                    response,
                    runtime.click_dialog_button(dialog_index, button_index),
                );
            }
            ActivityRuntimeCommand::ClickDialogItem {
                dialog_index,
                item_index,
                response,
            } => {
                send_activity_runtime_response(
                    response,
                    runtime.click_dialog_item(dialog_index, item_index),
                );
            }
            ActivityRuntimeCommand::TakeHostActions { response } => {
                send_activity_runtime_response(response, Ok(runtime.take_host_actions()));
            }
            ActivityRuntimeCommand::RegisterViewText {
                view_id,
                text,
                response,
            } => {
                send_activity_runtime_response(
                    response,
                    runtime.register_view_text(view_id, &text),
                );
            }
            ActivityRuntimeCommand::RegisterViewVisibility {
                view_id,
                visibility,
                response,
            } => {
                send_activity_runtime_response(
                    response,
                    runtime.register_view_visibility(view_id, visibility),
                );
            }
            ActivityRuntimeCommand::RegisterDeclaredOnClickHandler {
                view_id,
                method_name,
                response,
            } => {
                send_activity_runtime_response(
                    response,
                    runtime.register_declared_on_click_handler(view_id, &method_name),
                );
            }
            ActivityRuntimeCommand::ViewText { view_id, response } => {
                send_activity_runtime_response(response, runtime.view_text(view_id));
            }
            ActivityRuntimeCommand::ActivityOverlays { response } => {
                send_activity_runtime_response(response, Ok(runtime.activity_overlays()));
            }
            ActivityRuntimeCommand::DialogOverlays { response } => {
                send_activity_runtime_response(response, Ok(runtime.dialog_overlays()));
            }
            ActivityRuntimeCommand::DialogContentLayouts { response } => {
                send_activity_runtime_response(response, Ok(runtime.dialog_content_layouts()));
            }
            ActivityRuntimeCommand::PreferenceSnapshot { response } => {
                send_activity_runtime_response(response, Ok(runtime.preference_snapshot()));
            }
            ActivityRuntimeCommand::Shutdown => return,
        }
    }
}

fn send_activity_runtime_response<T>(response: mpsc::Sender<Result<T>>, result: Result<T>) {
    let _ = response.send(result);
}

fn receive_activity_runtime_response<T>(receiver: mpsc::Receiver<Result<T>>) -> Result<T> {
    match receiver.recv() {
        Ok(result) => result,
        Err(error) => Err(WinedroidError::with_source(
            "DEX runtime worker closed response channel",
            error,
        )),
    }
}
