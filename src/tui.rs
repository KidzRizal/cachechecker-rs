//! TUI interaktif (ratatui) — dashboard drive, live scan, hasil per tab, clean & export.

use cachechecker::cleaner::{self, CleanProgress, CleanResult};
use cachechecker::drives::{self, DriveInfo};
use cachechecker::model::*;
use cachechecker::report;
use cachechecker::scanner::{self, Progress, ScanOptions};
use cachechecker::util::{bar, fmt_duration, fmt_num, fmt_time, human_size, is_admin, trunc_left, trunc_right};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::DefaultTerminal;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// ===================================================================== tema (Tokyo Night)

mod th {
    use ratatui::style::Color;
    pub const BG: Color = Color::Rgb(26, 27, 38);
    pub const PANEL: Color = Color::Rgb(31, 35, 53);
    pub const BORDER: Color = Color::Rgb(59, 66, 97);
    pub const FG: Color = Color::Rgb(192, 202, 245);
    pub const DIM: Color = Color::Rgb(115, 122, 162);
    pub const MUTED: Color = Color::Rgb(65, 72, 104);
    pub const ACCENT: Color = Color::Rgb(122, 162, 247);
    pub const PURPLE: Color = Color::Rgb(187, 154, 247);
    pub const GREEN: Color = Color::Rgb(158, 206, 106);
    pub const YELLOW: Color = Color::Rgb(224, 175, 104);
    pub const RED: Color = Color::Rgb(247, 118, 142);
    pub const CYAN: Color = Color::Rgb(125, 207, 255);
    pub const ORANGE: Color = Color::Rgb(255, 158, 100);
    pub const TEAL: Color = Color::Rgb(115, 218, 202);
    pub const SEL: Color = Color::Rgb(41, 46, 66);
}

fn cat_color(c: &str) -> Color {
    match c {
        "system" => th::CYAN,
        "browser" => th::ACCENT,
        "app" => th::PURPLE,
        "dev" => th::GREEN,
        "dev-artifact" => th::TEAL,
        "gpu" => th::ORANGE,
        "game" => th::RED,
        "ai" => th::YELLOW,
        _ => th::DIM,
    }
}

fn usage_color(pct: f64) -> Color {
    if pct >= 90.0 {
        th::RED
    } else if pct >= 75.0 {
        th::YELLOW
    } else {
        th::GREEN
    }
}

fn grad(i: usize, n: usize) -> Color {
    let t = if n <= 1 { 0.0 } else { i as f64 / (n - 1) as f64 };
    let (a, b) = ((122.0, 162.0, 247.0), (187.0, 154.0, 247.0));
    Color::Rgb((a.0 + (b.0 - a.0) * t) as u8, (a.1 + (b.1 - a.1) * t) as u8, (a.2 + (b.2 - a.2) * t) as u8)
}

const LOGO: &[&str] = &[
    "  ██████╗ █████╗  ██████╗██╗  ██╗███████╗     ██████╗██╗  ██╗███████╗ ██████╗██╗  ██╗",
    " ██╔════╝██╔══██╗██╔════╝██║  ██║██╔════╝    ██╔════╝██║  ██║██╔════╝██╔════╝██║ ██╔╝",
    " ██║     ███████║██║     ███████║█████╗      ██║     ███████║█████╗  ██║     █████╔╝ ",
    " ██║     ██╔══██║██║     ██╔══██║██╔══╝      ██║     ██╔══██║██╔══╝  ██║     ██╔═██╗ ",
    " ╚██████╗██║  ██║╚██████╗██║  ██║███████╗    ╚██████╗██║  ██║███████╗╚██████╗██║  ██╗",
    "  ╚═════╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝     ╚═════╝╚═╝  ╚═╝╚══════╝ ╚═════╝╚═╝  ╚═╝",
];

const SPIN: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TABS: &[&str] = &["Ringkasan", "Cache", "Folder", "Ekstensi", "File Besar", "Statistik"];

// ===================================================================== helper widget

fn panel(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(th::BORDER))
        .title(Line::from(vec![
            Span::styled(" ▌", Style::new().fg(th::PURPLE)),
            Span::styled(format!("{title} "), Style::new().fg(th::FG).bold()),
        ]))
        .style(Style::new().bg(th::PANEL))
}

fn key(k: &str) -> Span<'static> {
    Span::styled(format!(" {k} "), Style::new().fg(th::BG).bg(th::ACCENT).bold())
}

fn hint(k: &str, label: &str) -> Vec<Span<'static>> {
    vec![key(k), Span::styled(format!(" {label}  "), Style::new().fg(th::DIM))]
}

fn bar_spans(ratio: f64, width: usize, color: Color) -> Vec<Span<'static>> {
    let b = bar(ratio, width);
    let used = b.chars().count();
    vec![
        Span::styled(b, Style::new().fg(color)),
        Span::styled("░".repeat(width.saturating_sub(used)), Style::new().fg(th::MUTED)),
    ]
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn stat_card(f: &mut Frame, area: Rect, label: &str, value: String, color: Color, sub: String) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(th::BORDER))
        .style(Style::new().bg(th::PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let lines = vec![
        Line::from(Span::styled(label.to_uppercase(), Style::new().fg(th::DIM))),
        Line::from(Span::styled(value, Style::new().fg(color).bold())),
        Line::from(Span::styled(sub, Style::new().fg(th::MUTED))),
    ];
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

fn open_path(p: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

fn rcell(s: String) -> Cell<'static> {
    Cell::from(Line::from(s).alignment(Alignment::Right))
}

fn rcell_s(s: String, st: Style) -> Cell<'static> {
    Cell::from(Line::from(Span::styled(s, st)).alignment(Alignment::Right))
}

// ===================================================================== state

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Home,
    Scanning,
    Results,
}

#[derive(PartialEq, Clone, Copy)]
enum SortKey {
    Size,
    Files,
    Name,
    Category,
    Newest,
}

impl SortKey {
    fn next(self) -> Self {
        match self {
            SortKey::Size => SortKey::Files,
            SortKey::Files => SortKey::Name,
            SortKey::Name => SortKey::Category,
            SortKey::Category => SortKey::Newest,
            SortKey::Newest => SortKey::Size,
        }
    }
    fn label(self) -> &'static str {
        match self {
            SortKey::Size => "size",
            SortKey::Files => "files",
            SortKey::Name => "nama",
            SortKey::Category => "kategori",
            SortKey::Newest => "terbaru",
        }
    }
}

struct ScanJob {
    progress: Arc<Progress>,
    handle: Option<JoinHandle<ScanResult>>,
    started: Instant,
    expected: u64,
    label: String,
    hist: Vec<u64>,
    last_files: u64,
    last_tick: Instant,
    peak: u64,
}

struct CleanItem {
    idx: usize,
    name: String,
    res: CleanResult,
}

struct CleanJob {
    progress: Arc<CleanProgress>,
    handle: Option<JoinHandle<Vec<CleanItem>>>,
    total: u64,
    dry: bool,
    count: usize,
}

enum Modal {
    Help,
    ConfirmClean,
    CleanReport { items: Vec<CleanItem>, dry: bool },
    Detail(usize),
    ConfirmQuit,
}

pub struct App {
    screen: Screen,
    threads: usize,
    drives: Vec<DriveInfo>,
    drive_sel: Vec<bool>,
    home_state: TableState,
    scan: Option<ScanJob>,
    result: Option<ScanResult>,
    tab: usize,
    states: [TableState; 6],
    marked: HashSet<usize>,
    filter: String,
    filter_mode: bool,
    safe_only: bool,
    sort: SortKey,
    modal: Option<Modal>,
    toast: Option<(String, Color, Instant)>,
    clean: Option<CleanJob>,
    frame: usize,
    quit: bool,
}

impl App {
    fn new(threads: usize) -> Self {
        let drives = drives::list_drives(false, true);
        // default: drive lokal terpilih, drive cloud tidak
        let drive_sel = drives.iter().map(|d| d.kind != "cloud").collect();
        let mut home_state = TableState::default();
        home_state.select(Some(0));
        App {
            screen: Screen::Home,
            threads,
            drives,
            drive_sel,
            home_state,
            scan: None,
            result: None,
            tab: 0,
            states: Default::default(),
            marked: HashSet::new(),
            filter: String::new(),
            filter_mode: false,
            safe_only: false,
            sort: SortKey::Size,
            modal: None,
            toast: None,
            clean: None,
            frame: 0,
            quit: false,
        }
    }

    fn toast(&mut self, msg: impl Into<String>, color: Color) {
        self.toast = Some((msg.into(), color, Instant::now()));
    }

    fn set_result(&mut self, r: ScanResult) {
        self.tab = if r.meta.mode == "full" { 0 } else { 1 };
        self.result = Some(r);
        self.screen = Screen::Results;
        self.marked.clear();
        self.filter.clear();
        for s in self.states.iter_mut() {
            s.select(Some(0));
        }
    }

    // ------------------------------------------------------------ scan

    fn start_scan(&mut self, opts: ScanOptions, label: String, expected: u64) {
        let progress = Arc::new(Progress::default());
        let pg = progress.clone();
        let handle = std::thread::spawn(move || scanner::run(&opts, &pg));
        self.scan = Some(ScanJob {
            progress,
            handle: Some(handle),
            started: Instant::now(),
            expected,
            label,
            hist: vec![0; 300],
            last_files: 0,
            last_tick: Instant::now(),
            peak: 0,
        });
        self.screen = Screen::Scanning;
    }

    fn start_full_scan(&mut self) {
        let sel: Vec<DriveInfo> =
            self.drives.iter().zip(&self.drive_sel).filter(|(_, s)| **s).map(|(d, _)| d.clone()).collect();
        if sel.is_empty() {
            self.toast("Pilih minimal 1 drive dengan [Space]", th::RED);
            return;
        }
        let roots: Vec<PathBuf> = sel.iter().map(|d| PathBuf::from(&d.mount)).collect();
        let expected = sel.iter().map(|d| d.used).sum();
        let label = format!(
            "Full scan {}",
            sel.iter().map(|d| d.mount.trim_end_matches('\\').to_string()).collect::<Vec<_>>().join(" ")
        );
        let opts = ScanOptions { roots, threads: self.threads, top_n: 200, ..Default::default() };
        self.start_scan(opts, label, expected);
    }

    fn start_cache_scan(&mut self) {
        let opts = scanner::cache_only_options(self.threads, &[], false, 0);
        self.start_scan(opts, "Quick cache scan".into(), 0);
    }

    // ------------------------------------------------------------ tick

    fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        if let Some((_, _, t)) = &self.toast {
            if t.elapsed() > Duration::from_secs(4) {
                self.toast = None;
            }
        }
        let mut finished: Option<ScanResult> = None;
        if let Some(job) = self.scan.as_mut() {
            if job.last_tick.elapsed() >= Duration::from_millis(200) {
                let files = job.progress.files.load(Ordering::Relaxed);
                let dt = job.last_tick.elapsed().as_secs_f64();
                let rate = (files.saturating_sub(job.last_files) as f64 / dt) as u64;
                job.hist.remove(0);
                job.hist.push(rate);
                job.peak = job.peak.max(rate);
                job.last_files = files;
                job.last_tick = Instant::now();
            }
            if job.handle.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
                if let Some(h) = job.handle.take() {
                    finished = h.join().ok();
                }
            }
        }
        if let Some(r) = finished {
            self.scan = None;
            let msg = format!(
                "Selesai: {} file · cache {} · {}",
                fmt_num(r.totals.files),
                human_size(r.totals.cache_size),
                fmt_duration(r.meta.elapsed)
            );
            self.set_result(r);
            self.toast(msg, th::GREEN);
        }

        let mut cleaned: Option<(Vec<CleanItem>, bool)> = None;
        if let Some(job) = self.clean.as_mut() {
            if job.handle.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
                let dry = job.dry;
                if let Some(h) = job.handle.take() {
                    cleaned = h.join().ok().map(|v| (v, dry));
                }
            }
        }
        if let Some((items, dry)) = cleaned {
            self.clean = None;
            let freed: u64 = items.iter().map(|i| i.res.freed).sum();
            if !dry {
                if let Some(r) = self.result.as_mut() {
                    for it in &items {
                        if let Some(c) = r.caches.get_mut(it.idx) {
                            c.size = c.size.saturating_sub(it.res.freed);
                            c.files = c.files.saturating_sub(it.res.files);
                        }
                    }
                    r.totals.cache_size = r.caches.iter().map(|c| c.size).sum();
                    r.totals.cache_files = r.caches.iter().map(|c| c.files).sum();
                    r.totals.cache_safe_size = r.caches.iter().filter(|c| c.safe).map(|c| c.size).sum();
                    let caches = r.caches.clone();
                    for cat in r.cache_by_category.iter_mut() {
                        cat.size = caches.iter().filter(|c| c.category == cat.category).map(|c| c.size).sum();
                        cat.safe_size =
                            caches.iter().filter(|c| c.category == cat.category && c.safe).map(|c| c.size).sum();
                    }
                }
                self.marked.clear();
                self.toast(format!("Dibebaskan {}", human_size(freed)), th::GREEN);
            } else {
                self.toast(format!("Dry-run: {} bisa dibebaskan", human_size(freed)), th::CYAN);
            }
            self.modal = Some(Modal::CleanReport { items, dry });
        }
    }

    // ------------------------------------------------------------ data helper

    fn visible(&self) -> Vec<usize> {
        let Some(r) = &self.result else { return vec![] };
        let q = self.filter.to_lowercase();
        let mut v: Vec<usize> = r
            .caches
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                (!self.safe_only || c.safe)
                    && (q.is_empty()
                        || c.name.to_lowercase().contains(&q)
                        || c.path.to_lowercase().contains(&q)
                        || c.category.contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        let cs = &r.caches;
        match self.sort {
            SortKey::Size => v.sort_by(|a, b| cs[*b].size.cmp(&cs[*a].size)),
            SortKey::Files => v.sort_by(|a, b| cs[*b].files.cmp(&cs[*a].files)),
            SortKey::Name => v.sort_by(|a, b| cs[*a].name.to_lowercase().cmp(&cs[*b].name.to_lowercase())),
            SortKey::Category => {
                v.sort_by(|a, b| cs[*a].category.cmp(&cs[*b].category).then(cs[*b].size.cmp(&cs[*a].size)))
            }
            SortKey::Newest => v.sort_by(|a, b| cs[*b].newest.total_cmp(&cs[*a].newest)),
        }
        v
    }

    fn folder_rows(&self) -> Vec<(String, NamedStat, u64)> {
        let Some(r) = &self.result else { return vec![] };
        let mut v: Vec<(String, NamedStat, u64)> = r
            .drives
            .iter()
            .flat_map(|d| d.top_dirs.iter().map(move |x| (d.root.clone(), x.clone(), d.size)))
            .collect();
        v.sort_by(|a, b| b.1.size.cmp(&a.1.size));
        v
    }

    fn tab_len(&self) -> usize {
        let Some(r) = &self.result else { return 0 };
        match self.tab {
            1 => self.visible().len(),
            2 => self.folder_rows().len(),
            3 => r.extensions.len().min(500),
            4 => r.largest.len(),
            _ => 0,
        }
    }

    fn move_sel(&mut self, delta: i64) {
        let n = self.tab_len();
        if n == 0 {
            return;
        }
        let st = &mut self.states[self.tab];
        let cur = st.selected().unwrap_or(0) as i64;
        st.select(Some((cur + delta).clamp(0, n as i64 - 1) as usize));
    }

    fn selected_cache(&self) -> Option<usize> {
        let v = self.visible();
        self.states[1].selected().and_then(|i| v.get(i).copied())
    }

    /// Target clean, buang yang berada di dalam target lain.
    fn marked_targets(&self) -> Vec<usize> {
        let Some(r) = &self.result else { return vec![] };
        let mut ids: Vec<usize> = self.marked.iter().copied().filter(|i| *i < r.caches.len()).collect();
        ids.sort_by_key(|i| r.caches[*i].path.len());
        let sep = std::path::MAIN_SEPARATOR;
        let key = |i: usize| format!("{}{sep}", r.caches[i].path.to_lowercase().trim_end_matches(sep));
        let mut out: Vec<usize> = Vec::new();
        for i in ids {
            let p = key(i);
            if !out.iter().any(|j| p.starts_with(&key(*j))) {
                out.push(i);
            }
        }
        out.sort_by(|a, b| r.caches[*b].size.cmp(&r.caches[*a].size));
        out
    }

    fn start_clean(&mut self, dry: bool) {
        let Some(r) = &self.result else { return };
        let targets = self.marked_targets();
        let items: Vec<(usize, String, String)> =
            targets.iter().map(|i| (*i, r.caches[*i].name.clone(), r.caches[*i].path.clone())).collect();
        let total = targets.iter().map(|i| r.caches[*i].size).sum();
        let progress = Arc::new(CleanProgress::default());
        let pg = progress.clone();
        let count = items.len();
        let handle = std::thread::spawn(move || {
            items
                .into_iter()
                .map(|(idx, name, path)| {
                    let res = cleaner::clean_cache(Path::new(&path), None, dry, Some(&pg));
                    CleanItem { idx, name, res }
                })
                .collect()
        });
        self.clean = Some(CleanJob { progress, handle: Some(handle), total, dry, count });
        self.modal = None;
    }

    fn export(&mut self) {
        let Some(r) = &self.result else { return };
        let html = report::default_report_name("html");
        let json = html.with_extension("json");
        let res = report::export(r, &html, None).and_then(|_| report::export(r, &json, None));
        match res {
            Ok(_) => {
                let p = std::path::absolute(&html).unwrap_or(html);
                self.toast(format!("Report disimpan: {} (+ .json)", p.display()), th::GREEN);
            }
            Err(e) => self.toast(format!("Gagal export: {e}"), th::RED),
        }
    }

    // ===================================================================== input

    fn on_key(&mut self, k: KeyEvent) {
        let ctrl_c = k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL);
        if self.clean.is_some() {
            return; // tunggu proses hapus selesai
        }

        if let Some(m) = &self.modal {
            match m {
                Modal::ConfirmClean => match k.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => self.start_clean(false),
                    KeyCode::Char('d') | KeyCode::Char('D') => self.start_clean(true),
                    _ => self.modal = None,
                },
                Modal::ConfirmQuit => match k.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('q') | KeyCode::Enter => self.quit = true,
                    _ => self.modal = None,
                },
                Modal::Detail(i) => {
                    if k.code == KeyCode::Char('o') {
                        if let Some(c) = self.result.as_ref().and_then(|r| r.caches.get(*i)) {
                            open_path(&c.path);
                        }
                    }
                    self.modal = None;
                }
                _ => {
                    if ctrl_c {
                        self.quit = true;
                    }
                    self.modal = None;
                }
            }
            return;
        }

        if self.filter_mode {
            match k.code {
                KeyCode::Esc => {
                    self.filter.clear();
                    self.filter_mode = false;
                }
                KeyCode::Enter => self.filter_mode = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                }
                KeyCode::Char(c) if !ctrl_c => self.filter.push(c),
                _ => {}
            }
            self.states[1].select(Some(0));
            return;
        }

        match self.screen {
            Screen::Home => self.key_home(k, ctrl_c),
            Screen::Scanning => {
                if matches!(k.code, KeyCode::Esc | KeyCode::Char('q')) || ctrl_c {
                    if let Some(j) = &self.scan {
                        j.progress.cancel.store(true, Ordering::Relaxed);
                    }
                    self.toast("Membatalkan scan… (hasil parsial akan ditampilkan)", th::YELLOW);
                }
            }
            Screen::Results => self.key_results(k, ctrl_c),
        }
    }

    fn key_home(&mut self, k: KeyEvent, ctrl_c: bool) {
        let n = self.drives.len();
        let cur = self.home_state.selected().unwrap_or(0);
        if ctrl_c {
            self.quit = true;
            return;
        }
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.home_state.select(Some(cur.saturating_sub(1))),
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.home_state.select(Some((cur + 1).min(n - 1))),
            KeyCode::Char(' ') if n > 0 => self.drive_sel[cur] = !self.drive_sel[cur],
            KeyCode::Char('a') => {
                let all = self.drive_sel.iter().all(|s| *s);
                self.drive_sel.iter_mut().for_each(|s| *s = !all);
            }
            KeyCode::Enter | KeyCode::Char('f') => self.start_full_scan(),
            KeyCode::Char('c') => self.start_cache_scan(),
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Tab if self.result.is_some() => self.screen = Screen::Results,
            _ => {}
        }
    }

    fn key_results(&mut self, k: KeyEvent, ctrl_c: bool) {
        if ctrl_c {
            self.quit = true;
            return;
        }
        match k.code {
            KeyCode::Char('q') => self.modal = Some(Modal::ConfirmQuit),
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                } else {
                    self.modal = Some(Modal::ConfirmQuit);
                }
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.tab = (self.tab + 1) % TABS.len(),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.tab = (self.tab + TABS.len() - 1) % TABS.len(),
            KeyCode::Char(c @ '1'..='6') => self.tab = (c as u8 - b'1') as usize,
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::PageUp => self.move_sel(-15),
            KeyCode::PageDown => self.move_sel(15),
            KeyCode::Home | KeyCode::Char('g') => self.move_sel(-1_000_000),
            KeyCode::End | KeyCode::Char('G') => self.move_sel(1_000_000),
            KeyCode::Char('e') => self.export(),
            KeyCode::Char('r') => self.screen = Screen::Home,
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char('/') => {
                self.tab = 1;
                self.filter_mode = true;
            }
            KeyCode::Char('s') if self.tab == 1 => {
                self.sort = self.sort.next();
                let l = self.sort.label();
                self.toast(format!("Urut berdasarkan {l}"), th::ACCENT);
            }
            KeyCode::Char('f') if self.tab == 1 => {
                self.safe_only = !self.safe_only;
                self.states[1].select(Some(0));
            }
            KeyCode::Char(' ') if self.tab == 1 => {
                if let Some(i) = self.selected_cache() {
                    if !self.marked.remove(&i) {
                        self.marked.insert(i);
                    }
                    self.move_sel(1);
                }
            }
            KeyCode::Char('a') if self.tab == 1 => {
                let safe: Vec<usize> = {
                    let r = self.result.as_ref().unwrap();
                    self.visible().into_iter().filter(|i| r.caches[*i].safe && r.caches[*i].size > 0).collect()
                };
                let n = safe.len();
                self.marked.extend(safe);
                self.toast(format!("{n} cache aman ditandai"), th::GREEN);
            }
            KeyCode::Char('n') | KeyCode::Char('u') if self.tab == 1 => self.marked.clear(),
            KeyCode::Char('c') | KeyCode::Delete => {
                if self.marked.is_empty() {
                    self.tab = 1;
                    self.toast("Tandai cache dulu dengan [Space], atau [a] untuk semua yang aman", th::YELLOW);
                } else {
                    self.modal = Some(Modal::ConfirmClean);
                }
            }
            KeyCode::Char('o') => {
                let path = match self.tab {
                    1 => self.selected_cache().map(|i| self.result.as_ref().unwrap().caches[i].path.clone()),
                    4 => self.states[4].selected().and_then(|i| {
                        self.result
                            .as_ref()
                            .unwrap()
                            .largest
                            .get(i)
                            .and_then(|f| Path::new(&f.path).parent().map(|p| p.to_string_lossy().into_owned()))
                    }),
                    _ => None,
                };
                if let Some(p) = path {
                    open_path(&p);
                    self.toast(format!("Membuka {}", trunc_left(&p, 60)), th::ACCENT);
                }
            }
            KeyCode::Enter if self.tab == 1 => {
                if let Some(i) = self.selected_cache() {
                    self.modal = Some(Modal::Detail(i));
                }
            }
            _ => {}
        }
    }

    // ===================================================================== render

    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        f.render_widget(Block::new().style(Style::new().bg(th::BG).fg(th::FG)), area);
        match self.screen {
            Screen::Home => self.draw_home(f, area),
            Screen::Scanning => self.draw_scanning(f, area),
            Screen::Results => self.draw_results(f, area),
        }
        if self.clean.is_some() {
            self.draw_clean_progress(f, area);
        }
        if self.modal.is_some() {
            self.draw_modal(f, area);
        }
    }

    fn draw_footer(&self, f: &mut Frame, area: Rect, keys: Vec<Span<'static>>) {
        if let Some((msg, color, _)) = &self.toast {
            let line = Line::from(vec![
                Span::styled(" ● ", Style::new().fg(th::BG).bg(*color).bold()),
                Span::styled(format!(" {msg}"), Style::new().fg(*color)),
            ]);
            f.render_widget(Paragraph::new(line), area);
        } else {
            f.render_widget(Paragraph::new(Line::from(keys)), area);
        }
    }

    fn scrollbar(&mut self, f: &mut Frame, area: Rect, len: usize, tab: usize) {
        let visible = area.height.saturating_sub(4) as usize;
        if len <= visible {
            return;
        }
        let mut st = ScrollbarState::new(len).position(self.states[tab].selected().unwrap_or(0));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::new().fg(th::ACCENT))
                .track_style(Style::new().fg(th::MUTED))
                .begin_symbol(None)
                .end_symbol(None),
            area.inner(Margin { horizontal: 0, vertical: 1 }),
            &mut st,
        );
    }

    // ------------------------------------------------------------ home

    fn draw_home(&mut self, f: &mut Frame, area: Rect) {
        let big = area.width >= 92 && area.height >= 30;
        let logo_h = if big { LOGO.len() as u16 + 2 } else { 2 };
        let [logo, drives_area, menu, foot] = Layout::vertical([
            Constraint::Length(logo_h),
            Constraint::Min(6),
            Constraint::Length(9),
            Constraint::Length(1),
        ])
        .margin(1)
        .areas(area);

        let mut lines: Vec<Line> = if big {
            LOGO.iter()
                .enumerate()
                .map(|(i, l)| Line::from(Span::styled(*l, Style::new().fg(grad(i, LOGO.len())).bold())))
                .collect()
        } else {
            vec![Line::from(Span::styled("◆ CACHECHECKER", Style::new().fg(th::ACCENT).bold()))]
        };
        lines.push(Line::from(vec![
            Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::new().fg(th::PURPLE)),
            Span::styled("  ·  disk cache auditor  ·  ", Style::new().fg(th::DIM)),
            if is_admin() {
                Span::styled("● Administrator", Style::new().fg(th::GREEN))
            } else {
                Span::styled("● User biasa — folder sistem dilewati", Style::new().fg(th::YELLOW))
            },
        ]));
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), logo);

        // ---- drive table
        let bw = drives_area.width.saturating_sub(100).clamp(10, 30) as usize;
        let rows: Vec<Row> = self
            .drives
            .iter()
            .zip(&self.drive_sel)
            .map(|(d, sel)| {
                let pct = d.percent();
                let mut gauge = bar_spans(pct / 100.0, bw, usage_color(pct));
                gauge.push(Span::styled(format!(" {pct:5.1}%"), Style::new().fg(usage_color(pct)).bold()));
                let kind_style = match d.kind.as_str() {
                    "cloud" => Style::new().fg(th::PURPLE),
                    "removable" => Style::new().fg(th::YELLOW),
                    _ => Style::new().fg(th::DIM),
                };
                Row::new(vec![
                    Cell::from(if *sel {
                        Span::styled("◉", Style::new().fg(th::GREEN).bold())
                    } else {
                        Span::styled("○", Style::new().fg(th::MUTED))
                    }),
                    Cell::from(Span::styled(d.mount.clone(), Style::new().fg(th::CYAN).bold())),
                    Cell::from(trunc_right(if d.label.is_empty() { "-" } else { &d.label }, 24)),
                    Cell::from(Span::styled(d.kind.clone(), kind_style)),
                    Cell::from(Span::styled(d.fstype.clone(), Style::new().fg(th::DIM))),
                    rcell(human_size(d.used)),
                    rcell(human_size(d.total)),
                    rcell_s(human_size(d.free), Style::new().fg(th::GREEN)),
                    Cell::from(Line::from(gauge)),
                ])
                .height(1)
            })
            .collect();
        let tot: u64 = self.drives.iter().map(|d| d.total).sum();
        let used: u64 = self.drives.iter().map(|d| d.used).sum();
        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(4),
                Constraint::Length(24),
                Constraint::Length(9),
                Constraint::Length(6),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Min(bw as u16 + 8),
            ],
        )
        .header(
            Row::new(["", "", "Label", "Tipe", "FS", "Terpakai", "Total", "Free", "Penggunaan"])
                .style(Style::new().fg(th::DIM).bold())
                .bottom_margin(1),
        )
        .row_highlight_style(Style::new().bg(th::SEL))
        .highlight_symbol("▶ ")
        .column_spacing(2)
        .block(
            panel("Disk").title_bottom(
                Line::from(Span::styled(
                    format!(
                        " total {} / {} · free {} ",
                        human_size(used),
                        human_size(tot),
                        human_size(tot.saturating_sub(used))
                    ),
                    Style::new().fg(th::DIM),
                ))
                .right_aligned(),
            ),
        );
        f.render_stateful_widget(table, drives_area, &mut self.home_state);

        // ---- menu
        let n_sel = self.drive_sel.iter().filter(|s| **s).count();
        let mut menu_lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                key("Enter"),
                Span::styled("  Full scan  ", Style::new().fg(th::FG).bold()),
                Span::styled(format!("{n_sel} drive terpilih"), Style::new().fg(th::CYAN)),
                Span::styled(" — total file, statistik, folder & cache", Style::new().fg(th::DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                key("  C  "),
                Span::styled("  Quick cache scan  ", Style::new().fg(th::FG).bold()),
                Span::styled("hanya lokasi cache yang dikenal, selesai dalam detik", Style::new().fg(th::DIM)),
            ]),
            Line::from(""),
        ];
        let mut nav = vec![Span::raw("  ")];
        nav.extend(hint("Space", "pilih drive"));
        nav.extend(hint("A", "semua"));
        if self.result.is_some() {
            nav.extend(hint("Tab", "hasil terakhir"));
        }
        nav.extend(hint("?", "bantuan"));
        nav.extend(hint("Q", "keluar"));
        menu_lines.push(Line::from(nav));
        f.render_widget(Paragraph::new(menu_lines).block(panel("Mulai")), menu);
        self.draw_footer(
            f,
            foot,
            vec![Span::styled(
                format!(" {} thread · drive cloud (Google Drive dll) tidak dipilih default karena lambat", self.threads),
                Style::new().fg(th::MUTED),
            )],
        );
    }

    // ------------------------------------------------------------ scanning

    fn draw_scanning(&mut self, f: &mut Frame, area: Rect) {
        let Some(job) = &self.scan else { return };
        let p = &job.progress;
        let files = p.files.load(Ordering::Relaxed);
        let dirs = p.dirs.load(Ordering::Relaxed);
        let size = p.size.load(Ordering::Relaxed);
        let errs = p.errors.load(Ordering::Relaxed);
        let caches = p.caches.load(Ordering::Relaxed);
        let el = job.started.elapsed().as_secs_f64();
        let rate = if el > 0.0 { files as f64 / el } else { 0.0 };
        let cancel = p.cancelled();

        let [head, gauge, cards, spark, cur, foot] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .margin(1)
        .areas(area);

        let spin = SPIN[self.frame % SPIN.len()];
        let title = Line::from(vec![
            Span::styled(format!(" {spin} "), Style::new().fg(th::PURPLE).bold()),
            Span::styled(job.label.clone(), Style::new().fg(th::FG).bold()),
            Span::styled(format!("   ·   {} thread   ·   {}", self.threads, fmt_duration(el)), Style::new().fg(th::DIM)),
            if cancel { Span::styled("   ·   membatalkan…", Style::new().fg(th::YELLOW)) } else { Span::raw("") },
        ]);
        f.render_widget(Paragraph::new(title).block(panel("Scanning")), head);

        // progress: perkiraan berdasarkan kapasitas disk terpakai
        let (ratio, label) = if job.expected > 0 {
            let r = (size as f64 / job.expected as f64).min(0.999);
            let eta = if r > 0.02 { format!("~{}", fmt_duration(el / r - el)) } else { "menghitung…".into() };
            (r, format!("{:.1}%  ·  {} / {}  ·  ETA {}", r * 100.0, human_size(size), human_size(job.expected), eta))
        } else {
            let t = (self.frame % 40) as f64 / 39.0;
            (if (self.frame / 40) % 2 == 0 { t } else { 1.0 - t }, format!("{} terkumpul", human_size(size)))
        };
        f.render_widget(
            Gauge::default()
                .block(panel("Progress"))
                .gauge_style(Style::new().fg(th::ACCENT).bg(th::SEL))
                .use_unicode(true)
                .ratio(ratio)
                .label(Span::styled(label, Style::new().fg(th::FG).bold())),
            gauge,
        );

        let cs = Layout::horizontal([Constraint::Ratio(1, 6); 6]).split(cards);
        stat_card(f, cs[0], "File", fmt_num(files), th::ACCENT, format!("{:.0} file/s rata2", rate));
        stat_card(f, cs[1], "Folder", fmt_num(dirs), th::PURPLE, String::new());
        stat_card(f, cs[2], "Ukuran", human_size(size), th::CYAN, String::new());
        stat_card(f, cs[3], "Lokasi cache", fmt_num(caches), th::YELLOW, String::new());
        stat_card(f, cs[4], "Error akses", fmt_num(errs), if errs > 0 { th::RED } else { th::DIM }, String::new());
        stat_card(f, cs[5], "Puncak", fmt_num(job.peak), th::GREEN, "file/detik".into());

        let w = spark.width.saturating_sub(2) as usize;
        let data: Vec<u64> = job.hist.iter().rev().take(w).rev().copied().collect();
        f.render_widget(
            Sparkline::default().block(panel("Throughput (file/detik)")).data(&data).style(Style::new().fg(th::TEAL)),
            spark,
        );

        let current = p.current.lock().map(|c| c.clone()).unwrap_or_default();
        f.render_widget(
            Paragraph::new(Span::styled(
                trunc_left(&current, cur.width.saturating_sub(4) as usize),
                Style::new().fg(th::DIM),
            ))
            .block(panel("Sedang dibaca")),
            cur,
        );
        self.draw_footer(f, foot, hint("Esc", "batalkan — hasil parsial tetap ditampilkan"));
    }

    // ------------------------------------------------------------ results

    fn draw_results(&mut self, f: &mut Frame, area: Rect) {
        let Some(r) = &self.result else { return };
        let [head, tabs, body, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .areas(area);

        let m = &r.meta;
        let head_line = Line::from(vec![
            Span::styled(" ◆ CACHECHECKER ", Style::new().fg(th::BG).bg(th::ACCENT).bold()),
            Span::styled(format!(" {} ", m.host), Style::new().fg(th::BG).bg(th::PURPLE).bold()),
            Span::styled(
                format!(
                    "  {}  ·  {}  ·  {}  ·  {}",
                    trunc_left(&report::short_roots(&m.roots), 40),
                    if m.mode == "full" { "full scan" } else { "cache scan" },
                    fmt_duration(m.elapsed),
                    fmt_time(m.started)
                ),
                Style::new().fg(th::DIM),
            ),
            if m.cancelled { Span::styled("  PARSIAL", Style::new().fg(th::YELLOW).bold()) } else { Span::raw("") },
        ]);
        f.render_widget(Paragraph::new(head_line), head);

        let n_caches = r.caches.len();
        let titles: Vec<Line> = TABS
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let extra = if i == 1 { format!(" ({n_caches})") } else { String::new() };
                Line::from(vec![Span::styled(format!("{} ", i + 1), Style::new().fg(th::MUTED)), Span::raw(format!("{t}{extra}"))])
            })
            .collect();
        f.render_widget(
            Tabs::new(titles)
                .select(self.tab)
                .style(Style::new().fg(th::DIM))
                .highlight_style(Style::new().fg(th::ACCENT).bold().underlined())
                .divider(Span::styled("│", Style::new().fg(th::MUTED)))
                .padding("  ", "  "),
            Rect { y: tabs.y + 1, height: 1, ..tabs },
        );

        let full = m.mode == "full";
        let body = body.inner(Margin { horizontal: 1, vertical: 0 });
        match self.tab {
            0 => self.draw_overview(f, body),
            1 => self.draw_caches(f, body),
            2 if full => self.draw_folders(f, body),
            3 if full => self.draw_exts(f, body),
            4 if full => self.draw_largest(f, body),
            5 if full => self.draw_stats(f, body),
            _ => {
                let msg = vec![
                    Line::from(""),
                    Line::from(""),
                    Line::from(Span::styled("Data ini hanya tersedia di Full scan.", Style::new().fg(th::YELLOW).bold())),
                    Line::from(Span::styled(
                        "Tekan [r] untuk kembali ke menu, lalu [Enter] untuk Full scan.",
                        Style::new().fg(th::DIM),
                    )),
                ];
                f.render_widget(Paragraph::new(msg).alignment(Alignment::Center).block(panel(TABS[self.tab])), body);
            }
        }

        let mut keys: Vec<Span<'static>> = match self.tab {
            1 => [
                hint("Space", "tandai"),
                hint("a", "tandai aman"),
                hint("c", "clean"),
                hint("/", "filter"),
                hint("s", "urut"),
                hint("f", "aman saja"),
                hint("Enter", "detail"),
            ]
            .concat(),
            4 => [hint("o", "buka folder"), hint("↑↓", "navigasi")].concat(),
            _ => [hint("Tab", "pindah tab"), hint("↑↓", "navigasi")].concat(),
        };
        keys.extend([hint("e", "export"), hint("r", "menu"), hint("?", "bantuan"), hint("q", "keluar")].concat());
        self.draw_footer(f, foot, keys);
    }

    fn draw_overview(&mut self, f: &mut Frame, area: Rect) {
        let Some(r) = &self.result else { return };
        let t = &r.totals;
        let [cards, mid, gauges] =
            Layout::vertical([Constraint::Length(5), Constraint::Min(8), Constraint::Length(5)]).areas(area);

        let cs = Layout::horizontal([Constraint::Ratio(1, 6); 6]).split(cards);
        stat_card(f, cs[0], "Total file", fmt_num(t.files), th::ACCENT, format!("{} folder", fmt_num(t.dirs)));
        stat_card(f, cs[1], "Total ukuran", human_size(t.size), th::CYAN, format!("{} root", r.drives.len()));
        stat_card(f, cs[2], "Total cache", human_size(t.cache_size), th::YELLOW, format!("{} file", fmt_num(t.cache_files)));
        stat_card(f, cs[3], "Aman dihapus", human_size(t.cache_safe_size), th::GREEN, "dibuat ulang otomatis".into());
        stat_card(f, cs[4], "Lokasi cache", fmt_num(t.cache_count), th::PURPLE, format!("{} kategori", r.cache_by_category.len()));
        stat_card(
            f,
            cs[5],
            "Error akses",
            fmt_num(t.errors),
            if t.errors > 0 { th::RED } else { th::DIM },
            if t.errors > 0 { "butuh admin".into() } else { "-".into() },
        );

        let [left, right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(mid);

        // kategori
        let mx = r.cache_by_category.first().map(|c| c.size).unwrap_or(1).max(1);
        let bw = left.width.saturating_sub(36).max(6) as usize;
        let lines: Vec<Line> = r
            .cache_by_category
            .iter()
            .map(|c| {
                let col = cat_color(&c.category);
                let mut sp = vec![
                    Span::styled(format!(" {:<13}", c.category), Style::new().fg(col).bold()),
                    Span::styled(format!("{:>4} ", c.count), Style::new().fg(th::DIM)),
                ];
                sp.extend(bar_spans(c.size as f64 / mx as f64, bw, col));
                sp.push(Span::styled(format!(" {:>10}", human_size(c.size)), Style::new().fg(th::FG).bold()));
                Line::from(sp)
            })
            .collect();
        f.render_widget(Paragraph::new(lines).block(panel("Cache per Kategori")), left);

        // top cache
        let n = right.height.saturating_sub(2) as usize;
        let mx = r.caches.first().map(|c| c.size).unwrap_or(1).max(1);
        let bw = 14usize;
        let nw = right.width.saturating_sub(bw as u16 + 22).max(10) as usize;
        let lines: Vec<Line> = r
            .caches
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, c)| {
                let col = cat_color(&c.category);
                let mut sp = vec![
                    Span::styled(format!(" {:>2} ", i + 1), Style::new().fg(th::MUTED)),
                    Span::styled(if c.safe { "✔ " } else { "● " }, Style::new().fg(if c.safe { th::GREEN } else { th::YELLOW })),
                    Span::styled(format!("{:<nw$}", trunc_right(&c.name, nw)), Style::new().fg(th::FG)),
                ];
                sp.extend(bar_spans(c.size as f64 / mx as f64, bw, col));
                sp.push(Span::styled(format!(" {:>10}", human_size(c.size)), Style::new().fg(th::FG).bold()));
                Line::from(sp)
            })
            .collect();
        f.render_widget(Paragraph::new(lines).block(panel("Cache Terbesar")), right);

        // gauges
        let [g1, g2] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(gauges);
        let disk_used: u64 = self.drives.iter().filter(|d| d.kind != "cloud").map(|d| d.used).sum();
        let r1 = if disk_used > 0 { (t.cache_size as f64 / disk_used as f64).min(1.0) } else { 0.0 };
        let r2 = if t.cache_size > 0 { t.cache_safe_size as f64 / t.cache_size as f64 } else { 0.0 };
        let gauge = |title: &str, ratio: f64, col: Color, label: String| {
            Gauge::default()
                .block(panel(title).padding(Padding::new(1, 1, 1, 0)))
                .gauge_style(Style::new().fg(col).bg(th::SEL))
                .use_unicode(true)
                .ratio(ratio)
                .label(Span::styled(label, Style::new().fg(th::FG).bold()))
        };
        f.render_widget(
            gauge(
                "Cache vs Disk Terpakai",
                r1,
                th::YELLOW,
                format!("{:.1}%  ·  {} dari {}", r1 * 100.0, human_size(t.cache_size), human_size(disk_used)),
            ),
            g1,
        );
        f.render_widget(
            gauge(
                "Porsi Aman Dihapus",
                r2,
                th::GREEN,
                format!("{:.1}%  ·  {} dari {}", r2 * 100.0, human_size(t.cache_safe_size), human_size(t.cache_size)),
            ),
            g2,
        );
    }

    fn draw_caches(&mut self, f: &mut Frame, area: Rect) {
        let vis = self.visible();
        let Some(r) = &self.result else { return };
        let mx = r.caches.iter().map(|c| c.size).max().unwrap_or(1).max(1);
        let fixed: u16 = 2 + 1 + 26 + 13 + 7 + 9 + 11 + 12 + 8 + 2;
        let pw = area.width.saturating_sub(fixed).max(10) as usize;

        let rows: Vec<Row> = vis
            .iter()
            .map(|&i| {
                let c = &r.caches[i];
                let marked = self.marked.contains(&i);
                let col = cat_color(&c.category);
                Row::new(vec![
                    Cell::from(if marked {
                        Span::styled("■", Style::new().fg(th::RED).bold())
                    } else {
                        Span::styled("□", Style::new().fg(th::MUTED))
                    }),
                    Cell::from(Span::styled(trunc_right(&c.name, 26), Style::new().fg(if marked { th::RED } else { th::FG }))),
                    Cell::from(Span::styled(c.category.clone(), Style::new().fg(col))),
                    Cell::from(if c.safe {
                        Span::styled("✔ aman", Style::new().fg(th::GREEN))
                    } else {
                        Span::styled("● cek", Style::new().fg(th::YELLOW))
                    }),
                    rcell(fmt_num(c.files)),
                    rcell_s(human_size(c.size), Style::new().bold()),
                    Cell::from(Line::from(bar_spans(c.size as f64 / mx as f64, 12, col))),
                    Cell::from(Span::styled(trunc_left(&c.path, pw), Style::new().fg(th::DIM))),
                ])
            })
            .collect();

        let marked_size: u64 = self.marked.iter().filter_map(|i| r.caches.get(*i)).map(|c| c.size).sum();
        let vis_size: u64 = vis.iter().map(|i| r.caches[*i].size).sum();
        let mut title_bits =
            vec![Span::styled(format!(" {} lokasi · {} ", vis.len(), human_size(vis_size)), Style::new().fg(th::DIM))];
        if self.filter_mode || !self.filter.is_empty() {
            title_bits.push(Span::styled(
                format!(" filter: {}{} ", self.filter, if self.filter_mode { "▏" } else { "" }),
                Style::new().fg(th::BG).bg(th::YELLOW).bold(),
            ));
            title_bits.push(Span::raw(" "));
        }
        if self.safe_only {
            title_bits.push(Span::styled(" aman saja ", Style::new().fg(th::BG).bg(th::GREEN).bold()));
            title_bits.push(Span::raw(" "));
        }
        title_bits.push(Span::styled(format!(" urut: {} ", self.sort.label()), Style::new().fg(th::BG).bg(th::ACCENT)));
        title_bits.push(Span::raw(" "));

        let bottom = if self.marked.is_empty() {
            Line::from(Span::styled(" belum ada yang ditandai ", Style::new().fg(th::MUTED)))
        } else {
            Line::from(vec![
                Span::styled(
                    format!(" ■ {} ditandai · {} ", self.marked.len(), human_size(marked_size)),
                    Style::new().fg(th::RED).bold(),
                ),
                Span::styled("— tekan [c] untuk clean ", Style::new().fg(th::DIM)),
            ])
        };

        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(26),
                Constraint::Length(12),
                Constraint::Length(6),
                Constraint::Length(9),
                Constraint::Length(11),
                Constraint::Length(12),
                Constraint::Min(10),
            ],
        )
        .header(
            Row::new(["", "Nama", "Kategori", "Status", "Files", "Size", "", "Path"])
                .style(Style::new().fg(th::DIM).bold())
                .bottom_margin(1),
        )
        .row_highlight_style(Style::new().bg(th::SEL).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ")
        .column_spacing(1)
        .block(panel("Lokasi Cache").title(Line::from(title_bits).right_aligned()).title_bottom(bottom.right_aligned()));
        f.render_stateful_widget(table, area, &mut self.states[1]);
        self.scrollbar(f, area, vis.len(), 1);
    }

    fn draw_folders(&mut self, f: &mut Frame, area: Rect) {
        let rows_data = self.folder_rows();
        let mx = rows_data.first().map(|x| x.1.size).unwrap_or(1).max(1);
        let multi = self.result.as_ref().map(|r| r.drives.len() > 1).unwrap_or(false);
        let rows: Vec<Row> = rows_data
            .iter()
            .map(|(d, x, dsize)| {
                let pct = if *dsize > 0 { x.size as f64 / *dsize as f64 * 100.0 } else { 0.0 };
                Row::new(vec![
                    Cell::from(Span::styled(d.clone(), Style::new().fg(th::CYAN))),
                    Cell::from(trunc_right(&x.name, 44)),
                    rcell(fmt_num(x.count)),
                    rcell_s(human_size(x.size), Style::new().bold()),
                    rcell_s(format!("{pct:.1}%"), Style::new().fg(th::DIM)),
                    Cell::from(Line::from(bar_spans(x.size as f64 / mx as f64, 30, th::ACCENT))),
                ])
            })
            .collect();
        let n = rows_data.len();
        let table = Table::new(
            rows,
            [
                Constraint::Length(if multi { 6 } else { 0 }),
                Constraint::Length(44),
                Constraint::Length(10),
                Constraint::Length(11),
                Constraint::Length(7),
                Constraint::Min(10),
            ],
        )
        .header(Row::new(["Root", "Folder", "Files", "Size", "%", ""]).style(Style::new().fg(th::DIM).bold()).bottom_margin(1))
        .row_highlight_style(Style::new().bg(th::SEL))
        .highlight_symbol("▶ ")
        .column_spacing(2)
        .block(panel("Folder Teratas (level 1 tiap root)"));
        f.render_stateful_widget(table, area, &mut self.states[2]);
        self.scrollbar(f, area, n, 2);
    }

    fn draw_exts(&mut self, f: &mut Frame, area: Rect) {
        let Some(r) = &self.result else { return };
        let total = r.totals.size.max(1);
        let mx = r.extensions.first().map(|e| e.size).unwrap_or(1).max(1);
        let palette = [th::ACCENT, th::PURPLE, th::CYAN, th::TEAL, th::GREEN, th::YELLOW, th::ORANGE, th::RED];
        let rows: Vec<Row> = r
            .extensions
            .iter()
            .take(500)
            .enumerate()
            .map(|(i, e)| {
                let col = palette[i % palette.len()];
                Row::new(vec![
                    Cell::from(Span::styled(e.name.clone(), Style::new().fg(col).bold())),
                    rcell(fmt_num(e.count)),
                    rcell_s(human_size(e.size), Style::new().bold()),
                    rcell_s(format!("{:.1}%", e.size as f64 / total as f64 * 100.0), Style::new().fg(th::DIM)),
                    Cell::from(Line::from(bar_spans(e.size as f64 / mx as f64, 40, col))),
                ])
            })
            .collect();
        let n = rows.len();
        let table = Table::new(
            rows,
            [Constraint::Length(14), Constraint::Length(12), Constraint::Length(11), Constraint::Length(7), Constraint::Min(10)],
        )
        .header(Row::new(["Ekstensi", "Jumlah", "Size", "%", ""]).style(Style::new().fg(th::DIM).bold()).bottom_margin(1))
        .row_highlight_style(Style::new().bg(th::SEL))
        .highlight_symbol("▶ ")
        .column_spacing(2)
        .block(panel("Ekstensi (berdasarkan ukuran)"));
        f.render_stateful_widget(table, area, &mut self.states[3]);
        self.scrollbar(f, area, n, 3);
    }

    fn draw_largest(&mut self, f: &mut Frame, area: Rect) {
        let Some(r) = &self.result else { return };
        let mx = r.largest.first().map(|e| e.size).unwrap_or(1).max(1);
        let pw = area.width.saturating_sub(2 + 11 + 17 + 16 + 6 + 4).max(10) as usize;
        let rows: Vec<Row> = r
            .largest
            .iter()
            .map(|x| {
                Row::new(vec![
                    rcell_s(human_size(x.size), Style::new().fg(th::YELLOW).bold()),
                    Cell::from(Span::styled(fmt_time(x.mtime), Style::new().fg(th::DIM))),
                    Cell::from(Line::from(bar_spans(x.size as f64 / mx as f64, 16, th::ORANGE))),
                    Cell::from(trunc_left(&x.path, pw)),
                ])
            })
            .collect();
        let n = rows.len();
        let table = Table::new(rows, [Constraint::Length(11), Constraint::Length(17), Constraint::Length(16), Constraint::Min(10)])
            .header(Row::new(["Size", "Modified", "", "Path"]).style(Style::new().fg(th::DIM).bold()).bottom_margin(1))
            .row_highlight_style(Style::new().bg(th::SEL))
            .highlight_symbol("▶ ")
            .column_spacing(2)
            .block(panel(&format!("{n} File Terbesar")));
        f.render_stateful_widget(table, area, &mut self.states[4]);
        self.scrollbar(f, area, n, 4);
    }

    fn draw_stats(&mut self, f: &mut Frame, area: Rect) {
        let Some(r) = &self.result else { return };
        let t = &r.totals;
        let [top, bottom] = Layout::vertical([Constraint::Length(9), Constraint::Min(5)]).areas(area);
        let [l, rr] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);

        let mk = |items: &[NamedStat], by_count: bool, col: Color, w: u16| -> Vec<Line<'static>> {
            let mx = items.iter().map(|x| if by_count { x.count } else { x.size }).max().unwrap_or(1).max(1);
            let bw = w.saturating_sub(48).max(6) as usize;
            items
                .iter()
                .map(|x| {
                    let v = if by_count { x.count } else { x.size };
                    let mut sp = vec![Span::styled(format!(" {:<17}", x.name), Style::new().fg(th::FG))];
                    sp.extend(bar_spans(v as f64 / mx as f64, bw, col));
                    sp.push(Span::styled(format!(" {:>11}", fmt_num(x.count)), Style::new().fg(th::DIM)));
                    sp.push(Span::styled(format!(" {:>10}", human_size(x.size)), Style::new().fg(th::FG).bold()));
                    Line::from(sp)
                })
                .collect()
        };
        f.render_widget(Paragraph::new(mk(&r.age, false, th::TEAL, l.width)).block(panel("Umur File (mtime) — by size")), l);
        f.render_widget(
            Paragraph::new(mk(&r.size_dist, true, th::ORANGE, rr.width)).block(panel("Distribusi Ukuran — by jumlah")),
            rr,
        );

        let mx = r.drives.iter().map(|d| d.size).max().unwrap_or(1).max(1);
        let rows: Vec<Row> = r
            .drives
            .iter()
            .map(|d| {
                let cache: u64 = r.caches.iter().filter(|c| c.drive == d.root).map(|c| c.size).sum();
                Row::new(vec![
                    Cell::from(Span::styled(d.root.clone(), Style::new().fg(th::CYAN).bold())),
                    rcell(fmt_num(d.files)),
                    rcell(fmt_num(d.dirs)),
                    rcell_s(human_size(d.size), Style::new().bold()),
                    rcell_s(human_size(cache), Style::new().fg(th::YELLOW)),
                    Cell::from(Line::from(bar_spans(d.size as f64 / mx as f64, 30, th::ACCENT))),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(20),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(11),
                Constraint::Length(11),
                Constraint::Min(10),
            ],
        )
        .header(Row::new(["Root", "Files", "Folder", "Size", "Cache", ""]).style(Style::new().fg(th::DIM).bold()).bottom_margin(1))
        .column_spacing(2)
        .block(panel(&format!("Per Drive · total {} file · {}", fmt_num(t.files), human_size(t.size))));
        f.render_widget(table, bottom);
    }

    // ------------------------------------------------------------ overlay

    fn draw_clean_progress(&self, f: &mut Frame, area: Rect) {
        let Some(job) = &self.clean else { return };
        let rect = centered(area, 70, 9);
        f.render_widget(Clear, rect);
        let freed = job.progress.freed.load(Ordering::Relaxed);
        let files = job.progress.files.load(Ordering::Relaxed);
        let ratio = if job.total > 0 { (freed as f64 / job.total as f64).min(1.0) } else { 0.0 };
        let block = panel(if job.dry { "Dry-run (simulasi)" } else { "Menghapus cache" });
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        let [txt, g] = Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).margin(1).areas(inner);
        let spin = SPIN[self.frame % SPIN.len()];
        f.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(format!("{spin} "), Style::new().fg(th::PURPLE)),
                    Span::styled(format!("{} lokasi · {} file", job.count, fmt_num(files)), Style::new().fg(th::FG).bold()),
                ]),
                Line::from(Span::styled("File yang terkunci / sedang dipakai akan dilewati.", Style::new().fg(th::DIM))),
            ]),
            txt,
        );
        f.render_widget(
            Gauge::default()
                .gauge_style(Style::new().fg(if job.dry { th::CYAN } else { th::RED }).bg(th::SEL))
                .use_unicode(true)
                .ratio(ratio)
                .label(format!("{} / {}", human_size(freed), human_size(job.total))),
            g,
        );
    }

    fn draw_modal(&mut self, f: &mut Frame, area: Rect) {
        let Some(modal) = &self.modal else { return };
        match modal {
            Modal::Help => {
                let rect = centered(area, 76, 28);
                f.render_widget(Clear, rect);
                let row = |k: &str, d: &str| {
                    Line::from(vec![
                        Span::styled(format!("  {k:<16}"), Style::new().fg(th::ACCENT).bold()),
                        Span::styled(d.to_string(), Style::new().fg(th::FG)),
                    ])
                };
                let head = |s: &str| Line::from(Span::styled(format!(" {s}"), Style::new().fg(th::PURPLE).bold()));
                let lines = vec![
                    head("Umum"),
                    row("Tab / ← →", "pindah tab (atau angka 1-6)"),
                    row("↑ ↓ / j k", "navigasi · PgUp PgDn · g/G awal/akhir"),
                    row("e", "export report HTML + JSON ke folder saat ini"),
                    row("r", "kembali ke menu (scan baru)"),
                    row("q / Esc", "keluar"),
                    Line::from(""),
                    head("Tab Cache"),
                    row("Space", "tandai / lepas cache"),
                    row("a", "tandai semua cache AMAN yang tampil"),
                    row("n", "hapus semua tanda"),
                    row("c / Del", "clean cache yang ditandai (ada konfirmasi)"),
                    row("/", "filter nama / path / kategori"),
                    row("s", "ganti urutan: size, files, nama, kategori, terbaru"),
                    row("f", "tampilkan yang aman saja"),
                    row("Enter", "detail cache"),
                    row("o", "buka folder di Explorer"),
                    Line::from(""),
                    head("Status"),
                    Line::from(vec![
                        Span::styled("  ✔ aman          ", Style::new().fg(th::GREEN).bold()),
                        Span::styled("dibuat ulang otomatis oleh aplikasinya", Style::new().fg(th::DIM)),
                    ]),
                    Line::from(vec![
                        Span::styled("  ● cek           ", Style::new().fg(th::YELLOW).bold()),
                        Span::styled("periksa dulu (node_modules, model AI, Maven…)", Style::new().fg(th::DIM)),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled("  tekan tombol apa saja untuk menutup", Style::new().fg(th::MUTED))),
                ];
                f.render_widget(Paragraph::new(lines).block(panel("Bantuan")), rect);
            }
            Modal::ConfirmQuit => {
                let rect = centered(area, 44, 7);
                f.render_widget(Clear, rect);
                let lines = vec![
                    Line::from(""),
                    Line::from(Span::styled("Keluar dari CacheChecker?", Style::new().fg(th::FG).bold())),
                    Line::from(""),
                    Line::from([hint("Y", "ya"), hint("Esc", "batal")].concat()),
                ];
                f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).block(panel("Keluar")), rect);
            }
            Modal::ConfirmClean => {
                let Some(r) = &self.result else { return };
                let targets = self.marked_targets();
                let total: u64 = targets.iter().map(|i| r.caches[*i].size).sum();
                let files: u64 = targets.iter().map(|i| r.caches[*i].files).sum();
                let unsafe_n = targets.iter().filter(|i| !r.caches[**i].safe).count();
                let h = (targets.len().min(10) + 11 + if unsafe_n > 0 { 1 } else { 0 }) as u16;
                let rect = centered(area, 90, h);
                f.render_widget(Clear, rect);
                let mut lines = vec![
                    Line::from(vec![
                        Span::styled(format!(" {} lokasi", targets.len()), Style::new().fg(th::FG).bold()),
                        Span::styled(format!(" · {} file · ", fmt_num(files)), Style::new().fg(th::DIM)),
                        Span::styled(human_size(total), Style::new().fg(th::RED).bold()),
                    ]),
                    Line::from(""),
                ];
                for i in targets.iter().take(10) {
                    let c = &r.caches[*i];
                    lines.push(Line::from(vec![
                        Span::styled(if c.safe { "  ✔ " } else { "  ● " }, Style::new().fg(if c.safe { th::GREEN } else { th::YELLOW })),
                        Span::styled(format!("{:<26}", trunc_right(&c.name, 26)), Style::new().fg(th::FG)),
                        Span::styled(format!("{:>10}  ", human_size(c.size)), Style::new().bold()),
                        Span::styled(trunc_left(&c.path, 44), Style::new().fg(th::DIM)),
                    ]));
                }
                if targets.len() > 10 {
                    lines.push(Line::from(Span::styled(
                        format!("  … dan {} lainnya", targets.len() - 10),
                        Style::new().fg(th::DIM),
                    )));
                }
                lines.push(Line::from(""));
                if unsafe_n > 0 {
                    lines.push(Line::from(Span::styled(
                        format!(" ⚠ {unsafe_n} lokasi berstatus 'cek' — pastikan memang tidak dibutuhkan!"),
                        Style::new().fg(th::YELLOW).bold(),
                    )));
                }
                lines.push(Line::from(Span::styled(
                    " Tutup browser / aplikasi terkait dulu. Isi folder dihapus, folder induk tetap ada.",
                    Style::new().fg(th::DIM),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from([hint("Y", "HAPUS sekarang"), hint("D", "dry-run (simulasi)"), hint("Esc", "batal")].concat()));
                f.render_widget(
                    Paragraph::new(lines).block(panel("Konfirmasi Clean").border_style(Style::new().fg(th::RED))),
                    rect,
                );
            }
            Modal::CleanReport { items, dry } => {
                let h = (items.len().min(14) + 8) as u16;
                let rect = centered(area, 90, h);
                f.render_widget(Clear, rect);
                let freed: u64 = items.iter().map(|i| i.res.freed).sum();
                let files: u64 = items.iter().map(|i| i.res.files).sum();
                let failed: u64 = items.iter().map(|i| i.res.failed).sum();
                let mut lines = vec![Line::from(vec![
                    Span::styled(if *dry { " Bisa dibebaskan: " } else { " Dibebaskan: " }, Style::new().fg(th::DIM)),
                    Span::styled(human_size(freed), Style::new().fg(th::GREEN).bold()),
                    Span::styled(format!(" dari {} file", fmt_num(files)), Style::new().fg(th::DIM)),
                    if failed > 0 {
                        Span::styled(format!(" · {} gagal (terkunci / butuh admin)", fmt_num(failed)), Style::new().fg(th::YELLOW))
                    } else {
                        Span::raw("")
                    },
                ])];
                lines.push(Line::from(""));
                for it in items.iter().take(14) {
                    lines.push(Line::from(vec![
                        if it.res.failed == 0 {
                            Span::styled("  ✔ ", Style::new().fg(th::GREEN))
                        } else {
                            Span::styled("  ! ", Style::new().fg(th::YELLOW))
                        },
                        Span::styled(format!("{:<30}", trunc_right(&it.name, 30)), Style::new().fg(th::FG)),
                        Span::styled(format!("{:>10}", human_size(it.res.freed)), Style::new().bold()),
                        Span::styled(format!("{:>10} file", fmt_num(it.res.files)), Style::new().fg(th::DIM)),
                        if it.res.failed > 0 {
                            Span::styled(format!("  {} gagal", it.res.failed), Style::new().fg(th::YELLOW))
                        } else {
                            Span::raw("")
                        },
                    ]));
                }
                lines.push(Line::from(""));
                if *dry {
                    lines.push(Line::from(Span::styled(
                        " Ini simulasi — tidak ada yang dihapus. Tekan [c] lalu [Y] untuk hapus beneran.",
                        Style::new().fg(th::CYAN),
                    )));
                }
                lines.push(Line::from(Span::styled(" tekan tombol apa saja untuk menutup", Style::new().fg(th::MUTED))));
                f.render_widget(Paragraph::new(lines).block(panel(if *dry { "Hasil Dry-run" } else { "Hasil Clean" })), rect);
            }
            Modal::Detail(i) => {
                let Some(r) = &self.result else { return };
                let Some(c) = r.caches.get(*i) else { return };
                let rect = centered(area, 92, 16);
                f.render_widget(Clear, rect);
                let kv = |k: &str, v: String, col: Color| {
                    Line::from(vec![Span::styled(format!("  {k:<12}"), Style::new().fg(th::DIM)), Span::styled(v, Style::new().fg(col))])
                };
                let lines = vec![
                    Line::from(""),
                    kv("Nama", c.name.clone(), th::FG),
                    kv("Kategori", c.category.clone(), cat_color(&c.category)),
                    kv(
                        "Status",
                        if c.safe { "✔ aman dihapus".into() } else { "● perlu dicek".into() },
                        if c.safe { th::GREEN } else { th::YELLOW },
                    ),
                    kv("Ukuran", format!("{}  ({} byte)", human_size(c.size), fmt_num(c.size)), th::FG),
                    kv("Isi", format!("{} file · {} folder", fmt_num(c.files), fmt_num(c.dirs)), th::FG),
                    kv("Terbaru", fmt_time(c.newest), th::FG),
                    kv("Terlama", fmt_time(c.oldest), th::FG),
                    kv("Deteksi", if c.source == "known" { "lokasi dikenal".into() } else { "pola nama folder".into() }, th::FG),
                    kv("Path", c.path.clone(), th::CYAN),
                    kv("Catatan", if c.note.is_empty() { "-".into() } else { c.note.clone() }, th::YELLOW),
                    Line::from(""),
                    Line::from(Span::styled("  [o] buka folder · tombol lain untuk menutup", Style::new().fg(th::MUTED))),
                ];
                f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(panel("Detail Cache")), rect);
            }
        }
    }

    // ===================================================================== loop

    fn run_loop(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            self.tick();
            term.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        self.on_key(k);
                    }
                }
            }
        }
        if let Some(j) = &self.scan {
            j.progress.cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

/// Jalankan TUI. `initial` = buka report yang sudah ada; `start` = "cache"/"full" untuk langsung scan.
pub fn run(initial: Option<ScanResult>, threads: usize, start: Option<&str>) -> Result<()> {
    let mut app = App::new(threads);
    if let Some(r) = initial {
        app.set_result(r);
    } else {
        match start {
            Some("cache") => app.start_cache_scan(),
            Some("full") => app.start_full_scan(),
            _ => {}
        }
    }
    let mut term = ratatui::init();
    let res = app.run_loop(&mut term);
    ratatui::restore();
    res
}

// ===================================================================== test render headless

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn sample() -> ScanResult {
        // folder dummy berisi cache (__pycache__, node_modules, target+Cargo.toml)
        let base = std::env::temp_dir().join("cachechecker_test");
        let _ = std::fs::remove_dir_all(&base);
        for d in ["proj/__pycache__", "proj/node_modules/x", "rs/target/release", "a/b/c"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("rs/Cargo.toml"), "[package]").unwrap();
        std::fs::write(base.join("proj/__pycache__/m.pyc"), vec![0u8; 4096]).unwrap();
        std::fs::write(base.join("proj/node_modules/x/i.js"), vec![0u8; 2048]).unwrap();
        std::fs::write(base.join("rs/target/release/app.exe"), vec![0u8; 8192]).unwrap();
        std::fs::write(base.join("a/b/c/data.txt"), "hello").unwrap();
        let o = ScanOptions { roots: vec![base], threads: 4, ..Default::default() };
        scanner::run(&o, &Progress::default())
    }

    fn render_all(app: &mut App, w: u16, h: u16) {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
    }

    #[test]
    fn semua_layar_render_tanpa_panic() {
        let mut app = App::new(4);
        for (w, h) in [(160, 50), (100, 30), (60, 20), (20, 8)] {
            app.screen = Screen::Home;
            render_all(&mut app, w, h);
        }
        app.set_result(sample());
        let r = app.result.clone().unwrap();
        assert_eq!(r.totals.files, 5); // + Cargo.toml
        assert_eq!(r.totals.size, 4096 + 2048 + 8192 + 5 + 9);
        let names: Vec<&str> = r.caches.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Python __pycache__"), "{names:?}");
        assert!(names.contains(&"node_modules"), "{names:?}");
        assert!(names.contains(&"Rust target/"), "{names:?}");
        assert_eq!(r.totals.cache_size, 4096 + 2048 + 8192);
        assert_eq!(r.totals.cache_safe_size, 4096 + 8192); // node_modules = 'cek'
        for (w, h) in [(180, 55), (120, 35), (80, 24), (40, 12)] {
            for tab in 0..TABS.len() {
                app.tab = tab;
                render_all(&mut app, w, h);
            }
            app.marked.insert(0);
            for m in [Modal::Help, Modal::ConfirmClean, Modal::ConfirmQuit, Modal::Detail(0)] {
                app.modal = Some(m);
                render_all(&mut app, w, h);
            }
            app.modal = Some(Modal::CleanReport {
                items: vec![CleanItem { idx: 0, name: "x".into(), res: CleanResult::default() }],
                dry: true,
            });
            render_all(&mut app, w, h);
            app.modal = None;
        }
        // navigasi + filter lewat key event
        for c in ['2', 'j', 'j', 's', 'f', '/', 'r', 's', '\n'] {
            let code = if c == '\n' { KeyCode::Enter } else { KeyCode::Char(c) };
            app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
            render_all(&mut app, 120, 35);
        }
    }

    #[test]
    fn scanning_screen_render() {
        let mut app = App::new(4);
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        app.start_scan(ScanOptions { roots: vec![p], threads: 2, ..Default::default() }, "test".into(), 1000);
        for _ in 0..20 {
            app.tick();
            render_all(&mut app, 120, 35);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
