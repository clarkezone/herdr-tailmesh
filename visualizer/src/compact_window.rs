//! Interactive window mode; compositor work never runs on the GPU event thread.
use herdr_mesh_visualizer::window_preferences::{Bounds, Preferences};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use winit::{
    dpi::{LogicalSize, PhysicalPosition},
    window::{Window, WindowLevel},
};

pub struct Controller {
    pub compact: bool,
    prefs: Arc<Mutex<WindowPreferences>>,
    path: Option<std::path::PathBuf>,
    changed: Option<Instant>,
    settle: Instant,
    #[cfg(target_os = "linux")]
    hypr: Option<hypr::Adapter>,
}
#[derive(Default)]
struct WindowPreferences {
    value: Preferences,
    #[cfg(any(target_os = "linux", test))]
    generation: u64,
}
impl WindowPreferences {
    fn step(&mut self, grow: bool) {
        #[cfg(any(target_os = "linux", test))]
        self.request();
        self.value.compact = self.value.compact.stepped(grow);
    }
    #[cfg(any(target_os = "linux", test))]
    fn request(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
    #[cfg(any(target_os = "linux", test))]
    fn publish(&mut self, generation: u64, compact: bool, bounds: Bounds) -> bool {
        if self.generation != generation {
            return false;
        }
        if compact {
            self.value.compact = bounds;
        } else {
            self.value.normal = bounds;
        }
        true
    }
}
impl Controller {
    pub fn load() -> Self {
        let path = Preferences::path()
            .map_err(|e| log::warn!("Window preferences: {e}"))
            .ok();
        let prefs = path
            .as_ref()
            .and_then(|p| {
                Preferences::read(p)
                    .map_err(|e| log::warn!("Window preferences: {e}"))
                    .ok()
            })
            .unwrap_or_default();
        Self {
            compact: false,
            prefs: Arc::new(Mutex::new(WindowPreferences {
                value: prefs,
                #[cfg(any(target_os = "linux", test))]
                generation: 0,
            })),
            path,
            changed: None,
            settle: Instant::now(),
            #[cfg(target_os = "linux")]
            hypr: None,
        }
    }
    pub fn initial_size(&self) -> LogicalSize<f64> {
        let p = self.prefs.lock().unwrap();
        LogicalSize::new(p.value.normal.size[0], p.value.normal.size[1])
    }
    pub fn attach(&mut self, window: &Window) {
        #[cfg(target_os = "linux")]
        {
            self.hypr = hypr::Adapter::start(self.prefs.clone(), self.path.clone());
        }
        self.apply(window);
    }
    pub fn toggle(&mut self, window: &Window) {
        self.capture(window);
        self.flush();
        self.compact = !self.compact;
        self.apply(window);
    }
    pub fn step(&mut self, window: &Window, grow: bool) {
        if !self.compact {
            return;
        }
        {
            let mut p = self.prefs.lock().unwrap();
            p.step(grow);
        }
        self.apply(window);
    }
    #[cfg(target_os = "windows")]
    pub fn scale_factor_changed(&self, scale: f64, writer: &mut winit::event::InnerSizeWriter) {
        if Instant::now() >= self.settle {
            return;
        }
        let prefs = self.prefs.lock().unwrap();
        let bounds = if self.compact {
            prefs.value.compact
        } else {
            prefs.value.normal
        };
        if let Err(e) = writer.request_inner_size(
            LogicalSize::new(bounds.size[0], bounds.size[1]).to_physical::<u32>(scale),
        ) {
            log::warn!("Cannot restore companion size after DPI change: {e}");
        }
    }
    fn apply(&mut self, window: &Window) {
        let mut p = self.prefs.lock().unwrap();
        #[cfg(target_os = "linux")]
        let generation = p.request();
        let b = if self.compact {
            &mut p.value.compact
        } else {
            &mut p.value.normal
        };
        *b = b.limited(self.compact);
        // Hyprland uses its own logical work areas; other backends use winit.
        #[cfg(target_os = "linux")]
        let compositor = self.hypr.is_some();
        #[cfg(not(target_os = "linux"))]
        let compositor = false;
        if !compositor
            && !cfg!(target_os = "windows")
            && let Some(m) = window.current_monitor()
        {
            let size = m.size().to_logical::<f64>(m.scale_factor());
            // Native positions remain desktop physical pixels; only extent is logical.
            let mut fitted = b.on_monitor([0, 0], [size.width, size.height]);
            if let Some(pos) = b.position {
                let origin = m.position();
                let physical = LogicalSize::new(fitted.size[0], fitted.size[1])
                    .to_physical::<u32>(m.scale_factor());
                fitted.position = Some([
                    pos[0].clamp(
                        origin.x,
                        origin
                            .x
                            .saturating_add(m.size().width.saturating_sub(physical.width) as i32),
                    ),
                    pos[1].clamp(
                        origin.y,
                        origin
                            .y
                            .saturating_add(m.size().height.saturating_sub(physical.height) as i32),
                    ),
                ]);
            }
            *b = fitted;
        }
        let bounds = *b;
        drop(p);
        window.set_decorations(!self.compact);
        window.set_window_level(if self.compact {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        });
        window.set_min_inner_size(Some(LogicalSize::new(
            if self.compact { 360. } else { 640. },
            if self.compact { 240. } else { 400. },
        )));
        window.set_max_inner_size(self.compact.then_some(LogicalSize::new(960., 720.)));
        if !compositor && !cfg!(target_os = "windows") {
            window.set_maximized(false);
            let _ = window.request_inner_size(LogicalSize::new(bounds.size[0], bounds.size[1]));
            if let Some(pos) = bounds.position {
                window.set_outer_position(PhysicalPosition::new(pos[0], pos[1]));
            }
        }
        #[cfg(target_os = "windows")]
        match windows::apply(window, self.compact, bounds) {
            Ok(fitted) => {
                let mut p = self.prefs.lock().unwrap();
                if self.compact {
                    p.value.compact = fitted;
                } else {
                    p.value.normal = fitted;
                }
            }
            Err(e) => log::warn!("Cannot place companion window: {e}"),
        }
        #[cfg(target_os = "linux")]
        if let Some(hypr) = &self.hypr {
            hypr.apply(self.compact, bounds, generation);
        }
        self.settle = Instant::now() + std::time::Duration::from_millis(650);
        self.changed = Some(Instant::now());
    }
    pub fn capture(&mut self, window: &Window) {
        #[cfg(target_os = "linux")]
        if self.hypr.is_some() {
            return;
        }
        if window.is_maximized() || window.is_minimized() == Some(true) {
            return;
        }
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        if size.width < 1. || size.height < 1. {
            return;
        }
        let mut prefs = self.prefs.lock().unwrap();
        let b = if self.compact {
            &mut prefs.value.compact
        } else {
            &mut prefs.value.normal
        };
        if Instant::now() < self.settle {
            // Win32 placement is synchronous; retain an immediate drag/toggle
            // once the requested client extent has arrived, not an interim resize.
            #[cfg(target_os = "windows")]
            if (size.width - b.size[0]).abs() > 1. || (size.height - b.size[1]).abs() > 1. {
                return;
            }
            #[cfg(not(target_os = "windows"))]
            return;
        }
        let next = Bounds {
            size: [size.width, size.height],
            position: window.outer_position().ok().map(|p| [p.x, p.y]),
        };
        if *b != next {
            *b = next;
            self.changed = Some(Instant::now());
        }
    }
    pub fn tick(&mut self, window: &Window) {
        self.capture(window);
        if self.changed.is_some_and(|t| t.elapsed().as_millis() >= 500) {
            self.flush();
        }
    }
    pub fn flush(&mut self) {
        if let Some(path) = &self.path
            && let Err(e) = self.prefs.lock().unwrap().value.write(path)
        {
            log::warn!("Cannot save window preferences: {e}");
            self.changed = Some(Instant::now());
            return;
        }
        self.changed = None;
    }
}
#[cfg(test)]
mod preference_tests {
    use super::*;

    #[test]
    fn stale_compositor_updates_cannot_lose_consecutive_resize_steps() {
        let mut prefs = WindowPreferences::default();
        let original = prefs.value.compact;
        let initial = prefs.request();
        assert!(prefs.publish(initial, true, original));

        prefs.step(true);
        let first = prefs.request();
        let first_target = prefs.value.compact;
        assert!(!prefs.publish(initial, true, original));
        prefs.step(true);
        assert!(!prefs.publish(first, true, first_target));
        let second = prefs.request();
        let second_target = prefs.value.compact;
        assert_eq!(second_target, original.stepped(true).stepped(true));
        assert!(!prefs.publish(first, true, first_target));
        assert!(prefs.publish(second, true, second_target));

        let normal = prefs.value.normal;
        let restore = prefs.request();
        let moved = Bounds {
            position: Some([1000, 700]),
            ..second_target
        };
        assert!(prefs.publish(restore, true, moved));
        assert!(prefs.publish(restore, false, normal));
        assert_eq!(prefs.value.compact, moved);
        assert_eq!(prefs.value.normal, normal);
    }
}
#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
            MonitorFromWindow,
        },
        UI::{
            HiDpi::AdjustWindowRectExForDpi,
            WindowsAndMessaging::{
                GWL_EXSTYLE, GWL_STYLE, GetWindowLongW, HWND_NOTOPMOST, HWND_TOPMOST,
                SWP_NOACTIVATE, SWP_NOOWNERZORDER, SetWindowPos,
            },
        },
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    fn fit(mut rect: RECT, work: RECT) -> RECT {
        let width = (rect.right - rect.left).min(work.right - work.left).max(1);
        let height = (rect.bottom - rect.top).min(work.bottom - work.top).max(1);
        rect.left = rect.left.clamp(work.left, work.right - width);
        rect.top = rect.top.clamp(work.top, work.bottom - height);
        rect.right = rect.left + width;
        rect.bottom = rect.top + height;
        rect
    }

    pub fn apply(window: &Window, compact: bool, bounds: Bounds) -> Result<Bounds, String> {
        let RawWindowHandle::Win32(handle) =
            window.window_handle().map_err(|e| e.to_string())?.as_raw()
        else {
            return Err("Expected a Win32 companion window".into());
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        window.set_maximized(false);
        let monitor = if let Some([x, y]) = bounds.position {
            // Select the saved origin, not the newly-created window's monitor.
            let rect = RECT {
                left: x,
                top: y,
                right: x.saturating_add(1),
                bottom: y.saturating_add(1),
            };
            unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) }
        } else {
            unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }
        };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return Err(format!(
                "Monitor work area: {}",
                std::io::Error::last_os_error()
            ));
        }
        let scale = window
            .available_monitors()
            .find(|m| m.position().x == info.rcMonitor.left && m.position().y == info.rcMonitor.top)
            .ok_or("Companion monitor is no longer available")?
            .scale_factor();
        let dpi = (scale * 96.).round() as u32;
        let mut frame = RECT::default();
        if !compact
            && unsafe {
                AdjustWindowRectExForDpi(
                    &mut frame,
                    GetWindowLongW(hwnd, GWL_STYLE) as u32,
                    0,
                    GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
                    dpi,
                )
            } == 0
        {
            return Err(format!(
                "Window frame extent: {}",
                std::io::Error::last_os_error()
            ));
        }
        let extra = [frame.right - frame.left, frame.bottom - frame.top];
        let size = LogicalSize::new(bounds.size[0], bounds.size[1]).to_physical::<i32>(scale);
        let position = bounds
            .position
            .or_else(|| window.outer_position().ok().map(|p| [p.x, p.y]))
            .unwrap_or([info.rcWork.left, info.rcWork.top]);
        let position = [
            position[0].clamp(info.rcWork.left, info.rcWork.right),
            position[1].clamp(info.rcWork.top, info.rcWork.bottom),
        ];
        let rect = fit(
            RECT {
                left: position[0],
                top: position[1],
                right: position[0].saturating_add(size.width.saturating_add(extra[0])),
                bottom: position[1].saturating_add(size.height.saturating_add(extra[1])),
            },
            info.rcWork,
        );
        if unsafe {
            SetWindowPos(
                hwnd,
                if compact {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                },
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        } == 0
        {
            return Err(format!(
                "Window placement: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Bounds {
            size: [
                f64::from((rect.right - rect.left - extra[0]).max(1)) / scale,
                f64::from((rect.bottom - rect.top - extra[1]).max(1)) / scale,
            ],
            position: Some([rect.left, rect.top]),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        #[allow(deprecated)]
        fn native_modes_restore_geometry_without_activation() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowRect, WS_EX_TOPMOST,
            };
            use winit::{
                event_loop::EventLoop,
                platform::{
                    pump_events::EventLoopExtPumpEvents, windows::EventLoopBuilderExtWindows,
                },
                window::WindowAttributes,
            };
            let mut event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
            let window = event_loop
                .create_window(WindowAttributes::default().with_visible(false))
                .unwrap();
            let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw() else {
                panic!("Expected Win32 test window");
            };
            let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
            let foreground = unsafe { GetForegroundWindow() };
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("window-preferences.json");
            let mut controller = Controller {
                compact: false,
                prefs: Arc::new(Mutex::new(WindowPreferences::default())),
                path: Some(path.clone()),
                changed: None,
                settle: Instant::now(),
            };
            controller.attach(&window);
            event_loop.pump_events(Some(std::time::Duration::ZERO), |_, _| {});
            let normal = controller.prefs.lock().unwrap().value.normal;
            controller.toggle(&window);
            event_loop.pump_events(Some(std::time::Duration::ZERO), |_, _| {});
            assert_ne!(
                unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST,
                0
            );
            for grow in [false, true] {
                controller.step(&window, grow);
                event_loop.pump_events(Some(std::time::Duration::ZERO), |_, _| {});
                let expected = controller.prefs.lock().unwrap().value.compact;
                let actual = window.inner_size().to_logical::<f64>(window.scale_factor());
                assert!(
                    (actual.width - expected.size[0]).abs() <= 1.,
                    "actual {actual:?}, expected {expected:?}"
                );
                assert!(
                    (actual.height - expected.size[1]).abs() <= 1.,
                    "actual {actual:?}, expected {expected:?}"
                );
            }
            for monitor in window.available_monitors() {
                {
                    let mut prefs = controller.prefs.lock().unwrap();
                    prefs.value.compact.position =
                        Some([monitor.position().x, monitor.position().y]);
                }
                controller.apply(&window);
                event_loop.pump_events(Some(std::time::Duration::ZERO), |event, _| {
                    if let winit::event::Event::WindowEvent {
                        event:
                            winit::event::WindowEvent::ScaleFactorChanged {
                                scale_factor,
                                mut inner_size_writer,
                            },
                        ..
                    } = event
                    {
                        controller.scale_factor_changed(scale_factor, &mut inner_size_writer);
                    }
                });
                let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                assert_ne!(unsafe { GetMonitorInfoW(monitor, &mut info) }, 0);
                let mut rect = RECT::default();
                assert_ne!(unsafe { GetWindowRect(hwnd, &mut rect) }, 0);
                assert!(rect.left >= info.rcWork.left && rect.top >= info.rcWork.top);
                assert!(rect.right <= info.rcWork.right && rect.bottom <= info.rcWork.bottom);
            }
            // A drag followed immediately by Restore must retain the new position.
            let position = window.outer_position().unwrap();
            window.set_outer_position(PhysicalPosition::new(position.x + 12, position.y + 12));
            event_loop.pump_events(Some(std::time::Duration::ZERO), |_, _| {});
            controller.capture(&window);
            let compact = controller.prefs.lock().unwrap().value.compact;
            assert_eq!(compact.position, Some([position.x + 12, position.y + 12]));
            controller.toggle(&window);
            event_loop.pump_events(Some(std::time::Duration::ZERO), |event, _| {
                if let winit::event::Event::WindowEvent {
                    event:
                        winit::event::WindowEvent::ScaleFactorChanged {
                            scale_factor,
                            mut inner_size_writer,
                        },
                    ..
                } = event
                {
                    controller.scale_factor_changed(scale_factor, &mut inner_size_writer);
                }
            });
            assert_eq!(
                unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST,
                0
            );
            assert_eq!(controller.prefs.lock().unwrap().value.normal, normal);
            let actual = window.inner_size().to_logical::<f64>(window.scale_factor());
            assert!((actual.width - normal.size[0]).abs() <= 1.);
            assert!((actual.height - normal.size[1]).abs() <= 1.);
            controller.toggle(&window);
            assert_eq!(controller.prefs.lock().unwrap().value.compact, compact);
            controller.capture(&window);
            controller.flush();
            assert_eq!(
                Preferences::read(&path).unwrap(),
                controller.prefs.lock().unwrap().value
            );
            assert_eq!(unsafe { GetForegroundWindow() }, foreground);
        }

        #[test]
        fn placement_respects_taskbars_negative_origins_and_removed_monitors() {
            let work = RECT {
                left: -1920,
                top: 40,
                right: 0,
                bottom: 1040,
            };
            for scale in [1., 1.25, 1.5, 2.] {
                let size = LogicalSize::new(520., 360.).to_physical::<i32>(scale);
                let fitted = fit(
                    RECT {
                        left: -3000,
                        top: 2000,
                        right: -3000 + size.width,
                        bottom: 2000 + size.height,
                    },
                    work,
                );
                assert_eq!(fitted.left, work.left);
                assert_eq!(fitted.bottom, work.bottom);
                assert_eq!(fitted.right - fitted.left, size.width);
                assert_eq!(fitted.bottom - fitted.top, size.height);
            }
            let fitted = fit(
                RECT {
                    left: 5000,
                    top: -900,
                    right: 9000,
                    bottom: 3000,
                },
                work,
            );
            assert_eq!(
                [fitted.left, fitted.top, fitted.right, fitted.bottom],
                [-1920, 40, 0, 1040]
            );
        }
    }
}
#[cfg(target_os = "linux")]
mod hypr {
    use super::*;
    use serde_json::Value;
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
        path::PathBuf,
        sync::mpsc,
        time::Duration,
    };
    pub struct Adapter {
        tx: mpsc::Sender<(bool, Bounds, u64)>,
        stop: Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    fn request(path: &std::path::Path, command: &str) -> Result<String, String> {
        let mut s = UnixStream::connect(path).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| e.to_string())?;
        s.set_write_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| e.to_string())?;
        s.write_all(command.as_bytes()).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut block = [0u8; 8192];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Compositor response deadline exceeded".into());
            }
            s.set_read_timeout(Some(remaining.min(Duration::from_millis(250))))
                .map_err(|e| e.to_string())?;
            let n = s.read(&mut block).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&block[..n]);
            if bytes.len() > 2 * 1024 * 1024 {
                return Err("Compositor response exceeds limit".into());
            }
        }
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }
    fn list(path: &std::path::Path, name: &str) -> Result<Value, String> {
        serde_json::from_str(&request(path, &format!("j/{name}"))?).map_err(|e| e.to_string())
    }
    fn owned(v: &Value, pid: u32) -> bool {
        v["pid"].as_u64() == Some(pid as u64)
            && v["title"] == "Herdr mesh visualizer"
            && v["mapped"] == true
    }
    fn address(v: &Value) -> Option<&str> {
        let a = v["address"].as_str()?;
        (a.starts_with("0x") && a.len() > 2 && a[2..].bytes().all(|b| b.is_ascii_hexdigit()))
            .then_some(a)
    }
    fn bounds(v: &Value) -> Option<Bounds> {
        Some(Bounds {
            size: [v["size"][0].as_f64()?, v["size"][1].as_f64()?],
            position: Some([
                i32::try_from(v["at"][0].as_i64()?).ok()?,
                i32::try_from(v["at"][1].as_i64()?).ok()?,
            ]),
        })
    }
    fn fit(path: &std::path::Path, b: Bounds) -> Result<Bounds, String> {
        let monitors = list(path, "monitors")?;
        fit_monitors(&monitors, b)
    }
    fn extent(m: &Value) -> ([i32; 2], [f64; 2]) {
        let scale = m["scale"].as_f64().filter(|n| *n > 0.).unwrap_or(1.);
        let mut size = [
            m["width"].as_f64().unwrap_or(1920.),
            m["height"].as_f64().unwrap_or(1080.),
        ];
        if m["transform"].as_u64().is_some_and(|t| t % 2 == 1) {
            size.swap(0, 1);
        }
        (
            [
                m["x"]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .unwrap_or(0),
                m["y"]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .unwrap_or(0),
            ],
            size.map(|n| n / scale),
        )
    }
    fn fit_monitors(monitors: &Value, b: Bounds) -> Result<Bounds, String> {
        let all = monitors.as_array().ok_or("No compositor monitors")?;
        let m = all
            .iter()
            .find(|m| {
                b.position.is_some_and(|p| {
                    let (origin, size) = extent(m);
                    (p[0] as f64) >= origin[0] as f64
                        && (p[1] as f64) >= origin[1] as f64
                        && (p[0] as f64) < origin[0] as f64 + size[0]
                        && (p[1] as f64) < origin[1] as f64 + size[1]
                })
            })
            .or_else(|| all.iter().find(|m| m["focused"] == true))
            .or_else(|| all.first())
            .ok_or("No compositor monitors")?;
        let (base, size) = extent(m);
        let reserved = &m["reserved"];
        let r = |i| reserved[i].as_f64().unwrap_or(0.);
        let origin = [
            base[0].saturating_add(r(0) as i32),
            base[1].saturating_add(r(1) as i32),
        ];
        let mut b = b.on_monitor(origin, [size[0] - r(0) - r(2), size[1] - r(1) - r(3)]);
        if b.position.is_none() {
            b.position = Some([origin[0] + 24, origin[1] + 24]);
            b = b.on_monitor(origin, [size[0] - r(0) - r(2), size[1] - r(1) - r(3)]);
        }
        Ok(b)
    }
    fn eval(path: &std::path::Path, v: &Value, pid: u32, body: &str) -> Result<(), String> {
        let a = address(v).ok_or("Invalid owned window address")?;
        let code = format!(
            "/eval local w=hl.get_window(\"address:{a}\"); if w and w.pid=={pid} and w.title==\"Herdr mesh visualizer\" then {body} end"
        );
        let r = request(path, &code)?;
        if r.trim() != "ok" {
            return Err(format!("Compositor rejected window control: {r}"));
        }
        Ok(())
    }
    fn size_matches(actual: Bounds, target: Bounds) -> bool {
        actual
            .size
            .iter()
            .zip(target.size)
            .all(|(a, b)| (a - b.round()).abs() <= 1.)
    }
    fn run(
        path: PathBuf,
        prefs: Arc<Mutex<WindowPreferences>>,
        saved: Option<PathBuf>,
        rx: mpsc::Receiver<(bool, Bounds, u64)>,
        stopping: Arc<std::sync::atomic::AtomicBool>,
    ) {
        let pid = std::process::id();
        let mut compact = false;
        let mut apply = true;
        let mut desired = prefs.lock().unwrap().value.normal;
        let mut generation = 0;
        let mut settle = Instant::now();
        let mut active = String::new();
        let mut dirty = None;
        let mut previous = None;
        let mut requested = None;
        let mut expected: Option<(Bounds, Instant)> = None;
        while !stopping.load(std::sync::atomic::Ordering::Relaxed) {
            while let Ok(next) = rx.try_recv() {
                requested = Some(next);
            }
            let result = (|| -> Result<(), String> {
                let clients = list(&path, "clients")?;
                let items = clients.as_array().ok_or("Invalid compositor inventory")?;
                let Some(v) = items.iter().find(|v| owned(v, pid)) else {
                    return Ok(());
                };
                if let Some(next) = requested.take() {
                    let changed = compact != next.0;
                    if changed
                        && !apply
                        && let Some(current) = bounds(v)
                        && expected.is_none_or(|(target, _)| size_matches(current, target))
                    {
                        prefs.lock().unwrap().publish(next.2, compact, current);
                    }
                    compact = next.0;
                    desired = next.1;
                    generation = next.2;
                    if !changed
                        && !apply
                        && let Some(current) = bounds(v)
                        && expected.is_none_or(|(target, _)| size_matches(current, target))
                    {
                        desired.position = current.position;
                    }
                    apply = true;
                }
                if apply {
                    // v0.56 floating dispatch can toggle an already-floating client.
                    // Establish it in a separate round trip before pin/size mutations.
                    if v["floating"] != true {
                        eval(
                            &path,
                            v,
                            pid,
                            "hl.dispatch(hl.dsp.window.float({action=\"set\",window=w}));",
                        )?;
                        return Ok(());
                    }
                    let b = fit(&path, desired.limited(compact))?;
                    let p = b.position.unwrap();
                    let pin = if (v["pinned"] == true) != compact {
                        "hl.dispatch(hl.dsp.window.pin({window=w}));"
                    } else {
                        ""
                    };
                    let body = format!(
                        "{pin} hl.dispatch(hl.dsp.window.set_prop({{prop=\"border_size\",value=\"{}\",window=w}}));hl.dispatch(hl.dsp.window.resize({{x={},y={},window=w}}));hl.dispatch(hl.dsp.window.move({{x={},y={},window=w}}));hl.dispatch(hl.dsp.window.alter_zorder({{mode=\"top\",window=w}}));",
                        if compact { "0" } else { "unset" },
                        b.size[0].round() as i32,
                        b.size[1].round() as i32,
                        p[0],
                        p[1]
                    );
                    eval(&path, v, pid, &body)?;
                    prefs.lock().unwrap().publish(generation, compact, b);
                    apply = false;
                    settle = Instant::now() + Duration::from_millis(650);
                    previous = None;
                    expected = Some((b, Instant::now() + Duration::from_secs(3)));
                    dirty = Some(Instant::now());
                } else if Instant::now() >= settle
                    && let Some(b) = bounds(v)
                {
                    if let Some((target, deadline)) = expected {
                        if !size_matches(b, target) && Instant::now() < deadline {
                            // A mode switch must first commit new Wayland size hints.
                            // Retry the requested geometry instead of saving the old min-clamped size.
                            let point = target.position.unwrap();
                            eval(
                                &path,
                                v,
                                pid,
                                &format!(
                                    "hl.dispatch(hl.dsp.window.resize({{x={},y={},window=w}}));hl.dispatch(hl.dsp.window.move({{x={},y={},window=w}}));",
                                    target.size[0].round() as i32,
                                    target.size[1].round() as i32,
                                    point[0],
                                    point[1]
                                ),
                            )?;
                            settle = Instant::now() + Duration::from_millis(250);
                            return Ok(());
                        }
                        expected = None;
                    }
                    if previous != Some(b) {
                        previous = Some(b);
                        dirty = Some(Instant::now());
                    }
                    prefs.lock().unwrap().publish(generation, compact, b);
                }
                let focused = items.iter().find(|v| v["focusHistoryID"] == 0);
                let current = focused.and_then(address).unwrap_or("");
                if compact
                    && current != active
                    && focused
                        .is_some_and(|f| !owned(f, pid) && f["fullscreen"].as_u64() == Some(0))
                {
                    eval(
                        &path,
                        v,
                        pid,
                        "hl.dispatch(hl.dsp.window.alter_zorder({mode=\"top\",window=w}));",
                    )?;
                }
                active = current.to_owned();
                if dirty.is_some_and(|t| t.elapsed() >= Duration::from_millis(450))
                    && let Some(file) = &saved
                {
                    prefs
                        .lock()
                        .unwrap()
                        .value
                        .write(file)
                        .map_err(|e| e.to_string())?;
                    dirty = None;
                }
                Ok(())
            })();
            if let Err(e) = result {
                log::warn!("Compact Hyprland control: {e}");
            }
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(next) => {
                    if !stopping.load(std::sync::atomic::Ordering::Relaxed) {
                        requested = Some(next);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(_) => {}
            }
        }
        // The owned native window still exists while the shell's controller drops.
        // Capture its last drag before writing, rather than relying on the poll cadence.
        if !apply
            && let Ok(clients) = list(&path, "clients")
            && let Some(current) = clients
                .as_array()
                .and_then(|a| a.iter().find(|v| owned(v, pid)))
                .and_then(bounds)
            && expected.is_none_or(|(target, _)| size_matches(current, target))
        {
            prefs.lock().unwrap().publish(generation, compact, current);
        }
        if let Some(path) = saved
            && let Err(e) = prefs.lock().unwrap().value.write(&path)
        {
            log::warn!("Cannot save window preferences: {e}");
        }
    }
    impl Adapter {
        pub fn start(prefs: Arc<Mutex<WindowPreferences>>, saved: Option<PathBuf>) -> Option<Self> {
            let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
            if !signature
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return None;
            }
            let path = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?)
                .join("hypr")
                .join(signature)
                .join(".socket.sock");
            if !path.is_absolute() || !path.exists() {
                return None;
            }
            let (tx, rx) = mpsc::channel::<(bool, Bounds, u64)>();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopping = stop.clone();
            let worker = std::thread::Builder::new()
                .name("compact-window".into())
                .spawn(move || run(path, prefs, saved, rx, stopping))
                .map_err(|e| log::warn!("Cannot start compositor adapter: {e}"))
                .ok()?;
            Some(Self {
                tx,
                stop,
                worker: Some(worker),
            })
        }
        pub fn apply(&self, compact: bool, bounds: Bounds, generation: u64) {
            if let Err(e) = self.tx.send((compact, bounds, generation)) {
                log::warn!("Cannot request companion placement: {e}");
            }
        }
    }
    impl Drop for Adapter {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = self.tx.send((false, Preferences::default().normal, 0));
            if let Some(w) = self.worker.take() {
                let _ = w.join();
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn target_acknowledgement_accepts_final_drag_but_rejects_old_mode_dimensions() {
            let requested = Preferences::default().compact;
            let moved = Bounds {
                position: Some([1000, 700]),
                ..requested
            };
            assert!(size_matches(moved, requested));
            assert!(!size_matches(Preferences::default().normal, requested));
        }
        #[test]
        fn restore_respects_scaled_rotated_work_area_and_missing_monitor() {
            let monitors = serde_json::json!([
                {"x":-1080,"y":0,"width":2160,"height":3840,"scale":2.,"transform":0,"focused":false,"reserved":[0,26,0,0]},
                {"x":0,"y":0,"width":1920,"height":1080,"scale":1.5,"transform":1,"focused":true,"reserved":[0,24,0,36]}
            ]);
            let mut b = Preferences::default().compact;
            b.position = Some([-1000, 40]);
            assert_eq!(fit_monitors(&monitors, b).unwrap().position, b.position);
            b.position = Some([10000, 10000]);
            let b = fit_monitors(&monitors, b).unwrap();
            assert_eq!(b.position, Some([200, 884]));
            assert_eq!(b.size, [520., 360.]);
        }
        #[test]
        fn targets_require_owned_pid_title_and_hex_address() {
            let v = serde_json::json!({"pid":42,"title":"Herdr mesh visualizer","mapped":true,"address":"0xabc"});
            assert!(owned(&v, 42));
            assert!(!owned(&v, 43));
            assert_eq!(address(&v), Some("0xabc"));
            let mut bad = v;
            bad["address"] = Value::String("0xabc\");evil()".into());
            assert!(address(&bad).is_none());
        }
    }
}
