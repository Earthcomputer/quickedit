use std::fs::File;
use std::{fs, io};
use std::path::{Path, PathBuf};
use ahash::AHashSet;
use path_slash::{PathBufExt, PathExt};
use zip::result::ZipError;
use zip::ZipArchive;
use crate::util;

#[profiling::function]
pub(super) fn get_resource_pack(path: &Path) -> io::Result<Box<dyn ResourcePack>> {
    if util::is_dir(path) {
        Ok(Box::new(DirectoryResourcePack::new(path.to_path_buf())))
    } else {
        let file = File::open(path)?;
        Ok(Box::new(ZipResourcePack::new(file)?))
    }
}

pub(super) trait ResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>>;
    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String>;
}

struct ZipResourcePack {
    zip: ZipArchive<File>,
    dirs: Vec<String>,
}

impl ZipResourcePack {
    fn new(file: File) -> io::Result<Self> {
        let zip = ZipArchive::new(file)?;
        let mut dirs = AHashSet::new();
        for filename in zip.file_names() {
            let parts: Vec<_> = filename.split('/').collect();
            for i in 0..parts.len() - 1 {
                dirs.insert(parts[..=i].join("/") + "/");
            }
        }
        Ok(Self { zip, dirs: dirs.into_iter().collect() })
    }
}

impl ResourcePack for ZipResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>> {
        let file = match self.zip.by_name(path) {
            Ok(file) => file,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(ZipError::Io(e)) => return Err(e),
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
        };
        Ok(Some(Box::new(file)))
    }

    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String> {
        let mut files = Vec::new();
        for file in self.zip.file_names().chain(self.dirs.iter().map(|d| d.as_str())) {
            if file.starts_with(path) && file[path.len()..].ends_with(suffix) && !file[path.len()..file.len()-suffix.len()].contains('/') {
                files.push(file[path.len()..file.len()-suffix.len()].to_string());
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
        let file = match File::open(self.path.join(PathBuf::from_slash(path))) {
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
                    if let Some(filename) = filename.strip_suffix(suffix) {
                        files.push(filename.to_string());
                    }
                }
            }
        }
        files
    }
}

#[profiling::function]
pub(super) fn get_resource<'a>(resource_packs: &'a mut [Box<dyn ResourcePack>], path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>> {
    for resource_pack in resource_packs {
        match resource_pack.get_reader(path) {
            Ok(Some(reader)) => return Ok(Some(reader)),
            Ok(None) => continue,
            Err(e) => return Err(e)
        }
    }
    Ok(None)
}
