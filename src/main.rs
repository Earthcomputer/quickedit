#![feature(int_log)]
#![feature(int_roundings)]
#![feature(option_result_contains)]
#![feature(can_vector)]
#![feature(read_buf)]

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

use conrod_core::text;
use glium::{
    glutin::{dpi, event, event_loop, window, ContextBuilder},
    Display,
    index,
    uniforms
};
use glium::{implement_vertex, Surface};
use image::GenericImageView;
use winit::window::Icon;
use crate::fname::CommonFNames;
use crate::util::ResourceLocation;

#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 2],
}
implement_vertex!(Vertex, position);

const WIDTH: f64 = 1920.0;
const HEIGHT: f64 = 1080.0;

const MAIN_VERT_SHADER: &str = include_str!("../res/main.vsh");
const MAIN_FRAG_SHADER: &str = include_str!("../res/main.fsh");

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
    let event_loop = event_loop::EventLoop::new();
    let wb = window::WindowBuilder::new()
        .with_title("MCEdit RS")
        .with_window_icon(Some(ICON.with(|i| i.clone())))
        .with_inner_size(dpi::LogicalSize::new(WIDTH, HEIGHT));
    let cb = ContextBuilder::new();
    let display = Display::new(wb, cb, &event_loop).unwrap();

    let v1 = Vertex { position: [-0.5, -0.5] };
    let v2 = Vertex { position: [0.0, 0.5] };
    let v3 = Vertex { position: [0.5, -0.25] };
    let shape = vec![v1, v2, v3];

    let vertex_buffer = glium::VertexBuffer::new(&display, &shape).unwrap();
    let indices = index::NoIndices(index::PrimitiveType::TrianglesList);

    let program = glium::Program::from_source(&display, MAIN_VERT_SHADER, MAIN_FRAG_SHADER, None).unwrap();

    let mut my_ui = conrod_core::UiBuilder::new([WIDTH, HEIGHT]).build();
    let image_map = conrod_core::image::Map::<glium::texture::Texture2d>::new();
    let mut renderer = conrod_glium::Renderer::new(&display).unwrap();

    my_ui.fonts.insert(text::Font::from_bytes(FONT_DATA).unwrap());

    let ids = ui::init_ui(&mut my_ui);

    run_loop(display, event_loop, move |request, display| {
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
                target.draw(&vertex_buffer, &indices, &program, &uniforms::EmptyUniforms, &Default::default()).unwrap();
                renderer.draw(display, &mut target, &image_map).unwrap();
                target.finish().unwrap();
            }
        }
    });
}
