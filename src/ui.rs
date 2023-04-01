use glam::{DMat2, DVec2};
use lazy_static::lazy_static;
use winit::{dpi, event};
use std::cell::RefCell;
use egui::Color32;
use egui_glium::egui_winit::winit::window::CursorGrabMode;
use log::info;
use crate::{minecraft, world, renderer};
use crate::util::MainThreadStore;

#[derive(Default)]
pub struct UiState {
    key_states: KeyStates,
}

#[derive(Default)]
struct KeyStates {
    mouse_grabbed: bool,
    mouse_dx: f64,
    mouse_dy: f64,
}

pub fn run_ui(_state: &UiState, egui_ctx: &egui::Context, _quit: &mut bool) {
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
    egui::TopBottomPanel::top("top_panel").show(egui_ctx, |ui| {
        if ui.button("Open")
            .clicked()
        {
            open_clicked();
        }
    });
    egui::SidePanel::left("left_panel").show(egui_ctx, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
            ui.colored_label(
                Color32::WHITE,
                format!("Pos: {:.2}, {:.2}, {:.2}, yaw: {:.2}, pitch: {:.2}", x, y, z, yaw, pitch).as_str()
            );
        });
    });
}

fn open_clicked() {
    let location = crate::get_config().last_open_path.clone();
    let path = native_dialog::FileDialog::new().set_location(&location).show_open_single_dir();
    if let Ok(Some(path)) = path {
        if let Some(parent_path) = path.parent() {
            crate::modify_config(|config| {
                config.last_open_path = parent_path.to_path_buf();
            });
        }
        let mut interaction_handler = UiInteractionHandler{};
        let executor = async_executor::LocalExecutor::new();
        let task = executor.spawn(async { world::World::load(path, &mut interaction_handler) });
        let world = futures_lite::future::block_on(executor.run(task)).expect("Failed to load world");
        let mut worlds = world::WORLDS.write().unwrap();
        worlds.push(world);
    }
}

lazy_static! {
    static ref WINDOW_SIZE: MainThreadStore<RefCell<Option<(u32, u32)>>> = MainThreadStore::new(RefCell::new(None));
}

fn move_cursor_to_middle() -> Result<(), winit::error::ExternalError> {
    let gl_window = {
        renderer::get_display().gl_window()
    };
    let window = gl_window.window();
    let window_size = {
        *WINDOW_SIZE.borrow_mut().get_or_insert_with(|| {
            let size = window.inner_size();
            (size.width, size.height)
        })
    };
    window.set_cursor_position(dpi::PhysicalPosition::new(window_size.0 as f32 * 0.5, window_size.1 as f32 * 0.5))
}

pub fn handle_event(ui_state: &mut UiState, event: &event::WindowEvent) {
    match event {
        event::WindowEvent::MouseInput {
            state: event::ElementState::Pressed,
            button: event::MouseButton::Left,
            ..
        } => {
            if !ui_state.key_states.mouse_grabbed {
                ui_state.key_states.mouse_grabbed = true;
                if renderer::get_display().gl_window().window().set_cursor_grab(CursorGrabMode::Locked).is_ok() {
                    renderer::get_display().gl_window().window().set_cursor_visible(false);
                    let _ = move_cursor_to_middle(); // ignore errors
                }
            }
        }
        event::WindowEvent::Focused(false) => {
            ui_state.key_states.mouse_grabbed = false;
            if renderer::get_display().gl_window().window().set_cursor_grab(CursorGrabMode::None).is_ok() {
                renderer::get_display().gl_window().window().set_cursor_visible(true);
            }
        }
        event::WindowEvent::Resized(_) => {
            *WINDOW_SIZE.borrow_mut() = None;
        }
        _ => {}
    }
}

pub fn handle_device_event(ui_state: &mut UiState, event: &event::DeviceEvent) {
    match event {
        event::DeviceEvent::MouseMotion { delta: (x, y) } => {
            if ui_state.key_states.mouse_grabbed {
                ui_state.key_states.mouse_dx += *x;
                ui_state.key_states.mouse_dy += *y;
                if move_cursor_to_middle().is_err() {
                    // unsupported on this platform
                    ui_state.key_states.mouse_dx = 0.0;
                    ui_state.key_states.mouse_dy = 0.0;
                }
            }
        }
        event::DeviceEvent::Key(
            event::KeyboardInput {
                state: event::ElementState::Pressed,
                virtual_keycode: Some(key),
                ..
            }
        ) => {
            if ui_state.key_states.mouse_grabbed && matches!(key, event::VirtualKeyCode::Escape) {
                ui_state.key_states.mouse_grabbed = false;
                if renderer::get_display().gl_window().window().set_cursor_grab(CursorGrabMode::None).is_ok() {
                    renderer::get_display().gl_window().window().set_cursor_visible(true);
                }
            }
        }
        _ => {}
    }
}

pub fn tick(ui_state: &mut UiState, egui_ctx: &egui::Context) {
    if ui_state.key_states.mouse_grabbed {
        handle_camera(ui_state, egui_ctx);
    }
}

fn handle_camera(ui_state: &mut UiState, egui_ctx: &egui::Context) {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut yaw = 0.0;
    let mut pitch = 0.0;

    let movement_speed = 1.0;
    let rotation_speed = 3.0;
    let mouse_sensitivity = 0.05;

    if egui_ctx.input().key_down(egui::Key::A) {
        x -= movement_speed;
    }
    if egui_ctx.input().key_down(egui::Key::D) {
        x += movement_speed;
    }
    if egui_ctx.input().modifiers.shift {
        y -= movement_speed;
    }
    if egui_ctx.input().key_down(egui::Key::Space) {
        y += movement_speed;
    }
    if egui_ctx.input().key_down(egui::Key::W) {
        z -= movement_speed;
    }
    if egui_ctx.input().key_down(egui::Key::S) {
        z += movement_speed;
    }
    if egui_ctx.input().key_down(egui::Key::ArrowRight) {
        yaw -= rotation_speed;
    }
    if egui_ctx.input().key_down(egui::Key::ArrowLeft) {
        yaw += rotation_speed;
    }
    if egui_ctx.input().key_down(egui::Key::ArrowDown) {
        pitch -= rotation_speed;
    }
    if egui_ctx.input().key_down(egui::Key::ArrowUp) {
        pitch += rotation_speed;
    }
    yaw -= ui_state.key_states.mouse_dx as f32 * mouse_sensitivity;
    pitch -= ui_state.key_states.mouse_dy as f32 * mouse_sensitivity;
    ui_state.key_states.mouse_dx = 0.0;
    ui_state.key_states.mouse_dy = 0.0;
    if x != 0.0 || y != 0.0 || z != 0.0 || yaw != 0.0 || pitch != 0.0 {
        let mut worlds = world::WORLDS.write().unwrap();
        if let Some(world) = worlds.last_mut() {
            let mut camera = world.camera.write().unwrap();
            let xz = DMat2::from_angle(-(camera.yaw as f64).to_radians()).mul_vec2(DVec2::new(x, z));
            camera.move_camera(xz.x, y, xz.y, yaw, pitch);
        }
    }
}

struct UiInteractionHandler;

// TODO: make this an actual UI handler
impl minecraft::DownloadInteractionHandler for UiInteractionHandler {
    fn show_download_prompt(&mut self, mc_version: &str) -> bool {
        info!("Downloading {}", mc_version);
        true
    }

    fn on_start_download(&mut self) {
        info!("Download started");
    }

    fn on_finish_download(&mut self) {
        info!("Download finished");
    }
}
