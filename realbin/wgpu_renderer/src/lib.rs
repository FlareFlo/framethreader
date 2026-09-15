mod gpu;

use std::sync::Arc;
use std::time::Instant;
use gpu::WgpuState;

pub fn submain() {
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    // Default control flow is Wait, we will manage it in about_to_wait
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        window: None,
        state: None,
        last_update_inst: Instant::now(),
    };
    event_loop.run_app(&mut app).unwrap();
}


struct App {
    window: Option<Arc<Window>>,
    state: Option<WgpuState>,
    last_update_inst: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = Window::default_attributes().with_title("WGPU Video Player");
            let window = Arc::new(event_loop.create_window(attributes).unwrap());
            let folder_path = "/home/flareflo/Downloads/catch";
            self.state = Some(pollster::block_on(WgpuState::new(window.clone(), folder_path)));
            self.window = Some(window);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let state = if let Some(state) = &mut self.state { state } else { return };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(physical_size) => {
                state.resize(physical_size);
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    state: ElementState::Pressed,
                    physical_key: PhysicalKey::Code(KeyCode::Space),
                    ..
                },
                ..
            } => {
                state.is_playing = !state.is_playing;
            }
            WindowEvent::RedrawRequested => {
                let target_framerate = 30.0;
                let target_frame_time = std::time::Duration::from_secs_f32(1.0 / target_framerate);
                if state.is_playing && self.last_update_inst.elapsed() >= target_frame_time {
                    state.update_frame();
                    self.last_update_inst = Instant::now();
                }
                let _ = state.render();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            if state.is_playing {
                let target_framerate = 30.0;
                let target_frame_time = std::time::Duration::from_secs_f32(1.0 / target_framerate);
                let next_frame_time = self.last_update_inst + target_frame_time;

                if Instant::now() >= next_frame_time {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    event_loop.set_control_flow(ControlFlow::Wait);
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame_time));
                }
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }
}
