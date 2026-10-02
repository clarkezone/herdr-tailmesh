//! Native shell adapted from clarkezone/wgputests commit
//! 56551e298420764e43d6851efc9a2702273af0e1 (see README).
mod animation;
mod fleet_panel;
mod ui;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use herdr_mesh_visualizer::client::{self, Shared};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

struct Renderer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    context: egui::Context,
    state: egui_winit::State,
    egui: EguiRenderer,
    ui: ui::UiState,
}
impl Renderer {
    async fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
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
        let caps = surface.get_capabilities(&adapter);
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
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
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
        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            context,
            state,
            egui,
            ui: Default::default(),
        })
    }
    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }
    fn render(&mut self, shared: &Shared, port: u16) -> Result<(), String> {
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
        let mut output = self
            .context
            .run_ui(input, |root| self.ui.draw(root, &view, port));
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
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mesh text and connections"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.025,
                            g: 0.035,
                            b: 0.05,
                            a: 1.,
                        }),
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
struct App {
    renderer: Option<Renderer>,
    shared: Arc<Shared>,
    port: u16,
    next_tick: Instant,
    error: Option<String>,
}
impl ApplicationHandler<()> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return;
        }
        let result = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Herdr mesh visualizer")
                    .with_inner_size(PhysicalSize::new(1440, 900)),
            )
            .map_err(|e| e.to_string())
            .and_then(|w| pollster::block_on(Renderer::new(Arc::new(w))));
        match result {
            Ok(r) => self.renderer = Some(r),
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }
    fn user_event(&mut self, _: &ActiveEventLoop, _: ()) {
        if let Some(r) = &self.renderer {
            r.window.request_redraw();
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.renderer else {
            return;
        };
        if r.window.id() != id {
            return;
        }
        let response = r.state.on_window_event(&r.window, &event);
        if response.repaint {
            r.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => r.resize(size),
            WindowEvent::RedrawRequested => {
                if let Err(e) = r.render(&self.shared, self.port) {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if Instant::now() >= self.next_tick {
            if let Some(r) = &self.renderer {
                r.window.request_redraw();
            }
            self.next_tick = Instant::now() + Duration::from_secs(1);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}
fn options() -> Result<(u16, bool), String> {
    let mut args = std::env::args().skip(1);
    let mut port = 8790;
    let mut check = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .ok_or("--port needs a value")?
                    .parse::<u16>()
                    .map_err(|_| "--port must be 1..65535")?;
                if port == 0 {
                    return Err("--port must be 1..65535".into());
                }
            }
            "--check" => check = true,
            "--help" | "-h" => {
                println!(
                    "herdr-mesh-visualizer [--port 8790] [--check]\nRead the local daemon; --check verifies a snapshot without opening a window."
                );
                std::process::exit(0);
            }
            _ => return Err(format!("Unknown option: {arg}")),
        }
    }
    Ok((port, check))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let (port, check) = options()?;
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
    let event_loop = EventLoop::<()>::with_user_event().build()?;
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
        renderer: None,
        shared,
        port,
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
