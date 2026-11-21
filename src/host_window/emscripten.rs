use std::cell::RefCell;

use crate::error::{Result, WinedroidError};
use crate::frame_clock::FrameClock;
use crate::host_window::session::{
    apply_runtime_frame_interval, drive_session_frame, initial_render_scroll_y,
    install_render_surface_state, interactive_pointer_release_action, refresh_idle_render_surface,
    rendered_click_target_at, scroll_active_surface, scrolled_render_surface, session_key_input,
    session_touch_swipe, ActiveClickHandlerGuard, ActiveRenderSurfaceGuard,
    InteractivePointerMotionState, InteractivePointerReleaseAction, NativeTextUpdate,
    RenderedClickTarget, SessionSurfaceHost, ACTIVE_RENDER_SURFACE, ACTIVE_RENDER_SURFACE_SIZE,
    FALLBACK_FRAMES_PER_SECOND,
};
use crate::host_window::{ActivityClickHandler, ActivityWindowRequest};
use crate::input::InputKey;
use crate::skia_renderer;

// cite: frameworks-base ViewConfiguration.java#TOUCH_SLOP
const POINTER_TOUCH_SLOP: f64 = 8.0;

thread_local! {
    static POINTER_GESTURE: RefCell<Option<PointerGesture>> = const { RefCell::new(None) };
    static LOOP_STATE: RefCell<Option<BrowserLoopState>> = const { RefCell::new(None) };
}

struct PointerGesture {
    start: (f64, f64),
    last: (f64, f64),
    target: Option<RenderedClickTarget>,
    started_millis: f64,
    motion: InteractivePointerMotionState,
}

struct BrowserLoopState {
    frame_clock: FrameClock,
    wants_frames: bool,
}

struct BrowserHost;

impl SessionSurfaceHost for BrowserHost {
    fn request_redraw(&self) {
        crate::render_metrics::record_host_redraw_request();
        if let Err(error) = present_active_surface() {
            report_error("Browser render failed", &error);
        }
    }

    fn refresh_native_text(
        &self,
        _handler: &ActivityClickHandler,
    ) -> Result<Vec<NativeTextUpdate>> {
        Ok(Vec::new())
    }
}

extern "C" {
    fn emscripten_get_now() -> f64;
    fn emscripten_set_main_loop(
        callback: Option<extern "C" fn()>,
        frames_per_second: i32,
        simulate_infinite_loop: i32,
    );
    fn emscripten_cancel_main_loop();
    fn winedroid_web_install(width: u32, height: u32);
    fn winedroid_web_present_png(data: *const u8, length: usize, width: u32, height: u32);
    fn winedroid_web_device_pixel_ratio() -> f64;
    fn winedroid_web_status(data: *const u8, length: usize);
    fn winedroid_web_error(data: *const u8, length: usize);
    fn winedroid_web_ui_dump(data: *const u8, length: usize);
}

pub(super) fn open_activity_window(
    request: &ActivityWindowRequest,
    click_handler: Option<ActivityClickHandler>,
) -> Result<()> {
    let _click_handler_guard = ActiveClickHandlerGuard::install(click_handler);
    let _render_surface_guard = ActiveRenderSurfaceGuard::install(initial_render_scroll_y()?);
    crate::host_window::session::set_screenshot_dir(None);
    install_render_surface_state(request)?;

    let interval_nanos = request.frame_interval_nanos.unwrap_or_else(|| {
        crate::frame_clock::frame_interval_nanos_for_fps(FALLBACK_FRAMES_PER_SECOND)
    });
    apply_runtime_frame_interval(interval_nanos)?;
    LOOP_STATE.with(|state| {
        *state.borrow_mut() = Some(BrowserLoopState {
            frame_clock: FrameClock::new(interval_nanos),
            wants_frames: true,
        });
    });

    unsafe {
        winedroid_web_install(request.width, request.height);
    }
    present_active_surface()?;
    set_status(&format!("Running {}", request.package_name));

    unsafe {
        emscripten_set_main_loop(Some(browser_animation_frame), 0, 1);
    }

    Err(WinedroidError::new(
        "browser event loop returned unexpectedly",
    ))
}

extern "C" fn browser_animation_frame() {
    let refresh_pending = refresh_idle_render_surface();
    let now_nanos = unsafe { emscripten_get_now() * 1_000_000.0 } as i64;
    let pace = LOOP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut()?;
        if refresh_pending {
            state.wants_frames = true;
            state.frame_clock.reset();
        }
        if !state.wants_frames {
            return None;
        }
        state.frame_clock.poll(now_nanos)
    });
    let Some(pace) = pace else {
        return;
    };

    match drive_session_frame(&BrowserHost, pace.skipped_intervals()) {
        Ok(wants_frames) => {
            LOOP_STATE.with(|state| {
                let mut state = state.borrow_mut();
                let Some(state) = state.as_mut() else {
                    return;
                };
                state.wants_frames = wants_frames;
                if !wants_frames {
                    state.frame_clock.reset();
                }
            });
        }
        Err(error) => {
            LOOP_STATE.with(|state| {
                *state.borrow_mut() = None;
            });
            unsafe {
                emscripten_cancel_main_loop();
            }
            report_error("Browser frame failed", &error);
        }
    }
}

#[no_mangle]
pub extern "C" fn winedroid_web_pointer_down(x: f64, y: f64, now_millis: f64) {
    let target = active_surface_size()
        .and_then(|(width, height)| rendered_click_target_at((x, y), width, height));
    POINTER_GESTURE.with(|gesture| {
        *gesture.borrow_mut() = Some(PointerGesture {
            start: (x, y),
            last: (x, y),
            target,
            started_millis: now_millis,
            motion: InteractivePointerMotionState::new(),
        });
    });
}

#[no_mangle]
pub extern "C" fn winedroid_web_pointer_move(x: f64, y: f64) {
    POINTER_GESTURE.with(|gesture| {
        let mut gesture = gesture.borrow_mut();
        let Some(current) = gesture.as_mut() else {
            return;
        };
        current.last = (x, y);
        let Some((width, height)) = active_surface_size() else {
            return;
        };
        current.motion.update(
            current.start,
            current.last,
            POINTER_TOUCH_SLOP,
            current.target.as_ref(),
            width,
            height,
        );
    });
}

#[no_mangle]
pub extern "C" fn winedroid_web_pointer_up(x: f64, y: f64, now_millis: f64) {
    let Some(mut gesture) = POINTER_GESTURE.with(|gesture| gesture.borrow_mut().take()) else {
        return;
    };

    let Some((width, height)) = active_surface_size() else {
        return;
    };
    update_pointer_gesture(&mut gesture, (x, y), width, height);
    let result = match interactive_pointer_release_action(
        gesture.start,
        gesture.last,
        POINTER_TOUCH_SLOP,
        gesture.motion,
        gesture.target,
        width,
        height,
    ) {
        InteractivePointerReleaseAction::RuntimeSwipe => {
            let start = clamped_point(gesture.start, width, height);
            let end = clamped_point(gesture.last, width, height);
            let duration = (now_millis - gesture.started_millis).max(1.0) as u64;
            session_touch_swipe(&BrowserHost, start, end, duration, None)
        }
        InteractivePointerReleaseAction::ProjectedScroll(delta_y) => {
            scroll_active_surface(&BrowserHost, delta_y, gesture.start, width, height);
            Ok(())
        }
        InteractivePointerReleaseAction::Click(target) => {
            crate::host_window::session::click_rendered_target(
                &BrowserHost,
                target,
                "Browser renderer click",
            );
            Ok(())
        }
        InteractivePointerReleaseAction::Ignore => Ok(()),
    };

    if let Err(error) = result {
        report_error("Browser pointer input failed", &error);
    }
    wake_runtime_frames();
}

#[no_mangle]
pub extern "C" fn winedroid_web_pointer_cancel() {
    POINTER_GESTURE.with(|gesture| *gesture.borrow_mut() = None);
}

fn update_pointer_gesture(
    gesture: &mut PointerGesture,
    point: (f64, f64),
    width: u32,
    height: u32,
) {
    gesture.last = point;
    gesture.motion.update(
        gesture.start,
        gesture.last,
        POINTER_TOUCH_SLOP,
        gesture.target.as_ref(),
        width,
        height,
    );
}

#[no_mangle]
pub extern "C" fn winedroid_web_wheel(delta_y: f64, x: f64, y: f64) {
    let Some((width, height)) = active_surface_size() else {
        return;
    };
    if crate::host_window::session::active_rotary_input() {
        let axis_scroll =
            -delta_y / crate::host_window::session::HOST_WHEEL_PIXELS_PER_ROTARY_DETENT;
        if axis_scroll != 0.0 {
            if let Err(error) =
                crate::host_window::session::session_rotary_scroll(&BrowserHost, axis_scroll as f32)
            {
                crate::trace::event("input", format!("browser rotary failed: {error}"));
            }
            wake_runtime_frames();
        }
        return;
    }
    let delta_y = delta_y.round() as i32;
    if delta_y == 0 {
        return;
    }
    scroll_active_surface(&BrowserHost, delta_y, (x, y), width, height);
    wake_runtime_frames();
}

#[no_mangle]
pub extern "C" fn winedroid_web_key(key: u32, code_point: u32) {
    let key = match key {
        1 => InputKey::Enter,
        2 => InputKey::DeleteBackward,
        3 => InputKey::DeleteForward,
        4 => InputKey::Left,
        5 => InputKey::Right,
        6 => InputKey::Home,
        7 => InputKey::End,
        _ => InputKey::Text,
    };
    let characters = char::from_u32(code_point)
        .map(|character| character.to_string())
        .unwrap_or_default();
    if let Err(error) = session_key_input(&BrowserHost, &characters, key) {
        report_error("Browser keyboard input failed", &error);
    }
    wake_runtime_frames();
}

#[no_mangle]
pub extern "C" fn winedroid_web_request_ui_dump() -> i32 {
    let Some(dump) = crate::host_window::session::active_surface_ui_dump() else {
        return 0;
    };
    unsafe {
        winedroid_web_ui_dump(dump.as_ptr(), dump.len());
    }
    1
}

fn wake_runtime_frames() {
    LOOP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return;
        };
        state.wants_frames = true;
        state.frame_clock.reset();
    });
}

fn present_active_surface() -> Result<()> {
    let (width, height) = active_surface_size()
        .ok_or_else(|| WinedroidError::new("browser renderer has no active surface size"))?;
    let surface = ACTIVE_RENDER_SURFACE
        .with(|surface| surface.borrow().clone())
        .ok_or_else(|| WinedroidError::new("browser renderer has no active surface"))?;
    let surface = scrolled_render_surface(&surface, width, height);
    let pixel_scale = unsafe { winedroid_web_device_pixel_ratio() }.clamp(1.0, 3.0) as f32;
    let image = crate::host_window::session::present_on_display_panel(
        skia_renderer::render_activity_surface_scaled(
            width,
            height,
            &surface.message,
            &surface.text_elements,
            &surface.runtime_canvas_frames,
            &surface.overlays,
            &surface.dialogs,
            surface.action_bar.as_ref(),
            surface.floating_panel,
            surface.window_background,
            pixel_scale,
        )?,
    )?;
    unsafe {
        winedroid_web_present_png(
            image.png.as_ptr(),
            image.png.len(),
            image.width,
            image.height,
        );
    }
    Ok(())
}

fn active_surface_size() -> Option<(u32, u32)> {
    ACTIVE_RENDER_SURFACE_SIZE.with(|size| *size.borrow())
}

fn clamped_point(point: (f64, f64), width: u32, height: u32) -> (u32, u32) {
    let x = point
        .0
        .round()
        .clamp(0.0, f64::from(width.saturating_sub(1))) as u32;
    let y = point
        .1
        .round()
        .clamp(0.0, f64::from(height.saturating_sub(1))) as u32;
    (x, y)
}

fn set_status(message: &str) {
    unsafe {
        winedroid_web_status(message.as_ptr(), message.len());
    }
}

fn report_error(context: &str, error: &WinedroidError) {
    let message = format!("{context}\n{error}");
    eprintln!("{message}");
    unsafe {
        winedroid_web_error(message.as_ptr(), message.len());
    }
}
