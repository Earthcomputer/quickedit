use conrod_core::{Colorable, event, input, Labelable, Positionable, Sizeable, widget, Widget, widget_ids};
use glam::{DMat2, DVec2};
use winit::dpi;
use crate::{minecraft, world, renderer};

widget_ids!(pub struct Ids {
    debug,
    open_button,
});

pub struct UiState {
    ids: Ids,
    key_states: KeyStates,
}

#[derive(Default)]
struct KeyStates {
    mouse_grabbed: bool,
    neg_x_down: bool,
    pos_x_down: bool,
    neg_y_down: bool,
    pos_y_down: bool,
    neg_z_down: bool,
    pos_z_down: bool,
    neg_yaw_down: bool,
    pos_yaw_down: bool,
    neg_pitch_down: bool,
    pos_pitch_down: bool,
    mouse_dx: f64,
    mouse_dy: f64,
}

pub fn init_ui(ui: &mut conrod_core::Ui) -> UiState {
    return UiState { ids: Ids::new(ui.widget_id_generator()), key_states: KeyStates::default() };
}

pub fn set_ui(state: &UiState, ui: &mut conrod_core::UiCell) {
    let (x, y, z, yaw, pitch) = {
        let worlds = world::WORLDS.read().unwrap();
        match worlds.last() {
            Some(world) => {
                let camera = &world.camera.read().unwrap();
                (camera.pos.x, camera.pos.y, camera.pos.z, camera.yaw, camera.pitch)
            }
            None => (0.0, 0.0, 0.0, 0.0, 0.0),
        }
    };
    widget::Text::new(format!("Pos: {:.2}, {:.2}, {:.2}, yaw: {:.2}, pitch: {:.2}", x, y, z, yaw, pitch).as_str())
        .mid_left_of(ui.window)
        .color(conrod_core::color::WHITE)
        .set(state.ids.debug, ui);

    if widget::Button::new()
        .label("Open")
        .top_left_of(ui.window)
        .w_h(200.0, 50.0)
        .set(state.ids.open_button, ui)
        .was_clicked() {
        open_clicked();
    }
}

fn open_clicked() {
    let path = native_dialog::FileDialog::new().show_open_single_dir();
    if let Ok(Some(path)) = path {
        let mut interaction_handler = UiInteractionHandler{};
        let executor = async_executor::LocalExecutor::new();
        let task = executor.spawn(async { world::World::load(path, &mut interaction_handler) });
        let world = futures_lite::future::block_on(executor.run(task)).expect("Failed to load world");
        let mut worlds = world::WORLDS.write().unwrap();
        worlds.push(world);
    }
}

pub fn handle_event(ui_state: &mut UiState, ui: &conrod_core::Ui, event: &event::Event) {
    fn move_cursor_to_middle() -> Result<(), winit::error::ExternalError> {
        let gl_window = renderer::get_display().gl_window();
        let window = gl_window.window();
        let window_size = window.inner_size();
        window.set_cursor_position(dpi::PhysicalPosition::new(window_size.width as f32 * 0.5, window_size.height as f32 * 0.5))
    }

    match event {
        event::Event::Ui(event::Ui::Press(Some(pressed_id), event::Press{button: event::Button::Mouse(input::MouseButton::Left, _), ..})) => {
            if *pressed_id == ui.window && !ui_state.key_states.mouse_grabbed {
                ui_state.key_states.mouse_grabbed = true;
                if renderer::get_display().gl_window().window().set_cursor_grab(true).is_ok() {
                    renderer::get_display().gl_window().window().set_cursor_visible(false);
                    let _ = move_cursor_to_middle(); // ignore errors
                }
            }
        }
        event::Event::Raw(event::Input::Press(input::Button::Keyboard(key))) => {
            if ui_state.key_states.mouse_grabbed {
                match key {
                    input::Key::Escape => {
                        ui_state.key_states.mouse_grabbed = false;
                        if renderer::get_display().gl_window().window().set_cursor_grab(false).is_ok() {
                            renderer::get_display().gl_window().window().set_cursor_visible(true);
                        }
                    }
                    input::Key::A => ui_state.key_states.neg_x_down = true,
                    input::Key::D => ui_state.key_states.pos_x_down = true,
                    input::Key::Space => ui_state.key_states.pos_y_down = true,
                    input::Key::LShift => ui_state.key_states.neg_y_down = true,
                    input::Key::S => ui_state.key_states.pos_z_down = true,
                    input::Key::W => ui_state.key_states.neg_z_down = true,
                    input::Key::Left => ui_state.key_states.pos_yaw_down = true,
                    input::Key::Right => ui_state.key_states.neg_yaw_down = true,
                    input::Key::Up => ui_state.key_states.pos_pitch_down = true,
                    input::Key::Down => ui_state.key_states.neg_pitch_down = true,
                    _ => {}
                }
            }
        }
        event::Event::Raw(event::Input::Release(input::Button::Keyboard(key))) => {
            match key {
                input::Key::A => ui_state.key_states.neg_x_down = false,
                input::Key::D => ui_state.key_states.pos_x_down = false,
                input::Key::Space => ui_state.key_states.pos_y_down = false,
                input::Key::LShift => ui_state.key_states.neg_y_down = false,
                input::Key::S => ui_state.key_states.pos_z_down = false,
                input::Key::W => ui_state.key_states.neg_z_down = false,
                input::Key::Left => ui_state.key_states.pos_yaw_down = false,
                input::Key::Right => ui_state.key_states.neg_yaw_down = false,
                input::Key::Up => ui_state.key_states.pos_pitch_down = false,
                input::Key::Down => ui_state.key_states.neg_pitch_down = false,
                _ => {}
            }
        }
        event::Event::Raw(event::Input::Motion(input::Motion::MouseCursor { x, y })) => {
            if ui_state.key_states.mouse_grabbed {
                let window_point = ui.xy_of(ui.window).unwrap();
                ui_state.key_states.mouse_dx += x - window_point[0];
                ui_state.key_states.mouse_dy += y - window_point[1];
                if move_cursor_to_middle().is_err() {
                    // unsupported on this platform
                    ui_state.key_states.mouse_dx = 0.0;
                    ui_state.key_states.mouse_dy = 0.0;
                }
            }
        }
        event::Event::Raw(event::Input::Focus(false)) => {
            ui_state.key_states.mouse_grabbed = false;
            if renderer::get_display().gl_window().window().set_cursor_grab(false).is_ok() {
                renderer::get_display().gl_window().window().set_cursor_visible(true);
            }
        }
        _ => {}
    }
}

pub fn needs_tick(ui_state: &UiState) -> bool {
    ui_state.key_states.neg_x_down || ui_state.key_states.pos_x_down ||
    ui_state.key_states.neg_y_down || ui_state.key_states.pos_y_down ||
    ui_state.key_states.neg_z_down || ui_state.key_states.pos_z_down ||
    ui_state.key_states.neg_yaw_down || ui_state.key_states.pos_yaw_down ||
    ui_state.key_states.neg_pitch_down || ui_state.key_states.pos_pitch_down ||
    ui_state.key_states.mouse_dx != 0.0 || ui_state.key_states.mouse_dy != 0.0
}

pub fn tick(ui_state: &mut UiState) {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut yaw = 0.0;
    let mut pitch = 0.0;

    let movement_speed = 1.0;
    let rotation_speed = 3.0;
    let mouse_sensitivity = 0.05;

    if ui_state.key_states.neg_x_down {
        x -= movement_speed;
    }
    if ui_state.key_states.pos_x_down {
        x += movement_speed;
    }
    if ui_state.key_states.neg_y_down {
        y -= movement_speed;
    }
    if ui_state.key_states.pos_y_down {
        y += movement_speed;
    }
    if ui_state.key_states.neg_z_down {
        z -= movement_speed;
    }
    if ui_state.key_states.pos_z_down {
        z += movement_speed;
    }
    if ui_state.key_states.neg_yaw_down {
        yaw -= rotation_speed;
    }
    if ui_state.key_states.pos_yaw_down {
        yaw += rotation_speed;
    }
    if ui_state.key_states.neg_pitch_down {
        pitch -= rotation_speed;
    }
    if ui_state.key_states.pos_pitch_down {
        pitch += rotation_speed;
    }
    yaw -= ui_state.key_states.mouse_dx as f32 * mouse_sensitivity;
    pitch += ui_state.key_states.mouse_dy as f32 * mouse_sensitivity;
    ui_state.key_states.mouse_dx = 0.0;
    ui_state.key_states.mouse_dy = 0.0;
    if x != 0.0 || y != 0.0 || z != 0.0 || yaw != 0.0 || pitch != 0.0 {
        let mut worlds = world::WORLDS.write().unwrap();
        let world = worlds.last_mut().unwrap();
        let mut camera = world.camera.write().unwrap();
        let xz = DMat2::from_angle(-(camera.yaw as f64).to_radians()).mul_vec2(DVec2::new(x, z));
        camera.move_camera(xz.x, y, xz.y, yaw, pitch);
    }
}

struct UiInteractionHandler;

// TODO: make this an actual UI handler
impl minecraft::DownloadInteractionHandler for UiInteractionHandler {
    fn show_download_prompt(&mut self, mc_version: &str) -> bool {
        println!("Downloading {}", mc_version);
        true
    }

    fn on_start_download(&mut self) {
        println!("Download started");
    }

    fn on_finish_download(&mut self) {
        println!("Download finished");
    }
}
