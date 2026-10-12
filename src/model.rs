//! Struktur data hasil scan (serializable ke JSON).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    pub tool: String,
    pub started: f64,
    pub elapsed: f64,
    pub roots: Vec<String>,
    pub threads: usize,
    pub cancelled: bool,
    pub host: String,
    pub os: String,
    pub mode: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Totals {
    pub files: u64,
    pub dirs: u64,
    pub size: u64,
    pub errors: u64,
    pub cache_count: u64,
    pub cache_files: u64,
    pub cache_size: u64,
    pub cache_safe_size: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CacheHit {
    pub path: String,
    pub name: String,
    pub category: String,
    pub safe: bool,
    pub note: String,
    pub source: String,
    pub drive: String,
    pub files: u64,
    pub dirs: u64,
    pub size: u64,
    pub newest: f64,
    pub oldest: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CatStat {
    pub category: String,
    pub count: u64,
    pub files: u64,
    pub size: u64,
    pub safe_size: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NamedStat {
    pub name: String,
    pub count: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DriveStat {
    pub root: String,
    pub files: u64,
    pub dirs: u64,
    pub size: u64,
    pub top_dirs: Vec<NamedStat>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileStat {
    pub path: String,
    pub size: u64,
    pub mtime: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub meta: Meta,
    pub totals: Totals,
    pub drives: Vec<DriveStat>,
    pub caches: Vec<CacheHit>,
    pub cache_by_category: Vec<CatStat>,
    pub extensions: Vec<NamedStat>,
    pub age: Vec<NamedStat>,
    pub size_dist: Vec<NamedStat>,
    pub largest: Vec<FileStat>,
    pub error_samples: Vec<String>,
}
