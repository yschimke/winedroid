use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{Window, WindowId};

use crate::error::{Result, WinedroidError};
use crate::frame_clock::{frame_interval_nanos_for_fps, FrameClock};
use crate::host_window::session::{
    apply_runtime_frame_interval, auto_click_targets, autoclose_after,
    automation_interactive_input, automation_script_from_env, click_rendered_target,
    drive_session_frame, idle_wake_seconds, initial_render_scroll_y, install_render_surface_state,
    interactive_pointer_release_action, perform_session_auto_clicks, refresh_idle_render_surface,
    rendered_click_target_at, run_automation_script, save_screenshot_png, scrolled_render_surface,
    session_key_input, session_touch_swipe, should_autoclose, ActiveClickHandlerGuard,
    ActiveRenderSurfaceGuard, InteractivePointerMotionState, InteractivePointerReleaseAction,
    NativeTextUpdate, SessionLoopHost, SessionSurfaceHost, ACTIVE_RENDER_SURFACE,
    FALLBACK_FRAMES_PER_SECOND, MAX_EVENT_WAKE_SECONDS, NANOS_PER_SECOND_F64,
};
use crate::host_window::{ActivityClickHandler, ActivityWindowRequest};
use crate::input::InputKey;
use crate::skia_renderer;

// cite: frameworks-base ViewConfiguration.java#TOUCH_SLOP
const MOUSE_TOUCH_SLOP: f64 = 8.0;
const NANOS_PER_SECOND_MILLIHERTZ: u64 = 1_000_000_000_000;

struct PointerGesture {
    start: (f64, f64),
    last: (f64, f64),
    target: Option<crate::host_window::session::RenderedClickTarget>,
    pressed_at: Instant,
    motion: InteractivePointerMotionState,
}

struct ScreenshotFlash {
    started_at: Instant,
    snapshot_png: Vec<u8>,
}

struct DesktopApp {
    context: Context<OwnedDisplayHandle>,
    window: Option<Arc<Window>>,
    surface: Option<Surface<OwnedDisplayHandle, Arc<Window>>>,
    title: String,
    logical_size: (u32, u32),
    cursor: (f64, f64),
    gesture: Option<PointerGesture>,
    modifiers: ModifiersState,
    screenshot_flash: Option<ScreenshotFlash>,
    interactive_input: bool,
    alive: bool,
    error: Option<String>,
}

impl DesktopApp {
    fn new(
        context: Context<OwnedDisplayHandle>,
        request: &ActivityWindowRequest,
        interactive_input: bool,
    ) -> Self {
        Self {
            context,
            window: None,
            surface: None,
            title: request.title.clone(),
            logical_size: (request.width, request.height),
            cursor: (0.0, 0.0),
            gesture: None,
            modifiers: ModifiersState::empty(),
            screenshot_flash: None,
            interactive_input,
            alive: true,
            error: None,
        }
    }

    fn set_error(&mut self, error: impl std::fmt::Display) {
        self.error = Some(error.to_string());
        self.alive = false;
    }

    fn window_host(&self) -> Option<DesktopWindowHost> {
        self.window
            .as_ref()
            .map(|window| DesktopWindowHost(Arc::clone(window)))
    }

    fn logical_point(&self, position: PhysicalPosition<f64>) -> (f64, f64) {
        let scale = self
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0);
        (position.x / scale, position.y / scale)
    }

    fn render_activity_image(
        &self,
        include_png: bool,
    ) -> Result<Option<skia_renderer::SkiaSurfaceImage>> {
        let Some(window) = self.window.as_ref() else {
            return Ok(None);
        };
        let Some(render_state) = ACTIVE_RENDER_SURFACE.with(|active| active.borrow().clone())
        else {
            return Ok(None);
        };
        let (width, height) = self.logical_size;
        let render_state = scrolled_render_surface(&render_state, width, height);
        let image = if include_png {
            skia_renderer::render_activity_surface_pixels_and_png_scaled(
                width,
                height,
                &render_state.message,
                &render_state.text_elements,
                &render_state.runtime_canvas_frames,
                &render_state.overlays,
                &render_state.dialogs,
                render_state.action_bar.as_ref(),
                render_state.floating_panel,
                render_state.window_background,
                window.scale_factor() as f32,
            )?
        } else {
            skia_renderer::render_activity_surface_pixels_scaled(
                width,
                height,
                &render_state.message,
                &render_state.text_elements,
                &render_state.runtime_canvas_frames,
                &render_state.overlays,
                &render_state.dialogs,
                render_state.action_bar.as_ref(),
                render_state.floating_panel,
                render_state.window_background,
                window.scale_factor() as f32,
            )?
        };
        crate::host_window::session::present_on_display_panel(image).map(Some)
    }

    fn render_present_image(&mut self) -> Result<Option<skia_renderer::SkiaSurfaceImage>> {
        if let Some(flash) = self.screenshot_flash.as_ref() {
            let elapsed_ms = flash.started_at.elapsed().as_millis() as u32;
            if elapsed_ms < skia_renderer::SCREENSHOT_FLASH_DURATION_MS {
                let Some(window) = self.window.as_ref() else {
                    return Ok(None);
                };
                let (width, height) = self.logical_size;
                let Some(background) = self.render_activity_image(false)? else {
                    return Ok(None);
                };
                return skia_renderer::render_screenshot_flash_pixels_scaled(
                    width,
                    height,
                    &background.pixels,
                    &flash.snapshot_png,
                    elapsed_ms,
                    window.scale_factor() as f32,
                )
                .map(Some);
            }
            self.screenshot_flash = None;
        }

        let include_png = std::env::var_os("WINEDROID_RENDER_DUMP_PNG").is_some();
        self.render_activity_image(include_png)
    }

    fn present(&mut self) -> Result<()> {
        let Some(image) = self.render_present_image()? else {
            return Ok(());
        };
        let Some(surface) = self.surface.as_mut() else {
            return Ok(());
        };
        let image_width = NonZeroU32::new(image.width)
            .ok_or_else(|| WinedroidError::new("desktop renderer produced zero width"))?;
        let image_height = NonZeroU32::new(image.height)
            .ok_or_else(|| WinedroidError::new("desktop renderer produced zero height"))?;
        surface.resize(image_width, image_height).map_err(|error| {
            WinedroidError::new(format!("failed to resize desktop surface\nerror: {error}"))
        })?;
        let mut buffer = surface.buffer_mut().map_err(|error| {
            WinedroidError::new(format!(
                "failed to acquire desktop surface buffer\nerror: {error}"
            ))
        })?;
        if buffer.len() != image.pixels.len() {
            return Err(WinedroidError::new(format!(
                "desktop surface pixel count mismatch\nbuffer: {}\nimage: {}",
                buffer.len(),
                image.pixels.len()
            )));
        }
        buffer.copy_from_slice(&image.pixels);
        buffer.present().map_err(|error| {
            WinedroidError::new(format!(
                "failed to present desktop surface buffer\nerror: {error}"
            ))
        })?;
        if !image.png.is_empty() {
            crate::host_window::session::dump_renderer_png_if_requested(&image)?;
        }
        Ok(())
    }

    fn save_screenshot(&mut self) -> Result<()> {
        let Some(image) = self.render_activity_image(true)? else {
            return Ok(());
        };
        let Some(path) = save_screenshot_png(&image.png)? else {
            println!("Host screenshot skipped");
            println!("  reason: no storage directory configured");
            return Ok(());
        };

        println!("Host screenshot saved");
        println!("  path: {}", path.display());
        self.screenshot_flash = Some(ScreenshotFlash {
            started_at: Instant::now(),
            snapshot_png: image.png,
        });
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        Ok(())
    }

    fn screenshot_flash_pending(&self) -> bool {
        self.screenshot_flash.is_some()
    }

    fn pointer_pressed(&mut self) {
        if !self.interactive_input {
            return;
        }
        if !crate::host_window::session::display_panel_contains(
            self.cursor,
            self.logical_size.0,
            self.logical_size.1,
        ) {
            return;
        }
        let target =
            rendered_click_target_at(self.cursor, self.logical_size.0, self.logical_size.1);
        self.gesture = Some(PointerGesture {
            start: self.cursor,
            last: self.cursor,
            target,
            pressed_at: Instant::now(),
            motion: InteractivePointerMotionState::new(),
        });
    }

    fn pointer_released(&mut self) {
        if !self.interactive_input {
            return;
        }
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        let Some(host) = self.window_host() else {
            return;
        };
        let (width, height) = self.logical_size;
        match interactive_pointer_release_action(
            gesture.start,
            gesture.last,
            MOUSE_TOUCH_SLOP,
            gesture.motion,
            gesture.target,
            width,
            height,
        ) {
            InteractivePointerReleaseAction::RuntimeSwipe => {
                let clamp = |value: f64, limit: u32| {
                    value.round().clamp(0.0, f64::from(limit.saturating_sub(1))) as u32
                };
                let start = (
                    clamp(gesture.start.0, width),
                    clamp(gesture.start.1, height),
                );
                let end = (clamp(gesture.last.0, width), clamp(gesture.last.1, height));
                let duration_ms = gesture.pressed_at.elapsed().as_millis().max(1) as u64;
                if let Err(error) = session_touch_swipe(&host, start, end, duration_ms, None) {
                    self.set_error(error);
                }
            }
            InteractivePointerReleaseAction::ProjectedScroll(delta_y) => {
                crate::host_window::session::scroll_active_surface(
                    &host,
                    delta_y,
                    gesture.start,
                    width,
                    height,
                );
            }
            InteractivePointerReleaseAction::Click(target) => {
                click_rendered_target(&host, target, "Host renderer click");
            }
            InteractivePointerReleaseAction::Ignore => {}
        }
    }

    fn key_input(&mut self, event: &KeyEvent) {
        if !self.interactive_input {
            return;
        }
        if is_screenshot_shortcut(self.modifiers, event) {
            if let Err(error) = self.save_screenshot() {
                eprintln!("Host screenshot failed");
                eprintln!("  error: {error}");
            }
            return;
        }
        let Some(host) = self.window_host() else {
            return;
        };
        let Some((characters, key)) = desktop_input_key(event) else {
            return;
        };
        if let Err(error) = session_key_input(&host, &characters, key) {
            self.set_error(error);
        }
    }
}

impl ApplicationHandler for DesktopApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(LogicalSize::new(self.logical_size.0, self.logical_size.1))
            .with_resizable(false);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.set_error(error);
                event_loop.exit();
                return;
            }
        };
        let surface = match Surface::new(&self.context, Arc::clone(&window)) {
            Ok(surface) => surface,
            Err(error) => {
                self.set_error(error);
                event_loop.exit();
                return;
            }
        };
        window.set_ime_allowed(self.interactive_input);
        self.surface = Some(surface);
        self.window = Some(window);
        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.alive = false;
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let started_at = crate::render_metrics::start_timing();
                if let Err(error) = self.present() {
                    self.set_error(error);
                    event_loop.exit();
                }
                crate::render_metrics::record_surface_present_elapsed(started_at);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = self.logical_point(position);
                if let Some(gesture) = self.gesture.as_mut() {
                    gesture.last = self.cursor;
                    gesture.motion.update(
                        gesture.start,
                        gesture.last,
                        MOUSE_TOUCH_SLOP,
                        gesture.target.as_ref(),
                        self.logical_size.0,
                        self.logical_size.1,
                    );
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.pointer_pressed(),
                ElementState::Released => self.pointer_released(),
            },
            WindowEvent::MouseWheel { delta, .. }
                if self.interactive_input && crate::host_window::session::active_rotary_input() =>
            {
                let axis_scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(position) => {
                        position.y
                            / crate::host_window::session::HOST_WHEEL_PIXELS_PER_ROTARY_DETENT
                    }
                };
                if axis_scroll != 0.0 {
                    if let Some(host) = self.window_host() {
                        if let Err(error) = crate::host_window::session::session_rotary_scroll(
                            &host,
                            axis_scroll as f32,
                        ) {
                            self.set_error(error);
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.interactive_input => {
                let delta_y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * -48.0).round() as i32,
                    MouseScrollDelta::PixelDelta(position) => (-position.y).round() as i32,
                };
                if delta_y != 0 {
                    if let Some(host) = self.window_host() {
                        crate::host_window::session::scroll_active_surface(
                            &host,
                            delta_y,
                            self.cursor,
                            self.logical_size.0,
                            self.logical_size.1,
                        );
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                self.key_input(&event);
            }
            WindowEvent::Ime(Ime::Commit(text)) if self.interactive_input => {
                if let Some(host) = self.window_host() {
                    if let Err(error) = session_key_input(&host, &text, InputKey::Text) {
                        self.set_error(error);
                    }
                }
            }
            _ => {}
        }
    }
}

struct DesktopWindowHost(Arc<Window>);

impl SessionSurfaceHost for DesktopWindowHost {
    fn request_redraw(&self) {
        crate::render_metrics::record_host_redraw_request();
        self.0.request_redraw();
    }

    fn refresh_native_text(
        &self,
        _handler: &ActivityClickHandler,
    ) -> Result<Vec<NativeTextUpdate>> {
        self.request_redraw();
        Ok(Vec::new())
    }
}

struct DesktopSession {
    event_loop: RefCell<EventLoop<()>>,
    app: RefCell<DesktopApp>,
    alive: Cell<bool>,
}

impl DesktopSession {
    fn new(request: &ActivityWindowRequest, interactive_input: bool) -> Result<Self> {
        let event_loop = EventLoop::new().map_err(|error| {
            WinedroidError::with_source("failed to create desktop event loop", error)
        })?;
        let context = Context::new(event_loop.owned_display_handle()).map_err(|error| {
            WinedroidError::new(format!(
                "failed to create desktop presentation context\nerror: {error}"
            ))
        })?;
        Ok(Self {
            event_loop: RefCell::new(event_loop),
            app: RefCell::new(DesktopApp::new(context, request, interactive_input)),
            alive: Cell::new(true),
        })
    }

    fn check_error(&self) -> Result<()> {
        let error = self.app.borrow_mut().error.take();
        match error {
            Some(error) => Err(WinedroidError::new(error)),
            None => Ok(()),
        }
    }

    fn host_refresh_rate_millihertz(&self) -> Option<u32> {
        self.app
            .borrow()
            .window
            .as_ref()?
            .current_monitor()?
            .refresh_rate_millihertz()
            .filter(|refresh_rate| *refresh_rate > 0)
    }

    fn screenshot_flash_pending(&self) -> bool {
        self.app.borrow().screenshot_flash_pending()
    }
}

impl SessionSurfaceHost for DesktopSession {
    fn request_redraw(&self) {
        if let Some(window) = self.app.borrow().window.as_ref() {
            crate::render_metrics::record_host_redraw_request();
            window.request_redraw();
        }
    }

    fn refresh_native_text(
        &self,
        _handler: &ActivityClickHandler,
    ) -> Result<Vec<NativeTextUpdate>> {
        self.request_redraw();
        Ok(Vec::new())
    }
}

impl SessionLoopHost for DesktopSession {
    fn pump_events(&self, wake_seconds: f64) -> Result<()> {
        let timeout = Duration::from_secs_f64(wake_seconds.max(0.0));
        let status = self
            .event_loop
            .borrow_mut()
            .pump_app_events(Some(timeout), &mut *self.app.borrow_mut());
        if matches!(status, PumpStatus::Exit(_)) {
            self.alive.set(false);
        }
        self.check_error()
    }

    fn after_pump(&self) -> Result<()> {
        self.check_error()
    }

    fn is_alive(&self) -> bool {
        self.alive.get() && self.app.borrow().alive
    }
}

pub fn open_activity_window(
    request: &ActivityWindowRequest,
    click_handler: Option<ActivityClickHandler>,
) -> Result<()> {
    let autoclose = autoclose_after()?;
    let auto_clicks = auto_click_targets()?;
    let automation_active =
        std::env::var_os("WINEDROID_AUTOMATION_SCRIPT").is_some() || !auto_clicks.is_empty();
    let interactive_input = !automation_active || automation_interactive_input()?;
    let _click_handler_guard = ActiveClickHandlerGuard::install(click_handler);
    let _render_surface_guard = ActiveRenderSurfaceGuard::install(initial_render_scroll_y()?);
    crate::host_window::session::set_screenshot_dir(request.screenshots_dir.clone());
    install_render_surface_state(request)?;
    let session = Rc::new(DesktopSession::new(request, interactive_input)?);
    session.pump_events(0.0)?;
    if !session.is_alive() {
        return session.check_error();
    }
    let interval_nanos = request.frame_interval_nanos.unwrap_or_else(|| {
        frame_interval_nanos_for_refresh_rate(session.host_refresh_rate_millihertz())
    });
    apply_runtime_frame_interval(interval_nanos)?;

    if let Some((script, log, opcode, usage, render, method)) = automation_script_from_env()? {
        return run_automation_script(
            session.as_ref(),
            script,
            autoclose,
            interval_nanos,
            log,
            opcode,
            usage,
            render,
            method,
        );
    }

    perform_session_auto_clicks(session.as_ref(), &auto_clicks)?;

    let started_at = Instant::now();
    let wake_seconds = (interval_nanos as f64 / NANOS_PER_SECOND_F64).min(MAX_EVENT_WAKE_SECONDS);
    let mut frame_clock = FrameClock::new(interval_nanos);
    let mut screenshot_flash_clock = FrameClock::new(interval_nanos);
    let mut wants_frames = true;
    while session.is_alive() {
        if should_autoclose(started_at, autoclose) {
            break;
        }
        let this_wake_seconds = if wants_frames || session.screenshot_flash_pending() {
            wake_seconds
        } else {
            idle_wake_seconds(wake_seconds)
        };
        session.pump_events(this_wake_seconds)?;

        if refresh_idle_render_surface() {
            wants_frames = true;
            frame_clock.reset();
        }

        let host_now_nanos = started_at.elapsed().as_nanos() as i64;
        if wants_frames {
            if let Some(pace) = frame_clock.poll(host_now_nanos) {
                wants_frames = drive_session_frame(session.as_ref(), pace.skipped_intervals())?;
                if !wants_frames {
                    frame_clock.reset();
                }
            }
        }

        if session.screenshot_flash_pending() {
            if screenshot_flash_clock.poll(host_now_nanos).is_some() {
                session.request_redraw();
            }
        } else {
            screenshot_flash_clock.reset();
        }

        session.after_pump()?;
    }
    session.check_error()
}

fn frame_interval_nanos_for_refresh_rate(refresh_rate_millihertz: Option<u32>) -> i64 {
    let Some(refresh_rate_millihertz) = refresh_rate_millihertz.filter(|rate| *rate > 0) else {
        return frame_interval_nanos_for_fps(FALLBACK_FRAMES_PER_SECOND);
    };
    (NANOS_PER_SECOND_MILLIHERTZ / u64::from(refresh_rate_millihertz)) as i64
}

fn is_screenshot_shortcut(modifiers: ModifiersState, event: &KeyEvent) -> bool {
    is_screenshot_shortcut_parts(modifiers, event.physical_key, &event.logical_key)
}

fn is_screenshot_shortcut_parts(
    modifiers: ModifiersState,
    physical_key: PhysicalKey,
    logical_key: &Key,
) -> bool {
    if !modifiers.super_key() {
        return false;
    }
    if physical_key == PhysicalKey::Code(KeyCode::KeyS) {
        return true;
    }
    matches!(logical_key, Key::Character(text) if text.eq_ignore_ascii_case("s"))
}

fn desktop_input_key(event: &KeyEvent) -> Option<(String, InputKey)> {
    desktop_input_key_parts(&event.logical_key, event.text.as_deref())
}

fn desktop_input_key_parts(key: &Key, text: Option<&str>) -> Option<(String, InputKey)> {
    match key {
        Key::Named(NamedKey::Enter) => Some((String::new(), InputKey::Enter)),
        Key::Named(NamedKey::Backspace) => Some((String::new(), InputKey::DeleteBackward)),
        Key::Named(NamedKey::Delete) => Some((String::new(), InputKey::DeleteForward)),
        Key::Named(NamedKey::ArrowLeft) => Some((String::new(), InputKey::Left)),
        Key::Named(NamedKey::ArrowRight) => Some((String::new(), InputKey::Right)),
        Key::Named(NamedKey::Home) => Some((String::new(), InputKey::Home)),
        Key::Named(NamedKey::End) => Some((String::new(), InputKey::End)),
        Key::Character(_) => text.map(|text| (text.to_owned(), InputKey::Text)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winit_millihertz_refresh_rate_maps_to_nanosecond_interval() {
        assert_eq!(
            frame_interval_nanos_for_refresh_rate(Some(60_000)),
            16_666_666
        );
        assert_eq!(
            frame_interval_nanos_for_refresh_rate(Some(59_940)),
            16_683_350
        );
        assert_eq!(
            frame_interval_nanos_for_refresh_rate(Some(120_000)),
            8_333_333
        );
    }

    #[test]
    fn unavailable_winit_refresh_rate_uses_documented_fallback() {
        assert_eq!(
            frame_interval_nanos_for_refresh_rate(None),
            frame_interval_nanos_for_fps(FALLBACK_FRAMES_PER_SECOND)
        );
    }

    #[test]
    fn winit_keyboard_input_uses_produced_text_instead_of_logical_key_label() {
        let logical_key = Key::Character("e".into());

        assert_eq!(
            desktop_input_key_parts(&logical_key, Some("é")),
            Some(("é".to_owned(), InputKey::Text))
        );
    }

    #[test]
    fn winit_dead_key_waits_for_ime_commit() {
        assert_eq!(desktop_input_key_parts(&Key::Dead(Some('`')), None), None);
    }

    #[test]
    fn winit_command_s_is_consumed_as_the_host_screenshot_shortcut() {
        assert!(is_screenshot_shortcut_parts(
            ModifiersState::SUPER,
            PhysicalKey::Code(KeyCode::KeyS),
            &Key::Character("ß".into())
        ));
        assert!(is_screenshot_shortcut_parts(
            ModifiersState::SUPER,
            PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
            &Key::Character("S".into())
        ));
        assert!(!is_screenshot_shortcut_parts(
            ModifiersState::empty(),
            PhysicalKey::Code(KeyCode::KeyS),
            &Key::Character("s".into())
        ));
        assert!(!is_screenshot_shortcut_parts(
            ModifiersState::SUPER,
            PhysicalKey::Code(KeyCode::KeyA),
            &Key::Character("a".into())
        ));
    }
}
