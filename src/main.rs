#![feature(can_vector)]
#![feature(derive_default_enum)]
#![feature(downcast_unchecked)]
#![feature(exact_size_is_empty)]
#![feature(explicit_generic_args_with_impl_trait)]
#![feature(int_log)]
#![feature(int_roundings)]
#![feature(map_try_insert)]
#![feature(once_cell)]
#![feature(option_result_contains)]
#![feature(read_buf)]
#![feature(try_blocks)]
#![feature(try_find)]

// These two lines are just to trick intellij-rust into highlighting functions with profiling::function.
// https://github.com/intellij-rust/intellij-rust/issues/8504#issuecomment-1028447198
#![feature(register_tool)]
#![register_tool(profiling)]

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
mod debug;
mod convert;

use std::path::PathBuf;
use std::sync::{Mutex, RwLock, RwLockReadGuard};
use std::{thread, time};
use std::collections::vec_deque::VecDeque;
use std::lazy::SyncOnceCell;
use egui::{FontData, FontDefinitions, FontFamily};
use flexi_logger::Logger;
use glium::{glutin::{dpi, event, event_loop, window, ContextBuilder}, Display};
use glium::Surface;
use image::GenericImageView;
use lazy_static::lazy_static;
use log::{info, warn};
use winit::window::Icon;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use crate::fname::CommonFNames;
use crate::ui::UiState;
use crate::util::ResourceLocation;
use crate::world::{workers, World};

#[allow(clippy::all)]
mod gl {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

#[derive(Debug, StructOpt)]
#[structopt(name = "quickedit", about = "A Minecraft world editor")]
pub struct CmdLineArgs {
    #[structopt(short = "w", long = "world")]
    world: Option<PathBuf>,

    #[cfg(feature = "debug-chunk-deserialization")]
    #[structopt(long = "debug-chunk-deserialization")]
    pub debug_chunk_deserialization: Option<String>,
}

const FONT_DATA: &[u8] = include_bytes!("../res/MinecraftRegular-Bmg3.ttf");
const ICON_DATA: &[u8] = include_bytes!("../res/icon_32x32.png");

const MSPT: u64 = 1000 / 60;

#[cfg(feature = "profile-with-tracy")]
#[global_allocator]
static GLOBAL: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 100);

thread_local! {
    static ICON: Icon = {
        let image = image::load_from_memory_with_format(ICON_DATA, image::ImageFormat::Png).unwrap();
        Icon::from_rgba(image.as_rgba8().unwrap().as_raw().clone(), image.width(), image.height()).unwrap()
    };
}

lazy_static! {
    static ref QUEUED_TASKS: Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>> = Mutex::new(Vec::new());
    static ref NON_URGENT_QUEUED_TASKS: Mutex<VecDeque<Box<dyn FnOnce() + Send + 'static>>> = Mutex::new(VecDeque::new());
}

pub fn add_queued_task(task: impl FnOnce() + Send + 'static) {
    QUEUED_TASKS.lock().unwrap().push(Box::new(task));
}

pub fn add_non_urgent_queued_task(task: impl FnOnce() + Send + 'static) {
    NON_URGENT_QUEUED_TASKS.lock().unwrap().push_back(Box::new(task));
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

fn create_display(event_loop: &event_loop::EventLoop<()>) -> glium::Display {
    let window_builder = window::WindowBuilder::new()
        .with_resizable(true)
        .with_title("QuickEdit")
        .with_window_icon(Some(ICON.with(|i| i.clone())))
        .with_inner_size(dpi::LogicalSize::new(get_config().window_width as f64, get_config().window_height as f64));

    let context_builder = ContextBuilder::new()
        .with_depth_buffer(24)
        .with_srgb(true)
        .with_stencil_buffer(0)
        .with_vsync(true);

    Display::new(window_builder, context_builder, event_loop).unwrap()
}

#[cfg(feature = "log-files")]
fn make_logger() -> Logger {
    use flexi_logger::{Age, Cleanup, Criterion, Duplicate, FileSpec, FlexiLoggerError, Naming, WriteMode};
    Logger::try_with_str("info").unwrap()
        .log_to_file(FileSpec::default().directory("logs"))
        .write_mode(WriteMode::BufferAndFlush)
        .duplicate_to_stderr(Duplicate::All)
        .rotate(Criterion::Age(Age::Day), Naming::Timestamps, Cleanup::KeepLogAndCompressedFiles(1, 20))
}

#[cfg(not(feature = "log-files"))]
fn make_logger() -> Logger {
    Logger::try_with_str("info").unwrap().log_to_stderr()
}

fn main() {
    let _logger = match make_logger().start() {
        Ok(logger) => logger,
        Err(err) => {
            warn!("Failed to initialize logger: {}", err);
            std::process::exit(1);
        }
    };
    log_panics::init();

    profiling::register_thread!("main");

    CMD_LINE_ARGS.set(CmdLineArgs::from_args()).unwrap();

    let event_loop = event_loop::EventLoop::with_user_event();
    let display = create_display(&event_loop);

    let mut egui_glium = egui_glium::EguiGlium::new(&display);

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("MinecraftRegular".to_owned(), FontData::from_static(FONT_DATA));
    fonts.fonts_for_family.get_mut(&FontFamily::Proportional).unwrap().insert(0, "MinecraftRegular".to_owned());
    fonts.fonts_for_family.get_mut(&FontFamily::Monospace).unwrap().push("MinecraftRegular".to_owned());
    egui_glium.egui_ctx.set_fonts(fonts);

    let _main_thread_data = unsafe {
        MAIN_THREAD = Some(thread::current().id());
        util::MainThreadData::new()
    };

    unsafe {
        renderer::set_display(&display);
    }
    gl::load_with(|s| display.gl_window().get_proc_address(s) as *const _);

    if let Some(world_folder) = get_cmd_line_args().world.as_ref().cloned().or_else(|| get_config().auto_open_world.clone()) {
        struct CmdLineInteractionHandler;
        impl minecraft::DownloadInteractionHandler for CmdLineInteractionHandler {
            fn show_download_prompt(&mut self, mc_version: &str) -> bool {
                info!("Downloading Minecraft {}", mc_version);
                true
            }

            fn on_start_download(&mut self) {
                info!("Starting download...");
            }

            fn on_finish_download(&mut self) {
                info!("Finished download");
            }
        }
        let mut interaction_handler = CmdLineInteractionHandler{};
        let world = match world::World::load(world_folder, &mut interaction_handler) {
            Ok(world) => world,
            Err(err) => {
                warn!("Failed to load world: {}", err);
                return;
            }
        };

        let mut worlds = world::WORLDS.write().unwrap();
        worlds.push(world);
    }

    let mut ui_state = UiState::default();

    unsafe {
        renderer::clear_display();
    }

    event_loop.run(move |event, _, control_flow| {
        unsafe {
            renderer::set_display(&display);
        }

        let mut redraw = || {
            let start_time = std::time::Instant::now();

            {
                profiling::scope!("queued_tasks");
                let mut queued_tasks = QUEUED_TASKS.lock().unwrap();
                for task in queued_tasks.drain(..) {
                    task();
                }
            }

            let mut quit = false;

            let (_, shapes) = egui_glium.run(&display, |egui_ctx| {
                ui::run_ui(&ui_state, egui_ctx, &mut quit);
                ui::tick(&mut ui_state, egui_ctx);
            });

            workers::tick();

            {
                profiling::scope!("non_urgent_queued_tasks");
                let mut non_urgent_queued_tasks = NON_URGENT_QUEUED_TASKS.lock().unwrap();
                while !non_urgent_queued_tasks.is_empty() && time::Instant::now() - start_time < time::Duration::from_millis(MSPT) {
                    let task = non_urgent_queued_tasks.pop_front().unwrap();
                    task();
                }
            }

            *control_flow = if quit {
                event_loop::ControlFlow::Exit
            } else {
                let next_time = start_time + std::time::Duration::from_millis(MSPT);
                event_loop::ControlFlow::WaitUntil(next_time)
            };

            {
                let mut target = display.draw();
                target.clear_color_and_depth((0.0, 0.0, 0.0, 1.0), 1.0);
                {
                    let worlds = world::WORLDS.read().unwrap();
                    if let Some(world) = worlds.last() {
                        world.renderer.render_world(&*world, &mut target);
                    }
                }
                egui_glium.paint(&display, &mut target, shapes);
                target.finish().unwrap();
            }
        };

        match event {
            event::Event::RedrawEventsCleared if cfg!(windows) => redraw(),
            event::Event::RedrawRequested(_) if !cfg!(windows) => redraw(),
            event::Event::NewEvents(start_cause) => {
                if matches!(start_cause, event::StartCause::Init | event::StartCause::ResumeTimeReached { .. }) {
                    display.gl_window().window().request_redraw();
                }
            }
            event::Event::WindowEvent { event, .. } => {
                if matches!(event, event::WindowEvent::CloseRequested | event::WindowEvent::Destroyed) {
                    *control_flow = event_loop::ControlFlow::Exit;
                }

                if !egui_glium.on_event(&event) {
                    ui::handle_event(&mut ui_state, &event);
                }

                display.gl_window().window().request_redraw();
            }
            event::Event::DeviceEvent { event, .. } => {
                ui::handle_device_event(&mut ui_state, &event);

                display.gl_window().window().request_redraw();
            }
            _ => {}
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
                    warn!("Failed to load config: {}", err);
                    Config::default()
                })
            },
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    warn!("Failed to open config file: {}", e);
                }
                Config::default()
            },
        })
    };
}

static CMD_LINE_ARGS: SyncOnceCell<CmdLineArgs> = SyncOnceCell::new();

pub fn get_cmd_line_args() -> &'static CmdLineArgs {
    CMD_LINE_ARGS.get().unwrap()
}

pub fn get_config() -> RwLockReadGuard<'static, Config> {
    CONFIG.read().unwrap()
}

pub fn modify_config(f: impl FnOnce(&mut Config)) {
    let mut config = CONFIG.write().unwrap();
    f(&mut *config);
    let json = serde_json::to_string_pretty(&*config).unwrap();
    if let Err(e) = std::fs::write("quickedit_config.json", json) {
        warn!("Failed to save config: {}", e);
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub window_width: u32,
    pub window_height: u32,
    pub last_open_path: PathBuf,
    pub auto_open_world: Option<PathBuf>,
    render_distance: u32,
    unloaded_render_distance: u32,
}

impl Config {
    pub fn render_distance(&self) -> u32 {
        self.render_distance.clamp(2, 64)
    }

    pub fn unloaded_render_distance(&self) -> u32 {
        self.unloaded_render_distance.clamp(2, 64)
    }
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
            render_distance: 16,
            unloaded_render_distance: 32,
        }
    }
}
