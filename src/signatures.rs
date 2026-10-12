//! Database signature cache.
//!
//! 1. KNOWN   -> lokasi pasti (env var / glob), contoh: {LOCALAPPDATA}/npm-cache
//! 2. PATTERN -> nama folder yang dikenali di mana pun saat full scan, contoh: __pycache__
//!
//! safe=true  -> aman dihapus (dibuat ulang otomatis oleh aplikasinya)
//! safe=false -> hanya dilaporkan; perlu dicek manual (node_modules, model AI, dll)

use crate::util::norm_path;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug)]
pub struct Sig {
    pub name: &'static str,
    pub category: &'static str,
    pub safe: bool,
    pub note: &'static str,
}

pub const fn s(name: &'static str, category: &'static str, safe: bool, note: &'static str) -> Sig {
    Sig { name, category, safe, note }
}

pub struct Known {
    pub sig: Sig,
    pub paths: Vec<String>,
}

fn k(sig: Sig, paths: &[&str]) -> Known {
    Known { sig, paths: paths.iter().map(|p| p.to_string()).collect() }
}

pub struct DirPattern {
    pub sig: Sig,
    pub dirname: &'static str,
    pub parent: Option<&'static str>,
    pub marker: Option<&'static str>,
}

const fn p(sig: Sig, dirname: &'static str) -> DirPattern {
    DirPattern { sig, dirname, parent: None, marker: None }
}
const fn pp(sig: Sig, dirname: &'static str, parent: &'static str) -> DirPattern {
    DirPattern { sig, dirname, parent: Some(parent), marker: None }
}
const fn pm(sig: Sig, dirname: &'static str, marker: &'static str) -> DirPattern {
    DirPattern { sig, dirname, parent: None, marker: Some(marker) }
}

// ===================================================================== KNOWN (Windows)

fn chromium(name: &'static str, ud: &str) -> Known {
    let subs = [
        "Cache", "Code Cache", "GPUCache", "DawnCache", "DawnGraphiteCache", "DawnWebGPUCache",
        "GrShaderCache", "ShaderCache", "Service Worker/CacheStorage",
    ];
    let mut paths = Vec::new();
    for prof in ["Default", "Profile *", "Guest Profile"] {
        for sub in subs {
            paths.push(format!("{ud}/{prof}/{sub}"));
        }
    }
    for sub in ["GrShaderCache", "ShaderCache", "component_crx_cache", "extensions_crx_cache"] {
        paths.push(format!("{ud}/{sub}"));
    }
    Known { sig: s(name, "browser", true, "tutup browser dulu sebelum clean"), paths }
}

fn electron(name: &'static str, base: &str, category: &'static str) -> Known {
    let subs = [
        "Cache", "Code Cache", "GPUCache", "DawnCache", "DawnGraphiteCache",
        "Service Worker/CacheStorage", "Crashpad/reports",
    ];
    Known {
        sig: s(name, category, true, "tutup aplikasi dulu"),
        paths: subs.iter().map(|x| format!("{base}/{x}")).collect(),
    }
}

pub fn windows_known() -> Vec<Known> {
    vec![
        // --- system
        k(s("Windows Temp (user)", "system", true, ""), &["{TEMP}"]),
        k(s("Windows Temp (system)", "system", true, "butuh admin"), &["{WINDIR}/Temp"]),
        k(s("Windows Update Download", "system", true, "butuh admin; stop service wuauserv dulu"),
          &["{WINDIR}/SoftwareDistribution/Download"]),
        k(s("Delivery Optimization", "system", true, "butuh admin"),
          &["{WINDIR}/ServiceProfiles/NetworkService/AppData/Local/Microsoft/Windows/DeliveryOptimization/Cache"]),
        k(s("Prefetch", "system", false, "dipakai Windows untuk mempercepat startup app"), &["{WINDIR}/Prefetch"]),
        k(s("Windows Logs (CBS/DISM)", "system", true, "butuh admin"), &["{WINDIR}/Logs/CBS", "{WINDIR}/Logs/DISM"]),
        k(s("Memory dumps", "system", true, "butuh admin"), &["{WINDIR}/Minidump", "{WINDIR}/LiveKernelReports"]),
        k(s("Error Reporting (WER)", "system", true, ""),
          &["{LOCALAPPDATA}/Microsoft/Windows/WER", "{PROGRAMDATA}/Microsoft/Windows/WER"]),
        k(s("Crash Dumps", "system", true, ""), &["{LOCALAPPDATA}/CrashDumps"]),
        k(s("Thumbnail & Icon cache", "system", true, "beberapa file dikunci Explorer"),
          &["{LOCALAPPDATA}/Microsoft/Windows/Explorer"]),
        k(s("INetCache", "system", true, ""), &["{LOCALAPPDATA}/Microsoft/Windows/INetCache"]),
        k(s("Package Cache (installer)", "system", false, "dipakai untuk repair/uninstall"), &["{PROGRAMDATA}/Package Cache"]),
        // --- gpu
        k(s("DirectX Shader Cache", "gpu", true, ""), &["{LOCALAPPDATA}/D3DSCache"]),
        k(s("NVIDIA Shader Cache", "gpu", true, ""), &[
            "{LOCALAPPDATA}/NVIDIA/DXCache", "{LOCALAPPDATA}/NVIDIA/GLCache",
            "{USERPROFILE}/AppData/LocalLow/NVIDIA/PerDriverVersion/DXCache",
            "{USERPROFILE}/AppData/LocalLow/NVIDIA/PerDriverVersion/GLCache",
            "{PROGRAMDATA}/NVIDIA Corporation/NV_Cache",
        ]),
        k(s("NVIDIA Installer cache", "gpu", true, ""), &["{PROGRAMDATA}/NVIDIA Corporation/Downloader", "C:/NVIDIA"]),
        k(s("AMD Shader Cache", "gpu", true, ""), &[
            "{LOCALAPPDATA}/AMD/DxCache", "{LOCALAPPDATA}/AMD/DxcCache",
            "{LOCALAPPDATA}/AMD/GLCache", "{LOCALAPPDATA}/AMD/VkCache",
        ]),
        k(s("Intel Shader Cache", "gpu", true, ""), &["{USERPROFILE}/AppData/LocalLow/Intel/ShaderCache"]),
        // --- browser
        chromium("Google Chrome", "{LOCALAPPDATA}/Google/Chrome/User Data"),
        chromium("Microsoft Edge", "{LOCALAPPDATA}/Microsoft/Edge/User Data"),
        chromium("Brave", "{LOCALAPPDATA}/BraveSoftware/Brave-Browser/User Data"),
        chromium("Vivaldi", "{LOCALAPPDATA}/Vivaldi/User Data"),
        chromium("Chromium", "{LOCALAPPDATA}/Chromium/User Data"),
        k(s("Opera", "browser", true, "tutup browser dulu"), &[
            "{LOCALAPPDATA}/Opera Software/*/Cache", "{APPDATA}/Opera Software/*/Cache",
            "{APPDATA}/Opera Software/*/Code Cache", "{APPDATA}/Opera Software/*/GPUCache",
        ]),
        k(s("Firefox", "browser", true, "tutup browser dulu"), &[
            "{LOCALAPPDATA}/Mozilla/Firefox/Profiles/*/cache2",
            "{LOCALAPPDATA}/Mozilla/Firefox/Profiles/*/startupCache",
            "{LOCALAPPDATA}/Mozilla/Firefox/Profiles/*/thumbnails",
        ]),
        // --- app
        electron("Discord", "{APPDATA}/discord", "app"),
        electron("Slack", "{APPDATA}/Slack", "app"),
        electron("Microsoft Teams (classic)", "{APPDATA}/Microsoft/Teams", "app"),
        electron("VS Code", "{APPDATA}/Code", "dev"),
        electron("Cursor", "{APPDATA}/Cursor", "dev"),
        electron("Postman", "{APPDATA}/Postman", "dev"),
        electron("Notion", "{APPDATA}/Notion", "app"),
        electron("WhatsApp Desktop", "{APPDATA}/WhatsApp", "app"),
        electron("Figma", "{APPDATA}/Figma", "app"),
        electron("Obsidian", "{APPDATA}/obsidian", "app"),
        k(s("VS Code extra", "dev", true, ""), &[
            "{APPDATA}/Code/CachedData", "{APPDATA}/Code/CachedExtensionVSIXs",
            "{APPDATA}/Code/CachedProfilesData", "{APPDATA}/Code/logs",
        ]),
        k(s("Microsoft Teams (new)", "app", true, ""), &[
            "{LOCALAPPDATA}/Packages/MSTeams_*/LocalCache/Microsoft/MSTeams/EBWebView/Default/Cache",
            "{LOCALAPPDATA}/Packages/MSTeams_*/LocalCache/Microsoft/MSTeams/EBWebView/Default/Code Cache",
        ]),
        k(s("Spotify", "app", true, "cache lagu offline ikut terhapus"),
          &["{LOCALAPPDATA}/Spotify/Data", "{LOCALAPPDATA}/Spotify/Browser/Cache"]),
        k(s("Telegram Desktop", "app", true, ""),
          &["{APPDATA}/Telegram Desktop/tdata/user_data", "{APPDATA}/Telegram Desktop/tdata/emoji"]),
        k(s("Adobe Media Cache", "app", true, ""), &[
            "{APPDATA}/Adobe/Common/Media Cache Files", "{APPDATA}/Adobe/Common/Media Cache",
            "{LOCALAPPDATA}/Adobe/*/Cache",
        ]),
        // --- game
        k(s("Steam", "game", true, ""), &[
            "{PROGRAMFILES(X86)}/Steam/appcache/httpcache", "{PROGRAMFILES(X86)}/Steam/depotcache",
            "{PROGRAMFILES(X86)}/Steam/logs", "{LOCALAPPDATA}/Steam/htmlcache",
        ]),
        k(s("Epic Games Launcher", "game", true, ""),
          &["{LOCALAPPDATA}/EpicGamesLauncher/Saved/webcache*", "{LOCALAPPDATA}/EpicGamesLauncher/Saved/Logs"]),
        // --- dev
        k(s("npm cache", "dev", true, "atau: npm cache clean --force"), &["{LOCALAPPDATA}/npm-cache"]),
        k(s("Yarn cache", "dev", true, "atau: yarn cache clean"), &["{LOCALAPPDATA}/Yarn/Cache", "{LOCALAPPDATA}/Yarn/Berry/cache"]),
        k(s("pnpm store", "dev", false, "pakai: pnpm store prune"), &["{LOCALAPPDATA}/pnpm/store", "{LOCALAPPDATA}/pnpm-store"]),
        k(s("Bun cache", "dev", true, ""), &["{USERPROFILE}/.bun/install/cache"]),
        k(s("Deno cache", "dev", true, ""), &["{LOCALAPPDATA}/deno"]),
        k(s("pip cache", "dev", true, "atau: pip cache purge"), &["{LOCALAPPDATA}/pip/Cache"]),
        k(s("uv cache", "dev", true, "atau: uv cache clean"), &["{LOCALAPPDATA}/uv/cache"]),
        k(s("Poetry cache", "dev", true, ""), &["{LOCALAPPDATA}/pypoetry/Cache"]),
        k(s("Conda pkgs", "dev", true, "atau: conda clean --all"),
          &["{USERPROFILE}/anaconda3/pkgs", "{USERPROFILE}/miniconda3/pkgs", "{USERPROFILE}/.conda/pkgs"]),
        k(s("Go build cache", "dev", true, "atau: go clean -cache"), &["{LOCALAPPDATA}/go-build"]),
        k(s("Go module cache", "dev", true, "atau: go clean -modcache"), &["{USERPROFILE}/go/pkg/mod"]),
        k(s("Cargo registry cache", "dev", true, ""), &[
            "{USERPROFILE}/.cargo/registry/cache", "{USERPROFILE}/.cargo/registry/src",
            "{USERPROFILE}/.cargo/git/checkouts",
        ]),
        k(s("Gradle cache", "dev", true, ""), &["{USERPROFILE}/.gradle/caches", "{USERPROFILE}/.gradle/wrapper/dists"]),
        k(s("Maven repository", "dev", false, "download ulang saat build"), &["{USERPROFILE}/.m2/repository"]),
        k(s("NuGet packages", "dev", false, "atau: dotnet nuget locals all --clear"),
          &["{USERPROFILE}/.nuget/packages", "{LOCALAPPDATA}/NuGet/v3-cache", "{LOCALAPPDATA}/NuGet/Cache"]),
        k(s("Composer cache", "dev", true, ""), &["{LOCALAPPDATA}/Composer"]),
        k(s("Playwright browsers", "dev", false, "browser dipakai Playwright"), &["{LOCALAPPDATA}/ms-playwright"]),
        k(s("Puppeteer browsers", "dev", false, ""), &["{USERPROFILE}/.cache/puppeteer"]),
        k(s("Electron cache", "dev", true, ""), &["{LOCALAPPDATA}/electron/Cache", "{LOCALAPPDATA}/electron-builder/Cache"]),
        k(s("node-gyp", "dev", true, ""), &["{LOCALAPPDATA}/node-gyp/Cache"]),
        k(s("JetBrains caches", "dev", true, "tutup IDE dulu"), &["{LOCALAPPDATA}/JetBrains/*/caches", "{LOCALAPPDATA}/JetBrains/*/log"]),
        k(s("Android Studio / SDK temp", "dev", true, ""),
          &["{LOCALAPPDATA}/Google/AndroidStudio*/caches", "{USERPROFILE}/.android/cache"]),
        k(s("Docker Desktop data", "dev", false, "pakai: docker system prune"), &["{LOCALAPPDATA}/Docker/wsl"]),
        // --- ai
        k(s("HuggingFace models", "ai", false, "model AI besar, download ulang lama"), &["{USERPROFILE}/.cache/huggingface"]),
        k(s("PyTorch hub", "ai", false, ""), &["{USERPROFILE}/.cache/torch"]),
        k(s("Ollama models", "ai", false, ""), &["{USERPROFILE}/.ollama/models"]),
        k(s("Generic ~/.cache", "dev", false, "isi campuran, cek manual"), &["{USERPROFILE}/.cache"]),
    ]
}

// ===================================================================== KNOWN (Linux / macOS)

pub fn posix_known() -> Vec<Known> {
    vec![
        k(s("System temp", "system", true, ""), &["/tmp", "/var/tmp"]),
        k(s("System package cache", "system", true, "butuh root; apt clean / dnf clean all"),
          &["/var/cache/apt/archives", "/var/cache/dnf", "/var/cache/pacman/pkg", "/var/cache/yum"]),
        k(s("System logs (journal)", "system", false, "pakai: journalctl --vacuum-size=200M"), &["/var/log/journal"]),
        k(s("Trash", "trash", false, "kosongkan via file manager"), &["{HOME}/.local/share/Trash", "{HOME}/.Trash"]),
        k(s("Thumbnails", "system", true, ""), &["{HOME}/.cache/thumbnails", "{HOME}/.thumbnails"]),
        k(s("Google Chrome", "browser", true, ""), &["{HOME}/.cache/google-chrome", "{HOME}/Library/Caches/Google/Chrome"]),
        k(s("Chromium", "browser", true, ""), &["{HOME}/.cache/chromium"]),
        k(s("Firefox", "browser", true, ""), &["{HOME}/.cache/mozilla/firefox", "{HOME}/Library/Caches/Firefox"]),
        k(s("npm cache", "dev", true, ""), &["{HOME}/.npm/_cacache"]),
        k(s("Yarn cache", "dev", true, ""), &["{HOME}/.cache/yarn", "{HOME}/Library/Caches/Yarn"]),
        k(s("pip cache", "dev", true, ""), &["{HOME}/.cache/pip", "{HOME}/Library/Caches/pip"]),
        k(s("uv cache", "dev", true, ""), &["{HOME}/.cache/uv"]),
        k(s("Go build cache", "dev", true, ""), &["{HOME}/.cache/go-build", "{HOME}/Library/Caches/go-build"]),
        k(s("Cargo registry cache", "dev", true, ""), &["{HOME}/.cargo/registry/cache", "{HOME}/.cargo/registry/src"]),
        k(s("Gradle cache", "dev", true, ""), &["{HOME}/.gradle/caches"]),
        k(s("Maven repository", "dev", false, ""), &["{HOME}/.m2/repository"]),
        k(s("Playwright browsers", "dev", false, ""), &["{HOME}/.cache/ms-playwright", "{HOME}/Library/Caches/ms-playwright"]),
        k(s("HuggingFace models", "ai", false, ""), &["{HOME}/.cache/huggingface"]),
        k(s("Xcode DerivedData", "dev", true, ""), &["{HOME}/Library/Developer/Xcode/DerivedData"]),
        k(s("macOS user caches", "app", false, "isi campuran"), &["{HOME}/Library/Caches"]),
        k(s("Generic ~/.cache", "dev", false, "isi campuran"), &["{HOME}/.cache"]),
    ]
}

// ===================================================================== PATTERN (semua OS)

pub static DIR_PATTERNS: &[DirPattern] = &[
    p(s("Python __pycache__", "dev", true, ""), "__pycache__"),
    p(s("pytest cache", "dev", true, ""), ".pytest_cache"),
    p(s("mypy cache", "dev", true, ""), ".mypy_cache"),
    p(s("ruff cache", "dev", true, ""), ".ruff_cache"),
    p(s("tox envs", "dev", true, ""), ".tox"),
    pp(s("Next.js cache", "dev", true, ""), "cache", ".next"),
    p(s("Nuxt cache", "dev", true, ""), ".nuxt"),
    p(s("Parcel cache", "dev", true, ""), ".parcel-cache"),
    p(s("Turborepo cache", "dev", true, ""), ".turbo"),
    pp(s("Vite cache", "dev", true, ""), ".vite", "node_modules"),
    pp(s("Webpack/Babel cache", "dev", true, ""), ".cache", "node_modules"),
    pp(s("Angular cache", "dev", true, ""), "cache", ".angular"),
    pm(s("Gradle project cache", "dev", true, ""), ".gradle", "settings.gradle"),
    pm(s("Gradle project cache", "dev", true, ""), ".gradle", "build.gradle"),
    pm(s("Gradle project cache", "dev", true, ""), ".gradle", "settings.gradle.kts"),
    pm(s("Rust target/", "dev", true, "dibuat ulang saat cargo build"), "target", "Cargo.toml"),
    p(s("node_modules", "dev-artifact", false, "perlu npm install lagi"), "node_modules"),
    pp(s("Steam shader cache", "game", true, ""), "shadercache", "steamapps"),
    pp(s("Steam download temp", "game", true, ""), "downloading", "steamapps"),
    pp(s("Steam temp", "game", true, ""), "temp", "steamapps"),
    p(s("Recycle Bin", "trash", false, "kosongkan lewat Explorer"), "$recycle.bin"),
    p(s("Chromium/Electron GPU cache", "app", true, ""), "gpucache"),
    p(s("Chromium/Electron code cache", "app", true, ""), "code cache"),
    p(s("Chromium/Electron Dawn cache", "app", true, ""), "dawncache"),
    p(s("Chromium/Electron shader cache", "app", true, ""), "grshadercache"),
    p(s("Unity/Unreal DerivedDataCache", "game", true, ""), "deriveddatacache"),
    pp(s("Unity Library cache", "dev", true, ""), "shadercache", "library"),
    p(s("Generic 'Cache' folder", "other", false, "nama folder generic, cek manual"), "cache"),
    p(s("Generic 'Cache' folder", "other", false, "nama folder generic, cek manual"), "caches"),
    p(s("Generic temp folder", "other", false, "cek manual"), "temp"),
    p(s("Generic temp folder", "other", false, "cek manual"), "tmp"),
];

static PATTERN_IDX: LazyLock<HashMap<&'static str, Vec<&'static DirPattern>>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, Vec<&'static DirPattern>> = HashMap::new();
    for d in DIR_PATTERNS {
        m.entry(d.dirname).or_default().push(d);
    }
    m
});

pub fn match_pattern(lname: &str, parent_path: &Path, parent_lname: &str) -> Option<&'static Sig> {
    let cands = PATTERN_IDX.get(lname)?;
    for d in cands {
        if let Some(par) = d.parent {
            if par != parent_lname {
                continue;
            }
        }
        if let Some(mk) = d.marker {
            if !parent_path.join(mk).is_file() {
                continue;
            }
        }
        return Some(&d.sig);
    }
    None
}

// ===================================================================== resolve

fn env_map() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().map(|(k, v)| (k.to_uppercase(), v)).collect();
    let home = env
        .get("USERPROFILE")
        .or_else(|| env.get("HOME"))
        .cloned()
        .unwrap_or_else(|| "/".into());
    env.entry("HOME".into()).or_insert_with(|| home.clone());
    env.entry("USERPROFILE".into()).or_insert_with(|| home.clone());
    if cfg!(windows) {
        let local = format!("{home}\\AppData\\Local");
        env.entry("LOCALAPPDATA".into()).or_insert_with(|| local.clone());
        env.entry("APPDATA".into()).or_insert_with(|| format!("{home}\\AppData\\Roaming"));
        env.entry("WINDIR".into()).or_insert_with(|| "C:\\Windows".into());
        env.entry("PROGRAMDATA".into()).or_insert_with(|| "C:\\ProgramData".into());
        env.entry("PROGRAMFILES(X86)".into()).or_insert_with(|| "C:\\Program Files (x86)".into());
        env.entry("TEMP".into()).or_insert_with(|| format!("{local}\\Temp"));
    }
    env
}

fn expand(tpl: &str, env: &HashMap<String, String>) -> Option<String> {
    let mut out = tpl.to_string();
    while let Some(st) = out.find('{') {
        let en = st + out[st..].find('}')?;
        let key = out[st + 1..en].to_uppercase();
        let val = env.get(&key)?;
        out = format!("{}{}{}", &out[..st], val, &out[en + 1..]);
    }
    if cfg!(windows) {
        out = out.replace('/', "\\");
    }
    Some(out)
}

fn is_real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.is_dir() && !m.file_type().is_symlink()).unwrap_or(false)
}

/// Lokasi cache yang dikenal & benar-benar ada di sistem ini.
pub fn resolve_known_locations() -> Vec<(PathBuf, Sig)> {
    let env = env_map();
    let known = if cfg!(windows) { windows_known() } else { posix_known() };
    let mut out: std::collections::BTreeMap<String, (PathBuf, Sig)> = Default::default();
    let opts = glob::MatchOptions { case_sensitive: !cfg!(windows), ..Default::default() };
    for kl in &known {
        for tpl in &kl.paths {
            let Some(path) = expand(tpl, &env) else { continue };
            let mut found: Vec<PathBuf> = Vec::new();
            if tpl.contains(['*', '?', '[']) {
                if let Ok(it) = glob::glob_with(&path, opts) {
                    found.extend(it.flatten());
                }
            } else {
                found.push(PathBuf::from(&path));
            }
            for m in found {
                if is_real_dir(&m) {
                    let key = norm_path(&m);
                    out.entry(key).or_insert((m, kl.sig));
                }
            }
        }
    }
    out.into_values().collect()
}
