use std::path::PathBuf;
use std::sync::Arc;
use conrod_core::{Labelable, Positionable, Sizeable, widget, Widget, widget_ids};
use crate::{CommonFNames, minecraft, world};

widget_ids!(pub struct Ids {
    open_button
});

pub fn init_ui(ui: &mut conrod_core::Ui) -> Ids {
    return Ids::new(ui.widget_id_generator());
}

pub fn set_ui(ids: &Ids, ui: &mut conrod_core::UiCell) {
    if widget::Button::new()
        .label("Open")
        .top_left_of(ui.window)
        .w_h(200.0, 50.0)
        .set(ids.open_button, ui)
        .was_clicked() {
        open_clicked();
    }
}

fn open_clicked() {
    //let path = native_dialog::FileDialog::new().show_open_single_dir();
    let path = PathBuf::from("/home/joe/.local/share/multimc/instances/1.18.1/.minecraft/saves/New World/");
    /*if let Ok(Some(path)) = path*/ {
        let mut interaction_handler = UiInteractionHandler{};
        let executor = async_executor::LocalExecutor::new();
        let task = executor.spawn(async { world::World::new(path, &mut interaction_handler) });
        let world = futures_lite::future::block_on(executor.run(task)).expect("Failed to load world");
        {
            let mut worlds = world::WORLDS.write().unwrap();
            worlds.push(Arc::new(world));
        }
        let worlds = world::WORLDS.read().unwrap();
        worlds.last().unwrap().get_dimension(CommonFNames.OVERWORLD.clone()).unwrap().load_chunk(worlds.last().unwrap(), world::ChunkPos::new(0, 0));
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
