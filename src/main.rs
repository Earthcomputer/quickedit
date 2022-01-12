#![feature(int_log)]
#![feature(int_roundings)]
#![feature(option_result_contains)]
#![feature(can_vector)]
#![feature(read_buf)]
#![feature(try_find)]

#![allow(dead_code)]
#![allow(clippy::needless_return)]

mod world;
mod util;
mod fname;
mod minecraft;
mod ui;
mod world_renderer;
mod resources;

extern crate conrod_core;
extern crate conrod_glium;
extern crate conrod_winit;
extern crate glium;
extern crate native_dialog;

use std::path::PathBuf;
use std::sync::Arc;
use conrod_core::text;
use glium::{glutin::{dpi, event, event_loop, window, ContextBuilder}, Display};
use glium::Surface;
use image::GenericImageView;
use winit::window::Icon;
use structopt::StructOpt;
use crate::fname::CommonFNames;
use crate::util::ResourceLocation;

#[allow(clippy::all)]
mod gl {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

#[derive(StructOpt)]
#[structopt(name = "mcedit_rs", about = "A Minecraft world editor")]
struct CmdLineArgs {
    #[structopt(short = "w", long = "world")]
    world: Option<PathBuf>,
}

const WIDTH: f64 = 1920.0;
const HEIGHT: f64 = 1080.0;

const FONT_DATA: &[u8] = include_bytes!("../res/MinecraftRegular-Bmg3.ttf");
const ICON_DATA: &[u8] = include_bytes!("../res/icon_32x32.png");

thread_local! {
    static ICON: Icon = {
        let image = image::load_from_memory_with_format(ICON_DATA, image::ImageFormat::Png).unwrap();
        Icon::from_rgba(image.as_rgba8().unwrap().as_raw().clone(), image.width(), image.height()).unwrap()
    };
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

fn run_loop<F>(display: Display, event_loop: event_loop::EventLoop<()>, mut callback: F) -> !
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
                } else {
                    next_update = None;
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
        .with_title("MCEdit RS")
        .with_window_icon(Some(ICON.with(|i| i.clone())))
        .with_inner_size(dpi::LogicalSize::new(WIDTH, HEIGHT));
    let cb = ContextBuilder::new();
    let display = Display::new(wb, cb, &event_loop).unwrap();
    unsafe {
        world_renderer::set_display(&display);
    }
    gl::load_with(|s| display.gl_window().get_proc_address(s) as *const _);

    if let Some(world_folder) = args.world {
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
        let world = match world::World::new(world_folder, &mut interaction_handler) {
            Ok(world) => world,
            Err(err) => {
                println!("Failed to load world: {}", err);
                return;
            }
        };

        let mut worlds = world::WORLDS.write().unwrap();
        worlds.push(Arc::new(world));
    }

    let mut my_ui = conrod_core::UiBuilder::new([WIDTH, HEIGHT]).build();
    let image_map = conrod_core::image::Map::<glium::texture::Texture2d>::new();
    let mut renderer = conrod_glium::Renderer::new(&display).unwrap();

    my_ui.fonts.insert(text::Font::from_bytes(FONT_DATA).unwrap());

    let ids = ui::init_ui(&mut my_ui);

    unsafe {
        world_renderer::clear_display();
    }

    run_loop(display, event_loop, move |request, display| {
        unsafe {
            world_renderer::set_display(display);
        }
        match request {
            Request::Event {
                event,
                should_update_ui,
                should_exit,
            } => {
                if let Some(event) = convert_event(event, display.gl_window().window()) {
                    my_ui.handle_event(event);
                    *should_update_ui = true;
                }

                if let event::Event::WindowEvent { event: event::WindowEvent::CloseRequested, .. } = event {
                    *should_exit = true
                }
            },
            Request::SetUi { needs_redraw } => {
                let my_ui = &mut my_ui.set_widgets();
                ui::set_ui(&ids, my_ui);
                *needs_redraw = my_ui.has_changed();
            },
            Request::Redraw => {
                let primitives = my_ui.draw();
                renderer.fill(display, primitives, &image_map);
                let mut target = display.draw();
                target.clear_color(0.0, 0.0, 0.0, 1.0);

                {
                    let worlds = world::WORLDS.read().unwrap();
                    if let Some(world) = worlds.last() {
                        world.renderer.render_world(world, &mut target);
                    }
                }

                renderer.draw(display, &mut target, &image_map).unwrap();
                target.finish().unwrap();
            }
        }
        unsafe {
            world_renderer::clear_display();
        }
    });
}
