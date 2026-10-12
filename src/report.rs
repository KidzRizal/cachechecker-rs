//! Output report: console (CLI berwarna), JSON, CSV, Markdown, HTML, compare.

use crate::model::*;
use crate::util::*;
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        "0.0%".into()
    } else {
        format!("{:.1}%", part as f64 / whole as f64 * 100.0)
    }
}

/// Ringkas daftar root (mode cache bisa > 100 lokasi).
pub fn short_roots(roots: &[String]) -> String {
    if roots.len() > 3 {
        format!("{} lokasi cache yang dikenal", roots.len())
    } else {
        roots.join(", ")
    }
}

// ===================================================================== banner

pub fn banner() {
    let lines = [
        r"   ____           _          ____ _               _             ",
        r"  / ___|__ _  ___| |__   ___/ ___| |__   ___  ___| | _____ _ __ ",
        r" | |   / _` |/ __| '_ \ / _ \ |   | '_ \ / _ \/ __| |/ / _ \ '__|",
        r" | |__| (_| | (__| | | |  __/ |___| | | |  __/ (__|   <  __/ |   ",
        r"  \____\__,_|\___|_| |_|\___|\____|_| |_|\___|\___|_|\_\___|_|   ",
    ];
    // gradient biru -> ungu
    let (a, b) = ((122.0, 162.0, 247.0), (187.0, 154.0, 247.0));
    for (i, l) in lines.iter().enumerate() {
        let t = i as f64 / (lines.len() - 1) as f64;
        let c = format!(
            "1;38;2;{};{};{}",
            (a.0 + (b.0 - a.0) * t) as u8,
            (a.1 + (b.1 - a.1) * t) as u8,
            (a.2 + (b.2 - a.2) * t) as u8
        );
        println!("{}", paint(l, Box::leak(c.into_boxed_str())));
    }
    println!("{}", paint(format!("  v{}  ·  disk cache auditor  ·  rust edition\n", env!("CARGO_PKG_VERSION")), C_DIM));
}

fn section(title: &str) {
    println!("\n{} {}", paint("▌", C_PURPLE), paint(title, "1;38;2;192;202;245"));
}

fn kv(k: &str, v: String) {
    println!("  {}  {}", paint(format!("{k:<12}"), C_DIM), v);
}

// ===================================================================== console summary

pub fn print_summary(r: &ScanResult, top: usize, show_stats: bool) {
    let (m, t) = (&r.meta, &r.totals);
    println!();
    println!("{}", paint("╭──────────────────────────────────────────────────────────────────────╮", C_ACCENT));
    let title = format!(" CACHECHECKER REPORT · {} · {}", fmt_time(m.started), m.host);
    println!("{}{}{}", paint("│", C_ACCENT), paint(format!("{title:<70}"), "1"), paint("│", C_ACCENT));
    println!("{}", paint("╰──────────────────────────────────────────────────────────────────────╯", C_ACCENT));
    if m.cancelled {
        println!("{}", paint("  ⚠ Scan dibatalkan (Ctrl+C) — hasil parsial", C_YELLOW));
    }
    let roots = short_roots(&m.roots);
    kv("Root", roots);
    kv("Durasi", format!("{}  ({} thread)", fmt_duration(m.elapsed), m.threads));
    kv("Total file", format!("{}   {} folder", bold(fmt_num(t.files)), fmt_num(t.dirs)));
    kv("Total size", bold(human_size(t.size)));
    kv(
        "Cache",
        format!(
            "{} di {} lokasi ({} file, {} dari total)",
            paint(human_size(t.cache_size), "1;38;2;224;175;104"),
            t.cache_count,
            fmt_num(t.cache_files),
            pct(t.cache_size, t.size)
        ),
    );
    kv("Aman hapus", paint(human_size(t.cache_safe_size), "1;38;2;158;206;106"));
    if t.errors > 0 {
        kv("Error akses", format!("{} {}", paint(fmt_num(t.errors), C_RED), paint("(jalankan sebagai Administrator untuk akses penuh)", C_DIM)));
    }

    if !r.drives.is_empty() {
        section("Per Drive");
        let rows = r
            .drives
            .iter()
            .map(|d| vec![Cell::c(&d.root, C_CYAN), Cell::p(fmt_num(d.files)), Cell::p(fmt_num(d.dirs)), Cell::p(human_size(d.size))])
            .collect();
        print_table(&["Drive/Root", "Files", "Folders", "Size"], rows, "lrrr", &[(0, 50)]);
    }

    if !r.cache_by_category.is_empty() {
        section("Cache per Kategori");
        let mx = r.cache_by_category.first().map(|c| c.size).unwrap_or(1);
        let rows = r
            .cache_by_category
            .iter()
            .map(|c| {
                vec![
                    Cell::c(&c.category, cat_code(&c.category)),
                    Cell::p(c.count.to_string()),
                    Cell::p(fmt_num(c.files)),
                    Cell::p(human_size(c.size)),
                    Cell::c(human_size(c.safe_size), C_GREEN),
                    Cell::c(bar(c.size as f64 / mx as f64, 24), cat_code(&c.category)),
                ]
            })
            .collect();
        print_table(&["Kategori", "Lokasi", "Files", "Size", "Aman", ""], rows, "lrrrrl", &[]);
    }

    if !r.caches.is_empty() {
        section(&format!("Top {} Cache Terbesar", top.min(r.caches.len())));
        let rows = r
            .caches
            .iter()
            .take(top)
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
    }

    if !show_stats || m.mode != "full" {
        println!();
        return;
    }

    for d in &r.drives {
        if d.top_dirs.is_empty() {
            continue;
        }
        section(&format!("Folder Teratas · {}", d.root));
        let rows = d
            .top_dirs
            .iter()
            .take(top.min(15))
            .map(|x| {
                vec![
                    Cell::p(trunc_right(&x.name, 40)),
                    Cell::p(fmt_num(x.count)),
                    Cell::p(human_size(x.size)),
                    Cell::c(pct(x.size, d.size), C_DIM),
                    Cell::c(bar(x.size as f64 / d.size.max(1) as f64, 20), C_ACCENT),
                ]
            })
            .collect();
        print_table(&["Folder", "Files", "Size", "%", ""], rows, "lrrrl", &[]);
    }

    if !r.extensions.is_empty() {
        section("Top Ekstensi (by size)");
        let rows = r
            .extensions
            .iter()
            .take(top.min(15))
            .map(|e| {
                vec![
                    Cell::c(&e.name, C_CYAN),
                    Cell::p(fmt_num(e.count)),
                    Cell::p(human_size(e.size)),
                    Cell::c(pct(e.size, t.size), C_DIM),
                    Cell::c(bar(e.size as f64 / t.size.max(1) as f64, 20), C_PURPLE),
                ]
            })
            .collect();
        print_table(&["Ekstensi", "Count", "Size", "%", ""], rows, "lrrrl", &[]);
    }

    section("Umur File (mtime)");
    let rows = r
        .age
        .iter()
        .map(|a| {
            vec![
                Cell::p(&a.name),
                Cell::p(fmt_num(a.count)),
                Cell::p(human_size(a.size)),
                Cell::c(bar(a.size as f64 / t.size.max(1) as f64, 20), C_TEAL),
            ]
        })
        .collect();
    print_table(&["Umur", "Files", "Size", ""], rows, "lrrl", &[]);

    section("Distribusi Ukuran File");
    let rows = r
        .size_dist
        .iter()
        .map(|s| {
            vec![
                Cell::p(&s.name),
                Cell::p(fmt_num(s.count)),
                Cell::c(pct(s.count, t.files), C_DIM),
                Cell::p(human_size(s.size)),
                Cell::c(bar(s.count as f64 / t.files.max(1) as f64, 20), C_ORANGE),
            ]
        })
        .collect();
    print_table(&["Ukuran", "Files", "% file", "Total size", ""], rows, "lrrrl", &[]);

    if !r.largest.is_empty() {
        section(&format!("Top {} File Terbesar", top.min(r.largest.len())));
        let rows = r
            .largest
            .iter()
            .take(top)
            .map(|f| vec![Cell::c(human_size(f.size), "1"), Cell::c(fmt_time(f.mtime), C_DIM), Cell::p(&f.path)])
            .collect();
        print_table(&["Size", "Modified", "Path"], rows, "rll", &[(2, 90)]);
    }

    if !r.error_samples.is_empty() {
        println!("\n{}", paint(format!("  Contoh error ({} pertama):", r.error_samples.len().min(5)), C_DIM));
        for e in r.error_samples.iter().take(5) {
            println!("{}", paint(format!("    {e}"), C_DIM));
        }
    }
    println!();
}

// ===================================================================== export

pub fn load_json(path: &Path) -> Result<ScanResult> {
    let s = fs::read_to_string(path).with_context(|| format!("gagal baca {}", path.display()))?;
    serde_json::from_str(&s).with_context(|| format!("format report tidak valid: {}", path.display()))
}

pub fn save_json(r: &ScanResult, path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(r)?)?;
    Ok(())
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn write_csv(path: &Path, header: &[&str], rows: Vec<Vec<String>>) -> Result<()> {
    let mut out = String::from("\u{feff}"); // BOM agar rapi di Excel
    out.push_str(&header.join(","));
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&r.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    fs::write(path, out)?;
    Ok(())
}

/// CSV: <base>_caches.csv, _drives.csv, _largest.csv, _extensions.csv, _topdirs.csv
pub fn save_csv(r: &ScanResult, path: &Path) -> Result<Vec<PathBuf>> {
    let stem = path.with_extension("");
    let mk = |n: &str| PathBuf::from(format!("{}_{n}.csv", stem.display()));
    let mut out = Vec::new();

    let p = mk("caches");
    write_csv(
        &p,
        &["name", "category", "safe", "source", "files", "dirs", "size_bytes", "size", "newest", "oldest", "path", "note"],
        r.caches
            .iter()
            .map(|c| {
                vec![
                    c.name.clone(), c.category.clone(), c.safe.to_string(), c.source.clone(), c.files.to_string(),
                    c.dirs.to_string(), c.size.to_string(), human_size(c.size), fmt_time(c.newest), fmt_time(c.oldest),
                    c.path.clone(), c.note.clone(),
                ]
            })
            .collect(),
    )?;
    out.push(p);

    let p = mk("drives");
    write_csv(
        &p,
        &["root", "files", "dirs", "size_bytes", "size"],
        r.drives
            .iter()
            .map(|d| vec![d.root.clone(), d.files.to_string(), d.dirs.to_string(), d.size.to_string(), human_size(d.size)])
            .collect(),
    )?;
    out.push(p);

    if r.meta.mode == "full" {
        let p = mk("largest");
        write_csv(
            &p,
            &["size_bytes", "size", "modified", "path"],
            r.largest.iter().map(|f| vec![f.size.to_string(), human_size(f.size), fmt_time(f.mtime), f.path.clone()]).collect(),
        )?;
        out.push(p);
        let p = mk("extensions");
        write_csv(
            &p,
            &["ext", "count", "size_bytes", "size"],
            r.extensions.iter().map(|e| vec![e.name.clone(), e.count.to_string(), e.size.to_string(), human_size(e.size)]).collect(),
        )?;
        out.push(p);
        let p = mk("topdirs");
        write_csv(
            &p,
            &["drive", "folder", "files", "size_bytes", "size"],
            r.drives
                .iter()
                .flat_map(|d| {
                    d.top_dirs.iter().map(move |x| {
                        vec![d.root.clone(), x.name.clone(), x.count.to_string(), x.size.to_string(), human_size(x.size)]
                    })
                })
                .collect(),
        )?;
        out.push(p);
    }
    Ok(out)
}

pub fn save_markdown(r: &ScanResult, path: &Path, top: usize) -> Result<()> {
    let (m, t) = (&r.meta, &r.totals);
    let mut s = String::new();
    let _ = writeln!(s, "# CacheChecker Report\n");
    let _ = writeln!(s, "- **Waktu**: {}", fmt_time(m.started));
    let _ = writeln!(s, "- **Host**: {} ({})", m.host, m.os);
    let _ = writeln!(s, "- **Root**: {}", short_roots(&m.roots));
    let _ = writeln!(s, "- **Durasi**: {}", fmt_duration(m.elapsed));
    let _ = writeln!(s, "- **Total file**: {} ({} folder)", fmt_num(t.files), fmt_num(t.dirs));
    let _ = writeln!(s, "- **Total size**: {}", human_size(t.size));
    let _ = writeln!(s, "- **Cache**: {} di {} lokasi", human_size(t.cache_size), t.cache_count);
    let _ = writeln!(s, "- **Aman dihapus**: {}", human_size(t.cache_safe_size));
    let _ = writeln!(s, "- **Error akses**: {}\n", fmt_num(t.errors));

    let mut table = |title: &str, head: &[&str], rows: Vec<Vec<String>>| {
        let _ = writeln!(s, "## {title}\n");
        let _ = writeln!(s, "| {} |", head.join(" | "));
        let _ = writeln!(s, "|{}|", head.iter().map(|_| "---").collect::<Vec<_>>().join("|"));
        for r in rows {
            let _ = writeln!(s, "| {} |", r.iter().map(|c| c.replace('|', "\\|")).collect::<Vec<_>>().join(" | "));
        }
        let _ = writeln!(s);
    };
    table(
        "Per Drive",
        &["Root", "Files", "Folders", "Size"],
        r.drives.iter().map(|d| vec![d.root.clone(), fmt_num(d.files), fmt_num(d.dirs), human_size(d.size)]).collect(),
    );
    table(
        "Cache per Kategori",
        &["Kategori", "Lokasi", "Files", "Size", "Aman"],
        r.cache_by_category
            .iter()
            .map(|c| vec![c.category.clone(), c.count.to_string(), fmt_num(c.files), human_size(c.size), human_size(c.safe_size)])
            .collect(),
    );
    table(
        "Lokasi Cache",
        &["#", "Nama", "Kategori", "Aman", "Files", "Size", "Path"],
        r.caches
            .iter()
            .take(top)
            .enumerate()
            .map(|(i, c)| {
                vec![
                    (i + 1).to_string(), c.name.clone(), c.category.clone(),
                    if c.safe { "ya" } else { "cek" }.into(), fmt_num(c.files), human_size(c.size), format!("`{}`", c.path),
                ]
            })
            .collect(),
    );
    if m.mode == "full" {
        table(
            "Top Ekstensi",
            &["Ext", "Count", "Size"],
            r.extensions.iter().take(30).map(|e| vec![e.name.clone(), fmt_num(e.count), human_size(e.size)]).collect(),
        );
        table(
            "Umur File",
            &["Umur", "Files", "Size"],
            r.age.iter().map(|a| vec![a.name.clone(), fmt_num(a.count), human_size(a.size)]).collect(),
        );
        table(
            "File Terbesar",
            &["Size", "Modified", "Path"],
            r.largest.iter().take(top).map(|f| vec![human_size(f.size), fmt_time(f.mtime), format!("`{}`", f.path)]).collect(),
        );
    }
    fs::write(path, s)?;
    Ok(())
}

// ===================================================================== HTML

const CSS: &str = r#"
*{box-sizing:border-box}body{font-family:'Segoe UI',system-ui,sans-serif;margin:0;background:#1a1b26;color:#c0caf5}
header{padding:28px 36px;background:linear-gradient(135deg,#24283b 0%,#2d2a4a 100%);border-bottom:1px solid #3b4261}
h1{margin:0 0 6px;font-weight:600;background:linear-gradient(90deg,#7aa2f7,#bb9af7);-webkit-background-clip:text;color:transparent}
.sub{color:#737aa2;font-size:14px}main{padding:24px 36px;max-width:1500px}
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:14px;margin-bottom:22px}
.card{background:#24283b;border-radius:12px;padding:16px 18px;border:1px solid #3b4261}
.card .v{font-size:26px;font-weight:600;margin-top:6px}.card .l{color:#737aa2;font-size:12px;text-transform:uppercase;letter-spacing:.06em}
.y{color:#e0af68}.g{color:#9ece6a}.r{color:#f7768e}.b{color:#7aa2f7}
.grid2{display:grid;grid-template-columns:1fr 1fr;gap:20px}@media(max-width:1000px){.grid2{grid-template-columns:1fr}}
section{background:#24283b;border-radius:12px;padding:16px 20px;margin-bottom:20px;border:1px solid #3b4261}
h2{margin:0 0 12px;font-size:16px;font-weight:600;color:#c0caf5}h2:before{content:'▌';color:#bb9af7;margin-right:6px}
table{width:100%;border-collapse:collapse;font-size:13px}
th,td{padding:7px 8px;text-align:left;border-bottom:1px solid #2f3549}th{color:#737aa2;cursor:pointer;user-select:none;font-weight:500}
th:hover{color:#7aa2f7}tr:hover td{background:#292e42}
td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}td.p{font-family:'Cascadia Code',Consolas,monospace;font-size:12px;word-break:break-all;color:#a9b1d6}
.bar{height:8px;background:#1a1b26;border-radius:4px;overflow:hidden;min-width:90px}.bar i{display:block;height:100%;border-radius:4px;background:linear-gradient(90deg,#7aa2f7,#bb9af7)}
.tag{padding:2px 9px;border-radius:10px;font-size:11px;font-weight:500}.tag.s{background:#2a3a2a;color:#9ece6a}.tag.c{background:#3d3424;color:#e0af68}
.cat{padding:2px 8px;border-radius:6px;font-size:11px;background:#2f3549}
input{background:#1a1b26;border:1px solid #3b4261;color:#c0caf5;padding:8px 12px;border-radius:8px;width:320px;margin-bottom:12px;outline:none}
input:focus{border-color:#7aa2f7}
.donut{display:flex;align-items:center;gap:24px;flex-wrap:wrap}.legend div{margin:5px 0;font-size:13px}.legend i{display:inline-block;width:10px;height:10px;border-radius:3px;margin-right:8px}
"#;

const JS: &str = r#"
document.querySelectorAll('table.sortable th').forEach((th,i)=>th.onclick=()=>{
 const tb=th.closest('table').tBodies[0],rows=[...tb.rows],asc=th.dataset.a!=='1';th.dataset.a=asc?'1':'0';
 rows.sort((a,b)=>{let x=a.cells[i].dataset.v??a.cells[i].innerText,y=b.cells[i].dataset.v??b.cells[i].innerText;
 const nx=parseFloat(x),ny=parseFloat(y);if(!isNaN(nx)&&!isNaN(ny)){x=nx;y=ny}return (x>y?1:x<y?-1:0)*(asc?1:-1)});
 rows.forEach(r=>tb.appendChild(r))});
const f=document.getElementById('flt');if(f)f.oninput=()=>{const q=f.value.toLowerCase();
 document.querySelectorAll('#cachetbl tbody tr').forEach(r=>r.style.display=r.innerText.toLowerCase().includes(q)?'':'none')};
"#;

const PALETTE: &[&str] = &["#7aa2f7", "#bb9af7", "#9ece6a", "#e0af68", "#f7768e", "#7dcfff", "#ff9e64", "#73daca", "#c0caf5", "#565f89"];

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn donut_svg(parts: &[(String, u64)]) -> String {
    let total: u64 = parts.iter().map(|p| p.1).sum();
    if total == 0 {
        return String::new();
    }
    let (r, c) = (60.0, 2.0 * std::f64::consts::PI * 60.0);
    let mut off = 0.0;
    let mut svg = String::from(r#"<svg width="170" height="170" viewBox="0 0 170 170"><g transform="rotate(-90 85 85)">"#);
    let mut legend = String::from(r#"<div class="legend">"#);
    for (i, (name, v)) in parts.iter().enumerate() {
        let frac = *v as f64 / total as f64;
        let len = frac * c;
        let col = PALETTE[i % PALETTE.len()];
        let _ = write!(
            svg,
            r#"<circle cx="85" cy="85" r="{r}" fill="none" stroke="{col}" stroke-width="22" stroke-dasharray="{len:.2} {:.2}" stroke-dashoffset="{:.2}"/>"#,
            c - len,
            -off
        );
        off += len;
        let _ = write!(legend, r#"<div><i style="background:{col}"></i>{} — <b>{}</b> <span class="sub">({:.1}%)</span></div>"#, esc(name), human_size(*v), frac * 100.0);
    }
    let _ = write!(svg, r##"</g><text x="85" y="82" text-anchor="middle" fill="#c0caf5" font-size="16" font-weight="600">{}</text><text x="85" y="102" text-anchor="middle" fill="#737aa2" font-size="11">total</text></svg>"##, human_size(total));
    legend.push_str("</div>");
    format!(r#"<div class="donut">{svg}{legend}</div>"#)
}

pub fn save_html(r: &ScanResult, path: &Path, top: usize) -> Result<()> {
    let (m, t) = (&r.meta, &r.totals);
    let card = |l: &str, v: String, cls: &str| format!(r#"<div class="card"><div class="l">{}</div><div class="v {cls}">{}</div></div>"#, esc(l), esc(&v));
    let barh = |part: u64, whole: u64| {
        let p = if whole == 0 { 0.0 } else { part as f64 / whole as f64 * 100.0 };
        format!(r#"<div class="bar"><i style="width:{p:.1}%"></i></div>"#)
    };
    let num = |v: u64| format!(r#"<td class="n" data-v="{v}">{}</td>"#, fmt_num(v));
    let sz = |v: u64| format!(r#"<td class="n" data-v="{v}">{}</td>"#, human_size(v));
    let sec = |title: &str, head: &[(&str, bool)], body: String, extra: &str, id: &str| {
        let ths: String = head.iter().map(|(h, n)| format!(r#"<th class="{}">{}</th>"#, if *n { "n" } else { "" }, esc(h))).collect();
        let idattr = if id.is_empty() { String::new() } else { format!(r#" id="{id}""#) };
        format!(r#"<section><h2>{}</h2>{extra}<table class="sortable"{idattr}><thead><tr>{ths}</tr></thead><tbody>{body}</tbody></table></section>"#, esc(title))
    };

    let mut parts = Vec::new();
    parts.push(format!(
        r#"<div class="cards">{}{}{}{}{}{}{}{}</div>"#,
        card("Total File", fmt_num(t.files), "b"),
        card("Total Folder", fmt_num(t.dirs), ""),
        card("Total Size", human_size(t.size), ""),
        card("Total Cache", human_size(t.cache_size), "y"),
        card("Aman Dihapus", human_size(t.cache_safe_size), "g"),
        card("Lokasi Cache", t.cache_count.to_string(), ""),
        card("Error Akses", fmt_num(t.errors), if t.errors > 0 { "r" } else { "" }),
        card("Durasi Scan", fmt_duration(m.elapsed), ""),
    ));

    // donut kategori + per drive
    let cat_parts: Vec<(String, u64)> = r.cache_by_category.iter().map(|c| (c.category.clone(), c.size)).collect();
    let mut cat_body = String::new();
    for c in &r.cache_by_category {
        let _ = write!(cat_body, r#"<tr><td><span class="cat">{}</span></td>{}{}{}{}<td>{}</td></tr>"#, esc(&c.category), num(c.count), num(c.files), sz(c.size), sz(c.safe_size), barh(c.size, t.cache_size));
    }
    parts.push(format!(
        r#"<div class="grid2"><section><h2>Komposisi Cache</h2>{}</section>{}</div>"#,
        donut_svg(&cat_parts),
        sec("Cache per Kategori", &[("Kategori", false), ("Lokasi", true), ("Files", true), ("Size", true), ("Aman", true), ("", false)], cat_body, "", "")
    ));

    let mut body = String::new();
    for d in &r.drives {
        let _ = write!(body, r#"<tr><td>{}</td>{}{}{}<td>{}</td></tr>"#, esc(&d.root), num(d.files), num(d.dirs), sz(d.size), barh(d.size, t.size));
    }
    parts.push(sec("Per Drive / Root", &[("Root", false), ("Files", true), ("Folders", true), ("Size", true), ("", false)], body, "", ""));

    let mx = r.caches.first().map(|c| c.size).unwrap_or(0);
    let mut body = String::new();
    for c in r.caches.iter().take(top) {
        let _ = write!(
            body,
            r#"<tr><td>{}</td><td><span class="cat">{}</span></td><td><span class="tag {}">{}</span></td>{}{}<td>{}</td><td data-v="{}">{}</td><td class="p" title="{}">{}</td></tr>"#,
            esc(&c.name), esc(&c.category),
            if c.safe { "s" } else { "c" }, if c.safe { "aman" } else { "cek" },
            num(c.files), sz(c.size), barh(c.size, mx), c.newest, fmt_time(c.newest), esc(&c.note), esc(&c.path)
        );
    }
    parts.push(sec(
        &format!("Lokasi Cache ({})", r.caches.len()),
        &[("Nama", false), ("Kategori", false), ("Status", false), ("Files", true), ("Size", true), ("", false), ("Terbaru", false), ("Path", false)],
        body,
        r#"<input id="flt" placeholder="🔍  Filter cache...">"#,
        "cachetbl",
    ));

    if m.mode == "full" {
        for d in &r.drives {
            let mut body = String::new();
            for x in d.top_dirs.iter().take(30) {
                let _ = write!(body, r#"<tr><td>{}</td>{}{}<td>{}</td></tr>"#, esc(&x.name), num(x.count), sz(x.size), barh(x.size, d.size));
            }
            parts.push(sec(&format!("Folder Teratas — {}", d.root), &[("Folder", false), ("Files", true), ("Size", true), ("", false)], body, "", ""));
        }
        let ext_parts: Vec<(String, u64)> = {
            let mut v: Vec<(String, u64)> = r.extensions.iter().take(9).map(|e| (e.name.clone(), e.size)).collect();
            let rest: u64 = r.extensions.iter().skip(9).map(|e| e.size).sum();
            if rest > 0 {
                v.push(("lainnya".into(), rest));
            }
            v
        };
        let mut body = String::new();
        for x in r.extensions.iter().take(50) {
            let _ = write!(body, r#"<tr><td>{}</td>{}{}<td>{}</td></tr>"#, esc(&x.name), num(x.count), sz(x.size), barh(x.size, t.size));
        }
        parts.push(format!(
            r#"<div class="grid2"><section><h2>Komposisi Ekstensi</h2>{}</section>{}</div>"#,
            donut_svg(&ext_parts),
            sec("Top Ekstensi", &[("Ext", false), ("Count", true), ("Size", true), ("", false)], body, "", "")
        ));
        let mut b1 = String::new();
        for x in &r.age {
            let _ = write!(b1, r#"<tr><td>{}</td>{}{}<td>{}</td></tr>"#, esc(&x.name), num(x.count), sz(x.size), barh(x.size, t.size));
        }
        let mut b2 = String::new();
        for x in &r.size_dist {
            let _ = write!(b2, r#"<tr><td>{}</td>{}{}<td>{}</td></tr>"#, esc(&x.name), num(x.count), sz(x.size), barh(x.count, t.files));
        }
        parts.push(format!(
            r#"<div class="grid2">{}{}</div>"#,
            sec("Umur File", &[("Umur", false), ("Files", true), ("Size", true), ("", false)], b1, "", ""),
            sec("Distribusi Ukuran", &[("Ukuran", false), ("Files", true), ("Size", true), ("", false)], b2, "", "")
        ));
        let mut body = String::new();
        for f in r.largest.iter().take(top) {
            let _ = write!(body, r#"<tr>{}<td data-v="{}">{}</td><td class="p">{}</td></tr>"#, sz(f.size), f.mtime, fmt_time(f.mtime), esc(&f.path));
        }
        parts.push(sec("File Terbesar", &[("Size", true), ("Modified", false), ("Path", false)], body, "", ""));
    }
    if !r.error_samples.is_empty() {
        let body: String = r.error_samples.iter().map(|e| format!(r#"<tr><td class="p">{}</td></tr>"#, esc(e))).collect();
        parts.push(sec("Contoh Error Akses", &[("Pesan", false)], body, "", ""));
    }

    let doc = format!(
        r#"<!doctype html><html lang="id"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>CacheChecker — {host}</title><style>{CSS}</style></head><body><header><h1>CacheChecker Report</h1><div class="sub">{host} · {os} · {time} · {roots}{partial}</div></header><main>{body}</main><script>{JS}</script></body></html>"#,
        host = esc(&m.host),
        os = esc(&m.os),
        time = esc(&fmt_time(m.started)),
        roots = esc(&short_roots(&m.roots)),
        partial = if m.cancelled { r#" · <b class="r">PARSIAL</b>"# } else { "" },
        body = parts.join("")
    );
    fs::write(path, doc)?;
    Ok(())
}

pub fn export(r: &ScanResult, path: &Path, fmt: Option<&str>) -> Result<Vec<PathBuf>> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let fmt = fmt.map(|s| s.to_lowercase()).unwrap_or(if ext.is_empty() { "json".into() } else { ext });
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    match fmt.as_str() {
        "json" => save_json(r, path)?,
        "csv" => return save_csv(r, path),
        "md" | "markdown" => save_markdown(r, path, 50)?,
        "html" | "htm" => save_html(r, path, 300)?,
        _ => bail!("format tidak dikenal: {fmt} (json|csv|md|html)"),
    }
    Ok(vec![path.to_path_buf()])
}

pub fn default_report_name(ext: &str) -> PathBuf {
    PathBuf::from(format!("cachecheck_{}.{ext}", chrono::Local::now().format("%Y%m%d_%H%M%S")))
}

// ===================================================================== compare

pub fn print_compare(old: &ScanResult, new: &ScanResult, top: usize) {
    let (to, tn) = (&old.totals, &new.totals);
    let delta = |a: u64, b: u64, size: bool| -> Cell {
        let d = b as i128 - a as i128;
        let v = d.unsigned_abs() as u64;
        let s = if size { human_size(v) } else { fmt_num(v) };
        match d.signum() {
            1 => Cell::c(format!("▲ +{s}"), C_RED),
            -1 => Cell::c(format!("▼ -{s}"), C_GREEN),
            _ => Cell::c("0", C_DIM),
        }
    };
    println!();
    println!("  {} {}  →  {}", paint("COMPARE", C_HEAD), fmt_time(old.meta.started), fmt_time(new.meta.started));
    println!();
    print_table(
        &["Metric", "Lama", "Baru", "Delta"],
        vec![
            vec![Cell::p("Total file"), Cell::p(fmt_num(to.files)), Cell::p(fmt_num(tn.files)), delta(to.files, tn.files, false)],
            vec![Cell::p("Total size"), Cell::p(human_size(to.size)), Cell::p(human_size(tn.size)), delta(to.size, tn.size, true)],
            vec![Cell::p("Cache size"), Cell::p(human_size(to.cache_size)), Cell::p(human_size(tn.cache_size)), delta(to.cache_size, tn.cache_size, true)],
            vec![Cell::p("Cache aman"), Cell::p(human_size(to.cache_safe_size)), Cell::p(human_size(tn.cache_safe_size)), delta(to.cache_safe_size, tn.cache_safe_size, true)],
        ],
        "lrrr",
        &[],
    );
    let oc: HashMap<String, &CacheHit> = old.caches.iter().map(|c| (c.path.to_lowercase(), c)).collect();
    let nc: HashMap<String, &CacheHit> = new.caches.iter().map(|c| (c.path.to_lowercase(), c)).collect();
    let keys: HashSet<&String> = oc.keys().chain(nc.keys()).collect();
    let mut changes: Vec<(i128, &CacheHit, u64, u64)> = keys
        .into_iter()
        .filter_map(|k| {
            let a = oc.get(k).map(|c| c.size).unwrap_or(0);
            let b = nc.get(k).map(|c| c.size).unwrap_or(0);
            (a != b).then(|| (b as i128 - a as i128, *nc.get(k).or(oc.get(k)).unwrap(), a, b))
        })
        .collect();
    changes.sort_by_key(|c| std::cmp::Reverse(c.0.abs()));
    if !changes.is_empty() {
        section("Perubahan Cache Terbesar");
        let rows = changes
            .iter()
            .take(top)
            .map(|(_, c, a, b)| vec![Cell::p(trunc_right(&c.name, 28)), Cell::p(human_size(*a)), Cell::p(human_size(*b)), delta(*a, *b, true), Cell::c(&c.path, C_DIM)])
            .collect();
        print_table(&["Nama", "Lama", "Baru", "Delta", "Path"], rows, "lrrrl", &[(4, 64)]);
    }
    println!();
}
