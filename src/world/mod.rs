use std::sync::RwLock;
use lazy_static::lazy_static;
pub use structs::*;
use workers::WorldRef;

mod io;
mod palette;
mod structs;
mod versioned_io;
pub mod workers;

lazy_static! {
    pub static ref WORLDS: RwLock<Vec<WorldRef>> = RwLock::new(Vec::new());
}