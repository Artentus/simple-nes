mod cartridge;
mod cpu;
mod device;
mod system;

const SAMPLE_RATE: u32 = 44100;

type Sample = f32;
type SampleBuffer<'a> = ringbuf::HeapProducer<Sample>;

struct SampleBufferSource {
    buffer: ringbuf::HeapConsumer<Sample>,
}

impl Iterator for SampleBufferSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.buffer.pop().unwrap_or(0.0);
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
        SAMPLE_RATE
    }

    #[inline]
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(short, long, required = true, value_name = "FILE")]
    rom: std::path::PathBuf,
}

fn main() {
    use clap::Parser;
    use gilrs::{Button, Gilrs};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use winit::dpi::PhysicalSize;
    use winit::event::{ElementState, Event, VirtualKeyCode, WindowEvent};
    use winit::event_loop::EventLoop;
    use winit::window::WindowBuilder;

    const WINDOW_WIDTH: u32 = (device::ppu::SCREEN_WIDTH as u32) * 3;
    const WINDOW_HEIGHT: u32 = (device::ppu::SCREEN_HEIGHT as u32) * 3;

    let args = Args::parse();

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("SimpleNES")
        .with_inner_size(PhysicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .build(&event_loop)
        .unwrap();

    let surface_texture = pixels::SurfaceTexture::new(WINDOW_WIDTH, WINDOW_HEIGHT, &window);
    let mut renderer = pixels::Pixels::new(
        device::ppu::SCREEN_WIDTH as u32,
        device::ppu::SCREEN_HEIGHT as u32,
        surface_texture,
    )
    .unwrap();

    let sample_bufffer = ringbuf::HeapRb::<Sample>::new((SAMPLE_RATE as usize) * 5);
    let (sample_buffer, sample_source) = sample_bufffer.split();
    let (_stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
    stream_handle
        .play_raw(SampleBufferSource {
            buffer: sample_source,
        })
        .unwrap();

    let mut gilrs = Gilrs::new().unwrap();
    let mut active_gamepad = None;

    let cart = cartridge::load_cartridge(args.rom).unwrap();
    let system = Arc::new(Mutex::new(system::System::new(cart)));
    let running = Arc::new(AtomicBool::new(true));

    let system_clone = Arc::clone(&system);
    let running_clone = Arc::clone(&running);
    let mut join_handle = Some(std::thread::spawn(move || {
        use std::time::{Duration, Instant};

        let mut sample_buffer = sample_buffer;
        let system = system_clone;
        let running = running_clone;

        let mut last_frame_time = Instant::now();
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));

            let elapsed = last_frame_time.elapsed();
            last_frame_time = Instant::now();

            system
                .lock()
                .unwrap()
                .update(elapsed.as_secs_f64(), &mut sample_buffer);
        }
    }));

    let mut controller_a_kb = device::controller::Buttons::empty();
    let mut controller_a_joy = device::controller::Buttons::empty();

    event_loop.run(move |event, _, control_flow| {
        control_flow.set_poll();

        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => {
                    control_flow.set_exit();
                    running.store(false, Ordering::Relaxed);
                    join_handle.take().unwrap().join().unwrap();
                }
                WindowEvent::Resized(new_size) => {
                    renderer
                        .resize_surface(new_size.width.max(1), new_size.height.max(1))
                        .unwrap();
                }
                WindowEvent::KeyboardInput { input, .. } => {
                    match input.virtual_keycode {
                        Some(VirtualKeyCode::R) if input.state == ElementState::Pressed => {
                            system.lock().unwrap().reset();
                        }
                        _ => (),
                    }

                    let update = match input.virtual_keycode {
                        Some(VirtualKeyCode::Up) | Some(VirtualKeyCode::W) => {
                            controller_a_kb.set(
                                device::controller::Buttons::UP,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        Some(VirtualKeyCode::Down) | Some(VirtualKeyCode::S) => {
                            controller_a_kb.set(
                                device::controller::Buttons::DOWN,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        Some(VirtualKeyCode::Left) | Some(VirtualKeyCode::A) => {
                            controller_a_kb.set(
                                device::controller::Buttons::LEFT,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        Some(VirtualKeyCode::Right) | Some(VirtualKeyCode::D) => {
                            controller_a_kb.set(
                                device::controller::Buttons::RIGHT,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        Some(VirtualKeyCode::Return) => {
                            controller_a_kb.set(
                                device::controller::Buttons::START,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        Some(VirtualKeyCode::Back) => {
                            controller_a_kb.set(
                                device::controller::Buttons::SELECT,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        Some(VirtualKeyCode::J) => {
                            controller_a_kb.set(
                                device::controller::Buttons::A,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        Some(VirtualKeyCode::K) => {
                            controller_a_kb.set(
                                device::controller::Buttons::B,
                                input.state == ElementState::Pressed,
                            );
                            true
                        }
                        _ => false,
                    };

                    if update {
                        active_gamepad = None;

                        system.lock().unwrap().update_controller_state(
                            controller_a_kb,
                            device::controller::Buttons::empty(),
                        );
                    }
                }
                _ => {}
            },
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                {
                    while let Some(gilrs::Event { id, .. }) = gilrs.next_event() {
                        if Some(id) != active_gamepad {
                            active_gamepad = Some(id);
                            controller_a_joy = device::controller::Buttons::empty();
                        }
                    }

                    let mut system = system.lock().unwrap();

                    if let Some(gamepad) = active_gamepad.map(|id| gilrs.gamepad(id)) {
                        controller_a_joy.set(
                            device::controller::Buttons::UP,
                            gamepad.is_pressed(Button::DPadUp),
                        );
                        controller_a_joy.set(
                            device::controller::Buttons::DOWN,
                            gamepad.is_pressed(Button::DPadDown),
                        );
                        controller_a_joy.set(
                            device::controller::Buttons::LEFT,
                            gamepad.is_pressed(Button::DPadLeft),
                        );
                        controller_a_joy.set(
                            device::controller::Buttons::RIGHT,
                            gamepad.is_pressed(Button::DPadRight),
                        );
                        controller_a_joy.set(
                            device::controller::Buttons::START,
                            gamepad.is_pressed(Button::Start),
                        );
                        controller_a_joy.set(
                            device::controller::Buttons::SELECT,
                            gamepad.is_pressed(Button::Select),
                        );
                        controller_a_joy.set(
                            device::controller::Buttons::A,
                            gamepad.is_pressed(Button::East) | gamepad.is_pressed(Button::South),
                        );
                        controller_a_joy.set(
                            device::controller::Buttons::B,
                            gamepad.is_pressed(Button::West) | gamepad.is_pressed(Button::North),
                        );

                        system.update_controller_state(
                            controller_a_joy,
                            device::controller::Buttons::empty(),
                        );
                    }

                    renderer.frame_mut().copy_from_slice(system.framebuffer());
                }

                renderer.render().unwrap();
            }
            Event::RedrawEventsCleared => {
                window.request_redraw();
            }
            _ => {}
        }
    });
}
