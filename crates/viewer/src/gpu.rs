use std::sync::Arc;
use wgpu::*;
use winit::window::Window;

/// Core GPU state: device, queue, surface, and pipeline registry.
pub struct GpuState {
    pub device: Device,
    pub queue: Queue,
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
    pub format: TextureFormat,
    pub depth_texture: Texture,
    pub depth_view: TextureView,
}

impl GpuState {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::BROWSER_WEBGPU,
            flags: InstanceFlags::default(),
            backend_options: Default::default(),
            display: Default::default(),
            memory_budget_thresholds: Default::default(),
        });

        let surface = instance.create_surface(window).unwrap();

        // Try multiple adapter strategies — Chrome on some GPU/driver combos fails
        // surface-compatible discovery even when WebGPU is nominally available.
        let strategies: &[(&str, Option<PowerPreference>, bool, bool)] = &[
            ("surface+high", Some(PowerPreference::HighPerformance), false, true),
            ("surface+low", Some(PowerPreference::LowPower), false, true),
            ("surface+none", None, false, true),
            ("no-surface+high", Some(PowerPreference::HighPerformance), false, false),
            ("fallback", None, true, false),
        ];

        let mut adapter = None;
        let mut used_surface = false;
        for (label, pref, fallback, with_surface) in strategies {
            log::info!("adapter strategy '{label}': trying...");
            let result = instance
                .request_adapter(&RequestAdapterOptions {
                    power_preference: pref.unwrap_or(PowerPreference::None),
                    compatible_surface: if *with_surface { Some(&surface) } else { None },
                    force_fallback_adapter: *fallback,
                })
                .await;
            match result {
                Ok(a) => {
                    log::info!("adapter strategy '{label}': success — {:?}", a.get_info());
                    used_surface = *with_surface;
                    adapter = Some(a);
                    break;
                }
                Err(e) => {
                    log::warn!("adapter strategy '{label}': failed — {e}");
                }
            }
        }

        let adapter = adapter.expect(
            "No WebGPU adapter found after trying all strategies. \
             Check chrome://gpu — WebGPU must be hardware-accelerated and the GPU driver \
             must support a surface-compatible format.",
        );

        if !used_surface {
            log::warn!(
                "adapter acquired WITHOUT surface compatibility — \
                 surface configuration may fail or produce format mismatches"
            );
        }

        log::info!("Adapter: {:?}", adapter.get_info());

        let (device, queue): (Device, Queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("viewer device"),
                required_features: Features::FLOAT32_FILTERABLE,
                required_limits: Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .expect("Failed to create device");

        let caps = surface.get_capabilities(&adapter);
        let format = if caps.formats.is_empty() {
            log::warn!(
                "surface reports no compatible formats — falling back to Bgra8UnormSrgb"
            );
            TextureFormat::Bgra8UnormSrgb
        } else {
            let chosen = caps
                .formats
                .iter()
                .find(|f| f.is_srgb())
                .copied()
                .unwrap_or(caps.formats[0]);
            log::info!("surface format: {chosen:?} (from {:?})", caps.formats);
            chosen
        };

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: PresentMode::AutoVsync,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let (depth_texture, depth_view) = Self::create_depth(&device, size.width, size.height);

        Self {
            device,
            queue,
            surface,
            config,
            format,
            depth_texture,
            depth_view,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        let (dt, dv) = Self::create_depth(&self.device, width, height);
        self.depth_texture = dt;
        self.depth_view = dv;
    }

    fn create_depth(device: &Device, width: u32, height: u32) -> (Texture, TextureView) {
        let tex = device.create_texture(&TextureDescriptor {
            label: Some("depth"),
            size: Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth24Plus,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&TextureViewDescriptor::default());
        (tex, view)
    }
}
