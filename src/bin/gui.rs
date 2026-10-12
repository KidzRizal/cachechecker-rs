//! CacheChecker GUI (egui / eframe) — dashboard desktop native.
//!
//! Semua ikon digambar vektor (tanpa emoji) supaya tajam & konsisten di semua mesin.
//! Kerja berat (scan, clean) jalan di thread terpisah, UI tetap responsif.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // tanpa jendela console

use cachechecker::cleaner::{self, CleanProgress, CleanResult};
use cachechecker::drives::{self, DriveInfo};
use cachechecker::model::*;
use cachechecker::report;
use cachechecker::scanner::{self, Progress, ScanOptions};
use cachechecker::util::{fmt_duration, fmt_num, fmt_time, human_size, is_admin, trunc_left};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Painter, Pos2, Rect,
    RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use egui_extras::{Column, TableBuilder};
use std::collections::HashSet;
use std::f32::consts::{PI, TAU};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// ===================================================================== tema

mod th {
    use eframe::egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(14, 16, 24);
    pub const SIDEBAR: Color32 = Color32::from_rgb(19, 21, 32);
    pub const CARD: Color32 = Color32::from_rgb(25, 28, 42);
    pub const CARD_HI: Color32 = Color32::from_rgb(32, 36, 54);
    pub const INPUT: Color32 = Color32::from_rgb(17, 19, 29);
    pub const BORDER: Color32 = Color32::from_rgb(40, 45, 68);
    pub const FG: Color32 = Color32::from_rgb(226, 231, 252);
    pub const DIM: Color32 = Color32::from_rgb(138, 146, 186);
    pub const MUTED: Color32 = Color32::from_rgb(84, 92, 128);
    pub const ACCENT: Color32 = Color32::from_rgb(122, 162, 247);
    pub const PURPLE: Color32 = Color32::from_rgb(187, 154, 247);
    pub const PINK: Color32 = Color32::from_rgb(255, 121, 198);
    pub const GREEN: Color32 = Color32::from_rgb(158, 206, 106);
    pub const YELLOW: Color32 = Color32::from_rgb(224, 175, 104);
    pub const RED: Color32 = Color32::from_rgb(247, 118, 142);
    pub const CYAN: Color32 = Color32::from_rgb(125, 207, 255);
    pub const ORANGE: Color32 = Color32::from_rgb(255, 158, 100);
    pub const TEAL: Color32 = Color32::from_rgb(115, 218, 202);
}

const PALETTE: [Color32; 9] =
    [th::ACCENT, th::PURPLE, th::GREEN, th::YELLOW, th::RED, th::CYAN, th::ORANGE, th::TEAL, th::PINK];

fn cat_color(c: &str) -> Color32 {
    match c {
        "system" => th::CYAN,
        "browser" => th::ACCENT,
        "app" => th::PURPLE,
        "dev" => th::GREEN,
        "dev-artifact" => th::TEAL,
        "gpu" => th::ORANGE,
        "game" => th::RED,
        "ai" => th::YELLOW,
        "trash" => th::PINK,
        _ => th::DIM,
    }
}

fn cat_icon(c: &str) -> Ic {
    match c {
        "system" => Ic::Cpu,
        "browser" => Ic::Globe,
        "app" => Ic::Cube,
        "dev" => Ic::Code,
        "dev-artifact" => Ic::Folder,
        "gpu" => Ic::Monitor,
        "game" => Ic::Gamepad,
        "ai" => Ic::Sparkle,
        "trash" => Ic::Trash,
        _ => Ic::Dot,
    }
}

fn usage_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        th::RED
    } else if pct >= 75.0 {
        th::YELLOW
    } else {
        th::GREEN
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

// ===================================================================== font

const BOLD: &str = "bold";
const SEMI: &str = "semibold";

fn fr(size: f32) -> FontId {
    FontId::proportional(size)
}
fn fb(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}
fn fs(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMI.into()))
}
fn txt(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).font(fr(size)).color(color)
}
fn btxt(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).font(fb(size)).color(color)
}
fn stxt(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).font(fs(size)).color(color)
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let load = |name: &str| std::fs::read(Path::new(&windir).join("Fonts").join(name)).ok();
    let mut add = |key: &str, file: &str| -> bool {
        match load(file) {
            Some(b) => {
                fonts.font_data.insert(key.into(), Arc::new(egui::FontData::from_owned(b)));
                true
            }
            None => false,
        }
    };
    let has_regular = add("segoe", "segoeui.ttf");
    let has_bold = add("segoe_bold", "segoeuib.ttf");
    let has_semi = add("segoe_semi", "seguisb.ttf");
    let has_sym = add("segoe_sym", "seguisym.ttf");

    let mut fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if has_sym {
        fallback.push("segoe_sym".into());
    }
    let mut prop = Vec::new();
    if has_regular {
        prop.push("segoe".to_string());
    }
    prop.extend(fallback.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, prop.clone());

    let mut bold = Vec::new();
    if has_bold {
        bold.push("segoe_bold".to_string());
    }
    bold.extend(prop.iter().cloned());
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold.clone());

    let mut semi = Vec::new();
    if has_semi {
        semi.push("segoe_semi".to_string());
    }
    semi.extend(bold.iter().cloned());
    fonts.families.insert(FontFamily::Name(SEMI.into()), semi);

    if has_sym {
        fonts.families.entry(FontFamily::Monospace).or_default().push("segoe_sym".into());
    }
    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &egui::Context) {
    setup_fonts(ctx);
    // paksa tema gelap walau Windows memakai light theme
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.panel_fill = th::BG;
    v.window_fill = th::CARD;
    v.window_stroke = Stroke::new(1.0, th::BORDER);
    v.window_corner_radius = CornerRadius::same(16);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 12], blur: 40, spread: 0, color: Color32::from_black_alpha(140) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(120) };
    v.extreme_bg_color = th::INPUT;
    v.faint_bg_color = Color32::from_rgb(22, 25, 38);
    v.selection.bg_fill = th::ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, th::ACCENT);
    v.hyperlink_color = th::ACCENT;
    v.widgets.noninteractive.bg_fill = th::CARD;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, th::FG);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, th::BORDER);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(9);
    }
    v.widgets.inactive.bg_fill = th::CARD_HI;
    v.widgets.inactive.weak_bg_fill = th::CARD_HI;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, th::BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, th::FG);
    v.widgets.hovered.bg_fill = Color32::from_rgb(44, 50, 76);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 50, 76);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, th::MUTED);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, th::FG);
    v.widgets.active.bg_fill = th::ACCENT.gamma_multiply(0.6);
    v.widgets.active.weak_bg_fill = th::ACCENT.gamma_multiply(0.4);
    v.widgets.active.bg_stroke = Stroke::new(1.0, th::ACCENT);
    v.slider_trailing_fill = true;
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = vec2(10.0, 8.0);
        s.spacing.button_padding = vec2(14.0, 7.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.floating_allocated_width = 0.0;
        s.text_styles.insert(egui::TextStyle::Body, fr(14.5));
        s.text_styles.insert(egui::TextStyle::Button, fr(14.5));
        s.text_styles.insert(egui::TextStyle::Heading, fb(24.0));
        s.text_styles.insert(egui::TextStyle::Small, fr(12.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
    });
}

/// Title bar gelap + warna caption sama dengan sidebar (Windows 11).
#[cfg(windows)]
fn style_titlebar() {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
    let title: Vec<u16> = "CacheChecker".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() {
            return;
        }
        let on: i32 = 1;
        DwmSetWindowAttribute(hwnd, 20, &on as *const i32 as *const _, 4); // DWMWA_USE_IMMERSIVE_DARK_MODE
        let c = th::SIDEBAR;
        let caption: u32 = (c.b() as u32) << 16 | (c.g() as u32) << 8 | c.r() as u32;
        DwmSetWindowAttribute(hwnd, 35, &caption as *const u32 as *const _, 4); // DWMWA_CAPTION_COLOR
        let text: u32 = 0x00F0E0DC;
        DwmSetWindowAttribute(hwnd, 36, &text as *const u32 as *const _, 4); // DWMWA_TEXT_COLOR
        DwmSetWindowAttribute(hwnd, 34, &caption as *const u32 as *const _, 4); // DWMWA_BORDER_COLOR
    }
}

#[cfg(not(windows))]
fn style_titlebar() {}

// ===================================================================== ikon vektor (grid 24x24)

#[derive(Clone, Copy, PartialEq)]
enum Ic {
    Home,
    Search,
    Broom,
    Folder,
    Layers,
    File,
    Chart,
    Gear,
    Disk,
    Bolt,
    Trash,
    Check,
    Close,
    Warning,
    Cloud,
    Clock,
    Bars,
    Star,
    Pie,
    Refresh,
    Copy,
    External,
    Eye,
    Shield,
    Globe,
    Braces,
    Doc,
    Table,
    Upload,
    Cpu,
    Cube,
    Code,
    Gamepad,
    Sparkle,
    Monitor,
    Dot,
    Stop,
    Pulse,
}

fn draw_icon(p: &Painter, rect: Rect, ic: Ic, color: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - vec2(12.0 * s, 12.0 * s);
    let pt = |x: f32, y: f32| o + vec2(x * s, y * s);
    let st = Stroke::new((1.9 * s).max(1.2), color);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|(x, y)| pt(*x, *y)).collect(), st));
    };
    let closed = |pts: &[(f32, f32)]| {
        p.add(Shape::closed_line(pts.iter().map(|(x, y)| pt(*x, *y)).collect(), st));
    };
    let circle = |x: f32, y: f32, r: f32| {
        p.circle_stroke(pt(x, y), r * s, st);
    };
    let dot = |x: f32, y: f32, r: f32| {
        p.circle_filled(pt(x, y), r * s, color);
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| {
        let n = 32;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                pt(cx + a.cos() * r, cy + a.sin() * r)
            })
            .collect();
        p.add(Shape::line(pts, st));
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        p.rect_stroke(
            Rect::from_min_max(pt(x0, y0), pt(x1, y1)),
            CornerRadius::same((r * s) as u8),
            st,
            StrokeKind::Middle,
        );
    };
    match ic {
        Ic::Home => {
            line(&[(3.0, 11.0), (12.0, 3.5), (21.0, 11.0)]);
            line(&[(5.5, 9.5), (5.5, 20.5), (18.5, 20.5), (18.5, 9.5)]);
            line(&[(10.0, 20.5), (10.0, 14.5), (14.0, 14.5), (14.0, 20.5)]);
        }
        Ic::Search => {
            circle(10.5, 10.5, 6.5);
            line(&[(15.5, 15.5), (20.5, 20.5)]);
        }
        Ic::Broom => {
            line(&[(20.0, 3.5), (13.0, 10.5)]);
            closed(&[(11.5, 9.5), (15.0, 13.0), (10.5, 20.5), (3.5, 20.5), (5.0, 15.5)]);
            line(&[(7.5, 20.5), (9.0, 16.5)]);
            line(&[(4.5, 17.5), (7.0, 14.5)]);
        }
        Ic::Folder => closed(&[(3.0, 6.0), (9.0, 6.0), (11.0, 8.5), (21.0, 8.5), (21.0, 19.0), (3.0, 19.0)]),
        Ic::Layers => {
            closed(&[(12.0, 3.0), (21.0, 7.5), (12.0, 12.0), (3.0, 7.5)]);
            line(&[(3.0, 12.0), (12.0, 16.5), (21.0, 12.0)]);
            line(&[(3.0, 16.5), (12.0, 21.0), (21.0, 16.5)]);
        }
        Ic::File => {
            closed(&[(6.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (6.0, 21.0)]);
            line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
        }
        Ic::Doc => {
            closed(&[(6.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (6.0, 21.0)]);
            line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
            line(&[(9.0, 12.5), (16.0, 12.5)]);
            line(&[(9.0, 16.5), (16.0, 16.5)]);
        }
        Ic::Chart => {
            line(&[(3.0, 20.5), (21.0, 20.5)]);
            rrect(5.0, 12.0, 8.5, 20.5, 1.0);
            rrect(10.5, 7.0, 14.0, 20.5, 1.0);
            rrect(16.0, 3.5, 19.5, 20.5, 1.0);
        }
        Ic::Gear => {
            circle(12.0, 12.0, 3.2);
            circle(12.0, 12.0, 7.0);
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0;
                line(&[(12.0 + a.cos() * 7.0, 12.0 + a.sin() * 7.0), (12.0 + a.cos() * 9.5, 12.0 + a.sin() * 9.5)]);
            }
        }
        Ic::Disk => {
            rrect(3.0, 6.5, 21.0, 17.5, 2.5);
            line(&[(6.5, 12.0), (12.5, 12.0)]);
            dot(17.0, 12.0, 1.4);
        }
        Ic::Bolt => closed(&[(13.5, 2.5), (4.5, 13.5), (11.5, 13.5), (10.5, 21.5), (19.5, 10.0), (12.5, 10.0)]),
        Ic::Trash => {
            line(&[(4.0, 6.5), (20.0, 6.5)]);
            line(&[(9.0, 6.5), (9.5, 3.5), (14.5, 3.5), (15.0, 6.5)]);
            line(&[(6.0, 6.5), (7.0, 20.5), (17.0, 20.5), (18.0, 6.5)]);
            line(&[(10.0, 10.5), (10.0, 16.5)]);
            line(&[(14.0, 10.5), (14.0, 16.5)]);
        }
        Ic::Check => line(&[(4.5, 12.5), (9.5, 17.5), (19.5, 6.5)]),
        Ic::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Ic::Warning => {
            closed(&[(12.0, 3.0), (21.5, 20.0), (2.5, 20.0)]);
            line(&[(12.0, 9.5), (12.0, 14.0)]);
            dot(12.0, 17.0, 1.2);
        }
        Ic::Cloud => {
            arc(8.5, 14.5, 4.0, PI * 0.5, PI * 1.5);
            arc(13.0, 11.0, 5.5, PI * 1.05, PI * 1.95);
            arc(17.5, 15.0, 3.5, PI * 1.5, PI * 2.5);
            line(&[(8.5, 18.5), (17.5, 18.5)]);
        }
        Ic::Clock => {
            circle(12.0, 12.0, 9.0);
            line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.5)]);
        }
        Ic::Bars => {
            line(&[(5.0, 20.0), (5.0, 14.0)]);
            line(&[(10.0, 20.0), (10.0, 8.0)]);
            line(&[(15.0, 20.0), (15.0, 11.0)]);
            line(&[(20.0, 20.0), (20.0, 4.0)]);
        }
        Ic::Star => {
            let pts: Vec<(f32, f32)> = (0..10)
                .map(|k| {
                    let r = if k % 2 == 0 { 9.5 } else { 4.2 };
                    let a = -PI / 2.0 + k as f32 * PI / 5.0;
                    (12.0 + a.cos() * r, 12.5 + a.sin() * r)
                })
                .collect();
            closed(&pts);
        }
        Ic::Pie => {
            circle(12.0, 12.0, 9.0);
            line(&[(12.0, 3.0), (12.0, 12.0), (19.8, 16.5)]);
        }
        Ic::Refresh => {
            arc(12.0, 12.0, 8.0, -PI * 0.35, PI * 1.45);
            line(&[(18.5, 3.5), (18.6, 7.6), (14.5, 7.6)]);
        }
        Ic::Copy => {
            rrect(8.5, 8.5, 20.0, 20.0, 2.0);
            line(&[(15.5, 8.5), (15.5, 4.0), (4.0, 4.0), (4.0, 15.5), (8.5, 15.5)]);
        }
        Ic::External => {
            line(&[(18.0, 13.5), (18.0, 20.0), (4.0, 20.0), (4.0, 6.0), (10.5, 6.0)]);
            line(&[(14.0, 3.5), (20.5, 3.5), (20.5, 10.0)]);
            line(&[(20.5, 3.5), (11.0, 13.0)]);
        }
        Ic::Eye => {
            let up: Vec<(f32, f32)> =
                (0..=20).map(|i| (2.0 + i as f32, 12.0 - 6.5 * (PI * i as f32 / 20.0).sin())).collect();
            let dn: Vec<(f32, f32)> =
                (0..=20).map(|i| (2.0 + i as f32, 12.0 + 6.5 * (PI * i as f32 / 20.0).sin())).collect();
            line(&up);
            line(&dn);
            circle(12.0, 12.0, 3.0);
        }
        Ic::Shield => closed(&[
            (12.0, 2.5),
            (20.0, 5.5),
            (20.0, 12.0),
            (17.0, 17.0),
            (12.0, 21.0),
            (7.0, 17.0),
            (4.0, 12.0),
            (4.0, 5.5),
        ]),
        Ic::Globe => {
            circle(12.0, 12.0, 9.0);
            let ell: Vec<(f32, f32)> = (0..=40)
                .map(|i| {
                    let a = i as f32 / 40.0 * TAU;
                    (12.0 + a.cos() * 4.0, 12.0 + a.sin() * 9.0)
                })
                .collect();
            line(&ell);
            line(&[(3.0, 12.0), (21.0, 12.0)]);
        }
        Ic::Braces => {
            line(&[(9.0, 3.5), (7.5, 3.5), (6.5, 5.0), (6.5, 10.0), (4.5, 12.0), (6.5, 14.0), (6.5, 19.0), (7.5, 20.5), (9.0, 20.5)]);
            line(&[
                (15.0, 3.5),
                (16.5, 3.5),
                (17.5, 5.0),
                (17.5, 10.0),
                (19.5, 12.0),
                (17.5, 14.0),
                (17.5, 19.0),
                (16.5, 20.5),
                (15.0, 20.5),
            ]);
        }
        Ic::Table => {
            rrect(3.0, 4.0, 21.0, 20.0, 2.0);
            line(&[(3.0, 9.5), (21.0, 9.5)]);
            line(&[(3.0, 14.8), (21.0, 14.8)]);
            line(&[(9.5, 4.0), (9.5, 20.0)]);
        }
        Ic::Upload => {
            line(&[(12.0, 15.0), (12.0, 3.5)]);
            line(&[(7.0, 8.5), (12.0, 3.5), (17.0, 8.5)]);
            line(&[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)]);
        }
        Ic::Cpu => {
            rrect(6.0, 6.0, 18.0, 18.0, 2.0);
            rrect(9.5, 9.5, 14.5, 14.5, 1.0);
            for v in [10.0, 14.0] {
                line(&[(v, 2.5), (v, 6.0)]);
                line(&[(v, 18.0), (v, 21.5)]);
                line(&[(2.5, v), (6.0, v)]);
                line(&[(18.0, v), (21.5, v)]);
            }
        }
        Ic::Cube => {
            closed(&[(12.0, 2.5), (20.5, 7.0), (20.5, 17.0), (12.0, 21.5), (3.5, 17.0), (3.5, 7.0)]);
            line(&[(3.5, 7.0), (12.0, 11.5), (20.5, 7.0)]);
            line(&[(12.0, 11.5), (12.0, 21.5)]);
        }
        Ic::Code => {
            line(&[(8.0, 6.0), (2.5, 12.0), (8.0, 18.0)]);
            line(&[(16.0, 6.0), (21.5, 12.0), (16.0, 18.0)]);
            line(&[(13.5, 4.5), (10.5, 19.5)]);
        }
        Ic::Gamepad => {
            rrect(2.5, 7.0, 21.5, 17.5, 5.0);
            line(&[(6.0, 12.25), (10.0, 12.25)]);
            line(&[(8.0, 10.25), (8.0, 14.25)]);
            dot(15.5, 11.0, 1.2);
            dot(18.0, 13.5, 1.2);
        }
        Ic::Sparkle => closed(&[
            (12.0, 2.5),
            (14.0, 10.0),
            (21.5, 12.0),
            (14.0, 14.0),
            (12.0, 21.5),
            (10.0, 14.0),
            (2.5, 12.0),
            (10.0, 10.0),
        ]),
        Ic::Monitor => {
            rrect(2.5, 4.0, 21.5, 16.0, 2.0);
            line(&[(8.0, 20.5), (16.0, 20.5)]);
            line(&[(12.0, 16.0), (12.0, 20.5)]);
        }
        Ic::Dot => dot(12.0, 12.0, 4.0),
        Ic::Stop => {
            p.rect_filled(Rect::from_min_max(pt(6.5, 6.5), pt(17.5, 17.5)), CornerRadius::same((2.5 * s) as u8), color);
        }
        Ic::Pulse => line(&[(2.5, 12.5), (7.0, 12.5), (9.5, 6.0), (13.5, 19.0), (16.0, 12.5), (21.5, 12.5)]),
    }
}

fn icon(ui: &mut Ui, ic: Ic, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    draw_icon(ui.painter(), rect, ic, color);
    resp
}

/// Ikon di dalam kotak lembut berwarna.
fn icon_badge(p: &Painter, center: Pos2, radius: f32, ic: Ic, color: Color32) {
    p.rect_filled(
        Rect::from_center_size(center, Vec2::splat(radius * 2.0)),
        CornerRadius::same((radius * 0.6) as u8),
        color.gamma_multiply(0.14),
    );
    draw_icon(p, Rect::from_center_size(center, Vec2::splat(radius * 1.05)), ic, color);
}

// ===================================================================== widget

#[derive(Clone, Copy)]
enum Btn {
    Primary(Color32),
    Soft(Color32),
    Ghost,
}

fn button_size(ui: &Ui, ic: Option<Ic>, text: &str, h: f32) -> Vec2 {
    let g = ui.painter().layout_no_wrap(text.to_string(), fs(14.0), th::FG);
    let icon_w = if ic.is_some() { 18.0 + if text.is_empty() { 0.0 } else { 8.0 } } else { 0.0 };
    vec2(g.size().x + icon_w + 32.0, h)
}

fn button(ui: &mut Ui, ic: Option<Ic>, text: &str, kind: Btn) -> egui::Response {
    button_h(ui, ic, text, kind, 38.0)
}

fn button_h(ui: &mut Ui, ic: Option<Ic>, text: &str, kind: Btn, h: f32) -> egui::Response {
    let size = button_size(ui, ic, text, h);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let hover = resp.hovered() && enabled;
    let down = resp.is_pointer_button_down_on() && enabled;
    let (mut fill, mut stroke, mut fg) = match kind {
        Btn::Primary(c) => (if hover { lerp_color(c, Color32::WHITE, 0.12) } else { c }, Stroke::NONE, th::BG),
        Btn::Soft(c) => (
            c.gamma_multiply(if hover { 0.26 } else { 0.16 }),
            Stroke::new(1.0, c.gamma_multiply(if hover { 0.6 } else { 0.35 })),
            c,
        ),
        Btn::Ghost => (
            if hover { Color32::from_rgb(40, 45, 68) } else { th::CARD_HI },
            Stroke::new(1.0, if hover { th::MUTED } else { th::BORDER }),
            th::FG,
        ),
    };
    if !enabled {
        fill = th::CARD_HI;
        stroke = Stroke::new(1.0, th::BORDER);
        fg = th::DIM;
    }
    let rect = if down { rect.shrink(1.0) } else { rect };
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(10), fill, stroke, StrokeKind::Inside);
    let g = p.layout_no_wrap(text.to_string(), fs(14.0), fg);
    let mut x = rect.left() + 16.0;
    if let Some(ic) = ic {
        draw_icon(p, Rect::from_min_size(pos2(x, rect.center().y - 9.0), vec2(18.0, 18.0)), ic, fg);
        x += 18.0 + if text.is_empty() { 0.0 } else { 8.0 };
    }
    p.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
    if hover {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(th::CARD)
        .stroke(Stroke::new(1.0, th::BORDER))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(18))
        .show(ui, add)
        .inner
}

fn section_title(ui: &mut Ui, ic: Ic, title: &str) {
    ui.horizontal(|ui| {
        icon(ui, ic, 18.0, th::PURPLE);
        ui.label(stxt(title, 16.0, th::FG));
    });
    ui.add_space(6.0);
}

fn stat_card(ui: &mut Ui, width: f32, ic: Ic, label: &str, value: &str, sub: &str, color: Color32) {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 104.0), Sense::hover());
    let p = ui.painter_at(rect.expand(1.0));
    let hover = resp.hovered();
    p.rect(
        rect,
        CornerRadius::same(16),
        if hover { th::CARD_HI } else { th::CARD },
        Stroke::new(1.0, if hover { color.gamma_multiply(0.55) } else { th::BORDER }),
        StrokeKind::Inside,
    );
    let top = Rect::from_min_size(rect.min + vec2(18.0, 0.0), vec2(42.0, 3.0));
    p.rect_filled(top, CornerRadius::same(2), color);
    icon_badge(&p, pos2(rect.right() - 36.0, rect.top() + 36.0), 18.0, ic, color);
    p.text(rect.min + vec2(18.0, 18.0), Align2::LEFT_TOP, label.to_uppercase(), fs(11.0), th::DIM);
    p.text(rect.min + vec2(18.0, 38.0), Align2::LEFT_TOP, value, fb(25.0), th::FG);
    p.text(rect.min + vec2(18.0, 76.0), Align2::LEFT_TOP, sub, fr(12.5), color.gamma_multiply(0.9));
}

fn gradient_bar(ui: &mut Ui, ratio: f32, width: f32, height: f32, a: Color32, b: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width.max(height * 2.0), height), Sense::hover());
    paint_gradient_bar(ui.painter(), rect, ratio, a, b);
    resp
}

/// Progress bar bulat dengan gradient (ujung membulat).
fn paint_gradient_bar(p: &Painter, rect: Rect, ratio: f32, a: Color32, b: Color32) {
    let h = rect.height();
    let r = h / 2.0;
    p.rect_filled(rect, CornerRadius::same(r as u8), th::INPUT);
    let ratio = ratio.clamp(0.0, 1.0);
    if ratio <= 0.0 {
        return;
    }
    let w = (rect.width() * ratio).max(h);
    let fill = Rect::from_min_size(rect.min, vec2(w, h));
    let end_col = lerp_color(a, b, ratio.max(0.25));
    p.circle_filled(pos2(fill.left() + r, fill.center().y), r, a);
    p.circle_filled(pos2(fill.right() - r, fill.center().y), r, end_col);
    let inner = Rect::from_min_max(pos2(fill.left() + r, fill.top()), pos2(fill.right() - r, fill.bottom()));
    if inner.width() > 0.5 {
        let mut mesh = egui::Mesh::default();
        let n = 16;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let x = inner.left() + inner.width() * t;
            let col = lerp_color(a, end_col, t);
            mesh.colored_vertex(pos2(x, inner.top()), col);
            mesh.colored_vertex(pos2(x, inner.bottom()), col);
            if i > 0 {
                let k = (i as u32) * 2;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k - 1, k, k + 1);
            }
        }
        p.add(Shape::mesh(mesh));
    }
}

/// Donut chart, return index segmen yang di-hover.
fn donut(
    ui: &mut Ui,
    size: f32,
    parts: &[(String, u64, Color32)],
    center_top: &str,
    center_bottom: &str,
    highlight: Option<usize>,
) -> Option<usize> {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r_out = size / 2.0 - 8.0;
    let thick = size * 0.14;
    let total: u64 = parts.iter().map(|x| x.1).sum();

    let mut hovered = None;
    if let Some(hp) = resp.hover_pos() {
        let d = hp - c;
        let dist = d.length();
        if dist > r_out - thick - 4.0 && dist < r_out + 10.0 && total > 0 {
            let mut ang = d.y.atan2(d.x) + PI / 2.0;
            if ang < 0.0 {
                ang += TAU;
            }
            let mut acc = 0.0;
            for (i, (_, v, _)) in parts.iter().enumerate() {
                let sweep = *v as f32 / total as f32 * TAU;
                if ang >= acc && ang < acc + sweep {
                    hovered = Some(i);
                    break;
                }
                acc += sweep;
            }
        }
    }
    let active = hovered.or(highlight);
    p.circle_stroke(c, r_out - thick / 2.0, Stroke::new(thick, th::INPUT));
    if total > 0 {
        let mut start = -PI / 2.0;
        for (i, (_, v, col)) in parts.iter().enumerate() {
            let sweep = *v as f32 / total as f32 * TAU;
            if sweep <= 0.0 {
                continue;
            }
            let grow = if active == Some(i) { 6.0 } else { 0.0 };
            let dim = active.is_some() && active != Some(i);
            let col = if dim { col.gamma_multiply(0.45) } else { *col };
            let ro = r_out + grow;
            let ri = r_out - thick - grow * 0.3;
            let gap = if parts.len() > 1 { 0.018 } else { 0.0 };
            let (a0, a1) = (start + gap, start + sweep - gap);
            if a1 > a0 {
                let steps = ((sweep / TAU) * 140.0).ceil().max(2.0) as usize;
                let mut mesh = egui::Mesh::default();
                for s in 0..=steps {
                    let a = a0 + (a1 - a0) * s as f32 / steps as f32;
                    let (sn, cs) = a.sin_cos();
                    mesh.colored_vertex(c + vec2(cs * ro, sn * ro), col);
                    mesh.colored_vertex(c + vec2(cs * ri, sn * ri), col.gamma_multiply(0.7));
                    if s > 0 {
                        let k = (s as u32) * 2;
                        mesh.add_triangle(k - 2, k - 1, k);
                        mesh.add_triangle(k - 1, k, k + 1);
                    }
                }
                p.add(Shape::mesh(mesh));
            }
            start += sweep;
        }
    }
    let (top, bottom) = match active {
        Some(i) if i < parts.len() => (human_size(parts[i].1), parts[i].0.clone()),
        _ => (center_top.to_string(), center_bottom.to_string()),
    };
    p.text(c - vec2(0.0, 8.0), Align2::CENTER_CENTER, top, fb(size * 0.1), th::FG);
    p.text(c + vec2(0.0, size * 0.08), Align2::CENTER_CENTER, bottom, fr(12.5), th::DIM);
    hovered
}

fn spinner(ui: &mut Ui, size: f32, t: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r = size / 2.0 - 4.0;
    p.circle_stroke(c, r, Stroke::new(4.5, th::CARD_HI));
    let start = t * 4.5;
    let len = 1.7 + (t * 2.2).sin() * 0.9;
    let n = 40;
    for i in 0..n {
        let a0 = start + len * i as f32 / n as f32;
        let a1 = start + len * (i + 1) as f32 / n as f32;
        let col = lerp_color(th::ACCENT, th::PURPLE, i as f32 / n as f32);
        p.line_segment([c + vec2(a0.cos() * r, a0.sin() * r), c + vec2(a1.cos() * r, a1.sin() * r)], Stroke::new(4.5, col));
    }
}

fn sparkline(ui: &mut Ui, data: &[f32], size: Vec2, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    for k in 1..4 {
        let y = rect.top() + rect.height() * k as f32 / 4.0;
        p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, th::BORDER.gamma_multiply(0.5)));
    }
    if data.len() < 2 {
        return;
    }
    let mx = data.iter().cloned().fold(1.0f32, f32::max);
    let pts: Vec<Pos2> = data
        .iter()
        .enumerate()
        .map(|(i, v)| {
            pos2(
                rect.left() + rect.width() * i as f32 / (data.len() - 1) as f32,
                rect.bottom() - rect.height() * (v / mx) * 0.9,
            )
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    for (i, pt) in pts.iter().enumerate() {
        mesh.colored_vertex(*pt, color.gamma_multiply(0.32));
        mesh.colored_vertex(pos2(pt.x, rect.bottom()), color.gamma_multiply(0.0));
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    p.add(Shape::mesh(mesh));
    if let Some(last) = pts.last() {
        p.circle_filled(*last, 4.0, color);
        p.circle_stroke(*last, 7.0, Stroke::new(1.5, color.gamma_multiply(0.4)));
    }
    p.add(Shape::line(pts, Stroke::new(2.0, color)));
}

fn pill(ui: &mut Ui, ic: Option<Ic>, text: &str, color: Color32) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), fs(11.5), color);
    let iw = if ic.is_some() { 14.0 } else { 0.0 };
    let size = vec2(g.size().x + iw + 18.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(11), color.gamma_multiply(0.15));
    let mut x = rect.left() + 9.0;
    if let Some(ic) = ic {
        draw_icon(p, Rect::from_min_size(pos2(x - 1.0, rect.center().y - 6.5), vec2(13.0, 13.0)), ic, color);
        x += iw;
    }
    p.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, color);
    resp
}

fn safe_pill(ui: &mut Ui, safe: bool) {
    // lebar tetap supaya kolom status rapi
    let (rect, _) = ui.allocate_exact_size(vec2(66.0, 22.0), Sense::hover());
    let (ic, t, col) = if safe { (Ic::Check, "aman", th::GREEN) } else { (Ic::Warning, "cek", th::YELLOW) };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(11), col.gamma_multiply(0.15));
    draw_icon(p, Rect::from_min_size(pos2(rect.left() + 9.0, rect.center().y - 6.5), vec2(13.0, 13.0)), ic, col);
    p.text(pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, t, fs(11.5), col);
}

fn open_path(p: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

/// Potong teks agar muat lebar tertentu (dengan elipsis).
fn fit_text(p: &Painter, s: &str, font: &FontId, maxw: f32) -> String {
    if p.layout_no_wrap(s.to_string(), font.clone(), th::FG).size().x <= maxw {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let t: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if p.layout_no_wrap(t.clone(), font.clone(), th::FG).size().x <= maxw {
            return t;
        }
    }
    "…".into()
}

// ===================================================================== state

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Dashboard,
    Scan,
    Cache,
    Folders,
    Extensions,
    Largest,
    Report,
}

impl Page {
    const ALL: [Page; 7] =
        [Page::Dashboard, Page::Scan, Page::Cache, Page::Folders, Page::Extensions, Page::Largest, Page::Report];
    fn label(self) -> &'static str {
        match self {
            Page::Dashboard => "Dashboard",
            Page::Scan => "Scan",
            Page::Cache => "Cache",
            Page::Folders => "Folder",
            Page::Extensions => "Ekstensi",
            Page::Largest => "File Besar",
            Page::Report => "Report",
        }
    }
    fn icon(self) -> Ic {
        match self {
            Page::Dashboard => Ic::Home,
            Page::Scan => Ic::Search,
            Page::Cache => Ic::Broom,
            Page::Folders => Ic::Folder,
            Page::Extensions => Ic::Layers,
            Page::Largest => Ic::File,
            Page::Report => Ic::Chart,
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum SortBy {
    Size,
    Files,
    Name,
    Category,
    Newest,
}

struct ScanJob {
    progress: Arc<Progress>,
    handle: Option<JoinHandle<ScanResult>>,
    started: Instant,
    expected: u64,
    label: String,
    hist: Vec<f32>,
    last_files: u64,
    last_tick: Instant,
    peak: f32,
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

struct Toast {
    msg: String,
    color: Color32,
    at: Instant,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settings {
    threads: usize,
    include_cloud: bool,
    detect_patterns: bool,
    include_generic: bool,
    excludes: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            threads: scanner::default_threads(),
            include_cloud: false,
            detect_patterns: true,
            include_generic: false,
            excludes: String::new(),
        }
    }
}

struct App {
    page: Page,
    settings: Settings,
    drives: Vec<DriveInfo>,
    drive_sel: Vec<bool>,
    custom_path: String,
    scan: Option<ScanJob>,
    result: Option<ScanResult>,
    marked: HashSet<usize>,
    filter: String,
    cat_filter: Option<String>,
    safe_only: bool,
    sort: SortBy,
    sort_desc: bool,
    detail: Option<usize>,
    confirm_clean: bool,
    clean: Option<CleanJob>,
    clean_report: Option<(Vec<CleanItem>, bool)>,
    toasts: Vec<Toast>,
    last_export: Option<PathBuf>,
    admin: bool,
    t0: Instant,
    frames: u64,
    cat_hover: Option<usize>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        let drives = drives::list_drives(false, true);
        let drive_sel = drives.iter().map(|d| d.kind != "cloud" || settings.include_cloud).collect();
        App {
            page: Page::Dashboard,
            settings,
            drives,
            drive_sel,
            custom_path: String::new(),
            scan: None,
            result: None,
            marked: HashSet::new(),
            filter: String::new(),
            cat_filter: None,
            safe_only: false,
            sort: SortBy::Size,
            sort_desc: true,
            detail: None,
            confirm_clean: false,
            clean: None,
            clean_report: None,
            toasts: Vec::new(),
            last_export: None,
            admin: is_admin(),
            t0: Instant::now(),
            frames: 0,
            cat_hover: None,
        }
    }

    fn toast(&mut self, msg: impl Into<String>, color: Color32) {
        self.toasts.push(Toast { msg: msg.into(), color, at: Instant::now() });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
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
            hist: vec![0.0; 120],
            last_files: 0,
            last_tick: Instant::now(),
            peak: 0.0,
        });
        self.page = Page::Scan;
    }

    fn start_full_scan(&mut self) {
        let mut roots: Vec<PathBuf> = Vec::new();
        let mut expected = 0;
        let mut names = Vec::new();
        if !self.custom_path.trim().is_empty() {
            let p = PathBuf::from(self.custom_path.trim());
            if !p.is_dir() {
                self.toast(format!("Folder tidak ditemukan: {}", p.display()), th::RED);
                return;
            }
            names.push(p.display().to_string());
            roots.push(p);
        } else {
            for (d, s) in self.drives.iter().zip(&self.drive_sel) {
                if *s {
                    roots.push(PathBuf::from(&d.mount));
                    expected += d.used;
                    names.push(d.mount.trim_end_matches('\\').to_string());
                }
            }
        }
        if roots.is_empty() {
            self.toast("Pilih minimal 1 drive atau isi folder", th::YELLOW);
            return;
        }
        let excludes = self
            .settings
            .excludes
            .split([',', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let opts = ScanOptions {
            roots,
            threads: self.settings.threads,
            top_n: 300,
            detect_patterns: self.settings.detect_patterns,
            include_generic: self.settings.include_generic,
            excludes,
            ..Default::default()
        };
        self.start_scan(opts, format!("Full scan  {}", names.join("  ")), expected);
    }

    fn start_cache_scan(&mut self) {
        let opts = scanner::cache_only_options(self.settings.threads, &[], false, 0);
        self.start_scan(opts, "Quick cache scan".into(), 0);
    }

    fn set_result(&mut self, r: ScanResult) {
        self.result = Some(r);
        self.marked.clear();
        self.detail = None;
        self.cat_filter = None;
        self.filter.clear();
    }

    fn poll(&mut self) {
        let mut finished = None;
        if let Some(job) = self.scan.as_mut() {
            if job.last_tick.elapsed() >= Duration::from_millis(200) {
                let files = job.progress.files.load(Ordering::Relaxed);
                let dt = job.last_tick.elapsed().as_secs_f32();
                let rate = files.saturating_sub(job.last_files) as f32 / dt;
                job.hist.remove(0);
                job.hist.push(rate);
                job.peak = job.peak.max(rate);
                job.last_files = files;
                job.last_tick = Instant::now();
            }
            if job.handle.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
                finished = job.handle.take().and_then(|h| h.join().ok());
            }
        }
        if let Some(r) = finished {
            self.scan = None;
            let msg = format!(
                "Scan selesai — {} file, cache {} ({})",
                fmt_num(r.totals.files),
                human_size(r.totals.cache_size),
                fmt_duration(r.meta.elapsed)
            );
            let full = r.meta.mode == "full";
            self.set_result(r);
            self.page = if full { Page::Dashboard } else { Page::Cache };
            self.toast(msg, th::GREEN);
        }

        let mut cleaned = None;
        if let Some(job) = self.clean.as_mut() {
            if job.handle.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
                let dry = job.dry;
                cleaned = job.handle.take().and_then(|h| h.join().ok()).map(|v| (v, dry));
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
                    recompute_totals(r);
                }
                self.marked.clear();
                self.toast(format!("{} berhasil dibebaskan", human_size(freed)), th::GREEN);
            } else {
                self.toast(format!("Simulasi: {} bisa dibebaskan", human_size(freed)), th::CYAN);
            }
            self.clean_report = Some((items, dry));
        }
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_millis(3500));
    }

    fn visible_caches(&self) -> Vec<usize> {
        let Some(r) = &self.result else { return vec![] };
        let q = self.filter.to_lowercase();
        let mut v: Vec<usize> = r
            .caches
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                (!self.safe_only || c.safe)
                    && self.cat_filter.as_ref().map(|f| &c.category == f).unwrap_or(true)
                    && (q.is_empty() || c.name.to_lowercase().contains(&q) || c.path.to_lowercase().contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        let cs = &r.caches;
        v.sort_by(|a, b| {
            let (x, y) = (&cs[*a], &cs[*b]);
            let o = match self.sort {
                SortBy::Size => x.size.cmp(&y.size),
                SortBy::Files => x.files.cmp(&y.files),
                SortBy::Name => y.name.to_lowercase().cmp(&x.name.to_lowercase()),
                SortBy::Category => y.category.cmp(&x.category),
                SortBy::Newest => x.newest.total_cmp(&y.newest),
            };
            if self.sort_desc {
                o.reverse()
            } else {
                o
            }
        });
        v
    }

    fn marked_targets(&self) -> Vec<usize> {
        let Some(r) = &self.result else { return vec![] };
        let mut ids: Vec<usize> = self.marked.iter().copied().filter(|i| *i < r.caches.len()).collect();
        ids.sort_by_key(|i| r.caches[*i].path.len());
        let sep = std::path::MAIN_SEPARATOR;
        let key = |i: usize| format!("{}{sep}", r.caches[i].path.to_lowercase().trim_end_matches(sep));
        let mut out: Vec<usize> = Vec::new();
        for i in ids {
            if !out.iter().any(|j| key(i).starts_with(&key(*j))) {
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
                .map(|(idx, name, path)| CleanItem {
                    idx,
                    name,
                    res: cleaner::clean_cache(Path::new(&path), None, dry, Some(&pg)),
                })
                .collect()
        });
        self.clean = Some(CleanJob { progress, handle: Some(handle), total, dry, count });
        self.confirm_clean = false;
    }

    fn export(&mut self, fmt: &str) {
        let Some(r) = &self.result else { return };
        let default = report::default_report_name(fmt);
        let dlg = rfd::FileDialog::new().set_file_name(default.to_string_lossy());
        let dlg = match fmt {
            "html" => dlg.add_filter("HTML", &["html"]),
            "json" => dlg.add_filter("JSON", &["json"]),
            "md" => dlg.add_filter("Markdown", &["md"]),
            _ => dlg.add_filter("CSV", &["csv"]),
        };
        let Some(path) = dlg.save_file() else { return };
        match report::export(r, &path, Some(fmt)) {
            Ok(files) => {
                self.toast(format!("Report disimpan ({} file)", files.len()), th::GREEN);
                self.last_export = files.into_iter().next();
            }
            Err(e) => self.toast(format!("Gagal export: {e}"), th::RED),
        }
    }

    fn open_report(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("JSON report", &["json"]).pick_file() else { return };
        match report::load_json(&path) {
            Ok(r) => {
                let full = r.meta.mode == "full";
                self.set_result(r);
                self.page = if full { Page::Dashboard } else { Page::Cache };
                let name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast(format!("Report dibuka: {name}"), th::ACCENT);
            }
            Err(e) => self.toast(format!("Gagal membuka: {e:#}"), th::RED),
        }
    }

    // ===================================================================== sidebar

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(232.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(th::SIDEBAR).inner_margin(Margin { left: 16, right: 16, top: 22, bottom: 18 }))
            .show(ctx, |ui| {
                // logo
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                    let p = ui.painter_at(rect);
                    let mid = lerp_color(th::ACCENT, th::PURPLE, 0.5);
                    p.rect_filled(rect, CornerRadius::same(11), mid);
                    let inner = rect.shrink(3.0);
                    let mut mesh = egui::Mesh::default();
                    mesh.colored_vertex(inner.left_top(), th::ACCENT);
                    mesh.colored_vertex(inner.right_top(), mid);
                    mesh.colored_vertex(inner.left_bottom(), mid);
                    mesh.colored_vertex(inner.right_bottom(), th::PURPLE);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(1, 2, 3);
                    p.add(Shape::mesh(mesh));
                    p.rect_stroke(rect.shrink(1.5), CornerRadius::same(10), Stroke::new(3.0, mid), StrokeKind::Middle);
                    let c = rect.center();
                    let k = 9.5;
                    p.add(Shape::convex_polygon(
                        vec![c + vec2(0.0, -k), c + vec2(k, 0.0), c + vec2(0.0, k), c + vec2(-k, 0.0)],
                        th::BG,
                        Stroke::NONE,
                    ));
                    p.add(Shape::convex_polygon(
                        vec![c + vec2(0.0, -4.0), c + vec2(4.0, 0.0), c + vec2(0.0, 4.0), c + vec2(-4.0, 0.0)],
                        th::ACCENT,
                        Stroke::NONE,
                    ));
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.add_space(1.0);
                        ui.label(btxt("CacheChecker", 17.0, th::FG));
                        ui.label(txt(format!("v{}  ·  disk auditor", env!("CARGO_PKG_VERSION")), 11.5, th::DIM));
                    });
                });
                ui.add_space(26.0);
                ui.label(stxt("MENU", 10.5, th::MUTED));
                ui.add_space(6.0);

                for page in Page::ALL {
                    let sel = self.page == page;
                    let badge = match page {
                        Page::Cache => self.result.as_ref().map(|r| r.caches.len().to_string()),
                        _ => None,
                    };
                    let scanning = page == Page::Scan && self.scan.is_some();
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                    let p = ui.painter_at(rect);
                    if sel {
                        p.rect_filled(rect, CornerRadius::same(10), th::ACCENT.gamma_multiply(0.14));
                        p.rect_filled(
                            Rect::from_min_size(rect.min + vec2(0.0, 10.0), vec2(3.0, 20.0)),
                            CornerRadius::same(2),
                            th::ACCENT,
                        );
                    } else if resp.hovered() {
                        p.rect_filled(rect, CornerRadius::same(10), th::CARD);
                    }
                    let col = if sel || resp.hovered() { th::FG } else { th::DIM };
                    let icol = if sel { th::ACCENT } else { col };
                    draw_icon(&p, Rect::from_center_size(rect.left_center() + vec2(24.0, 0.0), vec2(18.0, 18.0)), page.icon(), icol);
                    p.text(rect.left_center() + vec2(46.0, 0.0), Align2::LEFT_CENTER, page.label(), if sel { fs(14.5) } else { fr(14.5) }, col);
                    if let Some(b) = badge {
                        let g = p.layout_no_wrap(b, fs(11.0), th::PURPLE);
                        let br = Rect::from_center_size(rect.right_center() - vec2(24.0, 0.0), vec2(g.size().x + 14.0, 20.0));
                        p.rect_filled(br, CornerRadius::same(10), th::PURPLE.gamma_multiply(0.18));
                        p.galley(br.center() - g.size() / 2.0, g, th::PURPLE);
                    }
                    if scanning {
                        let t = self.t0.elapsed().as_secs_f32();
                        let a = 0.5 + 0.5 * (t * 5.0).sin();
                        let c = rect.right_center() - vec2(22.0, 0.0);
                        p.circle_filled(c, 4.0, th::GREEN);
                        p.circle_stroke(c, 4.0 + a * 4.0, Stroke::new(1.5, th::GREEN.gamma_multiply(1.0 - a)));
                    }
                    if resp.clicked() {
                        self.page = page;
                    }
                    if resp.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    let (label, col) = if self.admin { ("Administrator", th::GREEN) } else { ("User biasa", th::YELLOW) };
                    let resp = ui
                        .horizontal(|ui| {
                            icon(ui, Ic::Shield, 15.0, col);
                            ui.label(txt(label, 12.5, col));
                        })
                        .response;
                    if !self.admin {
                        resp.on_hover_text("Jalankan sebagai Administrator untuk membaca folder sistem (Windows\\Temp, dll)");
                    }
                    ui.add_space(8.0);
                    if let Some(r) = &self.result {
                        let t = &r.totals;
                        egui::Frame::new()
                            .fill(th::GREEN.gamma_multiply(0.08))
                            .stroke(Stroke::new(1.0, th::GREEN.gamma_multiply(0.25)))
                            .corner_radius(CornerRadius::same(14))
                            .inner_margin(Margin::same(14))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(stxt("BISA DIBEBASKAN", 10.5, th::DIM));
                                ui.label(btxt(human_size(t.cache_safe_size), 22.0, th::GREEN));
                                let ratio = if t.cache_size > 0 { t.cache_safe_size as f32 / t.cache_size as f32 } else { 0.0 };
                                gradient_bar(ui, ratio, ui.available_width(), 6.0, th::TEAL, th::GREEN);
                                ui.label(txt(format!("dari total cache {}", human_size(t.cache_size)), 11.5, th::DIM));
                            });
                    }
                });
            });
    }

    // ===================================================================== header

    fn header(&mut self, ui: &mut Ui, title: &str, subtitle: &str) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(btxt(title, 28.0, th::FG));
                ui.label(txt(subtitle, 13.5, th::DIM));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.scan.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    // right_to_left: yang pertama ditambahkan ada di paling kanan
                    if button(ui, Some(Ic::Bolt), "Full Scan", Btn::Primary(th::ACCENT)).clicked() {
                        self.start_full_scan();
                    }
                    if button(ui, Some(Ic::Broom), "Quick Cache Scan", Btn::Soft(th::GREEN)).clicked() {
                        self.start_cache_scan();
                    }
                    if button(ui, Some(Ic::Upload), "Buka Report", Btn::Ghost)
                        .on_hover_text("Buka report JSON hasil scan sebelumnya")
                        .clicked()
                    {
                        self.open_report();
                    }
                });
            });
        });
        ui.add_space(18.0);
    }

    // ===================================================================== dashboard

    fn page_dashboard(&mut self, ui: &mut Ui) {
        let sub = match &self.result {
            Some(r) => format!(
                "Scan terakhir {}  ·  {}  ·  {}",
                fmt_time(r.meta.started),
                report::short_roots(&r.meta.roots),
                fmt_duration(r.meta.elapsed)
            ),
            None => "Pilih drive lalu jalankan scan. Quick Cache Scan selesai dalam hitungan detik.".into(),
        };
        self.header(ui, "Dashboard", &sub);

        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            let gap = 14.0;
            if let Some(r) = &self.result {
                let t = &r.totals;
                let w = (ui.available_width() - gap * 3.0) / 4.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    stat_card(ui, w, Ic::File, "Total File", &fmt_num(t.files), &format!("{} folder", fmt_num(t.dirs)), th::ACCENT);
                    stat_card(ui, w, Ic::Disk, "Total Ukuran", &human_size(t.size), &format!("{} root", r.drives.len()), th::CYAN);
                    stat_card(
                        ui,
                        w,
                        Ic::Broom,
                        "Total Cache",
                        &human_size(t.cache_size),
                        &format!("{} lokasi · {} file", t.cache_count, fmt_num(t.cache_files)),
                        th::YELLOW,
                    );
                    stat_card(ui, w, Ic::Check, "Aman Dihapus", &human_size(t.cache_safe_size), "dibuat ulang otomatis", th::GREEN);
                });
                ui.add_space(22.0);
            }

            // ---- disk
            ui.horizontal(|ui| {
                icon(ui, Ic::Disk, 18.0, th::PURPLE);
                ui.label(stxt("Disk", 16.0, th::FG));
                let n_sel = self.drive_sel.iter().filter(|s| **s).count();
                ui.label(txt(format!("{n_sel} dari {} dipilih untuk Full Scan", self.drives.len()), 13.0, th::DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let all = self.drive_sel.iter().all(|s| *s);
                    if button_h(ui, None, if all { "Batal pilih semua" } else { "Pilih semua" }, Btn::Ghost, 30.0).clicked() {
                        self.drive_sel.iter_mut().for_each(|s| *s = !all);
                    }
                });
            });
            ui.add_space(8.0);
            let min_w = 300.0;
            let cols = ((ui.available_width() + gap) / (min_w + gap)).floor().clamp(1.0, 4.0) as usize;
            let w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
            let n = self.drives.len();
            for row in 0..n.div_ceil(cols) {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for i in row * cols..((row + 1) * cols).min(n) {
                        self.drive_card(ui, i, w);
                    }
                });
                ui.add_space(gap - ui.spacing().item_spacing.y);
            }
            ui.add_space(4.0);

            // ---- folder custom
            egui::Frame::new()
                .fill(th::CARD)
                .stroke(Stroke::new(1.0, th::BORDER))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        icon(ui, Ic::Folder, 18.0, th::DIM);
                        ui.label(txt("Atau scan satu folder", 14.0, th::FG));
                        ui.add_space(6.0);
                        let has_clear = !self.custom_path.is_empty();
                        let btn_w = button_size(ui, Some(Ic::Folder), "Pilih folder", 38.0).x
                            + if has_clear { button_size(ui, Some(Ic::Close), "", 38.0).x + 10.0 } else { 0.0 };
                        // sisakan margin TextEdit + spacing supaya frame tidak melebar keluar
                        let w = (ui.available_width() - btn_w - 48.0).max(200.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.custom_path)
                                .hint_text(txt("contoh: D:\\Project  (kosongkan untuk memakai drive terpilih)", 13.5, th::MUTED))
                                .font(fr(14.0))
                                .desired_width(w)
                                .min_size(vec2(0.0, 36.0))
                                .vertical_align(Align::Center)
                                .margin(Margin::symmetric(12, 0)),
                        );
                        if button(ui, Some(Ic::Folder), "Pilih folder", Btn::Ghost).clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                self.custom_path = p.display().to_string();
                            }
                        }
                        if has_clear && button(ui, Some(Ic::Close), "", Btn::Ghost).on_hover_text("kosongkan").clicked() {
                            self.custom_path.clear();
                        }
                    });
                });
            ui.add_space(22.0);

            let Some(r) = self.result.clone() else {
                self.empty_state(ui);
                return;
            };

            // ---- grafik
            let avail = ui.available_width();
            let left_w = ((avail - gap) * 0.44).max(360.0);
            let right_w = avail - gap - left_w;
            let h = 360.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.allocate_ui(vec2(left_w, h), |ui| {
                    card(ui, |ui| {
                        ui.set_width(left_w - 38.0);
                        ui.set_height(h - 38.0);
                        section_title(ui, Ic::Pie, "Komposisi Cache");
                        let parts: Vec<(String, u64, Color32)> =
                            r.cache_by_category.iter().map(|c| (c.category.clone(), c.size, cat_color(&c.category))).collect();
                        ui.horizontal_top(|ui| {
                            let hov = donut(ui, 230.0, &parts, &human_size(r.totals.cache_size), "total cache", self.cat_hover);
                            self.cat_hover = None;
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.add_space(6.0);
                                for (i, c) in r.cache_by_category.iter().enumerate() {
                                    let col = cat_color(&c.category);
                                    let strong = hov == Some(i);
                                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                                    let p = ui.painter_at(rect);
                                    if resp.hovered() || strong {
                                        p.rect_filled(rect, CornerRadius::same(8), th::CARD_HI);
                                    }
                                    draw_icon(&p, Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(15.0, 15.0)), cat_icon(&c.category), col);
                                    p.text(
                                        rect.left_center() + vec2(30.0, 0.0),
                                        Align2::LEFT_CENTER,
                                        &c.category,
                                        fr(13.5),
                                        if strong || resp.hovered() { th::FG } else { th::DIM },
                                    );
                                    p.text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, human_size(c.size), fs(13.0), th::FG);
                                    if resp.hovered() {
                                        self.cat_hover = Some(i);
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if resp.clicked() {
                                        self.cat_filter = Some(c.category.clone());
                                        self.page = Page::Cache;
                                    }
                                }
                            });
                        });
                    });
                });
                ui.allocate_ui(vec2(right_w, h), |ui| {
                    card(ui, |ui| {
                        ui.set_width(right_w - 38.0);
                        ui.set_height(h - 38.0);
                        ui.horizontal(|ui| {
                            section_title(ui, Ic::Star, "Cache Terbesar");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let l = ui.add(egui::Label::new(txt("lihat semua  →", 13.0, th::ACCENT)).sense(Sense::click()));
                                if l.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if l.clicked() {
                                    self.page = Page::Cache;
                                }
                            });
                        });
                        let mx = r.caches.first().map(|c| c.size).unwrap_or(1).max(1) as f32;
                        for (i, c) in r.caches.iter().take(8).enumerate() {
                            let col = cat_color(&c.category);
                            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::click());
                            let p = ui.painter_at(rect);
                            if resp.hovered() {
                                p.rect_filled(rect, CornerRadius::same(8), th::CARD_HI);
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            p.text(rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, format!("{}", i + 1), fs(12.5), th::MUTED);
                            draw_icon(&p, Rect::from_center_size(rect.left_center() + vec2(34.0, 0.0), vec2(15.0, 15.0)), cat_icon(&c.category), col);
                            let bar_w = (rect.width() * 0.28).clamp(80.0, 200.0);
                            let size_w = 86.0;
                            let name_x = rect.left() + 50.0;
                            let name_w = rect.right() - size_w - bar_w - 18.0 - name_x;
                            let name = fit_text(&p, &c.name, &fr(13.5), name_w);
                            p.text(pos2(name_x, rect.center().y), Align2::LEFT_CENTER, name, fr(13.5), th::FG);
                            let br = Rect::from_min_size(pos2(rect.right() - size_w - bar_w - 8.0, rect.center().y - 4.0), vec2(bar_w, 8.0));
                            paint_gradient_bar(&p, br, c.size as f32 / mx, col.gamma_multiply(0.55), col);
                            p.text(rect.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, human_size(c.size), fs(13.0), th::FG);
                            if resp.on_hover_text(&c.path).clicked() {
                                self.detail = Some(i);
                            }
                        }
                    });
                });
            });

            // ---- gauge
            ui.add_space(gap);
            let disk_used: u64 = self.drives.iter().filter(|d| d.kind != "cloud").map(|d| d.used).sum();
            let t = &r.totals;
            let w2 = (ui.available_width() - gap) / 2.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (ic, title, ratio, label, a, b) in [
                    (
                        Ic::Pie,
                        "Cache vs disk terpakai",
                        if disk_used > 0 { t.cache_size as f32 / disk_used as f32 } else { 0.0 },
                        format!("{} dari {}", human_size(t.cache_size), human_size(disk_used)),
                        th::ORANGE,
                        th::YELLOW,
                    ),
                    (
                        Ic::Check,
                        "Porsi aman dihapus",
                        if t.cache_size > 0 { t.cache_safe_size as f32 / t.cache_size as f32 } else { 0.0 },
                        format!("{} dari {}", human_size(t.cache_safe_size), human_size(t.cache_size)),
                        th::TEAL,
                        th::GREEN,
                    ),
                ] {
                    ui.allocate_ui(vec2(w2, 100.0), |ui| {
                        card(ui, |ui| {
                            ui.set_width(w2 - 38.0);
                            ui.horizontal(|ui| {
                                icon(ui, ic, 16.0, b);
                                ui.label(txt(title, 14.0, th::DIM));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(btxt(format!("{:.1}%", ratio * 100.0), 20.0, b));
                                });
                            });
                            gradient_bar(ui, ratio, ui.available_width(), 12.0, a, b);
                            ui.label(txt(label, 12.5, th::DIM));
                        });
                    });
                }
            });

            if r.meta.mode == "full" {
                ui.add_space(gap);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    let w2 = (ui.available_width() - gap) / 2.0;
                    for (ic, title, items, by_count, color) in [
                        (Ic::Clock, "Umur File (berdasarkan ukuran)", &r.age, false, th::TEAL),
                        (Ic::Bars, "Distribusi Ukuran File (berdasarkan jumlah)", &r.size_dist, true, th::PURPLE),
                    ] {
                        ui.allocate_ui(vec2(w2, 260.0), |ui| {
                            card(ui, |ui| {
                                ui.set_width(w2 - 38.0);
                                section_title(ui, ic, title);
                                bar_chart(ui, items, by_count, color);
                            });
                        });
                    }
                });
            }
            ui.add_space(24.0);
        });
    }

    fn drive_card(&mut self, ui: &mut Ui, i: usize, w: f32) {
        let d = self.drives[i].clone();
        let sel = self.drive_sel[i];
        let pct = d.percent() as f32;
        let col = usage_color(pct);
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 112.0), Sense::click());
        let hover = resp.hovered();
        let p = ui.painter_at(rect.expand(2.0));
        p.rect(
            rect,
            CornerRadius::same(16),
            if sel {
                th::CARD_HI
            } else if hover {
                Color32::from_rgb(29, 32, 48)
            } else {
                th::CARD
            },
            Stroke::new(
                if sel { 1.5 } else { 1.0 },
                if sel {
                    th::ACCENT.gamma_multiply(0.75)
                } else if hover {
                    th::MUTED
                } else {
                    th::BORDER
                },
            ),
            StrokeKind::Inside,
        );
        // ring
        let c = pos2(rect.left() + 56.0, rect.center().y);
        let r = 34.0;
        p.circle_stroke(c, r, Stroke::new(8.0, th::INPUT));
        let end = (pct / 100.0).clamp(0.0, 1.0) * TAU;
        let n = 90;
        for k in 0..n {
            let a0 = -PI / 2.0 + end * k as f32 / n as f32;
            let a1 = -PI / 2.0 + end * (k + 1) as f32 / n as f32;
            let cc = lerp_color(col.gamma_multiply(0.7), col, k as f32 / n as f32);
            p.line_segment([c + vec2(a0.cos() * r, a0.sin() * r), c + vec2(a1.cos() * r, a1.sin() * r)], Stroke::new(8.0, cc));
        }
        p.text(c - vec2(0.0, 1.0), Align2::CENTER_CENTER, format!("{:.0}%", pct), fb(16.0), th::FG);

        // teks
        let x = rect.left() + 108.0;
        let maxw = rect.right() - x - 18.0;
        let letter = d.mount.trim_end_matches(['\\', '/']).to_string();
        let letter = if letter.is_empty() { "/".to_string() } else { letter };
        let lg = p.layout_no_wrap(letter, fb(22.0), th::FG);
        let lw = lg.size().x;
        p.galley(pos2(x, rect.top() + 12.0), lg, th::FG);
        if d.kind == "cloud" || d.kind == "removable" {
            let (ic, t, pc) = if d.kind == "cloud" { (Ic::Cloud, "cloud", th::PURPLE) } else { (Ic::Disk, "USB", th::YELLOW) };
            let g = p.layout_no_wrap(t.into(), fs(11.0), pc);
            let pr = Rect::from_min_size(pos2(x + lw + 10.0, rect.top() + 18.0), vec2(g.size().x + 32.0, 21.0));
            p.rect_filled(pr, CornerRadius::same(10), pc.gamma_multiply(0.16));
            draw_icon(&p, Rect::from_min_size(pr.min + vec2(7.0, 3.5), vec2(14.0, 14.0)), ic, pc);
            p.galley(pos2(pr.left() + 24.0, pr.center().y - g.size().y / 2.0), g, pc);
        }
        let label = if d.label.is_empty() { "-".to_string() } else { d.label.clone() };
        let label = fit_text(&p, &label, &fr(13.0), maxw);
        p.text(pos2(x, rect.top() + 45.0), Align2::LEFT_TOP, label, fr(13.0), th::DIM);
        p.text(pos2(x, rect.top() + 66.0), Align2::LEFT_TOP, format!("{} / {}", human_size(d.used), human_size(d.total)), fs(13.5), th::FG);
        p.text(pos2(x, rect.top() + 86.0), Align2::LEFT_TOP, format!("{} tersisa", human_size(d.free)), fr(12.5), th::GREEN);

        // checkbox custom
        let cb = Rect::from_min_size(pos2(rect.right() - 36.0, rect.top() + 14.0), vec2(22.0, 22.0));
        p.rect(
            cb,
            CornerRadius::same(7),
            if sel { th::ACCENT } else { Color32::TRANSPARENT },
            Stroke::new(1.5, if sel { th::ACCENT } else { th::MUTED }),
            StrokeKind::Inside,
        );
        if sel {
            draw_icon(&p, cb.shrink(3.0), Ic::Check, th::BG);
        }

        let resp = resp.on_hover_text(format!("{}  {}\nklik untuk pilih / batal pilih", d.mount, d.label));
        if resp.clicked() {
            self.drive_sel[i] = !self.drive_sel[i];
        }
        if hover {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(26.0);
                let (rect, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
                let p = ui.painter_at(rect.expand(4.0));
                let t = self.t0.elapsed().as_secs_f32();
                p.circle_filled(rect.center(), 40.0, th::ACCENT.gamma_multiply(0.08));
                p.circle_stroke(rect.center(), 40.0 + (t * 1.5).sin() * 1.5, Stroke::new(1.0, th::ACCENT.gamma_multiply(0.25)));
                draw_icon(&p, Rect::from_center_size(rect.center(), vec2(40.0, 40.0)), Ic::Search, th::ACCENT);
                ui.add_space(10.0);
                ui.label(btxt("Belum ada hasil scan", 20.0, th::FG));
                ui.add_space(2.0);
                ui.label(txt("Quick Cache Scan memeriksa 100+ lokasi cache yang dikenal dalam beberapa detik.", 13.5, th::DIM));
                ui.label(txt("Full Scan menghitung semua file di drive terpilih beserta statistik lengkap.", 13.5, th::DIM));
                ui.add_space(16.0);
                let w1 = button_size(ui, Some(Ic::Broom), "Quick Cache Scan", 40.0).x;
                let w2 = button_size(ui, Some(Ic::Bolt), "Full Scan", 40.0).x;
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - w1 - w2 - 12.0) / 2.0).max(0.0));
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if button_h(ui, Some(Ic::Broom), "Quick Cache Scan", Btn::Soft(th::GREEN), 40.0).clicked() {
                        self.start_cache_scan();
                    }
                    if button_h(ui, Some(Ic::Bolt), "Full Scan", Btn::Primary(th::ACCENT), 40.0).clicked() {
                        self.start_full_scan();
                    }
                });
                ui.add_space(26.0);
            });
        });
        ui.add_space(24.0);
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }

    // ===================================================================== scan page

    fn page_scan(&mut self, ui: &mut Ui) {
        let t = self.t0.elapsed().as_secs_f32();
        let Some(job) = &self.scan else {
            self.header(ui, "Scan", "Atur cara scan, lalu jalankan Full Scan atau Quick Cache Scan");
            self.scan_settings(ui);
            return;
        };
        let p = &job.progress;
        let files = p.files.load(Ordering::Relaxed);
        let dirs = p.dirs.load(Ordering::Relaxed);
        let size = p.size.load(Ordering::Relaxed);
        let errs = p.errors.load(Ordering::Relaxed);
        let caches = p.caches.load(Ordering::Relaxed);
        let el = job.started.elapsed().as_secs_f64();
        let cancel = p.cancelled();
        let label = job.label.clone();
        let expected = job.expected;
        let hist = job.hist.clone();
        let peak = job.peak;
        let current = p.current.lock().map(|c| c.clone()).unwrap_or_default();

        ui.horizontal(|ui| {
            spinner(ui, 50.0, t);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(btxt(if cancel { "Membatalkan…" } else { "Sedang memindai…" }, 28.0, th::FG));
                ui.label(txt(format!("{label}   ·   {} thread   ·   {}", self.settings.threads, fmt_duration(el)), 13.5, th::DIM));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!cancel, |ui| {
                    if button(ui, Some(Ic::Stop), "Batalkan", Btn::Soft(th::RED)).clicked() {
                        if let Some(j) = &self.scan {
                            j.progress.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                });
            });
        });
        ui.add_space(18.0);

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let (ratio, info) = if expected > 0 {
                let r = (size as f64 / expected as f64).min(0.999);
                let eta = if r > 0.02 { format!("sisa ~{}", fmt_duration(el / r - el)) } else { "menghitung perkiraan…".into() };
                (r as f32, format!("{}  /  {}   ·   {}", human_size(size), human_size(expected), eta))
            } else {
                let x = (t * 0.6).fract();
                (if (t * 0.6) as i32 % 2 == 0 { x } else { 1.0 - x }, format!("{} terkumpul", human_size(size)))
            };
            ui.horizontal(|ui| {
                ui.label(btxt(if expected > 0 { format!("{:.1}%", ratio * 100.0) } else { "…".into() }, 22.0, th::FG));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(txt(info, 13.5, th::DIM));
                });
            });
            ui.add_space(4.0);
            gradient_bar(ui, ratio, ui.available_width(), 14.0, th::ACCENT, th::PURPLE);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                icon(ui, Ic::Folder, 14.0, th::MUTED);
                ui.add(egui::Label::new(txt(trunc_left(&current, 160), 12.5, th::MUTED)).truncate());
            });
        });
        ui.add_space(14.0);

        let gap = 14.0;
        let w = (ui.available_width() - gap * 4.0) / 5.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let rate = if el > 0.0 { files as f64 / el } else { 0.0 };
            stat_card(ui, w, Ic::File, "File", &fmt_num(files), &format!("{:.0} file/detik", rate), th::ACCENT);
            stat_card(ui, w, Ic::Folder, "Folder", &fmt_num(dirs), "dipindai", th::PURPLE);
            stat_card(ui, w, Ic::Disk, "Ukuran", &human_size(size), "terbaca", th::CYAN);
            stat_card(ui, w, Ic::Broom, "Lokasi Cache", &fmt_num(caches), "terdeteksi", th::YELLOW);
            stat_card(
                ui,
                w,
                Ic::Warning,
                "Error Akses",
                &fmt_num(errs),
                if errs > 0 { "butuh admin" } else { "tidak ada" },
                if errs > 0 { th::RED } else { th::DIM },
            );
        });
        ui.add_space(14.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                section_title(ui, Ic::Pulse, "Throughput (file/detik)");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, None, &format!("puncak {}", fmt_num(peak as u64)), th::GREEN);
                });
            });
            sparkline(ui, &hist, vec2(ui.available_width(), (ui.available_height() - 8.0).clamp(120.0, 260.0)), th::TEAL);
        });
    }

    fn scan_settings(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, Ic::Gear, "Pengaturan Scan");
            ui.add_space(4.0);
            egui::Grid::new("settings").num_columns(2).spacing([28.0, 16.0]).show(ui, |ui| {
                ui.label(txt("Jumlah thread", 14.0, th::DIM));
                ui.add(egui::Slider::new(&mut self.settings.threads, 1..=64).suffix(" thread"));
                ui.end_row();
                ui.label(txt("Deteksi pola folder", 14.0, th::DIM));
                ui.checkbox(&mut self.settings.detect_patterns, txt("__pycache__, node_modules, target/, .next … di mana pun", 14.0, th::FG));
                ui.end_row();
                ui.label(txt("Folder generic", 14.0, th::DIM));
                ui.checkbox(&mut self.settings.include_generic, txt("ikutkan folder bernama 'cache' / 'temp' (banyak false positive)", 14.0, th::FG));
                ui.end_row();
                ui.label(txt("Drive cloud", 14.0, th::DIM));
                if ui
                    .checkbox(&mut self.settings.include_cloud, txt("pilih Google Drive / OneDrive secara default (lambat, bisa memicu download)", 14.0, th::FG))
                    .changed()
                {
                    for (d, s) in self.drives.iter().zip(self.drive_sel.iter_mut()) {
                        if d.kind == "cloud" {
                            *s = self.settings.include_cloud;
                        }
                    }
                }
                ui.end_row();
                ui.label(txt("Exclude", 14.0, th::DIM));
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.excludes)
                        .hint_text(txt("*node_modules*, D:\\Games\\*", 13.5, th::MUTED))
                        .font(fr(14.0))
                        .desired_width(460.0)
                        .min_size(vec2(0.0, 34.0))
                        .vertical_align(Align::Center)
                        .margin(Margin::symmetric(12, 0)),
                );
                ui.end_row();
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                let n = self.drive_sel.iter().filter(|s| **s).count();
                if button(ui, Some(Ic::Bolt), &format!("Full Scan ({n} drive)"), Btn::Primary(th::ACCENT)).clicked() {
                    self.start_full_scan();
                }
                if button(ui, Some(Ic::Broom), "Quick Cache Scan", Btn::Soft(th::GREEN)).clicked() {
                    self.start_cache_scan();
                }
                if button(ui, Some(Ic::Refresh), "Refresh daftar drive", Btn::Ghost).clicked() {
                    self.drives = drives::list_drives(false, true);
                    self.drive_sel = self.drives.iter().map(|d| d.kind != "cloud" || self.settings.include_cloud).collect();
                    self.toast("Daftar drive diperbarui", th::ACCENT);
                }
            });
        });
    }

    // ===================================================================== cache page

    fn page_cache(&mut self, ui: &mut Ui) {
        let Some(r) = self.result.clone() else {
            self.header(ui, "Cache", "Belum ada hasil scan");
            self.empty_state(ui);
            return;
        };
        self.header(
            ui,
            "Cache",
            &format!(
                "{} lokasi  ·  total {}  ·  {} aman dihapus",
                r.caches.len(),
                human_size(r.totals.cache_size),
                human_size(r.totals.cache_safe_size)
            ),
        );

        // ---- toolbar
        ui.horizontal(|ui| {
            // search box: ikon di dalam frame, input tanpa frame
            egui::Frame::new()
                .fill(th::INPUT)
                .stroke(Stroke::new(1.0, th::BORDER))
                .corner_radius(CornerRadius::same(9))
                .inner_margin(Margin { left: 10, right: 8, top: 0, bottom: 0 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        icon(ui, Ic::Search, 16.0, th::MUTED);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.filter)
                                .hint_text(txt("Cari nama atau path…", 13.5, th::MUTED))
                                .font(fr(14.0))
                                .frame(false)
                                .desired_width(230.0)
                                .min_size(vec2(0.0, 34.0))
                                .vertical_align(Align::Center),
                        );
                    });
                });
            ui.add_space(6.0);
            let chip = |ui: &mut Ui, ic: Option<Ic>, label: &str, sel: bool, col: Color32| -> bool {
                let g = ui.painter().layout_no_wrap(label.to_string(), fr(13.0), col);
                let iw = if ic.is_some() { 20.0 } else { 0.0 };
                let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + iw + 24.0, 32.0), Sense::click());
                let p = ui.painter();
                let fill = if sel {
                    col.gamma_multiply(0.9)
                } else if resp.hovered() {
                    col.gamma_multiply(0.22)
                } else {
                    col.gamma_multiply(0.1)
                };
                p.rect(rect, CornerRadius::same(16), fill, Stroke::new(1.0, col.gamma_multiply(if sel { 0.0 } else { 0.3 })), StrokeKind::Inside);
                let fg = if sel { th::BG } else { col };
                let mut x = rect.left() + 12.0;
                if let Some(ic) = ic {
                    draw_icon(p, Rect::from_min_size(pos2(x, rect.center().y - 7.5), vec2(15.0, 15.0)), ic, fg);
                    x += iw;
                }
                p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, fr(13.0), fg);
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                resp.clicked()
            };
            if chip(ui, None, "Semua", self.cat_filter.is_none(), th::DIM) {
                self.cat_filter = None;
            }
            for c in &r.cache_by_category {
                let sel = self.cat_filter.as_deref() == Some(c.category.as_str());
                if chip(ui, Some(cat_icon(&c.category)), &c.category, sel, cat_color(&c.category)) {
                    self.cat_filter = if sel { None } else { Some(c.category.clone()) };
                }
            }
            ui.add_space(6.0);
            if chip(ui, Some(Ic::Check), "aman saja", self.safe_only, th::GREEN) {
                self.safe_only = !self.safe_only;
            }
        });
        ui.add_space(10.0);

        let vis = self.visible_caches();
        let marked_size: u64 = self.marked.iter().filter_map(|i| r.caches.get(*i)).map(|c| c.size).sum();

        // ---- action bar
        let active = !self.marked.is_empty();
        egui::Frame::new()
            .fill(if active { th::RED.gamma_multiply(0.1) } else { th::CARD })
            .stroke(Stroke::new(1.0, if active { th::RED.gamma_multiply(0.45) } else { th::BORDER }))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if active {
                        icon(ui, Ic::Trash, 18.0, th::RED);
                        ui.label(stxt(format!("{} lokasi dipilih", self.marked.len()), 14.5, th::FG));
                        ui.label(btxt(human_size(marked_size), 17.0, th::RED));
                    } else {
                        icon(ui, Ic::Check, 18.0, th::DIM);
                        ui.label(txt(format!("Centang cache yang ingin dibersihkan  ·  {} ditampilkan", vis.len()), 14.0, th::DIM));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(active && self.clean.is_none(), |ui| {
                            if button(ui, Some(Ic::Trash), "Bersihkan", Btn::Primary(th::RED)).clicked() {
                                self.confirm_clean = true;
                            }
                        });
                        if active && button(ui, Some(Ic::Close), "Hapus pilihan", Btn::Ghost).clicked() {
                            self.marked.clear();
                        }
                        if button(ui, Some(Ic::Check), "Pilih semua yang aman", Btn::Soft(th::GREEN)).clicked() {
                            for i in &vis {
                                if r.caches[*i].safe && r.caches[*i].size > 0 {
                                    self.marked.insert(*i);
                                }
                            }
                        }
                    });
                });
            });
        ui.add_space(10.0);

        // ---- tabel
        let mx = r.caches.iter().map(|c| c.size).max().unwrap_or(1).max(1) as f32;
        let mut clicked_sort: Option<SortBy> = None;
        let mut open_detail = None;
        let mut toggles: Vec<usize> = Vec::new();
        egui::Frame::new()
            .fill(th::CARD)
            .stroke(Stroke::new(1.0, th::BORDER))
            .corner_radius(CornerRadius::same(16))
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let h = (ui.available_height() - 4.0).max(200.0);
                // lebar kolom menyesuaikan ruang supaya tabel tidak melebar keluar card
                let avail = ui.available_width();
                let fixed = 34.0 + 120.0 + 78.0 + 80.0 + 92.0 + 110.0 + 128.0 + 8.0 * 9.0;
                let flex = (avail - fixed).max(260.0);
                let name_w = (flex * 0.55).clamp(190.0, 340.0);
                TableBuilder::new(ui)
                    .striped(false)
                    .resizable(false)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::exact(34.0))
                    .column(Column::exact(name_w).clip(true))
                    .column(Column::exact(120.0).clip(true))
                    .column(Column::exact(78.0))
                    .column(Column::exact(80.0))
                    .column(Column::exact(92.0))
                    .column(Column::exact(110.0))
                    .column(Column::exact(128.0))
                    .column(Column::remainder().at_least(60.0).clip(true))
                    .min_scrolled_height(0.0)
                    .max_scroll_height(h)
                    .header(34.0, |mut hd| {
                        hd.col(|_| {});
                        for (name, s, right) in [
                            ("NAMA", Some(SortBy::Name), false),
                            ("KATEGORI", Some(SortBy::Category), false),
                            ("STATUS", None, false),
                            ("FILE", Some(SortBy::Files), true),
                            ("UKURAN", Some(SortBy::Size), true),
                            ("PROPORSI", None, false),
                            ("TERBARU", Some(SortBy::Newest), false),
                            ("PATH", None, false),
                        ] {
                            hd.col(|ui| {
                                let arrow = match s {
                                    Some(x) if x == self.sort => {
                                        if self.sort_desc {
                                            "  ↓"
                                        } else {
                                            "  ↑"
                                        }
                                    }
                                    _ => "",
                                };
                                let is_sorted = s.is_some() && s == Some(self.sort);
                                let lbl = egui::Label::new(stxt(format!("{name}{arrow}"), 11.5, if is_sorted { th::ACCENT } else { th::DIM }))
                                    .sense(Sense::click());
                                let resp = if right {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.add(lbl)).inner
                                } else {
                                    ui.add(lbl)
                                };
                                if let Some(x) = s {
                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if resp.clicked() {
                                        clicked_sort = Some(x);
                                    }
                                }
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(38.0, vis.len(), |mut row| {
                            let i = vis[row.index()];
                            let c = &r.caches[i];
                            let col = cat_color(&c.category);
                            let marked = self.marked.contains(&i);
                            row.set_selected(marked);
                            row.col(|ui| {
                                let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                                let p = ui.painter();
                                p.rect(
                                    rect,
                                    CornerRadius::same(6),
                                    if marked { th::RED } else { Color32::TRANSPARENT },
                                    Stroke::new(1.5, if marked { th::RED } else { th::MUTED }),
                                    StrokeKind::Inside,
                                );
                                if marked {
                                    draw_icon(p, rect.shrink(3.0), Ic::Check, th::BG);
                                }
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if resp.clicked() {
                                    toggles.push(i);
                                }
                            });
                            row.col(|ui| {
                                icon(ui, cat_icon(&c.category), 15.0, col);
                                let resp = ui.add(egui::Label::new(txt(&c.name, 14.0, th::FG)).truncate().sense(Sense::click()));
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if resp.clicked() {
                                    open_detail = Some(i);
                                }
                                resp.on_hover_text("klik untuk detail");
                            });
                            row.col(|ui| {
                                ui.label(txt(&c.category, 13.5, col));
                            });
                            row.col(|ui| safe_pill(ui, c.safe));
                            row.col(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(txt(fmt_num(c.files), 13.5, th::DIM));
                                });
                            });
                            row.col(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(stxt(human_size(c.size), 14.0, th::FG));
                                });
                            });
                            row.col(|ui| {
                                gradient_bar(ui, c.size as f32 / mx, 100.0, 7.0, col.gamma_multiply(0.5), col);
                            });
                            row.col(|ui| {
                                ui.label(txt(fmt_time(c.newest), 12.5, th::DIM));
                            });
                            row.col(|ui| {
                                let resp = ui.add(egui::Label::new(txt(&c.path, 12.5, th::MUTED)).truncate().sense(Sense::click()));
                                if resp.double_clicked() {
                                    open_path(&c.path);
                                }
                                resp.on_hover_text(format!("{}\n(double-click: buka di Explorer)", c.path));
                            });
                        });
                    });
            });
        for i in toggles {
            if !self.marked.remove(&i) {
                self.marked.insert(i);
            }
        }
        if let Some(s) = clicked_sort {
            if self.sort == s {
                self.sort_desc = !self.sort_desc;
            } else {
                self.sort = s;
                self.sort_desc = !matches!(s, SortBy::Name | SortBy::Category);
            }
        }
        if open_detail.is_some() {
            self.detail = open_detail;
        }
    }

    // ===================================================================== folders / ext / largest

    fn need_full(&mut self, ui: &mut Ui, title: &str) -> Option<ScanResult> {
        match &self.result {
            Some(r) if r.meta.mode == "full" => Some(r.clone()),
            _ => {
                self.header(ui, title, "Tersedia setelah Full Scan");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        let (rect, _) = ui.allocate_exact_size(vec2(80.0, 80.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 38.0, th::PURPLE.gamma_multiply(0.1));
                        draw_icon(ui.painter(), Rect::from_center_size(rect.center(), vec2(38.0, 38.0)), Ic::Chart, th::PURPLE);
                        ui.add_space(10.0);
                        ui.label(btxt("Data ini hanya ada di Full Scan", 19.0, th::FG));
                        ui.label(txt("Quick Cache Scan hanya memeriksa lokasi cache, tanpa statistik file.", 13.5, th::DIM));
                        ui.add_space(14.0);
                        let w = button_size(ui, Some(Ic::Bolt), "Jalankan Full Scan", 40.0).x;
                        ui.horizontal(|ui| {
                            ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                            if button_h(ui, Some(Ic::Bolt), "Jalankan Full Scan", Btn::Primary(th::ACCENT), 40.0).clicked() {
                                self.start_full_scan();
                            }
                        });
                        ui.add_space(40.0);
                    });
                });
                None
            }
        }
    }

    fn page_folders(&mut self, ui: &mut Ui) {
        let Some(r) = self.need_full(ui, "Folder") else { return };
        self.header(ui, "Folder Teratas", "Folder level pertama di tiap root, diurutkan berdasarkan ukuran");
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            for d in &r.drives {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        icon(ui, Ic::Disk, 20.0, th::CYAN);
                        ui.label(btxt(&d.root, 17.0, th::FG));
                        pill(ui, None, &format!("{} file", fmt_num(d.files)), th::DIM);
                        pill(ui, None, &human_size(d.size), th::CYAN);
                    });
                    ui.add_space(8.0);
                    let mx = d.top_dirs.first().map(|x| x.size).unwrap_or(1).max(1) as f32;
                    for (i, x) in d.top_dirs.iter().take(25).enumerate() {
                        let col = lerp_color(th::ACCENT, th::PURPLE, i as f32 / 25.0);
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
                        let p = ui.painter_at(rect);
                        if resp.hovered() {
                            p.rect_filled(rect, CornerRadius::same(8), th::CARD_HI);
                        }
                        draw_icon(&p, Rect::from_center_size(rect.left_center() + vec2(16.0, 0.0), vec2(16.0, 16.0)), Ic::Folder, col);
                        let name_w = (rect.width() * 0.3).max(160.0);
                        p.text(rect.left_center() + vec2(34.0, 0.0), Align2::LEFT_CENTER, fit_text(&p, &x.name, &fr(14.0), name_w - 40.0), fr(14.0), th::FG);
                        let right_w = 290.0;
                        let br = Rect::from_min_max(
                            pos2(rect.left() + name_w, rect.center().y - 5.0),
                            pos2(rect.right() - right_w, rect.center().y + 5.0),
                        );
                        if br.width() > 20.0 {
                            paint_gradient_bar(&p, br, x.size as f32 / mx, col.gamma_multiply(0.55), col);
                        }
                        p.text(pos2(rect.right() - right_w + 94.0, rect.center().y), Align2::RIGHT_CENTER, human_size(x.size), fs(14.0), th::FG);
                        p.text(
                            pos2(rect.right() - right_w + 160.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            format!("{:.1}%", x.size as f64 / d.size.max(1) as f64 * 100.0),
                            fr(13.0),
                            th::DIM,
                        );
                        p.text(pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, format!("{} file", fmt_num(x.count)), fr(12.5), th::MUTED);
                    }
                });
                ui.add_space(14.0);
            }
        });
    }

    fn page_extensions(&mut self, ui: &mut Ui) {
        let Some(r) = self.need_full(ui, "Ekstensi") else { return };
        self.header(ui, "Ekstensi", &format!("{} jenis ekstensi  ·  diurutkan berdasarkan ukuran", r.extensions.len()));
        let total = r.totals.size.max(1);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            card(ui, |ui| {
                ui.set_width(300.0);
                section_title(ui, Ic::Pie, "Komposisi");
                let mut parts: Vec<(String, u64, Color32)> =
                    r.extensions.iter().take(8).enumerate().map(|(i, e)| (e.name.clone(), e.size, PALETTE[i % PALETTE.len()])).collect();
                let rest: u64 = r.extensions.iter().skip(8).map(|e| e.size).sum();
                if rest > 0 {
                    parts.push(("lainnya".into(), rest, th::MUTED));
                }
                ui.vertical_centered(|ui| {
                    donut(ui, 240.0, &parts, &human_size(total), "total", None);
                });
                ui.add_space(8.0);
                for (n, v, c) in &parts {
                    ui.horizontal(|ui| {
                        let (rc, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().rect_filled(rc, CornerRadius::same(3), *c);
                        ui.label(txt(n, 13.5, th::FG));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(txt(format!("{:.1}%", *v as f64 / total as f64 * 100.0), 13.0, th::DIM));
                        });
                    });
                }
            });
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                let h = ui.available_height().max(300.0);
                let mx = r.extensions.first().map(|e| e.size).unwrap_or(1).max(1) as f32;
                TableBuilder::new(ui)
                    .striped(false)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::exact(120.0))
                    .column(Column::exact(100.0))
                    .column(Column::exact(100.0))
                    .column(Column::exact(70.0))
                    .column(Column::remainder())
                    .max_scroll_height(h)
                    .header(32.0, |mut hd| {
                        for t in ["EKSTENSI", "JUMLAH", "UKURAN", "%", ""] {
                            hd.col(|ui| {
                                ui.label(stxt(t, 11.5, th::DIM));
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(32.0, r.extensions.len().min(1000), |mut row| {
                            let i = row.index();
                            let e = &r.extensions[i];
                            let col = PALETTE[i % PALETTE.len()];
                            row.col(|ui| {
                                ui.label(stxt(&e.name, 14.0, col));
                            });
                            row.col(|ui| {
                                ui.label(txt(fmt_num(e.count), 13.5, th::DIM));
                            });
                            row.col(|ui| {
                                ui.label(stxt(human_size(e.size), 14.0, th::FG));
                            });
                            row.col(|ui| {
                                ui.label(txt(format!("{:.1}%", e.size as f64 / total as f64 * 100.0), 13.0, th::DIM));
                            });
                            row.col(|ui| {
                                let w = ui.available_width().max(60.0) - 10.0;
                                gradient_bar(ui, e.size as f32 / mx, w, 7.0, col.gamma_multiply(0.5), col);
                            });
                        });
                    });
            });
        });
    }

    fn page_largest(&mut self, ui: &mut Ui) {
        let Some(r) = self.need_full(ui, "File Besar") else { return };
        self.header(ui, "File Terbesar", &format!("{} file terbesar  ·  double-click untuk membuka folder", r.largest.len()));
        let mx = r.largest.first().map(|e| e.size).unwrap_or(1).max(1) as f32;
        egui::Frame::new()
            .fill(th::CARD)
            .stroke(Stroke::new(1.0, th::BORDER))
            .corner_radius(CornerRadius::same(16))
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let h = ui.available_height().max(300.0);
                TableBuilder::new(ui)
                    .striped(false)
                    .resizable(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::exact(40.0))
                    .column(Column::exact(100.0))
                    .column(Column::exact(150.0))
                    .column(Column::exact(140.0))
                    .column(Column::remainder().clip(true))
                    .max_scroll_height(h)
                    .header(32.0, |mut hd| {
                        for t in ["#", "UKURAN", "", "DIUBAH", "PATH"] {
                            hd.col(|ui| {
                                ui.label(stxt(t, 11.5, th::DIM));
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(34.0, r.largest.len(), |mut row| {
                            let i = row.index();
                            let f = &r.largest[i];
                            row.col(|ui| {
                                ui.label(txt((i + 1).to_string(), 13.0, th::MUTED));
                            });
                            row.col(|ui| {
                                ui.label(stxt(human_size(f.size), 14.0, th::YELLOW));
                            });
                            row.col(|ui| {
                                gradient_bar(ui, f.size as f32 / mx, 136.0, 7.0, th::ORANGE.gamma_multiply(0.5), th::ORANGE);
                            });
                            row.col(|ui| {
                                ui.label(txt(fmt_time(f.mtime), 13.0, th::DIM));
                            });
                            row.col(|ui| {
                                icon(ui, Ic::File, 14.0, th::MUTED);
                                let resp = ui.add(egui::Label::new(txt(&f.path, 13.5, th::FG)).truncate().sense(Sense::click()));
                                if resp.double_clicked() {
                                    if let Some(p) = Path::new(&f.path).parent() {
                                        open_path(&p.to_string_lossy());
                                    }
                                }
                                resp.on_hover_text(&f.path);
                            });
                        });
                    });
            });
    }

    // ===================================================================== report page

    fn page_report(&mut self, ui: &mut Ui) {
        self.header(ui, "Report", "Export hasil scan, atau buka report lama");
        let has = self.result.is_some();
        let gap = 14.0;
        let w = (ui.available_width() - gap * 3.0) / 4.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (ic, title, desc, fmt, col) in [
                (Ic::Globe, "HTML", "Dashboard interaktif dengan grafik, dibuka di browser", "html", th::ACCENT),
                (Ic::Braces, "JSON", "Data lengkap, bisa dibuka lagi di aplikasi ini", "json", th::PURPLE),
                (Ic::Doc, "Markdown", "Ringkasan teks untuk catatan atau GitHub", "md", th::GREEN),
                (Ic::Table, "CSV", "Beberapa file tabel untuk Excel / spreadsheet", "csv", th::YELLOW),
            ] {
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 170.0), Sense::click());
                let hover = resp.hovered() && has;
                let p = ui.painter_at(rect.expand(1.0));
                p.rect(
                    rect,
                    CornerRadius::same(16),
                    if hover { th::CARD_HI } else { th::CARD },
                    Stroke::new(if hover { 1.5 } else { 1.0 }, if hover { col } else { th::BORDER }),
                    StrokeKind::Inside,
                );
                icon_badge(&p, rect.left_top() + vec2(42.0, 42.0), 22.0, ic, if has { col } else { th::MUTED });
                p.text(rect.left_top() + vec2(20.0, 80.0), Align2::LEFT_TOP, title, fb(19.0), if has { th::FG } else { th::MUTED });
                let g = p.layout(desc.to_string(), fr(13.0), th::DIM, w - 40.0);
                p.galley(rect.left_top() + vec2(20.0, 110.0), g, th::DIM);
                if hover {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    draw_icon(&p, Rect::from_min_size(rect.right_top() + vec2(-40.0, 20.0), vec2(20.0, 20.0)), Ic::External, col);
                }
                if resp.clicked() {
                    if has {
                        self.export(fmt);
                    } else {
                        self.toast("Belum ada hasil scan untuk di-export", th::YELLOW);
                    }
                }
            }
        });
        ui.add_space(gap);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if button(ui, Some(Ic::Upload), "Buka report JSON…", Btn::Soft(th::PURPLE)).clicked() {
                    self.open_report();
                }
                if let Some(p) = self.last_export.clone() {
                    if button(ui, Some(Ic::External), "Buka export terakhir", Btn::Ghost).clicked() {
                        open_path(&p.to_string_lossy());
                    }
                    ui.label(txt(p.display().to_string(), 12.5, th::DIM));
                }
            });
            if let Some(r) = &self.result {
                ui.add_space(14.0);
                ui.label(stxt("INFO SCAN SAAT INI", 11.0, th::MUTED));
                ui.add_space(4.0);
                egui::Grid::new("meta").num_columns(2).spacing([36.0, 8.0]).show(ui, |ui| {
                    for (k, v) in [
                        ("Host", format!("{}  ({})", r.meta.host, r.meta.os)),
                        ("Waktu", fmt_time(r.meta.started)),
                        ("Mode", if r.meta.mode == "full" { "Full scan".into() } else { "Quick cache scan".into() }),
                        ("Root", report::short_roots(&r.meta.roots)),
                        ("Durasi", fmt_duration(r.meta.elapsed)),
                        ("Thread", r.meta.threads.to_string()),
                        ("Error akses", fmt_num(r.totals.errors)),
                    ] {
                        ui.label(txt(k, 13.5, th::DIM));
                        ui.label(txt(v, 13.5, th::FG));
                        ui.end_row();
                    }
                });
            }
        });
    }

    // ===================================================================== overlay

    fn modal_frame() -> egui::Frame {
        egui::Frame::new()
            .fill(th::CARD)
            .stroke(Stroke::new(1.0, th::BORDER))
            .corner_radius(CornerRadius::same(18))
            .inner_margin(Margin::same(24))
            .shadow(egui::epaint::Shadow { offset: [0, 16], blur: 48, spread: 0, color: Color32::from_black_alpha(160) })
    }

    fn modal(id: &str, width: f32) -> egui::Window<'static> {
        egui::Window::new(id.to_string())
            .id(egui::Id::new(id.to_string()))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .default_width(width)
            .min_width(width)
            .max_width(width)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(Self::modal_frame())
            .order(egui::Order::Foreground)
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        // ---- detail
        if let Some(i) = self.detail {
            let c = self.result.as_ref().and_then(|r| r.caches.get(i)).cloned();
            if let Some(c) = c {
                dim_background(ctx, "dim_detail");
                let mut close = false;
                Self::modal("detail", 600.0).show(ctx, |ui| {
                    let col = cat_color(&c.category);
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
                        icon_badge(ui.painter(), rect.center(), 23.0, cat_icon(&c.category), col);
                        ui.vertical(|ui| {
                            ui.label(btxt(&c.name, 20.0, th::FG));
                            ui.horizontal(|ui| {
                                pill(ui, Some(cat_icon(&c.category)), &c.category, col);
                                safe_pill(ui, c.safe);
                                pill(ui, None, if c.source == "known" { "lokasi dikenal" } else { "pola folder" }, th::DIM);
                            });
                        });
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            if button_h(ui, Some(Ic::Close), "", Btn::Ghost, 32.0).clicked() {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.label(btxt(human_size(c.size), 36.0, th::FG));
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.add_space(12.0);
                            ui.label(txt(format!("{} file  ·  {} folder", fmt_num(c.files), fmt_num(c.dirs)), 13.5, th::DIM));
                        });
                    });
                    ui.add_space(10.0);
                    egui::Frame::new().fill(th::INPUT).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(14)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::Grid::new("detail_grid").num_columns(2).spacing([20.0, 8.0]).show(ui, |ui| {
                            for (k, v) in [
                                ("Path", c.path.clone()),
                                ("File terbaru", fmt_time(c.newest)),
                                ("File terlama", fmt_time(c.oldest)),
                                ("Catatan", if c.note.is_empty() { "-".into() } else { c.note.clone() }),
                            ] {
                                ui.label(txt(k, 13.0, th::DIM));
                                ui.add(egui::Label::new(txt(v, 13.5, th::FG)).wrap());
                                ui.end_row();
                            }
                        });
                    });
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        if button(ui, Some(Ic::External), "Buka folder", Btn::Ghost).clicked() {
                            open_path(&c.path);
                        }
                        if button(ui, Some(Ic::Copy), "Salin path", Btn::Ghost).clicked() {
                            ui.ctx().copy_text(c.path.clone());
                            self.toast("Path disalin ke clipboard", th::ACCENT);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let marked = self.marked.contains(&i);
                            if marked {
                                if button(ui, Some(Ic::Close), "Batal pilih", Btn::Soft(th::YELLOW)).clicked() {
                                    self.marked.remove(&i);
                                }
                            } else if button(ui, Some(Ic::Check), "Pilih untuk dibersihkan", Btn::Primary(th::GREEN)).clicked() {
                                self.marked.insert(i);
                                close = true;
                            }
                        });
                    });
                });
                if close {
                    self.detail = None;
                }
            } else {
                self.detail = None;
            }
        }

        // ---- konfirmasi clean
        if self.confirm_clean {
            let r = self.result.clone().unwrap_or_default();
            let targets = self.marked_targets();
            let total: u64 = targets.iter().map(|i| r.caches[*i].size).sum();
            let files: u64 = targets.iter().map(|i| r.caches[*i].files).sum();
            let unsafe_n = targets.iter().filter(|i| !r.caches[**i].safe).count();
            dim_background(ctx, "dim_confirm");
            Self::modal("confirm", 660.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                    icon_badge(ui.painter(), rect.center(), 24.0, Ic::Trash, th::RED);
                    ui.vertical(|ui| {
                        ui.label(btxt("Bersihkan cache?", 20.0, th::FG));
                        ui.label(txt(format!("{} lokasi  ·  {} file", targets.len(), fmt_num(files)), 13.5, th::DIM));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(btxt(human_size(total), 28.0, th::RED));
                    });
                });
                ui.add_space(12.0);
                egui::Frame::new().fill(th::INPUT).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(230.0).show(ui, |ui| {
                        for i in &targets {
                            let c = &r.caches[*i];
                            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                            let p = ui.painter_at(rect);
                            draw_icon(
                                &p,
                                Rect::from_center_size(rect.left_center() + vec2(12.0, 0.0), vec2(14.0, 14.0)),
                                if c.safe { Ic::Check } else { Ic::Warning },
                                if c.safe { th::GREEN } else { th::YELLOW },
                            );
                            p.text(rect.left_center() + vec2(28.0, 0.0), Align2::LEFT_CENTER, fit_text(&p, &c.name, &fr(13.5), 210.0), fr(13.5), th::FG);
                            p.text(pos2(rect.left() + 330.0, rect.center().y), Align2::RIGHT_CENTER, human_size(c.size), fs(13.5), th::FG);
                            p.text(
                                pos2(rect.left() + 346.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                fit_text(&p, &c.path, &fr(12.0), rect.width() - 356.0),
                                fr(12.0),
                                th::MUTED,
                            );
                        }
                    });
                });
                ui.add_space(10.0);
                if unsafe_n > 0 {
                    ui.horizontal(|ui| {
                        icon(ui, Ic::Warning, 16.0, th::YELLOW);
                        ui.label(stxt(format!("{unsafe_n} lokasi berstatus 'cek' — pastikan memang tidak dibutuhkan"), 13.5, th::YELLOW));
                    });
                }
                ui.label(txt(
                    "Tutup browser / aplikasi terkait dulu. Isi folder dihapus, folder induknya tetap ada, file yang terkunci dilewati.",
                    12.5,
                    th::DIM,
                ));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if button(ui, None, "Batal", Btn::Ghost).clicked() {
                        self.confirm_clean = false;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, Some(Ic::Trash), "Hapus sekarang", Btn::Primary(th::RED)).clicked() {
                            self.start_clean(false);
                        }
                        if button(ui, Some(Ic::Eye), "Simulasi dulu", Btn::Soft(th::CYAN))
                            .on_hover_text("Hitung yang akan dihapus tanpa menghapus apa pun")
                            .clicked()
                        {
                            self.start_clean(true);
                        }
                    });
                });
            });
        }

        // ---- progress clean
        if let Some(job) = &self.clean {
            let freed = job.progress.freed.load(Ordering::Relaxed);
            let files = job.progress.files.load(Ordering::Relaxed);
            let ratio = if job.total > 0 { freed as f32 / job.total as f32 } else { 0.0 };
            let (dry, count, total) = (job.dry, job.count, job.total);
            let t = self.t0.elapsed().as_secs_f32();
            dim_background(ctx, "dim_clean");
            Self::modal("cleaning", 480.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    spinner(ui, 44.0, t);
                    ui.vertical(|ui| {
                        ui.label(btxt(if dry { "Menjalankan simulasi…" } else { "Membersihkan cache…" }, 18.0, th::FG));
                        ui.label(txt(format!("{count} lokasi  ·  {} file", fmt_num(files)), 13.5, th::DIM));
                    });
                });
                ui.add_space(14.0);
                gradient_bar(
                    ui,
                    ratio,
                    ui.available_width(),
                    12.0,
                    if dry { th::CYAN } else { th::ORANGE },
                    if dry { th::ACCENT } else { th::RED },
                );
                ui.add_space(4.0);
                ui.label(txt(format!("{}  /  {}", human_size(freed), human_size(total)), 13.5, th::FG));
            });
        }

        // ---- hasil clean
        if let Some((items, dry)) = &self.clean_report {
            let freed: u64 = items.iter().map(|i| i.res.freed).sum();
            let files: u64 = items.iter().map(|i| i.res.files).sum();
            let failed: u64 = items.iter().map(|i| i.res.failed).sum();
            let dry = *dry;
            let mut close = false;
            let mut redo = false;
            dim_background(ctx, "dim_report");
            Self::modal("clean_report", 600.0).show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                    let col = if dry { th::CYAN } else { th::GREEN };
                    ui.painter().circle_filled(rect.center(), 30.0, col.gamma_multiply(0.15));
                    draw_icon(ui.painter(), Rect::from_center_size(rect.center(), vec2(30.0, 30.0)), if dry { Ic::Eye } else { Ic::Check }, col);
                    ui.add_space(6.0);
                    ui.label(txt(
                        if dry { "Hasil simulasi — bisa dibebaskan" } else { "Pembersihan selesai — berhasil dibebaskan" },
                        14.0,
                        th::DIM,
                    ));
                    ui.label(btxt(human_size(freed), 40.0, col));
                    ui.label(txt(format!("{} file", fmt_num(files)), 13.5, th::DIM));
                    if failed > 0 {
                        ui.label(txt(format!("{} file dilewati (terkunci / butuh admin)", fmt_num(failed)), 13.0, th::YELLOW));
                    }
                });
                ui.add_space(12.0);
                egui::Frame::new().fill(th::INPUT).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        for it in items {
                            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
                            let p = ui.painter_at(rect);
                            let ok = it.res.failed == 0;
                            draw_icon(
                                &p,
                                Rect::from_center_size(rect.left_center() + vec2(12.0, 0.0), vec2(14.0, 14.0)),
                                if ok { Ic::Check } else { Ic::Warning },
                                if ok { th::GREEN } else { th::YELLOW },
                            );
                            p.text(rect.left_center() + vec2(28.0, 0.0), Align2::LEFT_CENTER, fit_text(&p, &it.name, &fr(13.5), 250.0), fr(13.5), th::FG);
                            p.text(pos2(rect.left() + 370.0, rect.center().y), Align2::RIGHT_CENTER, human_size(it.res.freed), fs(13.5), th::FG);
                            let extra = if it.res.failed > 0 {
                                format!("{} file · {} gagal", fmt_num(it.res.files), it.res.failed)
                            } else {
                                format!("{} file", fmt_num(it.res.files))
                            };
                            p.text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, extra, fr(12.5), if ok { th::DIM } else { th::YELLOW });
                        }
                    });
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if button(ui, None, "Tutup", Btn::Ghost).clicked() {
                        close = true;
                    }
                    if dry {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if button(ui, Some(Ic::Trash), "Hapus beneran", Btn::Primary(th::RED)).clicked() {
                                close = true;
                                redo = true;
                            }
                        });
                    }
                });
            });
            if close {
                self.clean_report = None;
            }
            if redo {
                self.confirm_clean = true;
            }
        }

        // ---- toast
        let n = self.toasts.len();
        for (k, t) in self.toasts.iter().enumerate() {
            let age = t.at.elapsed().as_secs_f32();
            // toast singkat (3.5 dtk) supaya tidak lama menutupi tabel
            let alpha = if age > 2.9 { ((3.5 - age) / 0.6).clamp(0.0, 1.0) } else { (age / 0.15).min(1.0) };
            let slide = (1.0 - (age / 0.22).min(1.0)).powi(2) * 60.0;
            egui::Area::new(egui::Id::new(("toast", k)))
                .anchor(Align2::CENTER_BOTTOM, vec2(116.0, -24.0 - (n - 1 - k) as f32 * 58.0 + slide))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(th::CARD_HI.gamma_multiply(alpha))
                        .stroke(Stroke::new(1.0, th::BORDER.gamma_multiply(alpha)))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin { left: 14, right: 18, top: 12, bottom: 12 })
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 8],
                            blur: 24,
                            spread: 0,
                            color: Color32::from_black_alpha((130.0 * alpha) as u8),
                        })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(vec2(4.0, 22.0), Sense::hover());
                                ui.painter().rect_filled(rect, CornerRadius::same(2), t.color.gamma_multiply(alpha));
                                ui.label(txt(&t.msg, 14.0, th::FG.gamma_multiply(alpha)));
                            });
                        });
                });
        }
    }
}

fn dim_background(ctx: &egui::Context, id: &str) {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(165));
            ui.allocate_rect(screen, Sense::click());
        });
}

fn bar_chart(ui: &mut Ui, items: &[NamedStat], by_count: bool, color: Color32) {
    let mx = items.iter().map(|x| if by_count { x.count } else { x.size }).max().unwrap_or(1).max(1) as f32;
    let h = 140.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + 44.0), Sense::hover());
    let p = ui.painter_at(rect);
    let n = items.len().max(1) as f32;
    let slot = w / n;
    let bw = (slot * 0.56).min(64.0);
    for k in 0..=3 {
        let y = rect.top() + 14.0 + (h - 14.0) * k as f32 / 3.0;
        p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, th::BORDER.gamma_multiply(0.45)));
    }
    for (i, x) in items.iter().enumerate() {
        let v = if by_count { x.count } else { x.size } as f32;
        let bh = (v / mx * (h - 22.0)).max(3.0);
        let cx = rect.left() + slot * (i as f32 + 0.5);
        let r = Rect::from_min_max(pos2(cx - bw / 2.0, rect.top() + h - bh), pos2(cx + bw / 2.0, rect.top() + h));
        let hover = resp.hover_pos().map(|hp| hp.x > cx - slot / 2.0 && hp.x < cx + slot / 2.0).unwrap_or(false);
        let top_col = if hover { lerp_color(color, Color32::WHITE, 0.25) } else { color };
        p.rect_filled(
            Rect::from_min_size(r.min, vec2(r.width(), 8.0_f32.min(r.height()))),
            CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 },
            top_col,
        );
        if r.height() > 6.0 {
            let body = Rect::from_min_max(pos2(r.left(), r.top() + 6.0), r.max);
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(body.left_top(), top_col);
            mesh.colored_vertex(body.right_top(), top_col);
            mesh.colored_vertex(body.left_bottom(), color.gamma_multiply(0.25));
            mesh.colored_vertex(body.right_bottom(), color.gamma_multiply(0.25));
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(1, 2, 3);
            p.add(Shape::mesh(mesh));
        }
        let val = if by_count { fmt_num(x.count) } else { human_size(x.size) };
        p.text(pos2(cx, r.top() - 4.0), Align2::CENTER_BOTTOM, val, fs(11.5), if hover { th::FG } else { th::DIM });
        p.text(pos2(cx, rect.top() + h + 8.0), Align2::CENTER_TOP, &x.name, fr(11.5), if hover { th::FG } else { th::DIM });
        let sub = if by_count { human_size(x.size) } else { format!("{} file", fmt_num(x.count)) };
        p.text(pos2(cx, rect.top() + h + 24.0), Align2::CENTER_TOP, sub, fr(10.5), if hover { color } else { th::MUTED });
    }
}

fn recompute_totals(r: &mut ScanResult) {
    r.totals.cache_size = r.caches.iter().map(|c| c.size).sum();
    r.totals.cache_files = r.caches.iter().map(|c| c.files).sum();
    r.totals.cache_safe_size = r.caches.iter().filter(|c| c.safe).map(|c| c.size).sum();
    let caches = r.caches.clone();
    for cat in r.cache_by_category.iter_mut() {
        cat.size = caches.iter().filter(|c| c.category == cat.category).map(|c| c.size).sum();
        cat.safe_size = caches.iter().filter(|c| c.category == cat.category && c.safe).map(|c| c.size).sum();
    }
    r.cache_by_category.sort_by(|a, b| b.size.cmp(&a.size));
}

// ===================================================================== eframe

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        th::BG.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        if self.frames == 2 || self.frames == 30 {
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
            style_titlebar();
        }
        self.poll();
        if self.scan.is_some() || self.clean.is_some() || !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(th::BG).inner_margin(Margin { left: 30, right: 30, top: 24, bottom: 20 }))
            .show(ctx, |ui| match self.page {
                Page::Dashboard => self.page_dashboard(ui),
                Page::Scan => self.page_scan(ui),
                Page::Cache => self.page_cache(ui),
                Page::Folders => self.page_folders(ui),
                Page::Extensions => self.page_extensions(ui),
                Page::Largest => self.page_largest(ui),
                Page::Report => self.page_report(ui),
            });
        self.overlays(ctx);

        ctx.input(|i| {
            if i.key_pressed(egui::Key::F5) && self.scan.is_none() {
                self.start_cache_scan();
            }
            if i.key_pressed(egui::Key::Escape) {
                self.detail = None;
                self.confirm_clean = false;
            }
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }
}

fn app_icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let r = 15.0f32;
            let dx = (fx - fx.clamp(r, n as f32 - r)).abs();
            let dy = (fy - fy.clamp(r, n as f32 - r)).abs();
            if (dx * dx + dy * dy).sqrt() > r {
                continue;
            }
            let t = (fx + fy) / (2.0 * n as f32);
            let c = lerp_color(th::ACCENT, th::PURPLE, t);
            let dd = (fx - 32.0).abs() + (fy - 32.0).abs();
            let col = if dd < 7.0 {
                th::ACCENT
            } else if dd < 16.0 {
                th::BG
            } else {
                c
            };
            let i = (y * n + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[col.r(), col.g(), col.b(), 255]);
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CacheChecker")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1040.0, 680.0])
            .with_icon(Arc::new(app_icon())),
        centered: true,
        ..Default::default()
    };
    eframe::run_native("CacheChecker", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
