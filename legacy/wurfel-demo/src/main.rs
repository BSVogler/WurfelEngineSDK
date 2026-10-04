use wurfel_engine::{
    core::{Engine, EngineConfig},
    graphics::Camera,
    map::{Chunk, IslandGenerator, Generator},
    Vec3,
};
use std::sync::Arc;
use winit::{
    event::*,
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::WindowBuilder,
};

/// Simple windowed demo
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting Wurfel Engine Visual Demo");

    // Create event loop and window
    let event_loop = EventLoop::new()?;
    let window = Arc::new(WindowBuilder::new()
        .with_title("Wurfel Engine Demo - Chunk Rendering")
        .with_inner_size(winit::dpi::LogicalSize::new(1024, 768))
        .build(&event_loop)?);

    // Create wgpu instance and surface
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let window_for_surface = window.clone();
    let surface = unsafe { instance.create_surface(window_for_surface.as_ref())? };
    let window_id = window.id();

    // Create engine
    let config = EngineConfig {
        width: window.inner_size().width,
        height: window.inner_size().height,
        title: "Wurfel Engine Demo".to_string(),
        vsync: true,
        max_fps: 60,
    };

    let mut engine = Engine::new(config);

    // Initialize engine with shared instance
    match engine.initialize_with_surface(&instance, &surface).await {
        Ok(_) => log::info!("Engine initialized successfully!"),
        Err(e) => {
            log::error!("Failed to initialize engine: {}", e);
            return Err(e.into());
        }
    }

    // Surface will be configured automatically during first render

    // Create camera with authentic Wurfel Engine settings
    let mut camera = Camera::new(1024.0 / 768.0);
    // Use view space positioning like original, but pulled back further to see full block faces
    camera.set_position(Vec3::new(0.0, 0.0, 500.0));
    camera.set_target(Vec3::new(0.0, 0.0, 70.0)); // Look at center of block height
    camera.set_fov(1200.0); // Much wider view to see all face vertices

    // Skip sprite atlas loading for now to test colored blocks
    log::info!("Using colored blocks instead of sprite atlas");

    // Generate a demo chunk
    let mut demo_chunk = Chunk::new((0, 0));
    let generator = IslandGenerator::new(12345).with_base_height(6);
    generator.generate(&mut demo_chunk);

    let mut block_count = 0;
    for ((_x, _y, _z), block) in demo_chunk.blocks() {
        if !block.is_air() {
            block_count += 1;
        }
    }
    log::info!("Generated chunk with {} solid blocks", block_count);

    // Create a debug render for testing
    if let Err(e) = engine.create_debug_render() {
        log::error!("Failed to create debug render: {}", e);
    }

    // Test authentic Wurfel Engine block face rendering
    if let Err(e) = engine.render_chunk_to_file_authentic(&demo_chunk, &mut camera, "chunk_render.png") {
        log::error!("Failed to render chunk with authentic system: {}", e);
    }

    // Simple movement state
    let mut move_forward = false;
    let mut move_backward = false;
    let mut move_left = false;
    let mut move_right = false;
    let mut zoom_in = false;
    let mut zoom_out = false;

    let mut last_time = std::time::Instant::now();

    log::info!("Demo ready! Controls:");
    log::info!("  WASD: Move camera");
    log::info!("  Q/E: Zoom in/out");
    log::info!("  ESC: Exit");

    event_loop.run(move |event, control_flow| {
        match event {
            Event::WindowEvent {
                ref event,
                window_id: event_window_id,
            } if event_window_id == window_id => {
                match event {
                    WindowEvent::CloseRequested => {
                        log::info!("Window close requested");
                        control_flow.exit();
                    }
                    WindowEvent::KeyboardInput {
                        event: KeyEvent {
                            physical_key: PhysicalKey::Code(key_code),
                            state,
                            ..
                        },
                        ..
                    } => {
                        let pressed = *state == ElementState::Pressed;
                        match key_code {
                            KeyCode::KeyW => move_forward = pressed,
                            KeyCode::KeyS => move_backward = pressed,
                            KeyCode::KeyA => move_left = pressed,
                            KeyCode::KeyD => move_right = pressed,
                            KeyCode::KeyQ => zoom_in = pressed,
                            KeyCode::KeyE => zoom_out = pressed,
                            KeyCode::Escape => {
                                log::info!("Escape pressed, exiting");
                                control_flow.exit();
                            }
                            _ => {}
                        }
                    }
                    WindowEvent::Resized(physical_size) => {
                        log::info!("Window resized to {:?}", physical_size);
                        camera.set_aspect(physical_size.width as f32 / physical_size.height as f32);
                    }
                    WindowEvent::RedrawRequested => {
                        // Update camera based on input
                        let now = std::time::Instant::now();
                        let dt = (now - last_time).as_secs_f32();
                        last_time = now;

                        let move_speed = 10.0 * dt;
                        let mut movement = Vec3::ZERO;

                        if move_forward {
                            movement.z -= move_speed;
                        }
                        if move_backward {
                            movement.z += move_speed;
                        }
                        if move_left {
                            movement.x -= move_speed;
                        }
                        if move_right {
                            movement.x += move_speed;
                        }

                        if movement.length() > 0.0 {
                            camera.translate(movement);
                        }

                        if zoom_in {
                            camera.zoom(0.99);
                        }
                        if zoom_out {
                            camera.zoom(1.01);
                        }

                        // Render the chunk
                        match engine.render_chunk(&demo_chunk, &mut camera, &surface) {
                            Ok(_) => {}
                            Err(e) => {
                                log::warn!("Render error: {}", e);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    })?;

    Ok(())
}