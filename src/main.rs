//! CacheChecker (Rust) — cek cache & total file di semua disk.
//!
//! Tanpa argumen  -> TUI interaktif.
//! Dengan subcommand -> mode CLI (cocok untuk script / scheduled task).

mod tui;

use cachechecker::{cleaner, drives, model, report, scanner, signatures, util};
use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use model::ScanResult;
use scanner::{Progress, ScanOptions};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use util::*;

#[derive(Parser)]
#[command(
    name = "cachechecker",
    version,
    about = "Cek cache & total file di semua disk — TUI interaktif, CLI, report & clean",
    after_help = "\
Tanpa argumen akan membuka TUI interaktif.

Contoh:
  cachechecker                               TUI interaktif
  cachechecker tui --cache                   TUI langsung quick cache scan
  cachechecker drives
  cachechecker cache                         cepat: cache yang dikenal saja
  cachechecker scan                          full scan semua disk lokal
  cachechecker scan -d C D -f html json      drive C & D, simpan report html+json
  cachechecker scan -p D:/Project -o r.html
  cachechecker clean                         preview cache aman lalu konfirmasi
  cachechecker clean -c browser dev --yes
  cachechecker clean --older-than 7d --yes
  cachechecker report hasil.json --tui       buka report lama di TUI
  cachechecker compare lama.json baru.json"
)]
struct Cli {
    /// Matikan warna
    #[arg(long, global = true)]
    no_color: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Args, Clone)]
struct Common {
    /// Jumlah thread
    #[arg(short = 't', long, default_value_t = scanner::default_threads())]
    threads: usize,
    /// Tanpa progress & ringkasan
    #[arg(short, long)]
    quiet: bool,
    /// Jumlah baris di ringkasan
    #[arg(long, default_value_t = 20)]
    top: usize,
    /// Abaikan cache lebih kecil dari ini, mis. 10MB
    #[arg(long, default_value = "0")]
    min_size: String,
}

#[derive(Args, Clone)]
struct Output {
    /// Simpan report (format dari ekstensi: .json .html .md .csv)
    #[arg(short, long, num_args = 1..)]
    output: Vec<PathBuf>,
    /// Simpan report dengan nama otomatis cachecheck_<waktu>.<fmt>
    #[arg(short, long, num_args = 1.., value_parser = ["json", "html", "md", "csv"])]
    format: Vec<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Buka TUI interaktif (default kalau tanpa argumen)
    Tui {
        /// Langsung jalankan quick cache scan
        #[arg(long)]
        cache: bool,
        /// Langsung jalankan full scan drive lokal
        #[arg(long)]
        full: bool,
        #[arg(short = 't', long, default_value_t = scanner::default_threads())]
        threads: usize,
    },
    /// Daftar disk & kapasitas
    Drives {
        /// Ikutkan network drive
        #[arg(long)]
        network: bool,
    },
    /// Full scan: total file, statistik, dan cache di semua disk
    Scan {
        /// Drive tertentu, mis. C D
        #[arg(short, long, num_args = 1..)]
        drive: Vec<String>,
        /// Scan folder tertentu, bukan drive
        #[arg(short, long, num_args = 1..)]
        path: Vec<PathBuf>,
        /// Exclude path/nama (glob), mis. *node_modules*
        #[arg(short = 'x', long, num_args = 1..)]
        exclude: Vec<String>,
        /// Ikutkan network drive
        #[arg(long)]
        network: bool,
        /// Ikutkan drive cloud (Google Drive/OneDrive virtual) — lambat & bisa memicu download
        #[arg(long)]
        cloud: bool,
        /// Tanpa deteksi cache
        #[arg(long)]
        no_cache: bool,
        /// Hanya lokasi cache yang dikenal (tanpa pola nama folder)
        #[arg(long)]
        no_patterns: bool,
        /// Ikutkan folder generic 'cache'/'temp'
        #[arg(long)]
        generic: bool,
        /// Jumlah file/folder terbesar yang disimpan
        #[arg(long, default_value_t = 100)]
        top_n: usize,
        /// Setelah selesai buka hasilnya di TUI
        #[arg(long)]
        tui: bool,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        out: Output,
    },
    /// Scan cepat lokasi cache yang dikenal
    Cache {
        /// Cari cache (pola) di folder ini saja
        #[arg(short, long, num_args = 1..)]
        path: Vec<PathBuf>,
        /// Ikutkan folder generic 'cache'/'temp'
        #[arg(long)]
        generic: bool,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        out: Output,
    },
    /// Bersihkan cache (default dry-run / konfirmasi)
    Clean {
        /// Pakai hasil report JSON (tanpa scan ulang)
        #[arg(short = 'r', long)]
        from_report: Option<PathBuf>,
        /// Cari cache (pola) di folder ini, mis. folder project
        #[arg(short, long, num_args = 1..)]
        path: Vec<PathBuf>,
        /// Kategori: system browser app dev gpu game ai trash ...
        #[arg(short, long, num_args = 1..)]
        category: Vec<String>,
        /// Filter nama cache (glob), mis. 'npm*' '*Chrome*'
        #[arg(short, long, num_args = 1..)]
        name: Vec<String>,
        /// Lewati nama cache (glob)
        #[arg(short, long, num_args = 1..)]
        skip: Vec<String>,
        /// Hanya hapus file lebih tua dari, mis. 7d 12h 2w
        #[arg(long)]
        older_than: Option<String>,
        /// Ikutkan cache berstatus 'cek' (hati-hati!)
        #[arg(long)]
        include_unsafe: bool,
        /// Kosongkan Recycle Bin juga (Windows)
        #[arg(long)]
        recycle_bin: bool,
        /// Paksa preview saja
        #[arg(long)]
        dry_run: bool,
        /// Hapus tanpa konfirmasi
        #[arg(short, long)]
        yes: bool,
        /// Tampilkan detail error
        #[arg(short, long)]
        verbose: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Tampilkan / konversi report JSON
    Report {
        input: PathBuf,
        /// Buka di TUI
        #[arg(long)]
        tui: bool,
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[command(flatten)]
        out: Output,
    },
    /// Bandingkan 2 report JSON
    Compare {
        old: PathBuf,
        new: PathBuf,
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Daftar signature cache yang dikenali
    Signatures,
}

// ===================================================================== helpers

/// Jalankan scan dengan spinner indicatif + Ctrl+C = batal (hasil parsial).
fn run_scan(opts: ScanOptions, show: bool) -> ScanResult {
    let progress = Arc::new(Progress::default());
    {
        let pg = progress.clone();
        let _ = ctrlc::set_handler(move || {
            if pg.cancel.swap(true, Ordering::Relaxed) {
                std::process::exit(130);
            }
        });
    }
    let show = show && std::io::stderr().is_terminal();
    let pb = if show {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("  {spinner:.magenta} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✔"]),
        );
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    } else {
        None
    };
    let pg = progress.clone();
    let handle = std::thread::spawn(move || scanner::run(&opts, &pg));
    let start = std::time::Instant::now();
    while !handle.is_finished() {
        if let Some(pb) = &pb {
            let files = progress.files.load(Ordering::Relaxed);
            let el = start.elapsed().as_secs_f64().max(0.001);
            let cur = progress.current.lock().map(|c| c.clone()).unwrap_or_default();
            pb.set_message(format!(
                "{} file │ {} │ {} cache │ {:.0} f/s │ {} │ {}",
                paint(fmt_num(files), C_ACCENT),
                paint(human_size(progress.size.load(Ordering::Relaxed)), C_CYAN),
                paint(progress.caches.load(Ordering::Relaxed).to_string(), C_YELLOW),
                files as f64 / el,
                fmt_duration(el),
                paint(trunc_left(&cur, 50), C_DIM)
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
    handle.join().expect("scan thread panic")
}

fn write_outputs(r: &ScanResult, out: &Output) -> Result<()> {
    let mut files = Vec::new();
    for o in &out.output {
        files.extend(report::export(r, o, None)?);
    }
    for f in &out.format {
        files.extend(report::export(r, &report::default_report_name(f), Some(f))?);
    }
    for f in files {
        println!("  {} {}", paint("✔ Report disimpan:", C_GREEN), abs_path(&f).display());
    }
    Ok(())
}

fn glob_match(pat: &str, s: &str) -> bool {
    glob::Pattern::new(&pat.to_lowercase()).map(|p| p.matches(&s.to_lowercase())).unwrap_or(false)
}

// ===================================================================== commands

fn cmd_drives(network: bool) -> Result<()> {
    let drives = drives::list_drives(network, true);
    println!();
    let rows = drives
        .iter()
        .map(|d| {
            let pct = d.percent();
            let col = if pct >= 90.0 { C_RED } else if pct >= 75.0 { C_YELLOW } else { C_GREEN };
            let b = bar(pct / 100.0, 24);
            let pad = 24 - b.chars().count();
            vec![
                Cell::c(&d.mount, C_CYAN),
                Cell::p(trunc_right(if d.label.is_empty() { "-" } else { &d.label }, 30)),
                Cell::c(&d.fstype, C_DIM),
                Cell::c(&d.kind, if d.kind == "cloud" { C_PURPLE } else { C_DIM }),
                Cell::p(human_size(d.total)),
                Cell::p(human_size(d.used)),
                Cell::c(human_size(d.free), C_GREEN),
                Cell::c(format!("{pct:.1}%"), col),
                Cell::c(format!("{b}{}", "░".repeat(pad)), col),
            ]
        })
        .collect();
    print_table(&["Drive", "Label", "FS", "Tipe", "Total", "Terpakai", "Free", "%", ""], rows, "llllrrrrl", &[]);
    let tot: u64 = drives.iter().map(|d| d.total).sum();
    let used: u64 = drives.iter().map(|d| d.used).sum();
    println!(
        "\n  Total {} / {} terpakai · free {}\n",
        bold(human_size(used)),
        human_size(tot),
        paint(human_size(tot.saturating_sub(used)), C_GREEN)
    );
    Ok(())
}

fn cache_scan(threads: usize, paths: &[PathBuf], generic: bool, min_size: u64, show: bool) -> ScanResult {
    run_scan(scanner::cache_only_options(threads, paths, generic, min_size), show)
}

fn cmd_clean(
    from_report: Option<PathBuf>,
    path: Vec<PathBuf>,
    category: Vec<String>,
    name: Vec<String>,
    skip: Vec<String>,
    older_than: Option<String>,
    include_unsafe: bool,
    recycle_bin: bool,
    dry_run: bool,
    yes: bool,
    verbose: bool,
    common: Common,
) -> Result<()> {
    let min = parse_size(&common.min_size).map_err(anyhow::Error::msg)?;
    let older = older_than.as_deref().map(parse_age).transpose().map_err(anyhow::Error::msg)?;
    let r = if let Some(p) = &from_report {
        println!("{}", paint(format!("  Pakai report: {}", p.display()), C_DIM));
        report::load_json(p)?
    } else {
        println!("  Scan lokasi cache…");
        cache_scan(common.threads, &path, false, 0, !common.quiet)
    };

    let mut targets: Vec<&model::CacheHit> = r
        .caches
        .iter()
        .filter(|c| include_unsafe || c.safe)
        .filter(|c| category.is_empty() || category.contains(&c.category))
        .filter(|c| name.is_empty() || name.iter().any(|n| glob_match(n, &c.name)))
        .filter(|c| !skip.iter().any(|n| glob_match(n, &c.name)))
        .filter(|c| c.size >= min && c.size > 0)
        .collect();
    // buang target nested
    targets.sort_by_key(|c| c.path.len());
    let sep = std::path::MAIN_SEPARATOR;
    let key = |p: &str| format!("{}{sep}", p.to_lowercase().trim_end_matches(sep));
    let mut fin: Vec<&model::CacheHit> = Vec::new();
    for c in targets {
        if !fin.iter().any(|f| key(&c.path).starts_with(&key(&f.path))) {
            fin.push(c);
        }
    }
    fin.sort_by(|a, b| b.size.cmp(&a.size));
    if fin.is_empty() {
        println!("{}", paint("  Tidak ada cache yang cocok dengan filter.", C_YELLOW));
        return Ok(());
    }

    println!();
    let rows = fin
        .iter()
        .enumerate()
        .map(|(i, c)| {
            vec![
                Cell::c((i + 1).to_string(), C_DIM),
                Cell::p(trunc_right(&c.name, 28)),
                Cell::c(&c.category, cat_code(&c.category)),
                if c.safe { Cell::c("✔ aman", C_GREEN) } else { Cell::c("● cek", C_YELLOW) },
                Cell::p(fmt_num(c.files)),
                Cell::c(human_size(c.size), "1"),
                Cell::c(&c.path, C_DIM),
            ]
        })
        .collect();
    print_table(&["#", "Nama", "Kategori", "Status", "Files", "Size", "Path"], rows, "rlllrrl", &[(6, 64)]);
    let total: u64 = fin.iter().map(|c| c.size).sum();
    println!(
        "\n  Target: {} lokasi, ~{}{}",
        fin.len(),
        paint(human_size(total), "1;38;2;247;118;142"),
        older_than.as_ref().map(|o| format!(" (hanya file lebih tua dari {o})")).unwrap_or_default()
    );

    let mut dry = !yes || dry_run;
    if !yes && !dry_run {
        if std::io::stdin().is_terminal() {
            print!("\n  {} ketik 'ya' untuk lanjut, Enter = dry-run: ", bold("Hapus sekarang?"));
            std::io::stdout().flush()?;
            let mut ans = String::new();
            std::io::stdin().read_line(&mut ans)?;
            dry = !matches!(ans.trim().to_lowercase().as_str(), "ya" | "y" | "yes");
        } else {
            println!("{}", paint("  Mode non-interaktif: pakai --yes untuk menghapus. Menjalankan dry-run.", C_YELLOW));
        }
    }

    println!(
        "\n  {}\n",
        if dry { paint("DRY-RUN — tidak ada yang dihapus", C_CYAN) } else { paint("MENGHAPUS…", C_RED) }
    );
    let (mut freed, mut files, mut failed) = (0u64, 0u64, 0u64);
    for c in &fin {
        let res = cleaner::clean_cache(Path::new(&c.path), older, dry, None);
        freed += res.freed;
        files += res.files;
        failed += res.failed;
        let status = if res.failed == 0 { paint(" ✔ ", C_GREEN) } else { paint(format!(" ! {} gagal", res.failed), C_YELLOW) };
        let extra = if res.skipped_new > 0 {
            paint(format!(" ({} file baru dilewati)", fmt_num(res.skipped_new)), C_DIM)
        } else {
            String::new()
        };
        println!("  {status} {:<32} {:>10}  {:>9} file{extra}", trunc_right(&c.name, 32), human_size(res.freed), fmt_num(res.files));
        if verbose {
            for e in res.errors.iter().take(5) {
                println!("{}", paint(format!("        {e}"), C_DIM));
            }
        }
    }
    if recycle_bin {
        if dry {
            println!("  {} Recycle Bin akan dikosongkan", paint(" DRY ", C_CYAN));
        } else {
            let ok = cleaner::empty_recycle_bin();
            println!("  {} Recycle Bin dikosongkan", if ok { paint(" ✔ ", C_GREEN) } else { paint(" ? ", C_YELLOW) });
        }
    }
    println!(
        "\n  {}: {} dari {} file{}",
        bold(if dry { "Akan dibebaskan" } else { "Dibebaskan" }),
        paint(human_size(freed), "1;38;2;158;206;106"),
        fmt_num(files),
        if failed > 0 { paint(format!(", {} gagal (file terkunci / butuh admin)", fmt_num(failed)), C_YELLOW) } else { String::new() }
    );
    if dry {
        println!("{}", paint("  Jalankan ulang dengan --yes untuk menghapus beneran.", C_DIM));
    }
    println!();
    Ok(())
}

fn cmd_signatures() {
    section_title("Lokasi cache yang dikenal & ADA di sistem ini");
    let rows = signatures::resolve_known_locations()
        .into_iter()
        .map(|(p, s)| {
            vec![
                Cell::p(s.name),
                Cell::c(s.category, cat_code(s.category)),
                if s.safe { Cell::c("✔ aman", C_GREEN) } else { Cell::c("● cek", C_YELLOW) },
                Cell::c(p.to_string_lossy(), C_DIM),
            ]
        })
        .collect();
    print_table(&["Nama", "Kategori", "Status", "Path"], rows, "llll", &[(3, 80)]);
    section_title("Pola folder (dideteksi saat full scan)");
    let rows = signatures::DIR_PATTERNS
        .iter()
        .map(|p| {
            let mut cond = vec![];
            if let Some(par) = p.parent {
                cond.push(format!("parent={par}"));
            }
            if let Some(m) = p.marker {
                cond.push(format!("ada {m}"));
            }
            vec![
                Cell::c(p.dirname, C_CYAN),
                Cell::p(p.sig.name),
                Cell::c(p.sig.category, cat_code(p.sig.category)),
                if p.sig.safe { Cell::c("✔ aman", C_GREEN) } else { Cell::c("● cek", C_YELLOW) },
                Cell::c(cond.join(", "), C_DIM),
            ]
        })
        .collect();
    print_table(&["Folder", "Nama", "Kategori", "Status", "Syarat"], rows, "lllll", &[]);
    println!();
}

fn section_title(s: &str) {
    println!("\n{} {}", paint("▌", C_PURPLE), paint(s, "1;38;2;192;202;245"));
}

// ===================================================================== main

fn main() {
    let cli = Cli::parse();
    init_console(cli.no_color);
    if let Err(e) = real_main(cli) {
        eprintln!("{} {e:#}", paint("  Error:", C_RED));
        std::process::exit(2);
    }
}

fn real_main(cli: Cli) -> Result<()> {
    let Some(cmd) = cli.cmd else {
        return tui::run(None, scanner::default_threads(), None);
    };
    match cmd {
        Cmd::Tui { cache, full, threads } => {
            let start = if cache { Some("cache") } else if full { Some("full") } else { None };
            tui::run(None, threads, start)
        }
        Cmd::Drives { network } => cmd_drives(network),
        Cmd::Scan { drive, path, exclude, network, cloud, no_cache, no_patterns, generic, top_n, tui, common, out } => {
            let roots: Vec<PathBuf> = if !path.is_empty() {
                path.iter().map(|p| abs_path(p)).collect()
            } else {
                let all = drives::list_drives(network, cloud || !drive.is_empty());
                let sel = drives::filter_drives(all, &drive);
                if !drive.is_empty() && sel.is_empty() {
                    bail!("drive tidak ditemukan: {}", drive.join(", "));
                }
                sel.into_iter().map(|d| PathBuf::from(d.mount)).collect()
            };
            if roots.is_empty() {
                bail!("tidak ada root untuk di-scan");
            }
            if !common.quiet {
                report::banner();
                if !is_admin() {
                    let hint = if cfg!(windows) { "Administrator" } else { "root (sudo)" };
                    println!("{}", paint(format!("  (info) Tidak berjalan sebagai {hint} — beberapa folder sistem akan dilewati."), C_DIM));
                }
                println!(
                    "  Scanning {} dengan {} thread… {}",
                    bold(roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ")),
                    common.threads,
                    paint("(Ctrl+C untuk berhenti)", C_DIM)
                );
            }
            let opts = ScanOptions {
                roots,
                threads: common.threads,
                top_n,
                detect_cache: !no_cache,
                detect_patterns: !no_patterns,
                include_generic: generic,
                excludes: exclude,
                min_cache_size: parse_size(&common.min_size).map_err(anyhow::Error::msg)?,
                ..Default::default()
            };
            let r = run_scan(opts, !common.quiet);
            write_outputs(&r, &out)?;
            if tui {
                return tui::run(Some(r), common.threads, None);
            }
            if !common.quiet {
                report::print_summary(&r, common.top, true);
            }
            Ok(())
        }
        Cmd::Cache { path, generic, common, out } => {
            if !common.quiet {
                report::banner();
                println!("  Scan lokasi cache yang dikenal… {}", paint("(Ctrl+C untuk berhenti)", C_DIM));
            }
            let min = parse_size(&common.min_size).map_err(anyhow::Error::msg)?;
            let r = cache_scan(common.threads, &path, generic, min, !common.quiet);
            if !common.quiet {
                report::print_summary(&r, common.top, false);
            }
            write_outputs(&r, &out)
        }
        Cmd::Clean { from_report, path, category, name, skip, older_than, include_unsafe, recycle_bin, dry_run, yes, verbose, common } => {
            cmd_clean(from_report, path, category, name, skip, older_than, include_unsafe, recycle_bin, dry_run, yes, verbose, common)
        }
        Cmd::Report { input, tui, top, out } => {
            let r = report::load_json(&input)?;
            if tui {
                return tui::run(Some(r), scanner::default_threads(), None);
            }
            if out.output.is_empty() && out.format.is_empty() {
                report::print_summary(&r, top, true);
                return Ok(());
            }
            write_outputs(&r, &out)
        }
        Cmd::Compare { old, new, top } => {
            report::print_compare(&report::load_json(&old)?, &report::load_json(&new)?, top);
            Ok(())
        }
        Cmd::Signatures => {
            cmd_signatures();
            Ok(())
        }
    }
}
