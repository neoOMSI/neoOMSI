use crate::*;

pub struct SurfaceState<'w> {
    pub surface: std::mem::ManuallyDrop<wgpu::Surface<'w>>,
    pub config: wgpu::SurfaceConfiguration,
    pub(crate) lost: Arc<std::sync::Mutex<Option<String>>>,
}

impl Drop for SurfaceState<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && self
                .lost
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        {
            // SAFETY: dropped only here, once
            unsafe { std::mem::ManuallyDrop::drop(&mut self.surface) };
        }
    }
}

impl<'w> SurfaceState<'w> {
    pub fn new(
        instance: &wgpu::Instance,
        window: Arc<winit_window::Window>,
        renderer: &Renderer,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        Self::new_with(instance, window, renderer, width, height, true)
    }

    pub fn new_with(
        instance: &wgpu::Instance,
        window: Arc<winit_window::Window>,
        renderer: &Renderer,
        width: u32,
        height: u32,
        vsync: bool,
    ) -> Result<Self> {
        let surface = instance.create_surface(window).context("create_surface")?;
        // copied from as well where the device allows it: the frosted overlays blur what is
        // beneath them (`Renderer::frost_backdrops`)
        let mut config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: renderer.format(),
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            present_mode: if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        config.width = width.max(1);
        config.height = height.max(1);
        let scope = renderer.device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&renderer.device, &config);
        if let Some(e) = pollster::block_on(scope.pop()) {
            log::warn!(
                "the window's picture cannot be copied from on this device ({}); overlays are not frosted",
                gpu_error_text(&e)
            );
            config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
            surface.configure(&renderer.device, &config);
        }
        Ok(SurfaceState {
            surface: std::mem::ManuallyDrop::new(surface),
            config,
            lost: renderer.device_lost.clone(),
        })
    }

    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        if renderer.device_lost().is_some() {
            return;
        }
        self.surface.configure(&renderer.device, &self.config);
    }

    pub fn set_vsync(&mut self, renderer: &Renderer, enabled: bool) {
        let mode = if enabled {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        if self.config.present_mode == mode || renderer.device_lost().is_some() {
            return;
        }
        self.config.present_mode = mode;
        self.config.desired_maximum_frame_latency = 2;
        self.surface.configure(&renderer.device, &self.config);
    }
}

pub mod winit_window {
    pub use winit::window::Window;
}
