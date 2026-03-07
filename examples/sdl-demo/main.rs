use crate::sdl_keycode::SdlKeycode;
use font_kit::handle::Handle::Path;
use frozen_collections::Scalar;
use input_system_demo::combo::ComboHandler;
use input_system_demo::config::{Action, Combo, Config};
use input_system_demo::types::{Event, Kind};
use sdl3::event;
use sdl3::hint;
use sdl3::joystick::JoystickId;
use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::video::WindowFlags;
use serde::Deserialize;
use std::convert::Into;
use std::time::{Duration, Instant};

mod sdl_keycode;

const FADE_OUT_SECS: u64 = 10;
const WINDOW_HEIGHT: u32 = 300;
const WINDOW_WIDTH: u32 = 200;

#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash, Scalar, Deserialize, Debug)]
enum Yes {
    Yes,
}

struct DisplayAction {
    key: SdlKeycode,
    modifier: Option<String>,
    action: usize,
    open: bool,
    error: bool,
    timestamp: Instant,
}

fn compile_config(template_config: Config<SdlKeycode, Yes>) -> Config<SdlKeycode, usize> {
    let mut i = usize::MAX;
    Config {
        modifiers: template_config.modifiers,
        actions: template_config
            .actions
            .into_iter()
            .map(|action| Action {
                key: action.key,
                action: action.action.map(|_| {
                    i = i.wrapping_add(1);
                    i
                }),
                immediate: action.immediate,
                modified: action
                    .modified
                    .into_iter()
                    .map(|combo| {
                        i = i.wrapping_add(1);
                        Combo {
                            modifier: combo.modifier,
                            action: i,
                        }
                    })
                    .collect(),
                latching: action.latching,
            })
            .collect(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let template_config: Config<SdlKeycode, Yes> = serde_yaml::from_str(include_str!("demo.yaml"))?;
    template_config.validate()?;
    let config = compile_config(template_config);
    let mut combo_handler = ComboHandler::new(&config);

    let mut actions: Vec<DisplayAction> = config
        .actions
        .iter()
        .flat_map(|action| {
            action
                .action
                .iter()
                .map(|a| DisplayAction {
                    key: action.key,
                    modifier: None,
                    action: *a,
                    open: false,
                    error: false,
                    timestamp: Instant::now() - Duration::from_secs(FADE_OUT_SECS),
                })
                .chain(action.modified.iter().map(|combo| DisplayAction {
                    key: action.key,
                    modifier: Some(combo.modifier.clone()),
                    action: combo.action,
                    open: false,
                    error: false,
                    timestamp: Instant::now() - Duration::from_secs(FADE_OUT_SECS),
                }))
        })
        .collect();

    // -- SDL init --
    let Path { path, .. } = font_kit::source::SystemSource::new().select_best_match(
        &[font_kit::family_name::FamilyName::Monospace],
        &font_kit::properties::Properties::default(),
    )?
    else {
        panic!("Font not found")
    };

    hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
    let sdl_context = sdl3::init()?;
    let gamepad_subsystem = sdl_context.gamepad()?;
    let video_subsystem = sdl_context.video()?;
    let ttf_context = sdl3::ttf::init()?;
    let font = ttf_context.load_font(path, 16_f32)?;

    let window = video_subsystem
        .window("Input Tracker", WINDOW_WIDTH, WINDOW_HEIGHT)
        .set_flags(WindowFlags::ALWAYS_ON_TOP)
        .position_centered()
        .build()?;
    // window.set_keyboard_grab(true);

    let mut canvas = window.into_canvas();
    let mut event_pump = sdl_context.event_pump()?;
    let mut active_gamepads = std::collections::HashMap::new();

    'gameloop: loop {
        for event in event_pump.poll_iter() {
            let event: Event<SdlKeycode> = match event {
                event::Event::Quit { .. } => break 'gameloop,

                event::Event::KeyDown {
                    keycode: Some(keycode),
                    repeat: false,
                    ..
                } => Event {
                    keycode: keycode.into(),
                    kind: Kind::Down,
                    value: 0,
                },

                event::Event::KeyUp {
                    keycode: Some(keycode),
                    repeat: false,
                    ..
                } => Event {
                    keycode: keycode.into(),
                    kind: Kind::Up,
                    value: 0,
                },

                event::Event::ControllerButtonDown {
                    button, ..
                } => Event {
                    keycode: button.into(),
                    kind: Kind::Down,
                    value: 0,
                },

                event::Event::ControllerButtonUp {
                    button, ..
                } => Event {
                    keycode: button.into(),
                    kind: Kind::Up,
                    value: 0,
                },

                event::Event::ControllerAxisMotion { axis, value, .. } => {
                    continue;
                }

                event::Event::ControllerDeviceAdded { which, timestamp } => {
                    let id = JoystickId::new(which);
                    if let Ok(gamepad) = gamepad_subsystem.open(id) {
                        println!(
                            "{timestamp}: Gamepad connected: {}",
                            gamepad.name().unwrap_or_default()
                        );
                        active_gamepads.insert(which, gamepad);
                    }
                    continue;
                }
                event::Event::ControllerDeviceRemoved { which, timestamp } => {
                    active_gamepads.remove(&which);
                    println!("{timestamp}: Gamepad disconnected (id: {})", which);
                    continue;
                }

                _ => {
                    continue;
                }
            };
            combo_handler.handle(event);
            while let Some(Event {
                               keycode: action,
                               kind,
                               ..
                           }) = combo_handler.events.pop_front()
            {
                actions[action].error = actions[action].open && kind == Kind::Down
                    || !actions[action].open && kind == Kind::Up;
                actions[action].timestamp = Instant::now();
                actions[action].open = kind == Kind::Down;
            }
        }
        // --- render ---
        canvas.set_draw_color(Color::RGB(30, 30, 30));
        canvas.clear();
        for (i, action) in actions
            .iter()
            .filter(|action| action.timestamp.elapsed() < Duration::from_secs(FADE_OUT_SECS))
            .enumerate()
        {
            let text = if let Some(modifier) = &action.modifier {
                format!("[{modifier}] {}", action.key)
            } else {
                format!("{}", action.key)
            };
            let color = match (action.open, action.error) {
                (true, true) => Color::RGB(178, 34, 34), // red: double keydown
                (false, true) => Color::RGB(204, 204, 0), // yellow: double keyup
                (true, false) => Color::RGB(11, 218, 81), // green: keydown
                (false, false) => Color::RGB(255, 255, 240), // white: keyup
            };

            let margin = (WINDOW_WIDTH as i32) / 50;
            let max_rows = ((WINDOW_HEIGHT as i32) - margin * 2) / 20;
            let surface = font.render(&text).blended(color)?;
            canvas.copy(
                &canvas
                    .texture_creator()
                    .create_texture_from_surface(&surface)?,
                None,
                Some(
                    Rect::new(
                        margin + (i as i32) / max_rows * ((WINDOW_WIDTH as i32 - margin * 2) / 2),
                        margin + (i as i32) % max_rows * 20,
                        surface.width(),
                        surface.height(),
                    )
                        .into(),
                ),
            )?;
        }
        canvas.present();
        std::thread::sleep(Duration::from_secs(1) / 30);
    }
    Ok(())
}
