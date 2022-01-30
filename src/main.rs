#![feature(can_vector)]
#![feature(derive_default_enum)]
#![feature(downcast_unchecked)]
#![feature(exact_size_is_empty)]
#![feature(explicit_generic_args_with_impl_trait)]
#![feature(int_log)]
#![feature(int_roundings)]
#![feature(option_result_contains)]
#![feature(read_buf)]
#![feature(try_find)]

#![allow(dead_code)]
#![allow(clippy::needless_return)]

mod world;
mod util;
mod fname;
mod minecraft;
mod ui;
mod renderer;
mod resources;
mod geom;
mod blocks;

extern crate conrod_core;
extern crate conrod_glium;
extern crate conrod_winit;
extern crate glium;
extern crate native_dialog;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, RwLock, RwLockReadGuard};
use std::thread;
use conrod_core::text;
use glium::{glutin::{dpi, event, event_loop, window, ContextBuilder}, Display};
use glium::Surface;
use image::GenericImageView;
use lazy_static::lazy_static;
use winit::window::Icon;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use crate::fname::CommonFNames;
use crate::ui::UiState;
use crate::util::ResourceLocation;
use crate::world::World;

#[allow(clippy::all)]
mod gl {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

#[derive(StructOpt)]
#[structopt(name = "quickedit", about = "A Minecraft world editor")]
struct CmdLineArgs {
    #[structopt(short = "w", long = "world")]
    world: Option<PathBuf>,
}

const FONT_DATA: &[u8] = include_bytes!("../res/MinecraftRegular-Bmg3.ttf");
const ICON_DATA: &[u8] = include_bytes!("../res/icon_32x32.png");

thread_local! {
    static ICON: Icon = {
        let image = image::load_from_memory_with_format(ICON_DATA, image::ImageFormat::Png).unwrap();
        Icon::from_rgba(image.as_rgba8().unwrap().as_raw().clone(), image.width(), image.height()).unwrap()
    };
}

lazy_static! {
    static ref QUEUED_TASKS: Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>> = Mutex::new(Vec::new());
}

pub fn add_queued_task(task: impl FnOnce() + Send + 'static) {
    QUEUED_TASKS.lock().unwrap().push(Box::new(task));
}

enum Request<'a, 'b: 'a> {
    Event {
        event: &'a event::Event<'b, ()>,
        should_update_ui: &'a mut bool,
        should_exit: &'a mut bool,
    },
    SetUi {
        needs_redraw: &'a mut bool,
    },
    Redraw,
}

pub static mut MAIN_THREAD: Option<thread::ThreadId> = None;

fn run_loop<F>(display: Display, event_loop: event_loop::EventLoop<()>, _ui_state: Rc<RefCell<UiState>>, mut callback: F) -> !
where
    F: 'static + FnMut(Request, &Display),
{
    let mut next_update = None;
    let mut ui_update_needed = false;
    event_loop.run(move |event, _, control_flow| {
        {
            let mut should_update_ui = false;
            let mut should_exit = false;
            callback(Request::Event {
                event: &event,
                should_update_ui: &mut should_update_ui,
                should_exit: &mut should_exit,
            }, &display);
            ui_update_needed |= should_update_ui;
            if should_exit {
                *control_flow = glium::glutin::event_loop::ControlFlow::Exit;
                return;
            }
        }

        let should_set_ui_on_main_events_cleared = next_update.is_none() && ui_update_needed;
        match (&event, should_set_ui_on_main_events_cleared) {
            (glium::glutin::event::Event::NewEvents(glium::glutin::event::StartCause::Init { .. }), _)
            | (glium::glutin::event::Event::NewEvents(glium::glutin::event::StartCause::ResumeTimeReached { .. }), _)
            | (glium::glutin::event::Event::MainEventsCleared, true) => {
                next_update = Some(std::time::Instant::now() + std::time::Duration::from_millis(16));
                ui_update_needed = false;
                let mut needs_redraw = false;
                callback(Request::SetUi {
                    needs_redraw: &mut needs_redraw,
                }, &display);
                if needs_redraw {
                    display.gl_window().window().request_redraw();
                }
            }
            _ => {}
        }
        if let Some(next_update) = next_update {
            *control_flow = glium::glutin::event_loop::ControlFlow::WaitUntil(next_update);
        } else {
            *control_flow = glium::glutin::event_loop::ControlFlow::Wait;
        }

        if let glium::glutin::event::Event::RedrawRequested(_) = &event {
            callback(Request::Redraw, &display);
        }
    })
}

conrod_winit::v023_conversion_fns!();

fn main() {
    let args: CmdLineArgs = CmdLineArgs::from_args();

    let event_loop = event_loop::EventLoop::new();
    let wb = window::WindowBuilder::new()
        .with_title("QuickEdit")
        .with_window_icon(Some(ICON.with(|i| i.clone())))
        .with_inner_size(dpi::LogicalSize::new(get_config().window_width as f64, get_config().window_height as f64));
    let cb = ContextBuilder::new().with_depth_buffer(24);
    let display = Display::new(wb, cb, &event_loop).unwrap();

    let _main_thread_data = unsafe {
        MAIN_THREAD = Some(thread::current().id());
        util::MainThreadData::new()
    };

    unsafe {
        renderer::set_display(&display);
    }
    gl::load_with(|s| display.gl_window().get_proc_address(s) as *const _);

    if let Some(world_folder) = args.world.or_else(|| get_config().auto_open_world.clone()) {
        struct CmdLineInteractionHandler;
        impl minecraft::DownloadInteractionHandler for CmdLineInteractionHandler {
            fn show_download_prompt(&mut self, mc_version: &str) -> bool {
                println!("Downloading Minecraft {}", mc_version);
                true
            }

            fn on_start_download(&mut self) {
                println!("Starting download...");
            }

            fn on_finish_download(&mut self) {
                println!("Finished download");
            }
        }
        let mut interaction_handler = CmdLineInteractionHandler{};
        let world = match world::World::load(world_folder, &mut interaction_handler) {
            Ok(world) => world,
            Err(err) => {
                println!("Failed to load world: {}", err);
                return;
            }
        };

        let mut worlds = world::WORLDS.write().unwrap();
        worlds.push(world);
    }

    let mut my_ui = conrod_core::UiBuilder::new([get_config().window_width as f64, get_config().window_height as f64]).build();
    let image_map = conrod_core::image::Map::<glium::texture::Texture2d>::new();
    let mut renderer = conrod_glium::Renderer::new(&display).unwrap();

    my_ui.fonts.insert(text::Font::from_bytes(FONT_DATA).unwrap());

    let ui_state = Rc::new(RefCell::new(ui::init_ui(&mut my_ui)));

    unsafe {
        renderer::clear_display();
    }

    run_loop(display, event_loop, ui_state.clone(), move |request, display| {
        unsafe {
            renderer::set_display(display);
        }
        match request {
            Request::Event {
                event,
                should_update_ui,
                should_exit,
            } => {
                if let Some(event) = convert_event(event, display.gl_window().window()) {
                    my_ui.handle_event(event);
                    for event in my_ui.global_input().events() {
                        ui::handle_event(&mut *ui_state.borrow_mut(), &my_ui, event);
                    }
                    *should_update_ui = true;
                }

                if let event::Event::WindowEvent { event: event::WindowEvent::CloseRequested, .. } = event {
                    *should_exit = true
                }
            },
            Request::SetUi { needs_redraw } => {
                {
                    let mut queued_tasks = QUEUED_TASKS.lock().unwrap();
                    for task in queued_tasks.drain(..) {
                        task();
                    }
                }
                let my_ui = &mut my_ui.set_widgets();
                ui::set_ui(&*(*ui_state).borrow(), my_ui);
                ui::tick(&mut *(*ui_state).borrow_mut());
                World::tick();
                *needs_redraw = my_ui.has_changed() || {
                    let worlds = world::WORLDS.read().unwrap();
                    worlds.iter().any(|world| world.renderer.has_changed())
                };
            },
            Request::Redraw => {
                let primitives = my_ui.draw();
                renderer.fill(display, primitives, &image_map);
                let mut target = display.draw();
                target.clear_color_and_depth((0.0, 0.0, 0.0, 1.0), 1.0);

                {
                    let worlds = world::WORLDS.read().unwrap();
                    if let Some(world) = worlds.last() {
                        world.renderer.render_world(&*world, &mut target);
                    }
                }

                renderer.draw(display, &mut target, &image_map).unwrap();
                target.finish().unwrap();
            }
        }
        unsafe {
            renderer::clear_display();
        }
    });
}

lazy_static! {
    static ref CONFIG: RwLock<Config> = {
        RwLock::new(match std::fs::File::open("quickedit_config.json") {
            Ok(file) => {
                serde_json::from_reader(file).unwrap_or_else(|err| {
                    eprintln!("Failed to load config: {}", err);
                    Config::default()
                })
            },
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("Failed to open config file: {}", e);
                }
                Config::default()
            },
        })
    };
}

pub fn get_config() -> RwLockReadGuard<'static, Config> {
    CONFIG.read().unwrap()
}

pub fn modify_config(f: impl FnOnce(&mut Config)) {
    let mut config = CONFIG.write().unwrap();
    f(&mut *config);
    let json = serde_json::to_string_pretty(&*config).unwrap();
    if let Err(e) = std::fs::write("quickedit_config.json", json) {
        eprintln!("Failed to save config: {}", e);
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub window_width: u32,
    pub window_height: u32,
    pub last_open_path: PathBuf,
    pub auto_open_world: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            window_width: 1920,
            window_height: 1080,
            last_open_path: minecraft::get_dot_minecraft()
                .map(|p| p.join("saves"))
                .filter(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from(".")),
            auto_open_world: None,
        }
    }
}
