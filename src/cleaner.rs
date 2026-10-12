//! Pembersih cache. Yang dihapus ISI folder cache (folder induk dipertahankan).

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default, Clone)]
pub struct CleanResult {
    pub freed: u64,
    pub files: u64,
    pub failed: u64,
    pub skipped_new: u64,
    pub errors: Vec<String>,
}

/// Counter live supaya UI bisa menampilkan progress hapus.
#[derive(Default)]
pub struct CleanProgress {
    pub freed: AtomicU64,
    pub files: AtomicU64,
}

fn remove_file_force(p: &Path) -> std::io::Result<()> {
    match fs::remove_file(p) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let md = fs::symlink_metadata(p)?;
            let mut perm = md.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            fs::set_permissions(p, perm)?;
            fs::remove_file(p)
        }
        r => r,
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
            return true;
        }
    }
    false
}

/// Return true kalau folder jadi kosong.
fn clean_dir(dir: &Path, res: &mut CleanResult, older: Option<f64>, now: f64, dry: bool, pg: Option<&CleanProgress>) -> bool {
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            res.failed += 1;
            res.errors.push(format!("{}: {}", dir.display(), e));
            return false;
        }
    };
    let mut empty = true;
    for ent in rd.flatten() {
        let path = ent.path();
        let md = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                res.failed += 1;
                empty = false;
                if res.errors.len() < 20 {
                    res.errors.push(format!("{}: {}", path.display(), e));
                }
                continue;
            }
        };
        if is_link(&md) {
            // link/junction: hapus link-nya saja, jangan ikuti target
            if !dry {
                let r = if md.is_dir() { fs::remove_dir(&path) } else { fs::remove_file(&path) };
                if r.is_err() {
                    empty = false;
                }
            }
            continue;
        }
        if md.is_dir() {
            let sub_empty = clean_dir(&path, res, older, now, dry, pg);
            if sub_empty && !dry {
                if fs::remove_dir(&path).is_err() {
                    empty = false;
                }
            } else if !sub_empty {
                empty = false;
            }
            continue;
        }
        if let Some(age) = older {
            let mt = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            if now - mt < age {
                res.skipped_new += 1;
                empty = false;
                continue;
            }
        }
        let ok = if dry { Ok(()) } else { remove_file_force(&path) };
        match ok {
            Ok(()) => {
                res.freed += md.len();
                res.files += 1;
                if let Some(p) = pg {
                    p.freed.fetch_add(md.len(), Ordering::Relaxed);
                    p.files.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                res.failed += 1;
                empty = false;
                if res.errors.len() < 20 {
                    res.errors.push(format!("{}: {}", path.display(), e));
                }
            }
        }
    }
    empty
}

pub fn clean_cache(path: &Path, older_than: Option<f64>, dry: bool, pg: Option<&CleanProgress>) -> CleanResult {
    let mut res = CleanResult::default();
    if path.is_dir() {
        clean_dir(path, &mut res, older_than, crate::util::now_secs(), dry, pg);
    }
    res
}

/// Kosongkan Recycle Bin Windows (semua drive).
pub fn empty_recycle_bin() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Shell::*;
        let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
        let hr = SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), flags);
        hr == 0 || hr == 0x8000FFFFu32 as i32 // S_OK / E_UNEXPECTED (sudah kosong)
    }
    #[cfg(not(windows))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_hapus_isi_tapi_folder_induk_tetap() {
        let base = std::env::temp_dir().join("cachechecker_clean_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("cache/sub/deep")).unwrap();
        fs::write(base.join("cache/a.bin"), vec![1u8; 1000]).unwrap();
        fs::write(base.join("cache/sub/b.bin"), vec![1u8; 500]).unwrap();
        fs::write(base.join("cache/sub/deep/c.bin"), vec![1u8; 250]).unwrap();
        // read-only file juga harus terhapus
        let ro = base.join("cache/ro.bin");
        fs::write(&ro, vec![1u8; 50]).unwrap();
        let mut p = fs::metadata(&ro).unwrap().permissions();
        p.set_readonly(true);
        fs::set_permissions(&ro, p).unwrap();

        // dry-run: tidak ada yang hilang
        let d = clean_cache(&base.join("cache"), None, true, None);
        assert_eq!((d.freed, d.files), (1800, 4));
        assert!(base.join("cache/sub/deep/c.bin").exists());

        // older_than besar: semua file "baru" -> dilewati
        let o = clean_cache(&base.join("cache"), Some(3600.0), false, None);
        assert_eq!((o.files, o.skipped_new), (0, 4));

        // hapus beneran
        let r = clean_cache(&base.join("cache"), None, false, None);
        assert_eq!((r.freed, r.files, r.failed), (1800, 4, 0));
        assert!(base.join("cache").is_dir(), "folder induk harus tetap ada");
        assert_eq!(fs::read_dir(base.join("cache")).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&base);
    }
}
