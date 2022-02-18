mod bakery;
pub mod draw;
mod liquid;
mod storage;
pub mod worker;

pub use crate::renderer::draw::*;
pub use crate::renderer::bakery::{BakedModel, Transparency};

struct DisplayHolder {
    display: *const glium::Display,
    #[cfg(debug_assertions)]
    thread: std::thread::ThreadId,
}

static mut DISPLAY: Option<DisplayHolder> = None;

pub unsafe fn set_display(display: &glium::Display) {
    DISPLAY = Some(DisplayHolder {
        display,
        #[cfg(debug_assertions)]
        thread: std::thread::current().id(),
    });
}

pub unsafe fn clear_display() {
    DISPLAY = None;
}

pub fn get_display() -> &'static glium::Display {
    unsafe {
        let holder = DISPLAY.as_ref().unwrap();
        #[cfg(debug_assertions)]
        assert_eq!(holder.thread, std::thread::current().id());
        &*holder.display
    }
}