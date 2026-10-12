//! Deteksi disk / partisi (Windows, Linux, macOS).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriveInfo {
    pub mount: String,
    pub label: String,
    pub fstype: String,
    pub kind: String,
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

impl DriveInfo {
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64 * 100.0
        }
    }
}

const CLOUD_HINTS: &[&str] = &["google drive", "onedrive", "dropbox", "icloud", "mega", "pcloud", "box drive"];

/// Drive virtual cloud (Google Drive for Desktop dll) - scan lambat & bisa memicu download.
pub fn is_cloud(d: &DriveInfo) -> bool {
    let l = d.label.to_lowercase();
    CLOUD_HINTS.iter().any(|h| l.contains(h))
}

#[cfg(windows)]
fn platform_drives() -> Vec<DriveInfo> {
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn from_wide(b: &[u16]) -> String {
        let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf16_lossy(&b[..n])
    }

    let mut out = Vec::new();
    unsafe {
        let mask = GetLogicalDrives();
        let old = SetErrorMode(SEM_FAILCRITICALERRORS); // cegah popup "insert disk"
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let root = format!("{}:\\", (b'A' + i as u8) as char);
            let w = wide(&root);
            let dtype = GetDriveTypeW(w.as_ptr());
            // 0 unknown, 1 no root, 5 cdrom
            if matches!(dtype, 0 | 1 | 5) {
                continue;
            }
            let mut label = [0u16; 261];
            let mut fs = [0u16; 261];
            let ok = GetVolumeInformationW(
                w.as_ptr(),
                label.as_mut_ptr(),
                261,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                261,
            ) != 0;
            let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
            if GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) == 0 {
                continue;
            }
            let kind = match dtype {
                2 => "removable",
                3 => "fixed",
                4 => "network",
                6 => "ramdisk",
                _ => "unknown",
            };
            out.push(DriveInfo {
                mount: root,
                label: if ok { from_wide(&label) } else { String::new() },
                fstype: if ok { from_wide(&fs) } else { String::new() },
                kind: kind.into(),
                total,
                used: total.saturating_sub(free),
                free,
            });
        }
        SetErrorMode(old);
    }
    out
}

#[cfg(unix)]
fn statvfs(path: &str) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path).ok()?;
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        let f = st.f_frsize as u64;
        Some((st.f_blocks as u64 * f, st.f_bavail as u64 * f))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_drives() -> Vec<DriveInfo> {
    const FS: &[&str] = &[
        "ext2", "ext3", "ext4", "xfs", "btrfs", "zfs", "ntfs", "ntfs3", "vfat", "exfat", "f2fs", "fuseblk",
        "reiserfs", "jfs", "hfsplus", "apfs",
    ];
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let text = std::fs::read_to_string("/proc/mounts").unwrap_or_else(|_| "rootfs / ext4 rw 0 0".into());
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let (dev, mnt, fs) = (parts[0], parts[1].replace("\\040", " "), parts[2]);
        if !FS.contains(&fs) || !seen.insert(dev.to_string()) {
            continue;
        }
        if ["/proc", "/sys", "/dev", "/snap", "/boot/efi"].iter().any(|p| mnt.starts_with(p)) {
            continue;
        }
        if let Some((total, free)) = statvfs(&mnt) {
            out.push(DriveInfo {
                mount: mnt.clone(),
                label: dev.rsplit('/').next().unwrap_or(dev).into(),
                fstype: fs.into(),
                kind: "fixed".into(),
                total,
                used: total.saturating_sub(free),
                free,
            });
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn platform_drives() -> Vec<DriveInfo> {
    let mut mounts = vec!["/".to_string()];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                mounts.push(e.path().to_string_lossy().into());
            }
        }
    }
    mounts
        .into_iter()
        .filter_map(|m| {
            let (total, free) = statvfs(&m)?;
            Some(DriveInfo {
                label: m.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("root").into(),
                mount: m,
                fstype: String::new(),
                kind: "fixed".into(),
                total,
                used: total.saturating_sub(free),
                free,
            })
        })
        .collect()
}

pub fn list_drives(include_network: bool, include_cloud: bool) -> Vec<DriveInfo> {
    let mut drives = platform_drives();
    for d in drives.iter_mut() {
        if is_cloud(d) {
            d.kind = "cloud".into();
        }
    }
    drives.retain(|d| (include_network || d.kind != "network") && (include_cloud || d.kind != "cloud"));
    drives
}

/// Selector: "C", "C:", "D:\", "/home"
pub fn filter_drives(drives: Vec<DriveInfo>, selectors: &[String]) -> Vec<DriveInfo> {
    if selectors.is_empty() {
        return drives;
    }
    let key = |s: &str| -> String {
        if cfg!(windows) {
            s.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()
        } else {
            let t = s.trim_end_matches('/');
            if t.is_empty() { "/".into() } else { t.into() }
        }
    };
    let want: std::collections::HashSet<String> = selectors.iter().map(|s| key(s.trim())).collect();
    drives.into_iter().filter(|d| want.contains(&key(&d.mount))).collect()
}
