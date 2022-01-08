use std::{fs, io};
use std::path::{Path, PathBuf};
use ahash::AHashMap;
use path_slash::{PathBufExt, PathExt};
use crate::{minecraft, ResourceLocation, util};
use serde::Deserialize;

pub struct Resources {
    blockstates: AHashMap<ResourceLocation, BlockstateFile>,
}

trait ResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>>;
    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String>;
}

struct ZipResourcePack {
    zip: zip::ZipArchive<fs::File>,
}

impl ZipResourcePack {
    fn new(file: fs::File) -> io::Result<Self> {
        let zip = zip::ZipArchive::new(file)?;
        Ok(Self { zip })
    }
}

impl ResourcePack for ZipResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>> {
        let file = match self.zip.by_name(path) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(zip::result::ZipError::Io(e)) => return Err(e),
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
        };
        Ok(Some(Box::new(file)))
    }

    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String> {
        let mut files = Vec::new();
        for file in self.zip.file_names() {
            if file.starts_with(path) && file.ends_with(suffix) {
                files.push(file.to_string());
            }
        }
        files
    }
}

struct DirectoryResourcePack {
    path: PathBuf,
}

impl DirectoryResourcePack {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ResourcePack for DirectoryResourcePack {
    fn get_reader(&mut self, path: &str) -> io::Result<Option<Box<dyn io::Read>>> {
        let file = match fs::File::open(self.path.join(PathBuf::from_slash(path))) {
            Ok(file) => file,
            Err(e) => {
                return if e.kind() == io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        };
        Ok(Some(Box::new(file)))
    }

    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String> {
        let mut files = Vec::new();
        let read_dir = match fs::read_dir(self.path.join(path)) {
            Ok(read_dir) => read_dir,
            Err(_) => return files,
        };
        for entry in read_dir.flatten() {
            if let Ok(relpath) = entry.path().strip_prefix(&self.path) {
                if let Some(filename) = relpath.to_slash() {
                    if filename.ends_with(suffix) {
                        files.push(filename.to_string());
                    }
                }
            }
        }
        files
    }
}

impl Resources {
    fn get_resource_pack(path: &Path) -> io::Result<Box<dyn ResourcePack>> {
        if util::is_dir(path) {
            Ok(Box::new(DirectoryResourcePack::new(path.to_path_buf())))
        } else {
            let file = fs::File::open(path)?;
            Ok(Box::new(ZipResourcePack::new(file)?))
        }
    }

    fn load_resource_pack(_mc_version: &str, resource_pack: &mut dyn ResourcePack, resources: &mut Resources) {
        for namespace in resource_pack.get_sub_files("assets/", "") {
            for blockstate_path in resource_pack.get_sub_files(&format!("assets/{}/blockstates/", &namespace), ".json") {
                let mut blockstate_reader = match resource_pack.get_reader(&blockstate_path) {
                    Ok(Some(reader)) => reader,
                    _ => continue
                };
                let block_name = blockstate_path.split('/').last().unwrap().strip_suffix(".json").unwrap();
                let blockstate: BlockstateFile = match serde_json::from_reader(util::ReadDelegate::new(&mut *blockstate_reader)) {
                    Ok(blockstate) => blockstate,
                    Err(_) => continue,
                };
                let _ = resources.blockstates.try_insert(ResourceLocation::new(&namespace, block_name), blockstate);
            }
        }
    }

    pub async fn load(mc_version: &str, resource_packs: &[&PathBuf], interaction_handler: &mut dyn minecraft::DownloadInteractionHandler) -> Option<Resources> {
        let mut resources = Resources {
            blockstates: AHashMap::new(),
        };
        for resource_pack in resource_packs.iter().rev() {
            let mut resource_pack = match Resources::get_resource_pack(resource_pack) {
                Ok(resource_pack) => resource_pack,
                Err(_) => continue // TODO: does this need logging?
            };
            Resources::load_resource_pack(mc_version, &mut *resource_pack, &mut resources);
        }
        let minecraft_jar = match minecraft::get_existing_jar(mc_version) {
            Some(jar) => jar,
            None => {
                if !interaction_handler.show_download_prompt(mc_version).await {
                    return None;
                }
                interaction_handler.on_start_download();
                let result = minecraft::download_jar(mc_version).await.ok()?;
                interaction_handler.on_finish_download();
                result
            },
        };
        Resources::load_resource_pack(mc_version, &mut *Resources::get_resource_pack(&minecraft_jar).ok()?, &mut resources);
        Some(resources)
    }
}

#[derive(Deserialize)]
pub struct BlockstateFile {

}
