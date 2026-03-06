use input_system_demo::combo::ComboHandler;
use input_system_demo::config::Config;
use input_system_demo::types;
use input_system_demo::types::Event;
use sdl3::event;
use sdl3::hint;
use sdl3::joystick::JoystickId;
use sdl3::keyboard::Keycode;
use sdl3::video::WindowFlags;
use std::convert::Into;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: Config = serde_yaml::from_reader(File::open("example.yaml")?)?;
    // serde_yaml::to_writer(File::create("out.yaml")?, &config)?;
    // return Ok(());
    config.validate()?;

    // Initialize SDL3
    let sdl_context = sdl3::init()?;

    // Video is required to capture keyboard focus, even in CLI
    let video_subsystem = sdl_context.video()?;
    let gamepad_subsystem = sdl_context.gamepad()?;

    hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
    let mut window = video_subsystem
        .window("Input Tracker", 100, 100)
        .set_flags(WindowFlags::ALWAYS_ON_TOP)
        .position_centered()
        .build()?;
    // window.set_keyboard_grab(true);
    // Using a canvas ensures the window is actually refreshed
    // and recognized by the OS as an active process.
    let mut canvas = window.into_canvas();
    canvas.set_draw_color(sdl3::pixels::Color::RGB(30, 30, 30));
    canvas.clear();
    canvas.present();

    println!("SDL3 CLI Input Handler Started. Press 'Esc' to exit.");

    let mut event_pump = sdl_context.event_pump()?;
    let mut active_gamepads = std::collections::HashMap::new();

    let mut combo_handler = ComboHandler::new(config);

    'running: loop {
        for event in event_pump.wait_iter() {
            match event {
                // --- Lifecycle & Keyboard ---
                event::Event::Quit { .. }
                | event::Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,

                event::Event::KeyDown {
                    keycode: Some(key),
                    timestamp,
                    repeat: false,
                    ..
                } => {
                    // println!("{timestamp}: {key} down on keyboard");
                    let queue = combo_handler.handle(Event {
                        keycode: key.into(),
                        kind: types::Kind::Down,
                        value: 0,
                    });
                    println!("{}", queue.len());
                    while let Some(event) = queue.pop_front() {
                        println!("{event:?}");
                    }
                }

                event::Event::KeyUp {
                    keycode: Some(key),
                    timestamp,
                    repeat: false,
                    ..
                } => {
                    // println!("{timestamp}: {key} up on keyboard");
                    let queue = combo_handler.handle(Event {
                        keycode: key.into(),
                        kind: types::Kind::Up,
                        value: 0,
                    });
                    println!("{}", queue.len());
                    while let Some(event) = queue.pop_front() {
                        println!("{event:?}");
                    }
                }

                // --- Gamepad Connection (using Controller nomenclature) ---
                event::Event::ControllerDeviceAdded { which, timestamp } => {
                    // Note: 'which' in current sdl3-rs is often a u32 index
                    // that needs to be converted to a JoystickId for .open()
                    let id = JoystickId::new(which);
                    if let Ok(gamepad) = gamepad_subsystem.open(id) {
                        println!(
                            "{timestamp}: Gamepad connected: {}",
                            gamepad.name().unwrap_or_default()
                        );
                        active_gamepads.insert(which, gamepad);
                    }
                }
                event::Event::ControllerDeviceRemoved { which, timestamp } => {
                    active_gamepads.remove(&which);
                    println!("{timestamp}: Gamepad disconnected (ID: {}).", which);
                }

                // --- Gamepad Input ---
                event::Event::ControllerButtonDown {
                    button,
                    which,
                    timestamp,
                } => {
                    // println!("{timestamp}: {button:?} down on {which}");

                    let queue = combo_handler.handle(Event {
                        keycode: button.into(),
                        kind: types::Kind::Down,
                        value: 0,
                    });
                    println!("{}", queue.len());
                    while let Some(event) = queue.pop_front() {
                        println!("{event:?}");
                    }
                }

                event::Event::ControllerButtonUp {
                    button,
                    which,
                    timestamp,
                } => {
                    // println!("{timestamp}: {button:?} up on {which}");

                    let queue = combo_handler.handle(Event {
                        keycode: button.into(),
                        kind: types::Kind::Up,
                        value: 0,
                    });
                    println!("{}", queue.len());
                    while let Some(event) = queue.pop_front() {
                        println!("{event:?}");
                    }
                }

                event::Event::ControllerAxisMotion { axis, value, .. } => {
                    //println!("{timestamp}: {axis:?}={value:?} on {which}");
                    let queue = combo_handler.handle(Event {
                        keycode: axis.into(),
                        kind: types::Kind::Axis,
                        value,
                    });
                    println!("{}", queue.len());
                    while let Some(event) = queue.pop_front() {
                        println!("{event:?}");
                    }
                }

                _ => {}
            }
        }

        // Keep CPU usage low
        // std::thread::sleep(Duration::from_millis(1));
    }

    Ok(())
}
