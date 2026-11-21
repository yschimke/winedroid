use std::path::Path;

use crate::config::WinedroidConfig;
use crate::dex_run;
use crate::error::Result;
use crate::input::InputKey;
use crate::trace;

const HOST_REFRESH_UI_WORK_ITEMS_PER_PASS: usize = 4;

pub(crate) struct ActivityRuntimeWorker {
    runtime: dex_run::DexActivityRuntime,
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
        let runtime = dex_run::start_activity_on_create_runtime_with_app_data_and_config(
            apk_path,
            activity_name,
            app_data_dir,
            app_device_data_dir,
            shared_storage_dir,
            config,
        )?;

        Ok(Self { runtime })
    }

    pub(crate) fn activity_start(&self) -> Result<dex_run::DexActivityStart> {
        Ok(self.runtime.activity_start())
    }

    fn pump_ui_thread(
        &self,
        width: u32,
        height: u32,
        max_passes: Option<usize>,
        max_work_items_per_pass: Option<usize>,
    ) -> Result<dex_run::DexUiPumpResult> {
        match max_passes {
            Some(max_passes) => self.runtime.pump_ui_thread_with_budget(
                width,
                height,
                max_passes,
                max_work_items_per_pass,
            ),
            None => self.runtime.pump_ui_thread(width, height),
        }
    }

    pub(crate) fn dispatch_view_tree_draw_listeners(&self) -> Result<usize> {
        self.runtime.dispatch_view_tree_draw_listeners()
    }

    pub(crate) fn set_frame_interval_nanos(&self, frame_interval_nanos: i64) -> Result<()> {
        self.runtime.set_frame_interval_nanos(frame_interval_nanos);
        Ok(())
    }

    pub(crate) fn advance_frame_clock(&self, skipped_intervals: i64) -> Result<()> {
        self.runtime.advance_frame_clock(skipped_intervals);
        Ok(())
    }

    pub(crate) fn finish_driven_frame(&self) -> Result<()> {
        self.runtime.finish_driven_frame();
        Ok(())
    }

    pub(crate) fn wants_frames(&self) -> Result<bool> {
        Ok(self.runtime.wants_frames())
    }

    pub(crate) fn main_thread_idle(&self) -> Result<bool> {
        Ok(self.runtime.main_thread_idle())
    }

    pub(crate) fn next_wake_delay_millis(&self) -> Result<Option<i64>> {
        Ok(self.runtime.next_wake_delay_millis())
    }

    pub(crate) fn click_view_id(&self, view_id: i32) -> Result<i32> {
        self.runtime.click_view_id(view_id)
    }

    pub(crate) fn click_preference(&self, preference_key: &str) -> Result<i32> {
        self.runtime.click_preference(preference_key)
    }

    pub(crate) fn click_touch(&self, x: u32, y: u32) -> Result<i32> {
        self.runtime.click_touch(x, y)
    }

    pub(crate) fn touch_hold(&self, x: u32, y: u32, hold_ms: u64) -> Result<i32> {
        self.runtime.touch_hold(x, y, hold_ms)
    }

    pub(crate) fn touch_swipe(
        &self,
        start: (u32, u32),
        end: (u32, u32),
        duration_ms: u64,
        steps: Option<u32>,
    ) -> Result<i32> {
        self.runtime.touch_swipe(start, end, duration_ms, steps)
    }

    pub(crate) fn rotary_scroll(&self, axis_scroll: f32) -> Result<i32> {
        self.runtime.rotary_scroll(axis_scroll)
    }

    pub(crate) fn focus_view(&self, view_id: i32) -> Result<i32> {
        self.runtime.focus_view(view_id)
    }

    pub(crate) fn key_input(&self, characters: &str, key: InputKey) -> Result<i32> {
        self.runtime.key_input(characters, key)
    }

    pub(crate) fn click_dialog_button(
        &self,
        dialog_index: usize,
        button_index: usize,
    ) -> Result<i32> {
        self.runtime.click_dialog_button(dialog_index, button_index)
    }

    pub(crate) fn click_dialog_item(&self, dialog_index: usize, item_index: usize) -> Result<i32> {
        self.runtime.click_dialog_item(dialog_index, item_index)
    }

    pub(crate) fn take_host_actions(&self) -> Result<Vec<dex_run::DexHostAction>> {
        Ok(self.runtime.take_host_actions())
    }

    pub(crate) fn register_view_text(&self, view_id: i32, text: &str) -> Result<()> {
        self.runtime.register_view_text(view_id, text)
    }

    pub(crate) fn register_view_visibility(&self, view_id: i32, visibility: i32) -> Result<()> {
        self.runtime.register_view_visibility(view_id, visibility)
    }

    pub(crate) fn register_declared_on_click_handler(
        &self,
        view_id: i32,
        method_name: &str,
    ) -> Result<()> {
        self.runtime
            .register_declared_on_click_handler(view_id, method_name)
    }

    pub(crate) fn view_text(&self, view_id: i32) -> Result<Option<String>> {
        self.runtime.view_text(view_id)
    }

    pub(crate) fn activity_overlays(&self) -> Result<Vec<dex_run::DexActivityOverlay>> {
        Ok(self.runtime.activity_overlays())
    }

    pub(crate) fn dialog_overlays(&self) -> Result<Vec<dex_run::DexDialogOverlay>> {
        Ok(self.runtime.dialog_overlays())
    }

    pub(crate) fn dialog_content_layouts(&self) -> Result<Vec<i32>> {
        Ok(self.runtime.dialog_content_layouts())
    }

    pub(crate) fn preference_snapshot(&self) -> Result<Vec<dex_run::DexPreferenceSnapshot>> {
        Ok(self.runtime.preference_snapshot())
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
        let max_passes = stop_after_first_change.then_some(1);
        let max_work_items_per_pass =
            stop_after_first_change.then_some(HOST_REFRESH_UI_WORK_ITEMS_PER_PASS);
        let pump =
            activity_runtime.pump_ui_thread(width, height, max_passes, max_work_items_per_pass)?;
        total_pumped += pump.dispatched;

        if pass > 0 && pump.dispatched == 0 {
            break;
        }
        if stop_after_first_change && pass > 0 && saw_changed_snapshot {
            break;
        }
        if stop_after_first_change && !pump.view_snapshot_changed && pump.pending_non_frame_work {
            break;
        }
        if pump.view_snapshot_changed {
            saw_changed_snapshot = true;
        }
        if pass > 0 && !pump.view_snapshot_changed && !pump.pending_non_frame_work {
            break;
        }
    }

    trace::event(
        "ui",
        format!("settle runtime view totals pumped={total_pumped}"),
    );

    Ok(saw_changed_snapshot)
}
