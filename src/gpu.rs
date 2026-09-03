use std::sync::{Arc, RwLock};
use std::path::PathBuf;
use rayon::prelude::*;
use winit::window::Window;

pub struct WgpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    diffuse_bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    uniform_buffer: wgpu::Buffer,
    
    frame_cache: Arc<RwLock<Vec<Option<Vec<u8>>>>>,
    total_frames: usize,
    current_frame: usize,
    pub is_playing: bool,
    image_size: wgpu::Extent3d,
}

impl WgpuState {
    pub async fn new(window: Arc<Window>, folder_path: &str) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        
        let surface = instance.create_surface(window.clone()).unwrap();
        
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }).await.unwrap();

        let device_descriptor = wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
            ..Default::default()
        };

        let device_request = adapter.request_device(&device_descriptor).await;
        let (device, queue) = device_request.unwrap();

        let mut config = surface.get_default_config(&adapter, size.width, size.height).unwrap();
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        surface.configure(&device, &config);

        let mut paths: Vec<PathBuf> = std::fs::read_dir(folder_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg")))
            .collect();
        paths.sort();
        
        let total_frames = paths.len();
        if total_frames == 0 {
            panic!("No jpeg files found in directory!");
        }

        let first_img = image::open(&paths[0]).unwrap().to_rgba8();
        let dimensions = first_img.dimensions();
        let image_size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            size: image_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("diffuse_texture"),
            view_formats: &[],
        });

        let mut initial_cache = vec![None; total_frames];
        initial_cache[0] = Some(first_img.into_raw());
        
        let frame_cache = Arc::new(RwLock::new(initial_cache));
        
        let cache_clone = Arc::clone(&frame_cache);
        let paths_clone = paths.clone();
        std::thread::spawn(move || {
            paths_clone.into_par_iter().enumerate().skip(1).for_each(|(i, path)| {
                if let Ok(img) = image::open(&path) {
                    let rgba = img.to_rgba8().into_raw();
                    let mut cache = cache_clone.write().unwrap();
                    cache[i] = Some(rgba);
                }
            });
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let scale = [1.0f32, 1.0f32];
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: std::mem::size_of::<[f32; 2]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&scale));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
            label: Some("texture_bind_group_layout"),
        });

        let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
            label: Some("diffuse_bind_group"),
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            ..Default::default()
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            surface,
            device,
            queue,
            config,
            render_pipeline,
            diffuse_bind_group,
            texture,
            uniform_buffer,
            frame_cache,
            total_frames,
            current_frame: 0,
            is_playing: true,
            image_size,
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);

            let window_aspect = new_size.width as f32 / new_size.height as f32;
            let image_aspect = self.image_size.width as f32 / self.image_size.height as f32;

            let scale = if window_aspect > image_aspect {
                [image_aspect / window_aspect, 1.0f32]
            } else {
                [1.0f32, window_aspect / image_aspect]
            };
            self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&scale));
        }
    }

    pub fn render(&mut self) -> Result<(), ()> {
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output) | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.resize(winit::dpi::PhysicalSize::new(self.config.width, self.config.height));
                return Err(());
            }
            _ => return Err(()),
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                ..Default::default()
            });
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.diffuse_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(output);
        Ok(())
    }

    pub fn update_frame(&mut self) {
        if !self.is_playing {
            return;
        }

        let cache = self.frame_cache.read().unwrap();
        if let Some(rgba) = &cache[self.current_frame] {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * self.image_size.width),
                    rows_per_image: Some(self.image_size.height),
                },
                self.image_size,
            );
            self.current_frame = (self.current_frame + 1) % self.total_frames;
        }
    }
}
