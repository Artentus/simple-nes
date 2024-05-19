mod cartridge;
mod cpu;
mod device;
mod system;

use bytemuck::{Pod, Zeroable};
use gilrs::{GamepadId, Gilrs};
use ouroboros::self_referencing;
use rodio::{OutputStream, OutputStreamHandle};
use std::mem;
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use wgpu::{
    Adapter, BindGroup, Buffer, Device, Extent3d, ImageDataLayout, Queue, RenderPipeline, Sampler,
    ShaderModule, Surface, SurfaceTexture, Texture,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

const SAMPLE_RATE: usize = 44100;

type Sample = f32;
type SampleBuffer = ringbuf::HeapProd<Sample>;
type SampleSource = ringbuf::HeapCons<Sample>;

struct SampleBufferSource {
    source: SampleSource,
}

impl Iterator for SampleBufferSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        use ringbuf::traits::Consumer;

        let sample = self.source.try_pop().unwrap_or(0.0);
        Some(sample * 10.0)
    }
}

impl rodio::Source for SampleBufferSource {
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    #[inline]
    fn channels(&self) -> u16 {
        1
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE as u32
    }

    #[inline]
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[allow(dead_code)] // Needed to keep the stream alive
struct AudioResources {
    stream: OutputStream,
    stream_handle: OutputStreamHandle,
}

impl AudioResources {
    fn create() -> (Self, SampleBuffer) {
        use ringbuf::traits::Split;

        let sample_buffer = ringbuf::HeapRb::<Sample>::new(SAMPLE_RATE / 20); // Buffer can store 50ms worth of samples
        let (sample_buffer, sample_source) = sample_buffer.split();
        let (stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
        stream_handle
            .play_raw(SampleBufferSource {
                source: sample_source,
            })
            .unwrap();

        (
            Self {
                stream,
                stream_handle,
            },
            sample_buffer,
        )
    }
}

const TEXTURE_SIZE: Extent3d = Extent3d {
    width: device::ppu::SCREEN_WIDTH as u32,
    height: device::ppu::SCREEN_HEIGHT as u32,
    depth_or_array_layers: 1,
};

const TEXTURE_LAYOUT: ImageDataLayout = ImageDataLayout {
    offset: 0,
    bytes_per_row: Some((device::ppu::SCREEN_WIDTH as u32) * 4),
    rows_per_image: None,
};

#[derive(Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

#[allow(dead_code)]
struct GpuResources<'w> {
    surface: Surface<'w>,
    adapter: Adapter,
    device: Device,
    queue: Queue,
    shader: ShaderModule,
    vertex_buffer: Buffer,
    texture: Texture,
    sampler: Sampler,
    bind_group: BindGroup,
    pipeline: RenderPipeline,
}

impl<'w> GpuResources<'w> {
    async fn create(window: &'w Window) -> Self {
        use wgpu::*;

        let instance_desc = InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        };

        let instance = Instance::new(instance_desc);

        let surface = instance
            .create_surface(window)
            .expect("failed to create surface");

        let adapter_opts = RequestAdapterOptions {
            power_preference: PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        };

        let adapter = instance
            .request_adapter(&adapter_opts)
            .await
            .expect("failed to find a graphics adapter");

        let device_desc = DeviceDescriptor {
            label: Some("W2D device"),
            required_features: Features::empty(),
            required_limits: Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits())
                .using_alignment(adapter.limits()),
        };

        let (device, queue) = adapter
            .request_device(&device_desc, None)
            .await
            .expect("failed to create device");

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: (mem::size_of::<Vertex>() * 6) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: TEXTURE_SIZE,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(
                        &texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[VertexBufferLayout {
                    array_stride: mem::size_of::<Vertex>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
        });

        let this = Self {
            surface,
            adapter,
            device,
            queue,
            shader,
            vertex_buffer,
            texture,
            sampler,
            bind_group,
            pipeline,
        };

        this.configure_surface(window.inner_size());

        this
    }

    fn configure_surface(&self, size: PhysicalSize<u32>) {
        let mut surface_config = self
            .surface
            .get_default_config(&self.adapter, size.width.max(1), size.height.max(1))
            .expect("failed to configure surface");
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;

        self.surface.configure(&self.device, &surface_config);
    }
}

#[self_referencing]
struct AppResources {
    window: Window,
    audio_resources: Option<AudioResources>,
    #[borrows(window)]
    #[not_covariant]
    gpu_resources: Option<GpuResources<'this>>,
}

fn run_emu(running: &AtomicBool, system: &Mutex<system::System>, mut sample_buffer: SampleBuffer) {
    use ringbuf::traits::Observer;
    use std::time::Duration;

    while running.load(atomic::Ordering::Acquire) {
        // Run emulation until we have at least 15ms worth of samples in the buffer
        {
            let mut system = system.lock().unwrap();
            while sample_buffer.occupied_len() < (SAMPLE_RATE / 67) {
                system.clock(1000, &mut sample_buffer);
            }
        }

        // Idle until we have less than 10ms worth of samples in the buffer
        let available_audio_duration =
            Duration::from_secs_f64((sample_buffer.occupied_len() as f64) / (SAMPLE_RATE as f64));
        spin_sleep::sleep(available_audio_duration.saturating_sub(Duration::from_millis(10)));
    }
}

fn update_gamepad(
    gilrs: Option<&mut Gilrs>,
    active_gamepad: &mut Option<GamepadId>,
) -> Option<device::controller::Buttons> {
    gilrs.and_then(|gilrs| {
        while let Some(gilrs::Event { id, .. }) = gilrs.next_event() {
            *active_gamepad = Some(id);
        }

        active_gamepad.map(|id| {
            let gamepad = gilrs.gamepad(id);
            let mut controller_a_joy = device::controller::Buttons::empty();

            controller_a_joy.set(
                device::controller::Buttons::UP,
                gamepad.is_pressed(gilrs::Button::DPadUp),
            );
            controller_a_joy.set(
                device::controller::Buttons::DOWN,
                gamepad.is_pressed(gilrs::Button::DPadDown),
            );
            controller_a_joy.set(
                device::controller::Buttons::LEFT,
                gamepad.is_pressed(gilrs::Button::DPadLeft),
            );
            controller_a_joy.set(
                device::controller::Buttons::RIGHT,
                gamepad.is_pressed(gilrs::Button::DPadRight),
            );
            controller_a_joy.set(
                device::controller::Buttons::START,
                gamepad.is_pressed(gilrs::Button::Start),
            );
            controller_a_joy.set(
                device::controller::Buttons::SELECT,
                gamepad.is_pressed(gilrs::Button::Select),
            );
            controller_a_joy.set(
                device::controller::Buttons::A,
                gamepad.is_pressed(gilrs::Button::East) | gamepad.is_pressed(gilrs::Button::South),
            );
            controller_a_joy.set(
                device::controller::Buttons::B,
                gamepad.is_pressed(gilrs::Button::West) | gamepad.is_pressed(gilrs::Button::North),
            );

            controller_a_joy
        })
    })
}

fn create_vertices(window_size: PhysicalSize<u32>) -> [Vertex; 6] {
    let width_scale = (window_size.width as f32) / (device::ppu::SCREEN_WIDTH as f32);
    let height_scale = (window_size.height as f32) / (device::ppu::SCREEN_HEIGHT as f32);
    let scale = width_scale.min(height_scale);

    let width_coord = scale / width_scale;
    let height_coord = scale / height_scale;

    [
        Vertex {
            position: [-width_coord, -height_coord],
            tex_coords: [0.0, 1.0],
        },
        Vertex {
            position: [width_coord, -height_coord],
            tex_coords: [1.0, 1.0],
        },
        Vertex {
            position: [width_coord, height_coord],
            tex_coords: [1.0, 0.0],
        },
        Vertex {
            position: [-width_coord, -height_coord],
            tex_coords: [0.0, 1.0],
        },
        Vertex {
            position: [width_coord, height_coord],
            tex_coords: [1.0, 0.0],
        },
        Vertex {
            position: [-width_coord, height_coord],
            tex_coords: [0.0, 0.0],
        },
    ]
}

fn draw(gpu_resources: &GpuResources, frame: SurfaceTexture) {
    use wgpu::{
        Color, CommandEncoderDescriptor, LoadOp, Operations, RenderPassColorAttachment,
        RenderPassDescriptor, StoreOp, TextureViewDescriptor,
    };

    let framebuffer = frame.texture.create_view(&TextureViewDescriptor::default());

    let mut encoder = gpu_resources
        .device
        .create_command_encoder(&CommandEncoderDescriptor { label: None });

    {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &framebuffer,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_pipeline(&gpu_resources.pipeline);
        pass.set_vertex_buffer(0, gpu_resources.vertex_buffer.slice(..));
        pass.set_bind_group(0, &gpu_resources.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }

    gpu_resources.queue.submit(Some(encoder.finish()));
    frame.present();
}

struct App {
    resources: Option<AppResources>,
    running: Arc<AtomicBool>,
    system: Arc<Mutex<system::System>>,
    thread_handle: Option<JoinHandle<()>>,
    gilrs: Option<Gilrs>,
    active_gamepad: Option<GamepadId>,
    controller_a_kb: device::controller::Buttons,
}

impl App {
    fn new(rom: impl AsRef<std::path::Path>) -> Self {
        let cart = cartridge::load_cartridge(rom).unwrap();

        Self {
            resources: None,
            running: Arc::new(AtomicBool::new(false)),
            system: Arc::new(Mutex::new(system::System::new(cart))),
            thread_handle: None,
            gilrs: Gilrs::new().ok(),
            active_gamepad: None,
            controller_a_kb: device::controller::Buttons::empty(),
        }
    }

    fn update_keyboard(&mut self, event: KeyEvent) {
        match event.physical_key {
            PhysicalKey::Code(KeyCode::KeyR) if event.state == ElementState::Pressed => {
                self.system.lock().unwrap().reset();
            }
            _ => (),
        }

        let button = match event.physical_key {
            PhysicalKey::Code(KeyCode::ArrowUp) | PhysicalKey::Code(KeyCode::KeyW) => {
                Some(device::controller::Buttons::UP)
            }
            PhysicalKey::Code(KeyCode::ArrowDown) | PhysicalKey::Code(KeyCode::KeyS) => {
                Some(device::controller::Buttons::DOWN)
            }
            PhysicalKey::Code(KeyCode::ArrowLeft) | PhysicalKey::Code(KeyCode::KeyA) => {
                Some(device::controller::Buttons::LEFT)
            }
            PhysicalKey::Code(KeyCode::ArrowRight) | PhysicalKey::Code(KeyCode::KeyD) => {
                Some(device::controller::Buttons::RIGHT)
            }
            PhysicalKey::Code(KeyCode::Enter) => Some(device::controller::Buttons::START),
            PhysicalKey::Code(KeyCode::Backspace) => Some(device::controller::Buttons::SELECT),
            PhysicalKey::Code(KeyCode::KeyJ) => Some(device::controller::Buttons::A),
            PhysicalKey::Code(KeyCode::KeyK) => Some(device::controller::Buttons::B),
            _ => None,
        };

        if let Some(button) = button {
            self.active_gamepad = None;
            self.controller_a_kb
                .set(button, event.state == ElementState::Pressed);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let (audio_resource, sample_buffer) = AudioResources::create();

        if let Some(resources) = &mut self.resources {
            resources.with_mut(|fields| {
                assert!(fields.audio_resources.is_none());
                assert!(fields.gpu_resources.is_none());

                *fields.audio_resources = Some(audio_resource);
                *fields.gpu_resources =
                    Some(pollster::block_on(GpuResources::create(fields.window)));
            })
        } else {
            const DEFAULT_WINDOW_WIDTH: u32 = (device::ppu::SCREEN_WIDTH as u32) * 3;
            const DEFAULT_WINDOW_HEIGHT: u32 = (device::ppu::SCREEN_HEIGHT as u32) * 3;

            let window_attrs = WindowAttributes::default()
                .with_title("SimpleNES")
                .with_inner_size(PhysicalSize::new(
                    DEFAULT_WINDOW_WIDTH,
                    DEFAULT_WINDOW_HEIGHT,
                ));
            let window = event_loop
                .create_window(window_attrs)
                .expect("failed to create window");

            let builder = AppResourcesBuilder {
                window,
                audio_resources: Some(audio_resource),
                gpu_resources_builder: |window| {
                    Some(pollster::block_on(GpuResources::create(window)))
                },
            };

            self.resources = Some(builder.build())
        }

        self.running.store(true, atomic::Ordering::Release);
        let running = Arc::clone(&self.running);
        let system = Arc::clone(&self.system);

        assert!(self.thread_handle.is_none());
        self.thread_handle = Some(thread::spawn(move || {
            let running = running;
            let system = system;
            run_emu(&*running, &*system, sample_buffer);
        }));
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.running.store(false, atomic::Ordering::Release);
        self.thread_handle.take().unwrap().join().unwrap();
        self.controller_a_kb = device::controller::Buttons::empty();

        self.resources.as_mut().unwrap().with_mut(|fields| {
            assert!(fields.audio_resources.is_some());
            assert!(fields.gpu_resources.is_some());

            *fields.audio_resources = None;
            *fields.gpu_resources = None;
        });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(resources) = &self.resources {
            if window_id == resources.borrow_window().id() {
                match event {
                    WindowEvent::CloseRequested => {
                        if let Some(thread_handle) = self.thread_handle.take() {
                            self.running.store(false, atomic::Ordering::Release);
                            thread_handle.join().unwrap();
                        }

                        event_loop.exit();
                    }
                    WindowEvent::Resized(new_size) => {
                        resources.with_gpu_resources(|gpu_resources| {
                            if let Some(gpu_resources) = gpu_resources {
                                gpu_resources.configure_surface(new_size);
                            }
                        });
                    }
                    WindowEvent::KeyboardInput { event, .. } => self.update_keyboard(event),
                    WindowEvent::RedrawRequested => {
                        let controller_a =
                            update_gamepad(self.gilrs.as_mut(), &mut self.active_gamepad)
                                .unwrap_or(self.controller_a_kb);

                        let mut system = self.system.lock().unwrap();

                        system.update_controller_state(
                            controller_a,
                            device::controller::Buttons::empty(),
                        );

                        resources.with_gpu_resources(|gpu_resources| {
                            if let Some(gpu_resources) = gpu_resources {
                                let frame = match gpu_resources.surface.get_current_texture() {
                                    Ok(frame) => frame,
                                    Err(wgpu::SurfaceError::Outdated) => return,
                                    Err(err) => panic!("failed to aquire framebuffer: {err:?}"),
                                };

                                gpu_resources.queue.write_texture(
                                    gpu_resources.texture.as_image_copy(),
                                    system.framebuffer(),
                                    TEXTURE_LAYOUT,
                                    TEXTURE_SIZE,
                                );

                                mem::drop(system);

                                gpu_resources.queue.write_buffer(
                                    &gpu_resources.vertex_buffer,
                                    0,
                                    bytemuck::cast_slice(&create_vertices(
                                        resources.borrow_window().inner_size(),
                                    )),
                                );

                                draw(gpu_resources, frame);
                            }
                        });

                        resources.borrow_window().request_redraw();
                    }
                    _ => (),
                }
            }
        }
    }
}

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(short, long, required = true, value_name = "FILE")]
    rom: std::path::PathBuf,
}

fn main() {
    use clap::Parser;
    use winit::event_loop::EventLoop;

    let args = Args::parse();
    let mut app = App::new(args.rom);

    let event_loop = EventLoop::new().expect("unable to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
        .run_app(&mut app)
        .expect("unable to run event loop");
}
