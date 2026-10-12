//! Helper umum: format ukuran/waktu, warna terminal, tabel teks, path.

use chrono::{Local, TimeZone};
use std::io::IsTerminal;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ------------------------------------------------------------------ warna CLI (ANSI truecolor)

pub static COLOR: AtomicBool = AtomicBool::new(true);

pub const C_ACCENT: &str = "38;2;122;162;247";
pub const C_PURPLE: &str = "38;2;187;154;247";
pub const C_GREEN: &str = "38;2;158;206;106";
pub const C_YELLOW: &str = "38;2;224;175;104";
pub const C_RED: &str = "38;2;247;118;142";
pub const C_CYAN: &str = "38;2;125;207;255";
pub const C_ORANGE: &str = "38;2;255;158;100";
pub const C_TEAL: &str = "38;2;115;218;202";
pub const C_DIM: &str = "38;2;105;114;156";
pub const C_HEAD: &str = "1;38;2;122;162;247";

pub fn paint(s: impl AsRef<str>, code: &str) -> String {
    if COLOR.load(Ordering::Relaxed) && !code.is_empty() {
        format!("\x1b[{}m{}\x1b[0m", code, s.as_ref())
    } else {
        s.as_ref().to_string()
    }
}

pub fn bold(s: impl AsRef<str>) -> String {
    paint(s, "1")
}

pub fn init_console(no_color: bool) {
    let on = !no_color && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    COLOR.store(on, Ordering::Relaxed);
    #[cfg(windows)]
    enable_vt();
}

#[cfg(windows)]
fn enable_vt() {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        for h in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(h);
            let mut mode = 0u32;
            if GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}

/// Kode warna ANSI per kategori cache (CLI).
pub fn cat_code(cat: &str) -> &'static str {
    match cat {
        "system" => C_CYAN,
        "browser" => C_ACCENT,
        "app" => C_PURPLE,
        "dev" => C_GREEN,
        "dev-artifact" => C_TEAL,
        "gpu" => C_ORANGE,
        "game" => C_RED,
        "ai" => C_YELLOW,
        _ => C_DIM,
    }
}

// ------------------------------------------------------------------ ukuran / angka / waktu

pub fn human_size(n: u64) -> String {
    const U: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", n)
    } else {
        format!("{:.2} {}", v, U[i])
    }
}

pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "500MB", "1.5G", "200k", "0" -> bytes
pub fn parse_size(s: &str) -> Result<u64, String> {
    let up = s.trim().to_ascii_uppercase();
    let t: &str = up.strip_suffix("IB").or_else(|| up.strip_suffix('B')).unwrap_or(&up);
    let (num, mult) = match t.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => {
            let m: u64 = match c {
                'K' => 1 << 10,
                'M' => 1 << 20,
                'G' => 1 << 30,
                'T' => 1 << 40,
                'P' => 1 << 50,
                _ => return Err(format!("format ukuran tidak valid: {s:?} (contoh: 100MB, 1.5GB)")),
            };
            (&t[..t.len() - 1], m)
        }
        _ => (t, 1),
    };
    let v: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("format ukuran tidak valid: {s:?} (contoh: 100MB, 1.5GB)"))?;
    Ok((v * mult as f64) as u64)
}

/// "7d", "12h", "30m", "2w" -> detik. Angka polos = hari.
pub fn parse_age(s: &str) -> Result<f64, String> {
    let t = s.trim().to_ascii_lowercase();
    let (num, mult) = match t.chars().last() {
        Some('s') => (&t[..t.len() - 1], 1.0),
        Some('m') => (&t[..t.len() - 1], 60.0),
        Some('h') => (&t[..t.len() - 1], 3600.0),
        Some('d') => (&t[..t.len() - 1], 86400.0),
        Some('w') => (&t[..t.len() - 1], 604800.0),
        _ => (t.as_str(), 86400.0),
    };
    let v: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("format umur tidak valid: {s:?} (contoh: 7d, 12h)"))?;
    Ok(v * mult)
}

pub fn fmt_time(ts: f64) -> String {
    if ts <= 0.0 {
        return "-".into();
    }
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".into())
}

pub fn fmt_duration(sec: f64) -> String {
    if sec < 60.0 {
        return format!("{:.1}s", sec);
    }
    let total = sec as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else {
        format!("{m}m{s:02}s")
    }
}

pub fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

pub fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".into())
}

pub fn is_admin() -> bool {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0
    }
    #[cfg(unix)]
    unsafe {
        libc::geteuid() == 0
    }
    #[cfg(not(any(windows, unix)))]
    false
}

// ------------------------------------------------------------------ path

/// Key path yang konsisten untuk perbandingan (lowercase di Windows, tanpa trailing separator).
#[cfg(windows)]
pub fn norm_path(p: &Path) -> String {
    let mut s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

#[cfg(not(windows))]
pub fn norm_path(p: &Path) -> String {
    let mut s = p.to_string_lossy().into_owned();
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

pub fn abs_path(p: &Path) -> std::path::PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

// ------------------------------------------------------------------ teks

pub fn trunc_left(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w || w < 2 {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - (w - 1)).collect();
    format!("…{tail}")
}

pub fn trunc_right(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w || w < 2 {
        return s.to_string();
    }
    let head: String = s.chars().take(w - 1).collect();
    format!("{head}…")
}

/// Bar horizontal dengan resolusi 1/8 karakter.
pub fn bar(ratio: f64, width: usize) -> String {
    const PART: [&str; 9] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
    let r = if ratio.is_finite() { ratio.clamp(0.0, 1.0) } else { 0.0 };
    let total = r * width as f64;
    let full = total.floor() as usize;
    let rem = ((total - full as f64) * 8.0).round() as usize;
    let mut s = "█".repeat(full);
    if full < width && rem > 0 {
        s.push_str(PART[rem]);
    }
    if r > 0.0 && s.is_empty() {
        s.push('▏');
    }
    s
}

// ------------------------------------------------------------------ tabel CLI

pub struct Cell {
    pub text: String,
    pub code: &'static str,
}

impl Cell {
    pub fn p(t: impl Into<String>) -> Self {
        Cell { text: t.into(), code: "" }
    }
    pub fn c(t: impl Into<String>, code: &'static str) -> Self {
        Cell { text: t.into(), code }
    }
}

/// align: string 'l'/'r' per kolom. maxw: (kolom, lebar maks) -> dipotong dari kiri.
pub fn print_table(headers: &[&str], mut rows: Vec<Vec<Cell>>, align: &str, maxw: &[(usize, usize)]) {
    for r in rows.iter_mut() {
        for &(i, w) in maxw {
            if let Some(c) = r.get_mut(i) {
                if c.text.chars().count() > w {
                    c.text = trunc_left(&c.text, w);
                }
            }
        }
    }
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(c.text.chars().count());
            }
        }
    }
    let al = |i: usize| align.as_bytes().get(i).copied().unwrap_or(b'l');
    let fmt_cell = |text: &str, i: usize| -> String {
        let pad = widths[i].saturating_sub(text.chars().count());
        if al(i) == b'r' {
            format!("{}{}", " ".repeat(pad), text)
        } else {
            format!("{}{}", text, " ".repeat(pad))
        }
    };
    let head: Vec<String> = headers.iter().enumerate().map(|(i, h)| paint(fmt_cell(h, i), C_HEAD)).collect();
    println!("  {}", head.join("  "));
    let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
    println!("  {}", paint(sep.join("  "), C_DIM));
    for r in &rows {
        let cells: Vec<String> = r.iter().enumerate().map(|(i, c)| paint(fmt_cell(&c.text, i), c.code)).collect();
        println!("  {}", cells.join("  ").trim_end());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_dan_format() {
        assert_eq!(parse_size("100MB").unwrap(), 100 << 20);
        assert_eq!(parse_size("1.5G").unwrap(), (1.5 * (1u64 << 30) as f64) as u64);
        assert_eq!(parse_size("200k").unwrap(), 200 << 10);
        assert_eq!(parse_size("0").unwrap(), 0);
        assert!(parse_size("abc").is_err());
        assert_eq!(parse_age("7d").unwrap(), 7.0 * 86400.0);
        assert_eq!(parse_age("12h").unwrap(), 43200.0);
        assert_eq!(parse_age("3").unwrap(), 3.0 * 86400.0);
        assert_eq!(human_size(1536), "1.50 KB");
        assert_eq!(fmt_num(1234567), "1,234,567");
        assert_eq!(bar(1.0, 4), "████");
        assert_eq!(bar(0.0, 4), "");
    }
}
