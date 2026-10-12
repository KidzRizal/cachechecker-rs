//! Scanner paralel berbasis rayon.
//!
//! Tiap direktori = 1 job rayon (work-stealing) -> paralelisme merata walau
//! struktur folder tidak seimbang, tanpa rekursi stack yang dalam.
//! Statistik dikumpulkan per-thread (Mutex milik thread itu sendiri -> tanpa
//! kontensi) lalu di-merge di akhir. Progress live lewat atomic, dipakai CLI & TUI.

use crate::model::*;
use crate::signatures::{self, Sig};
use crate::util::{abs_path, host_name, norm_path, now_secs};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, UNIX_EPOCH};

pub const AGE_BUCKETS: &[(&str, f64)] = &[
    ("< 30 hari", 30.0 * 86400.0),
    ("30-180 hari", 180.0 * 86400.0),
    ("180 hari - 1 thn", 365.0 * 86400.0),
    ("1-3 tahun", 3.0 * 365.0 * 86400.0),
    ("> 3 tahun", f64::INFINITY),
];

pub const SIZE_BUCKETS: &[(&str, u64)] = &[
    ("0 B", 0),
    ("< 4 KB", 4 * 1024),
    ("4 KB - 1 MB", 1 << 20),
    ("1 - 100 MB", 100 << 20),
    ("100 MB - 1 GB", 1 << 30),
    ("> 1 GB", u64::MAX),
];

const SKIP_NAMES: &[&str] = &["system volume information", "$windows.~ws", "$windows.~bt", "config.msi"];
const GENERIC_NAMES: &[&str] = &["cache", "caches", "temp", "tmp"];

// ===================================================================== opsi & progress

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
    pub threads: usize,
    pub top_n: usize,
    pub detect_cache: bool,
    pub detect_patterns: bool,
    pub include_generic: bool,
    pub excludes: Vec<String>,
    pub skip_paths: HashSet<String>,
    pub min_cache_size: u64,
    pub stats: bool,
    pub group_by_drive: bool,
    pub max_errors_sample: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            roots: vec![],
            threads: default_threads(),
            top_n: 100,
            detect_cache: true,
            detect_patterns: true,
            include_generic: false,
            excludes: vec![],
            skip_paths: HashSet::new(),
            min_cache_size: 0,
            stats: true,
            group_by_drive: false,
            max_errors_sample: 30,
        }
    }
}

pub fn default_threads() -> usize {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (n * 2).clamp(4, 48)
}

/// Counter live (dibaca thread UI / progress bar).
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub size: AtomicU64,
    pub errors: AtomicU64,
    pub caches: AtomicU64,
    pub cancel: AtomicBool,
    pub done: AtomicBool,
    pub current: Mutex<String>,
}

impl Progress {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

// ===================================================================== state internal

struct CacheEntry {
    path: String,
    sig: Sig,
    source: &'static str,
    drive: String,
}

#[derive(Default)]
struct Local {
    files: u64,
    dirs: u64,
    size: u64,
    errors: u64,
    error_samples: Vec<String>,
    per_drive: HashMap<Arc<str>, [u64; 3]>,
    top_dirs: HashMap<(Arc<str>, Arc<str>), [u64; 2]>,
    ext: HashMap<String, [u64; 2]>,
    age: [[u64; 2]; 5],
    sizes: [[u64; 2]; 6],
    largest: BinaryHeap<Reverse<(u64, String, i64)>>,
    cache: HashMap<usize, CacheAcc>,
}

#[derive(Default, Clone, Copy)]
struct CacheAcc {
    files: u64,
    dirs: u64,
    size: u64,
    newest: f64,
    oldest: f64,
}

struct Shared<'a> {
    o: &'a ScanOptions,
    progress: &'a Progress,
    known: HashMap<String, Sig>,
    excludes: Vec<glob::Pattern>,
    registry: Mutex<(Vec<CacheEntry>, HashMap<String, usize>)>,
    locals: Vec<Mutex<Local>>,
    now: f64,
}

struct Job {
    path: PathBuf,
    drive: Arc<str>,
    top: Option<Arc<str>>,
    cid: Option<usize>,
}

impl Shared<'_> {
    fn local(&self) -> std::sync::MutexGuard<'_, Local> {
        let i = rayon::current_thread_index().unwrap_or(0) % self.locals.len();
        self.locals[i].lock().unwrap()
    }

    fn register(&self, path: &Path, sig: Sig, source: &'static str, drive: &str) -> usize {
        let key = norm_path(path);
        let mut reg = self.registry.lock().unwrap();
        if let Some(&id) = reg.1.get(&key) {
            return id;
        }
        let id = reg.0.len();
        reg.0.push(CacheEntry { path: path.to_string_lossy().into(), sig, source, drive: drive.into() });
        reg.1.insert(key, id);
        self.progress.caches.fetch_add(1, Ordering::Relaxed);
        id
    }

    fn excluded(&self, path: &Path, lname: &str) -> bool {
        if SKIP_NAMES.contains(&lname) {
            return true;
        }
        if !self.o.skip_paths.is_empty() && self.o.skip_paths.contains(&norm_path(path)) {
            return true;
        }
        if self.excludes.is_empty() {
            return false;
        }
        let pl = path.to_string_lossy().to_lowercase();
        self.excludes.iter().any(|p| p.matches(&pl) || p.matches(lname))
    }

    fn error(&self, l: &mut Local, path: &Path, e: &std::io::Error) {
        l.errors += 1;
        self.progress.errors.fetch_add(1, Ordering::Relaxed);
        if l.error_samples.len() < self.o.max_errors_sample {
            l.error_samples.push(format!("{}: {}", path.display(), e));
        }
    }
}

fn is_link(md: &fs::Metadata) -> bool {
    if md.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if md.file_attributes() & 0x400 != 0 {
            return true; // reparse point (junction dll)
        }
    }
    false
}

fn mtime_of(md: &fs::Metadata) -> f64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ===================================================================== walk

fn visit<'s>(scope: &rayon::Scope<'s>, sh: &'s Shared<'s>, job: Job) {
    if sh.progress.cancelled() {
        return;
    }
    let rd = match fs::read_dir(&job.path) {
        Ok(r) => r,
        Err(e) => {
            sh.error(&mut sh.local(), &job.path, &e);
            return;
        }
    };
    let o = sh.o;
    let parent_lname = job.path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();

    let mut subdirs: Vec<Job> = Vec::new();
    let mut files: Vec<(String, PathBuf, u64, f64)> = Vec::new();
    let mut errs: Vec<(PathBuf, std::io::Error)> = Vec::new();

    for ent in rd {
        let ent = match ent {
            Ok(e) => e,
            Err(e) => {
                errs.push((job.path.clone(), e));
                continue;
            }
        };
        let ft = match ent.file_type() {
            Ok(t) => t,
            Err(e) => {
                errs.push((ent.path(), e));
                continue;
            }
        };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let path = ent.path();
            // junction / reparse point
            #[cfg(windows)]
            if let Ok(md) = ent.metadata() {
                if is_link(&md) {
                    continue;
                }
            }
            let name = ent.file_name().to_string_lossy().into_owned();
            let lname = name.to_lowercase();
            if sh.excluded(&path, &lname) {
                continue;
            }
            let mut sub_cid = job.cid;
            if o.detect_cache && job.cid.is_none() {
                let mut hit: Option<(Sig, &'static str)> = sh.known.get(&norm_path(&path)).map(|s| (*s, "known"));
                if hit.is_none() && o.detect_patterns {
                    if let Some(sig) = signatures::match_pattern(&lname, &job.path, &parent_lname) {
                        let generic = GENERIC_NAMES.contains(&lname.as_str()) && sig.category == "other";
                        if !generic || o.include_generic {
                            hit = Some((*sig, "pattern"));
                        }
                    }
                }
                if let Some((sig, src)) = hit {
                    sub_cid = Some(sh.register(&path, sig, src, &job.drive));
                }
            }
            let top = job.top.clone().or_else(|| Some(Arc::from(name.as_str())));
            subdirs.push(Job { path, drive: job.drive.clone(), top, cid: sub_cid });
            continue;
        }
        match ent.metadata() {
            Ok(md) => {
                let name = ent.file_name().to_string_lossy().into_owned();
                files.push((name, ent.path(), md.len(), mtime_of(&md)));
            }
            Err(e) => errs.push((ent.path(), e)),
        }
    }

    // ---- akumulasi ke statistik thread ini (satu kali lock per direktori)
    let mut d_size = 0u64;
    {
        let mut l = sh.local();
        for (p, e) in &errs {
            sh.error(&mut l, p, e);
        }
        l.dirs += subdirs.len() as u64;
        if let Some(cid) = job.cid {
            let c = l.cache.entry(cid).or_default();
            c.dirs += subdirs.iter().filter(|j| j.cid == Some(cid)).count() as u64;
        }
        for (name, path, sz, mt) in &files {
            let (sz, mt) = (*sz, *mt);
            d_size += sz;
            if let Some(cid) = job.cid {
                let c = l.cache.entry(cid).or_default();
                c.files += 1;
                c.size += sz;
                if mt > c.newest {
                    c.newest = mt;
                }
                if c.oldest == 0.0 || (mt > 0.0 && mt < c.oldest) {
                    c.oldest = mt;
                }
            }
            if o.stats {
                let ext = match name.rfind('.') {
                    Some(i) if i > 0 && i < name.len() - 1 => {
                        let e = name[i..].to_lowercase();
                        if e.len() > 12 { "(other)".to_string() } else { e }
                    }
                    _ => "(none)".to_string(),
                };
                let e = l.ext.entry(ext).or_default();
                e[0] += 1;
                e[1] += sz;
                let age = sh.now - mt;
                if let Some(i) = AGE_BUCKETS.iter().position(|(_, mx)| age < *mx) {
                    l.age[i][0] += 1;
                    l.age[i][1] += sz;
                }
                if let Some(i) = SIZE_BUCKETS.iter().position(|(_, mx)| sz <= *mx) {
                    l.sizes[i][0] += 1;
                    l.sizes[i][1] += sz;
                }
                if o.top_n > 0 {
                    if l.largest.len() < o.top_n {
                        l.largest.push(Reverse((sz, path.to_string_lossy().into(), mt as i64)));
                    } else if l.largest.peek().map(|r| sz > r.0 .0).unwrap_or(false) {
                        l.largest.pop();
                        l.largest.push(Reverse((sz, path.to_string_lossy().into(), mt as i64)));
                    }
                }
            }
        }
        let nf = files.len() as u64;
        l.files += nf;
        l.size += d_size;
        let pd = l.per_drive.entry(job.drive.clone()).or_default();
        pd[0] += nf;
        pd[1] += 1;
        pd[2] += d_size;
        let topk = job.top.clone().unwrap_or_else(|| Arc::from("(file di root)"));
        let td = l.top_dirs.entry((job.drive.clone(), topk)).or_default();
        td[0] += nf;
        td[1] += d_size;
    }

    let pg = sh.progress;
    pg.files.fetch_add(files.len() as u64, Ordering::Relaxed);
    pg.size.fetch_add(d_size, Ordering::Relaxed);
    pg.dirs.fetch_add(1, Ordering::Relaxed);
    if !files.is_empty() {
        if let Ok(mut cur) = pg.current.try_lock() {
            *cur = job.path.to_string_lossy().into_owned();
        }
    }

    for sub in subdirs {
        scope.spawn(move |s| visit(s, sh, sub));
    }
}

// ===================================================================== run

pub fn run(o: &ScanOptions, progress: &Progress) -> ScanResult {
    let t0 = Instant::now();
    let now = now_secs();
    let mut known = HashMap::new();
    if o.detect_cache {
        for (p, sig) in signatures::resolve_known_locations() {
            known.insert(norm_path(&p), sig);
        }
    }
    let excludes = o
        .excludes
        .iter()
        .filter_map(|e| glob::Pattern::new(&e.to_lowercase().replace('/', std::path::MAIN_SEPARATOR_STR)).ok())
        .collect();
    let threads = o.threads.max(1);
    let sh = Shared {
        o,
        progress,
        known,
        excludes,
        registry: Mutex::new((Vec::new(), HashMap::new())),
        locals: (0..threads + 1).map(|_| Mutex::new(Local::default())).collect(),
        now,
    };

    let roots: Vec<PathBuf> = o.roots.iter().map(|r| abs_path(r)).filter(|r| r.is_dir()).collect();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("thread pool");
    pool.install(|| {
        rayon::scope(|s| {
            for r in &roots {
                let drive: Arc<str> = if o.group_by_drive {
                    root_drive(r).into()
                } else {
                    r.to_string_lossy().into()
                };
                let cid = if o.detect_cache {
                    sh.known.get(&norm_path(r)).copied().map(|sig| sh.register(r, sig, "known", &drive))
                } else {
                    None
                };
                let job = Job { path: r.clone(), drive, top: None, cid };
                let shr = &sh;
                s.spawn(move |s2| visit(s2, shr, job));
            }
        });
    });
    progress.done.store(true, Ordering::Relaxed);
    merge(sh, roots, t0.elapsed().as_secs_f64(), now)
}

fn root_drive(p: &Path) -> String {
    use std::path::Component;
    match p.components().next() {
        Some(Component::Prefix(pre)) => format!("{}\\", pre.as_os_str().to_string_lossy()),
        _ => "/".into(),
    }
}

fn merge(sh: Shared<'_>, roots: Vec<PathBuf>, elapsed: f64, now: f64) -> ScanResult {
    let o = sh.o;
    let cancelled = sh.progress.cancelled();
    let (entries, _) = sh.registry.into_inner().unwrap();

    let mut t = Totals::default();
    let mut err_samples = Vec::new();
    let mut per_drive: HashMap<Arc<str>, [u64; 3]> = HashMap::new();
    let mut top_dirs: HashMap<(Arc<str>, Arc<str>), [u64; 2]> = HashMap::new();
    let mut ext: HashMap<String, [u64; 2]> = HashMap::new();
    let mut age = [[0u64; 2]; 5];
    let mut sizes = [[0u64; 2]; 6];
    let mut largest: Vec<(u64, String, i64)> = Vec::new();
    let mut cache_acc: HashMap<usize, CacheAcc> = HashMap::new();

    for lm in sh.locals {
        let l = lm.into_inner().unwrap();
        t.files += l.files;
        t.dirs += l.dirs;
        t.size += l.size;
        t.errors += l.errors;
        err_samples.extend(l.error_samples);
        for (k, v) in l.per_drive {
            let e = per_drive.entry(k).or_default();
            for i in 0..3 {
                e[i] += v[i];
            }
        }
        for (k, v) in l.top_dirs {
            let e = top_dirs.entry(k).or_default();
            e[0] += v[0];
            e[1] += v[1];
        }
        for (k, v) in l.ext {
            let e = ext.entry(k).or_default();
            e[0] += v[0];
            e[1] += v[1];
        }
        for i in 0..5 {
            age[i][0] += l.age[i][0];
            age[i][1] += l.age[i][1];
        }
        for i in 0..6 {
            sizes[i][0] += l.sizes[i][0];
            sizes[i][1] += l.sizes[i][1];
        }
        largest.extend(l.largest.into_iter().map(|r| r.0));
        for (id, v) in l.cache {
            let c = cache_acc.entry(id).or_default();
            c.files += v.files;
            c.dirs += v.dirs;
            c.size += v.size;
            c.newest = c.newest.max(v.newest);
            if v.oldest > 0.0 && (c.oldest == 0.0 || v.oldest < c.oldest) {
                c.oldest = v.oldest;
            }
        }
    }

    largest.sort_by(|a, b| b.0.cmp(&a.0));
    largest.truncate(o.top_n);

    let mut caches: Vec<CacheHit> = entries
        .into_iter()
        .enumerate()
        .map(|(id, e)| {
            let a = cache_acc.get(&id).copied().unwrap_or_default();
            CacheHit {
                path: e.path,
                name: e.sig.name.into(),
                category: e.sig.category.into(),
                safe: e.sig.safe,
                note: e.sig.note.into(),
                source: e.source.into(),
                drive: e.drive,
                files: a.files,
                dirs: a.dirs,
                size: a.size,
                newest: a.newest,
                oldest: a.oldest,
            }
        })
        .filter(|c| c.size >= o.min_cache_size && (c.files > 0 || c.size > 0))
        .collect();
    caches.sort_by(|a, b| b.size.cmp(&a.size));

    let mut by_cat: HashMap<String, CatStat> = HashMap::new();
    for c in &caches {
        let e = by_cat.entry(c.category.clone()).or_insert_with(|| CatStat {
            category: c.category.clone(),
            ..Default::default()
        });
        e.count += 1;
        e.files += c.files;
        e.size += c.size;
        if c.safe {
            e.safe_size += c.size;
        }
    }
    let mut by_cat: Vec<CatStat> = by_cat.into_values().collect();
    by_cat.sort_by(|a, b| b.size.cmp(&a.size));

    t.cache_count = caches.len() as u64;
    t.cache_files = caches.iter().map(|c| c.files).sum();
    t.cache_size = caches.iter().map(|c| c.size).sum();
    t.cache_safe_size = caches.iter().filter(|c| c.safe).map(|c| c.size).sum();

    let mut drives: Vec<DriveStat> = per_drive
        .into_iter()
        .map(|(d, v)| {
            let mut tops: Vec<NamedStat> = top_dirs
                .iter()
                .filter(|((dd, _), _)| *dd == d)
                .map(|((_, n), tv)| NamedStat { name: n.to_string(), count: tv[0], size: tv[1] })
                .collect();
            tops.sort_by(|a, b| b.size.cmp(&a.size));
            tops.truncate(o.top_n.max(30));
            DriveStat { root: d.to_string(), files: v[0], dirs: v[1], size: v[2], top_dirs: tops }
        })
        .collect();
    drives.sort_by(|a, b| a.root.cmp(&b.root));

    let mut exts: Vec<NamedStat> =
        ext.into_iter().map(|(k, v)| NamedStat { name: k, count: v[0], size: v[1] }).collect();
    exts.sort_by(|a, b| b.size.cmp(&a.size));

    err_samples.truncate(o.max_errors_sample);

    ScanResult {
        meta: Meta {
            tool: format!("cachechecker-rs {}", env!("CARGO_PKG_VERSION")),
            started: now,
            elapsed: (elapsed * 100.0).round() / 100.0,
            roots: roots.iter().map(|r| r.to_string_lossy().into()).collect(),
            threads: o.threads,
            cancelled,
            host: host_name(),
            os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            mode: if o.stats { "full".into() } else { "cache".into() },
        },
        totals: t,
        drives,
        caches,
        cache_by_category: by_cat,
        extensions: exts,
        age: AGE_BUCKETS
            .iter()
            .enumerate()
            .map(|(i, (l, _))| NamedStat { name: l.to_string(), count: age[i][0], size: age[i][1] })
            .collect(),
        size_dist: SIZE_BUCKETS
            .iter()
            .enumerate()
            .map(|(i, (l, _))| NamedStat { name: l.to_string(), count: sizes[i][0], size: sizes[i][1] })
            .collect(),
        largest: largest.into_iter().map(|(s, p, m)| FileStat { path: p, size: s, mtime: m as f64 }).collect(),
        error_samples: err_samples,
    }
}

// ===================================================================== preset

/// Opsi scan cepat: hanya lokasi cache yang dikenal (root di-scan langsung),
/// lokasi nested (mis. ~/.cache/huggingface di dalam ~/.cache) di-skip dari parent.
pub fn cache_only_options(threads: usize, extra_paths: &[PathBuf], include_generic: bool, min_size: u64) -> ScanOptions {
    let mut o = ScanOptions {
        threads,
        top_n: 0,
        stats: false,
        include_generic,
        min_cache_size: min_size,
        ..Default::default()
    };
    if !extra_paths.is_empty() {
        o.roots = extra_paths.to_vec();
        o.detect_patterns = true;
        return o;
    }
    let known: Vec<PathBuf> = signatures::resolve_known_locations().into_iter().map(|(p, _)| p).collect();
    let normed: Vec<String> = known.iter().map(|p| norm_path(p)).collect();
    let sep = std::path::MAIN_SEPARATOR;
    o.skip_paths = normed
        .iter()
        .filter(|p| normed.iter().any(|q| q != *p && p.starts_with(&format!("{q}{sep}"))))
        .cloned()
        .collect();
    // root yang nested tidak perlu jadi root terpisah? tetap perlu: dia di-skip dari parent,
    // jadi harus di-scan sebagai root sendiri.
    o.roots = known;
    o.detect_patterns = false;
    o.group_by_drive = true;
    o
}
