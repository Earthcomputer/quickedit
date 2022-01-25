use std::{fs, io, sync};
use std::any::Any;
use std::io::{BufRead, Write, Cursor};
use std::path::PathBuf;
use ahash::AHashMap;
use chrono::TimeZone;
use lazy_static::lazy_static;
use sha1::{Sha1, Digest};
use crate::util::{FastDashMap, make_fast_dash_map};
use serde::Deserialize;
use sha1::digest::generic_array::functional::FunctionalSequence;
use crate::fname;
use crate::fname::FName;

// ===== Getting the Minecraft jar ===== //

#[cfg(target_os = "windows")]
fn get_dot_minecraft() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let path = PathBuf::from(appdata).join(".minecraft");
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(home) = home::home_dir() {
        let path = home.join(".minecraft");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn get_dot_minecraft() -> Option<PathBuf> {
    if let Some(home) = home::home_dir() {
        let path = home.join("Library").join("Application Support").join("minecraft");
        if path.exists() {
            return Some(path);
        }
        let path = home.join(".minecraft");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn get_dot_minecraft() -> Option<PathBuf> {
    if let Some(home) = home::home_dir() {
        let path = home.join(".minecraft");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn get_launcher_minecraft_jar(version: &str) -> Option<PathBuf> {
    let dot_minecraft = get_dot_minecraft()?;
    let path = dot_minecraft.join("versions").join(version).join(format!("{}.jar", version));
    if path.exists() {
        return Some(path);
    }
    None
}

fn find_existing_downloaded_jar(version: &str) -> Option<PathBuf> {
    let path = get_minecraft_cache().join(format!("{}.jar", version));
    if path.exists() {
        return Some(path);
    }
    None
}

lazy_static! {
    static ref DOWNLOAD_CACHE: FastDashMap<String, Box<dyn Any + Sync + Send>> = make_fast_dash_map();
}

const VERSION_MANIFEST_FILE: &str = "version_manifest.json";
const VERSION_MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest.json";

fn download_if_changed<'a, T, U: 'a + ?Sized>(filename: &str, url: &U, force: bool) -> io::Result<T>
where
    T: serde::de::DeserializeOwned + Clone + Sync + Send + Any,
    U: AsRef<str>,
{
    if !force {
        if let Some(data) = DOWNLOAD_CACHE.get(filename) {
            unsafe {
                return Ok(data.downcast_ref_unchecked::<T>().clone());
            }
        }
    }

    let minecraft_cache = get_minecraft_cache();
    let path = minecraft_cache.join(filename);
    let mut request = attohttpc::get(url);
    let etag_file = path.with_extension("etag");
    if !force {
        if let Ok(etag) = fs::read_to_string(&etag_file) {
            request = request.header(attohttpc::header::IF_NONE_MATCH, etag);
        }
    }
    let response = request.send().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let status = response.status();
    if status == attohttpc::StatusCode::OK {
        let headers = response.headers().clone();
        let text = response.text().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(&path, text.clone())?;
        if let Some(etag) = headers.get(attohttpc::header::ETAG).and_then(|h| h.to_str().ok()) {
            fs::write(&etag_file, etag)?;
        }
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    } else {
        let result: io::Result<T> = fs::File::open(path)
            .and_then(|f| serde_json::from_reader(f).map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
        match result {
            Ok(result) => {
                DOWNLOAD_CACHE.insert(filename.to_string(), Box::new(result.clone()));
                Ok(result)
            }
            Err(e) => {
                if !force && status == attohttpc::StatusCode::NOT_MODIFIED {
                    download_if_changed(filename, url, true)
                } else {
                    Err(e)
                }
            }
        }
    }
}

pub fn download_jar(version: &str) -> Result<PathBuf, io::Error> {
    fs::create_dir_all(get_minecraft_cache())?;

    let version_manifest: VersionManifest = download_if_changed(
        VERSION_MANIFEST_FILE,
        VERSION_MANIFEST_URL,
        false,
    )?;

    let version_json_url = version_manifest.versions.iter()
        .find(|version_data| version_data.id == version)
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Could not find version"))?
        .url.clone();
    let version_json: VersionJson = download_if_changed(
        format!("{}.json", version).as_str(),
        &version_json_url,
        false,
    )?;

    let jar_path = get_minecraft_cache().join(format!("{}.jar", version));
    let actual_sha1 = match fs::File::open(jar_path.clone()) {
        Ok(mut existing_jar) => {
            let mut sha1 = Sha1::default();
            io::copy(&mut existing_jar, &mut sha1)?;
            Some(sha1.finalize().map(|b| format!("{:02x}", b)).join(""))
        }
        Err(..) => None
    };

    if actual_sha1.contains(&version_json.downloads.client.sha1) {
        return Ok(jar_path);
    }

    let jar_url = version_json.downloads.client.url;
    let response = attohttpc::get(jar_url).send().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    io::copy(&mut Cursor::new(response.bytes().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?), &mut fs::File::create(&jar_path)?)?;
    Ok(jar_path)
}

fn get_minecraft_cache() -> PathBuf {
    PathBuf::from("./.minecraft_cache")
}

pub fn get_existing_jar(version: &str) -> Option<PathBuf> {
    get_launcher_minecraft_jar(version).or_else(|| find_existing_downloaded_jar(version))
}

pub trait DownloadInteractionHandler {
    fn show_download_prompt(&mut self, mc_version: &str) -> bool;
    fn on_start_download(&mut self);
    fn on_finish_download(&mut self);
}

#[derive(Clone, Deserialize)]
struct VersionManifest {
    latest: LatestVersion,
    versions: Vec<Version>,
}

#[derive(Clone, Deserialize)]
struct LatestVersion {
    release: String,
    snapshot: String,
}

#[derive(Clone, Deserialize)]
struct Version {
    id: String,
    #[serde(rename = "type")]
    release_type: String,
    url: String,
    #[serde(rename = "releaseTime")]
    release_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Deserialize)]
struct VersionJson {
    downloads: Downloads
}

#[derive(Clone, Deserialize)]
struct Downloads {
    client: Download,
}

#[derive(Clone, Deserialize)]
struct Download {
    sha1: String,
    size: u64,
    url: String,
}

// ===== Prismarine data ===== //

const PRISMARINE_URL_PREFIX: &str = "https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/";
fn prismarine_url(suffix: &str) -> String {
    format!("{}{}", PRISMARINE_URL_PREFIX, suffix)
}

fn get_prismarine_version_data(mc_version: &str) -> io::Result<PrismarineVersionData> {
    let data_paths: PrismarineData = download_if_changed("dataPaths.json", prismarine_url("dataPaths.json").as_str(), false)?;
    let mc_version = if data_paths.pc.contains_key(mc_version) {
        mc_version.to_string()
    } else {
        let mut version_manifest: VersionManifest = download_if_changed(
            VERSION_MANIFEST_FILE,
            VERSION_MANIFEST_URL,
            false,
        )?;
        version_manifest.versions.sort_by_key(|version| version.release_time);
        let index = version_manifest.versions.iter()
            .position(|version| version.id == mc_version)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Could not find version"))?;
        let version = version_manifest.versions.iter().skip(index).find(|version| data_paths.pc.contains_key(version.id.as_str()))
            .or_else(|| version_manifest.versions.iter().take(index).rev().find(|version| data_paths.pc.contains_key(version.id.as_str())))
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Could not find version"))?;
        version.id.clone()
    };
    Ok(data_paths.pc.get(mc_version.as_str()).unwrap().clone())
}

pub fn get_biome_data(mc_version: &str) -> io::Result<AHashMap<FName, BiomeData>> {
    let version_data = get_prismarine_version_data(mc_version)?;
    let biome_data_location = version_data.biomes.ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No biome data"))?;
    let biome_data_url = format!("{}/biomes.json", prismarine_url(&biome_data_location));
    let biome_data: Vec<BiomeData> = download_if_changed(format!("biomes_{}.json", biome_data_location.replace('/', "_")).as_str(), biome_data_url.as_str(), false)?;
    let mut biome_data_map = AHashMap::new();
    for data in biome_data {
        biome_data_map.insert(fname::from_str(&data.name), data);
    }
    Ok(biome_data_map)
}

pub fn get_tint_data(mc_version: &str) -> io::Result<TintData> {
    let version_data = get_prismarine_version_data(mc_version)?;
    let tint_data_location = version_data.tints.ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No tint data"))?;
    let tint_data_url = format!("{}/tints.json", prismarine_url(&tint_data_location));
    let tint_data: TintData = download_if_changed(format!("tints_{}.json", tint_data_location.replace('/', "_")).as_str(), tint_data_url.as_str(), false)?;
    Ok(tint_data)
}

#[derive(Clone, Deserialize)]
struct PrismarineData {
    pc: AHashMap<String, PrismarineVersionData>,
}

#[derive(Clone, Deserialize)]
struct PrismarineVersionData {
    blocks: Option<String>,
    biomes: Option<String>,
    tints: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct BiomeData {
    name: String,
    pub temperature: f64,
    pub rainfall: f64,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "color")]
    pub sky_color: i32,
}

#[derive(Clone, Deserialize)]
pub struct TintData {
    pub grass: TintDataData,
    pub foliage: TintDataData,
    pub water: TintDataData,
}

#[derive(Clone, Deserialize)]
pub struct TintDataData {
    pub data: Vec<TintDataEntry>,
}

#[derive(Clone, Deserialize)]
pub struct TintDataEntry {
    pub keys: Vec<FName>,
    pub color: i32,
}

// ===== Getting the Minecraft version from the world version ===== //
// TODO: is all of this unnecessary?

macro_rules! make_bi_map {
    ($($key:expr => $value:expr),*) => {
        {
            let mut m1 = AHashMap::new();
            let mut m2 = AHashMap::new();
            $(
                m1.insert($key.to_string(), $value);
                m2.insert($value, $key.to_string());
            )*
            (m1, m2)
        }
    }
}

lazy_static! {
    static ref HARDCODED_WORLD_VERSIONS: (AHashMap<String, u32>, AHashMap<u32, String>) = make_bi_map!(
        "15w32a" => 100,
        "15w32b" => 103,
        "15w32c" => 104,
        "15w33a" => 111,
        "15w33b" => 111,
        "15w33c" => 112,
        "15w34a" => 114,
        "15w34b" => 115,
        "15w34c" => 116,
        "15w34d" => 117,
        "15w35a" => 118,
        "15w35b" => 119,
        "15w35c" => 120,
        "15w35d" => 121,
        "15w35e" => 122,
        "15w36a" => 123,
        "15w36b" => 124,
        "15w36c" => 125,
        "15w36d" => 126,
        "15w37a" => 127,
        "15w38a" => 128,
        "15w38b" => 129,
        "15w39a" => 130,
        "15w39b" => 131,
        "15w39c" => 132,
        "15w40a" => 133,
        "15w40b" => 134,
        "15w41a" => 136,
        "15w41b" => 137,
        "15w42a" => 138,
        "15w43a" => 139,
        "15w43b" => 140,
        "15w43c" => 141,
        "15w44a" => 142,
        "15w44b" => 143,
        "15w45a" => 145,
        "15w46a" => 146,
        "15w47a" => 148,
        "15w47b" => 149,
        "15w47c" => 150,
        "15w49a" => 151,
        "15w49b" => 152,
        "15w50a" => 153,
        "15w51a" => 154,
        "15w51b" => 155,
        "16w02a" => 156,
        "16w03a" => 157,
        "16w04a" => 158,
        "16w05a" => 159,
        "16w05b" => 160,
        "16w06a" => 161,
        "16w07a" => 162,
        "16w07b" => 163,
        "1.9-pre1" => 164,
        "1.9-pre2" => 165,
        "1.9-pre3" => 167,
        "1.9-pre4" => 168,
        "1.9" => 169,
        "1.9.1-pre1" => 170,
        "1.9.1-pre2" => 171,
        "1.9.1-pre3" => 172,
        "1.9.1" => 175,
        "1.9.2" => 176,
        "16w14a" => 177,
        "16w15a" => 178,
        "16w15b" => 179,
        "1.9.3-pre1" => 180,
        "1.9.3-pre2" => 181,
        "1.9.3-pre3" => 182,
        "1.9.3" => 183,
        "1.9.4" => 184,
        "16w20a" => 501,
        "16w21a" => 503,
        "16w21b" => 504,
        "1.10-pre1" => 506,
        "1.10-pre2" => 507,
        "1.10" => 510,
        "1.10.1" => 511,
        "1.10.2" => 512,
        "16w32a" => 800,
        "16w32b" => 801,
        "16w33a" => 802,
        "16w35a" => 803,
        "16w36a" => 805,
        "16w38a" => 807,
        "16w39a" => 809,
        "16w39b" => 811,
        "16w39c" => 812,
        "16w40a" => 813,
        "16w41a" => 814,
        "16w42a" => 815,
        "16w43a" => 816,
        "16w44a" => 817,
        "1.11-pre1" => 818,
        "1.11" => 819,
        "16w50a" => 920,
        "1.11.1" => 921
    );
}

lazy_static! {
    static ref WORLD_VERSION_CACHE: sync::RwLock<(AHashMap<String, u32>, AHashMap<u32, String>)> = sync::RwLock::new({
        // read the cache csv file
        fn read_file() -> io::Result<(AHashMap<String, u32>, AHashMap<u32, String>)> {
        let csv = get_minecraft_cache().join("world_version_cache.csv");
            let file = fs::File::open(csv)?;
            let reader = io::BufReader::new(file);
            let mut m1 = AHashMap::new();
            let mut m2 = AHashMap::new();
            for line_input in reader.lines().skip(1) {
                let line = line_input?;
                let parts: Vec<_> = line.splitn(2, ',').collect();
                if parts.len() >= 2 {
                    let n = parts[1].parse().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    m1.insert(parts[0].to_string(), n);
                    m2.insert(n, parts[0].to_string());
                }
            }
            Ok((m1, m2))
        }
        read_file().unwrap_or_else(|_| (AHashMap::new(), AHashMap::new()))
    });
}


fn get_world_version(mc_version: &str) -> io::Result<u32> {
    if let Some(&version) = HARDCODED_WORLD_VERSIONS.0.get(mc_version) {
        return Ok(version);
    }

    if let Some(&version) = WORLD_VERSION_CACHE.read().unwrap().0.get(mc_version) {
        return Ok(version);
    }
    let mut versions = WORLD_VERSION_CACHE.write().unwrap();
    if let Some(&version) = versions.0.get(mc_version) {
        return Ok(version);
    }

    let burger_json: BurgerJson = download_if_changed(
        format!("{}_burger.json", mc_version).as_str(),
        &format!("https://pokechu22.github.io/Burger/{}.json", mc_version),
        false,
    )?;

    versions.0.insert(mc_version.to_string(), burger_json.version.data);
    versions.1.insert(burger_json.version.data, mc_version.to_string());

    fn write_versions(versions: &AHashMap<String, u32>) -> io::Result<()> {
        let file = fs::File::create(get_minecraft_cache().join("world_version_cache.csv"))?;
        let mut writer = io::BufWriter::new(file);
        for (mc_version, version) in versions.iter() {
            writeln!(writer, "{},{}", mc_version, version)?;
        }
        Ok(())
    }
    let _ = write_versions(&versions.0); // ignore the output, it doesn't matter

    return Ok(burger_json.version.data);
}

pub const ABSENT_WORLD_VERSION: u32 = 99;
pub const ABSENT_MINECRAFT_VERSION: &str = "1.8.9";

enum BinarySearchResult {
    Present(String),
    Absent(i32, i32),
}

fn binary_search_versions(versions: &[&Version], world_version: u32, mut left: i32, mut right: i32) -> BinarySearchResult {
    while left < right {
        let mut mid = (left + right) / 2;
        let mut mid_version = versions[mid as usize];
        let mut mid_version_id = mid_version.id.clone();
        let mut mid_version_world_version = get_world_version(&mid_version_id).ok();
        let mut going_left = false;
        while mid_version_world_version.is_none() {
            if !going_left {
                mid += 1;
                if mid >= right {
                    mid = (left + right) / 2;
                    going_left = true;
                }
            }
            if going_left {
                mid -= 1;
                if mid < left {
                    return BinarySearchResult::Absent(left, right);
                }
            }
            mid_version = versions[mid as usize];
            mid_version_id = mid_version.id.clone();
            mid_version_world_version = get_world_version(&mid_version_id).ok();
        }
        if mid_version_world_version.unwrap() == world_version {
            return BinarySearchResult::Present(mid_version_id);
        }
        if mid_version_world_version.unwrap() < world_version {
            left = mid + 1;
        } else {
            right = mid;
        }
    }

    while left >= 0 && (left >= versions.len() as i32 || (get_world_version(&versions[left as usize].id).ok().map(|it| it > world_version)) != Some(false)) {
        left -= 1;
    }
    while right < versions.len() as i32 && (right < 0 || (get_world_version(&versions[right as usize].id).ok().map(|it| it < world_version)) != Some(false)) {
        right += 1;
    }

    BinarySearchResult::Absent(left, right)
}

pub fn get_minecraft_version(world_version: u32) -> Option<String> {
    if world_version < 922 {
        return HARDCODED_WORLD_VERSIONS.1.get(&world_version).cloned();
    }
    if let Some(mc_version) = WORLD_VERSION_CACHE.read().unwrap().1.get(&world_version) {
        return Some(mc_version.clone());
    }

    let version_manifest: VersionManifest = download_if_changed(
        VERSION_MANIFEST_FILE,
        VERSION_MANIFEST_URL,
        false,
    ).ok()?;
    // try the most likely options first
    if get_world_version(&version_manifest.latest.release).ok().contains(&world_version) {
        return Some(version_manifest.latest.release);
    }
    let snapshot_world_version = get_world_version(&version_manifest.latest.snapshot).ok();
    if snapshot_world_version.is_some() && snapshot_world_version.unwrap() >= world_version {
        return Some(version_manifest.latest.snapshot);
    }

    let release_date_1_11_1 = chrono::Utc.ymd(2016, 12, 20).and_hms(23, 59, 59);

    let mut versions: Vec<_> = version_manifest.versions.iter().filter(|v| v.release_time > release_date_1_11_1).collect();
    versions.sort_by_key(|v| v.release_time);
    let release_versions: Vec<_> = versions.iter().filter(|v| v.release_type == "release").copied().collect();
    match binary_search_versions(&release_versions, world_version, 0, release_versions.len() as i32) {
        BinarySearchResult::Present(version) => Some(version),
        BinarySearchResult::Absent(mut left, mut right) => {
            left = left.max(0);
            right = right.min(versions.len() as i32);
            left = versions.iter().position(|v| v.id == release_versions[left as usize].id).unwrap() as i32;
            right = versions.iter().position(|v| v.id == release_versions[right as usize].id).unwrap() as i32;
            match binary_search_versions(&versions, world_version, left, right) {
                BinarySearchResult::Present(version) => Some(version),
                BinarySearchResult::Absent(left, right) => {
                    Some(versions[left.max(right) as usize].id.clone())
                }
            }
        }
    }
}

#[derive(Clone, Deserialize)]
struct BurgerJson {
    version: BurgerVersion,
}

#[derive(Clone, Deserialize)]
struct BurgerVersion {
    data: u32,
}
