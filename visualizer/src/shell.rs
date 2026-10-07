// Native shell adapted from clarkezone/wgputests commit
// 56551e298420764e43d6851efc9a2702273af0e1 (see README).
#[cfg(target_os = "windows")]
use crate::windows_screensaver;
use crate::{
    launch::{self, Mode},
    ui,
};
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use herdr_mesh_visualizer::client::{self, Shared};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(not(target_os = "windows"))]
use winit::window::Fullscreen;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};

struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

struct Renderer {
    gpu: Arc<Gpu>,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    context: egui::Context,
    state: egui_winit::State,
    egui: EguiRenderer,
    ui: ui::UiState,
    orb_ui: crate::orb_ui::OrbUi,
    orb: Option<crate::orbital_sphere::OrbitalSphereScene>,
    depth: wgpu::TextureView,
    #[cfg(target_os = "windows")]
    _session_notifications: Option<windows_screensaver::SessionNotifications>,
}
impl Renderer {
    async fn new(
        window: Arc<Window>,
        shared_gpu: Option<Arc<Gpu>>,
        passive: bool,
        tree: bool,
    ) -> Result<Self, String> {
        let size = window.inner_size();
        let (gpu, surface) = match shared_gpu {
            Some(gpu) => {
                let surface = gpu
                    .instance
                    .create_surface(window.clone())
                    .map_err(|e| e.to_string())?;
                (gpu, surface)
            }
            None => {
                let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
                if cfg!(target_os = "windows") && passive {
                    descriptor.backends = wgpu::Backends::DX12;
                    descriptor.backend_options.dx12.presentation_system =
                        wgpu::Dx12SwapchainKind::DxgiFromHwnd;
                }
                let instance = wgpu::Instance::new(descriptor);
                let surface = instance
                    .create_surface(window.clone())
                    .map_err(|e| e.to_string())?;
                let adapter = instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: false,
                        apply_limit_buckets: false,
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                let (device, queue) = adapter
                    .request_device(&wgpu::DeviceDescriptor {
                        label: Some("mesh visualizer"),
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                (
                    Arc::new(Gpu {
                        instance,
                        adapter,
                        device,
                        queue,
                    }),
                    surface,
                )
            }
        };
        let device = gpu.device.clone();
        let queue = gpu.queue.clone();
        let caps = surface.get_capabilities(&gpu.adapter);
        if caps.formats.is_empty() || caps.alpha_modes.is_empty() {
            return Err("The selected GPU cannot present on this screensaver monitor".into());
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps
                .formats
                .iter()
                .copied()
                .find(wgpu::TextureFormat::is_srgb)
                .unwrap_or(caps.formats[0]),
            color_space: Default::default(),
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            // DXGI composition swapchains reject Auto's unspecified alpha mode.
            alpha_mode: if cfg!(target_os = "windows") && passive {
                caps.alpha_modes
                    .iter()
                    .copied()
                    .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
                    .ok_or("Windows screensaver surface does not support opaque presentation")?
            } else {
                caps.alpha_modes[0]
            },
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        log::info!(
            "Configuring {}x{} surface with {:?} on {:?}",
            config.width,
            config.height,
            config.format,
            gpu.adapter.get_info()
        );
        surface.configure(&device, &config);
        let context = egui::Context::default();
        context.set_visuals(egui::Visuals::dark());
        let state = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let egui = EguiRenderer::new(&device, config.format, RendererOptions::default());
        let depth = depth_target(&device, config.width, config.height);
        let orb = (!tree).then(|| {
            crate::orbital_sphere::OrbitalSphereScene::new(
                &device,
                config.format,
                wgpu::TextureFormat::Depth32Float,
            )
        });
        Ok(Self {
            gpu,
            window,
            surface,
            device,
            queue,
            config,
            context,
            state,
            egui,
            ui: Default::default(),
            orb_ui: Default::default(),
            orb,
            depth,
            #[cfg(target_os = "windows")]
            _session_notifications: None,
        })
    }
    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
            self.depth = depth_target(&self.device, size.width, size.height);
        }
    }
    fn render(
        &mut self,
        shared: &Shared,
        port: u16,
        passive: bool,
        clock: f64,
    ) -> Result<(), String> {
        if self.window.inner_size().width == 0 || self.window.inner_size().height == 0 {
            return Ok(());
        }
        // Acquire before producing texture deltas: an occluded/lost surface
        // must not discard texture frees or drop an unapplied egui delta.
        let (frame, reconfigure) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => (t, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => (t, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.window.inner_size());
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("GPU surface validation failed".into());
            }
        };
        let input = self.state.take_egui_input(&self.window);
        let view = shared.read();
        let mut orb_rect = None;
        let mut output = self.context.run_ui(input, |root| {
            if self.orb.is_some() {
                orb_rect = self.orb_ui.draw(root, &view, port, passive, clock);
            } else if passive {
                self.ui.draw_screensaver(root, &view, port);
            } else {
                self.ui.draw(root, &view, port);
            }
        });
        if output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|viewport| viewport.repaint_delay.is_zero())
        {
            self.window.request_redraw();
        }
        self.state
            .handle_platform_output(&self.window, output.platform_output);
        let jobs = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in deltas {
                self.egui
                    .update_texture(&self.device, &self.queue, id, &delta);
            }
        }
        let target = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        let mut commands =
            self.egui
                .update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);
        let orb_drawn = if let Some(viewport) = orb_rect.and_then(|rect| {
            crate::orb_viewport::Viewport::physical(
                rect,
                output.pixels_per_point,
                self.config.width,
                self.config.height,
            )
        }) && let Some(orb) = &mut self.orb
        {
            orb.update_mesh(&self.queue, &self.orb_ui.sim, viewport)?;
            orb.render(&mut encoder, &target, &self.depth, Some(viewport));
            true
        } else {
            false
        };
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mesh text and connections"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: if orb_drawn {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.025,
                                g: 0.035,
                                b: 0.05,
                                a: 1.,
                            })
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.egui
                .render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        commands.push(encoder.finish());
        self.queue.submit(commands);
        self.queue.present(frame);
        for id in output.textures_delta.free.drain() {
            self.egui.free_texture(&id);
        }
        if reconfigure {
            self.resize(self.window.inner_size());
        }
        Ok(())
    }
}
fn depth_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("Orb depth"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

struct App {
    renderers: Vec<Renderer>,
    shared: Arc<Shared>,
    port: u16,
    mode: Mode,
    tree: bool,
    started: Instant,
    focus_loss_pending: Option<Instant>,
    #[cfg(target_os = "windows")]
    pointer: launch::PointerDismissal,
    #[cfg(target_os = "windows")]
    topology: Vec<String>,
    #[cfg(target_os = "windows")]
    shutdown_requested: std::rc::Rc<std::cell::Cell<bool>>,
    next_tick: Instant,
    error: Option<String>,
}
fn take_due_focus_loss(pending: &mut Option<Instant>, started: Instant, now: Instant) -> bool {
    if now.duration_since(started).as_secs_f64() < launch::INPUT_GRACE_SECONDS
        || !pending.is_some_and(|deadline| now >= deadline)
    {
        return false;
    }
    *pending = None;
    true
}
impl ApplicationHandler<()> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.renderers.is_empty() {
            return;
        }
        let result = self.create_windows(event_loop);
        match result {
            Ok(renderers) => {
                self.renderers = renderers;
                self.started = Instant::now();
                #[cfg(target_os = "windows")]
                {
                    self.topology = monitor_topology(event_loop);
                }
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }
    fn user_event(&mut self, _: &ActiveEventLoop, _: ()) {
        for r in &self.renderers {
            r.window.request_redraw();
        }
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        if self.mode.passive() {
            event_loop.exit();
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.mode == Mode::Fullscreen && matches!(&event, WindowEvent::Focused(false)) {
            self.focus_loss_pending
                .get_or_insert_with(|| Instant::now() + Duration::from_millis(100));
        }
        let Some(r) = self.renderers.iter_mut().find(|r| r.window.id() == id) else {
            return;
        };
        if self.mode == Mode::Fullscreen {
            let dismiss = matches!(
                &event,
                WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed
            ) || matches!(
                &event,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                } | WindowEvent::MouseWheel { .. }
                    | WindowEvent::Touch(_)
            );
            if dismiss {
                log::info!("Screensaver dismissed by input");
                event_loop.exit();
                return;
            }
        }
        if !self.mode.passive()
            || matches!(
                &event,
                WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Resized(_)
            )
        {
            let response = r.state.on_window_event(&r.window, &event);
            if response.repaint {
                r.window.request_redraw();
            }
        }
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Resized(size) => r.resize(size),
            WindowEvent::RedrawRequested => {
                if let Err(e) = r.render(
                    &self.shared,
                    self.port,
                    self.mode.passive(),
                    self.started.elapsed().as_secs_f64(),
                ) {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Native activation briefly clears focus while transferring between monitors.
        if take_due_focus_loss(&mut self.focus_loss_pending, self.started, Instant::now()) {
            #[cfg(target_os = "windows")]
            let owns_focus = match windows_screensaver::owns_foreground(
                self.renderers.iter().map(|r| r.window.as_ref()),
            ) {
                Ok(owns_focus) => owns_focus,
                Err(error) => {
                    self.error = Some(error);
                    event_loop.exit();
                    return;
                }
            };
            #[cfg(not(target_os = "windows"))]
            let owns_focus = self.renderers.iter().any(|r| r.window.has_focus());
            if !owns_focus {
                log::info!("Screensaver dismissed by external focus loss");
                event_loop.exit();
                return;
            }
        }
        if Instant::now() >= self.next_tick {
            #[cfg(target_os = "windows")]
            if let Err(e) = self.screensaver_tick(event_loop) {
                self.error = Some(e);
                event_loop.exit();
                return;
            }
            for r in &self.renderers {
                r.window.request_redraw();
            }
            self.next_tick = Instant::now()
                + if self.mode.passive() {
                    Duration::from_millis(33)
                } else if !self.tree {
                    Duration::from_millis(16)
                } else {
                    Duration::from_secs(1)
                };
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}
#[cfg(target_os = "windows")]
fn monitor_topology(event_loop: &ActiveEventLoop) -> Vec<String> {
    let mut monitors: Vec<_> = event_loop
        .available_monitors()
        .map(|m| {
            format!(
                "{:?}:{:?}:{:?}:{}",
                m.name(),
                m.position(),
                m.size(),
                m.scale_factor()
            )
        })
        .collect();
    monitors.sort();
    monitors
}
impl App {
    fn create_windows(&self, event_loop: &ActiveEventLoop) -> Result<Vec<Renderer>, String> {
        let attributes = match self.mode {
            Mode::Viewer => vec![
                WindowAttributes::default()
                    .with_title("Herdr mesh visualizer")
                    .with_inner_size(PhysicalSize::new(1440, 900)),
            ],
            Mode::Fullscreen => event_loop
                .available_monitors()
                .map(|monitor| {
                    let attributes = WindowAttributes::default()
                        .with_title("Herdr mesh screensaver")
                        .with_decorations(false)
                        .with_window_level(WindowLevel::AlwaysOnTop);
                    #[cfg(target_os = "windows")]
                    {
                        use winit::platform::windows::WindowAttributesExtWindows;
                        attributes
                            .with_position(monitor.position())
                            .with_inner_size(monitor.size())
                            .with_resizable(false)
                            .with_skip_taskbar(true)
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        attributes.with_fullscreen(Some(Fullscreen::Borderless(Some(monitor))))
                    }
                })
                .collect(),
            Mode::Preview(parent) => {
                #[cfg(target_os = "windows")]
                {
                    vec![windows_screensaver::preview_attributes(parent)?]
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = parent;
                    return Err("Screensaver preview is Windows-only".into());
                }
            }
            Mode::Configure(_) => return Err("Configuration does not use the GPU shell".into()),
        };
        if attributes.is_empty() {
            return Err("No monitors are available for the screensaver".into());
        }
        let mut renderers = Vec::new();
        let mut shared_gpu = None;
        for attributes in attributes {
            let window = Arc::new(
                event_loop
                    .create_window(attributes)
                    .map_err(|e| e.to_string())?,
            );
            if self.mode == Mode::Fullscreen {
                #[cfg(target_os = "windows")]
                {
                    let monitor = window
                        .current_monitor()
                        .ok_or("Screensaver window has no monitor")?;
                    // Creation can rescale the initial size when entering a different-DPI monitor.
                    let _ = window.request_inner_size(monitor.size());
                }
                window.set_cursor_visible(false);
            }
            let renderer = pollster::block_on(Renderer::new(
                window,
                shared_gpu.clone(),
                self.mode.passive(),
                self.tree,
            ))?;
            shared_gpu = Some(renderer.gpu.clone());
            #[cfg(target_os = "windows")]
            let renderer = {
                let mut renderer = renderer;
                if self.mode == Mode::Fullscreen {
                    renderer._session_notifications =
                        Some(windows_screensaver::SessionNotifications::register(
                            renderer.window.clone(),
                            self.shutdown_requested.clone(),
                        )?);
                }
                renderer
            };
            renderers.push(renderer);
        }
        Ok(renderers)
    }
    #[cfg(target_os = "windows")]
    fn screensaver_tick(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.shutdown_requested.get() {
            log::info!("Screensaver dismissed by session lock or suspend");
            event_loop.exit();
            return Ok(());
        }
        match self.mode {
            Mode::Fullscreen => {
                if monitor_topology(event_loop) != self.topology {
                    log::info!("Screensaver dismissed by display topology change");
                    event_loop.exit();
                } else if self.pointer.moved(
                    self.started.elapsed().as_secs_f64(),
                    windows_screensaver::cursor_position()?,
                ) {
                    log::info!("Screensaver dismissed by pointer movement");
                    event_loop.exit();
                }
            }
            Mode::Preview(parent) => {
                if let Some(r) = self.renderers.first()
                    && !windows_screensaver::sync_preview(&r.window, parent)?
                {
                    event_loop.exit();
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let scr = std::env::current_exe()?
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("scr"));
    let options = launch::parse(std::env::args().skip(1), cfg!(target_os = "windows"), scr)?;
    let port = options.port;
    let check = options.check;
    if options.help {
        println!(
            "herdr-mesh-visualizer [--port 8790] [--tree] [--check]\nOrb is the default. --tree opens the legacy tree. --check verifies a snapshot without opening a window."
        );
        #[cfg(target_os = "windows")]
        println!("Windows screensaver: /s | /p HWND | /c [HWND] (also /p:HWND and /c:HWND)");
        return Ok(());
    }
    if let Mode::Configure(parent) = options.mode {
        #[cfg(target_os = "windows")]
        {
            return windows_screensaver::configure(parent, port).map_err(Into::into);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = parent;
            return Err("Screensaver configuration is Windows-only".into());
        }
    }
    if check {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let scene = rt.block_on(client::check(port))?;
        println!(
            "{} nodes, {} connected, {} agents, {} working, {} blocked",
            scene.nodes.len(),
            scene.connected,
            scene.agents,
            scene.working,
            scene.blocked
        );
        if let Some(coordinator) = scene.coordinator {
            println!("coordinator: {}", coordinator.label);
        }
        return Ok(());
    }
    let mut builder = EventLoop::<()>::with_user_event();
    #[cfg(target_os = "windows")]
    let shutdown_requested = std::rc::Rc::new(std::cell::Cell::new(false));
    let event_loop = builder.build()?;
    let proxy = event_loop.create_proxy();
    let shared = Shared::new(move || {
        let _ = proxy.send_event(());
    });
    let worker_shared = shared.clone();
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let worker = std::thread::Builder::new()
        .name("mesh-observer".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("observer runtime");
            rt.block_on(client::run(port, worker_shared, stop_rx));
        })?;
    let mut app = App {
        renderers: Vec::new(),
        shared,
        port,
        mode: options.mode,
        tree: options.tree,
        started: Instant::now(),
        focus_loss_pending: None,
        #[cfg(target_os = "windows")]
        pointer: Default::default(),
        #[cfg(target_os = "windows")]
        topology: Vec::new(),
        #[cfg(target_os = "windows")]
        shutdown_requested,
        next_tick: Instant::now(),
        error: None,
    };
    let result = event_loop.run_app(&mut app);
    let _ = stop_tx.send(true);
    worker.join().map_err(|_| "observer worker panicked")?;
    result?;
    if let Some(e) = app.error {
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_focus_loss_remains_pending_until_grace_expires() {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(300);
        let mut pending = Some(deadline);
        for millis in [200, 299, 300, 330, 999] {
            assert!(!take_due_focus_loss(
                &mut pending,
                started,
                started + Duration::from_millis(millis),
            ));
            assert_eq!(pending, Some(deadline));
        }
        assert!(take_due_focus_loss(
            &mut pending,
            started,
            started + Duration::from_secs(1),
        ));
        assert_eq!(pending, None);
        assert!(!take_due_focus_loss(
            &mut pending,
            started,
            started + Duration::from_secs(2),
        ));
    }

    #[test]
    fn focus_loss_waits_for_activation_settling_after_startup() {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(1300);
        let mut pending = Some(deadline);
        assert!(!take_due_focus_loss(
            &mut pending,
            started,
            started + Duration::from_millis(1299),
        ));
        assert_eq!(pending, Some(deadline));
        assert!(take_due_focus_loss(&mut pending, started, deadline));
        assert_eq!(pending, None);

        let next_deadline = started + Duration::from_secs(2);
        pending.get_or_insert(next_deadline);
        assert!(take_due_focus_loss(&mut pending, started, next_deadline));
        assert_eq!(pending, None);
    }
}
