//! The look: palette from the theme, system fonts, and the few custom widgets
//! everything is built from. Inspired by Langfuse: paper background, hairlines
//! instead of filled panels, one dark primary button, a highlighter accent,
//! mono for numbers, and a keyboard chip on every action.

use caturankam_theme::{Rgb, Theme};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, Vec2, pos2,
};

/// The theme's `[ui]` colours, ready to paint with.
#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,
    pub surface: Color32,
    pub line: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub primary: Color32,
    /// Readable ink on `primary`.
    pub on_primary: Color32,
    pub accent: Color32,
    pub dark: bool,
}

impl Pal {
    /// Dark mode uses the theme's `[ui_dark]`, if it has one.
    pub fn new(theme: &Theme, dark: bool) -> Self {
        let dark_ui = theme.ui_dark.as_ref().filter(|_| dark);
        let u = dark_ui.unwrap_or(&theme.ui);
        Self {
            dark: dark_ui.is_some(),
            on_primary: ink_on(rgb(u.primary)),
            bg: rgb(u.background),
            surface: rgb(u.surface),
            line: rgb(u.line),
            text: rgb(u.text),
            muted: rgb(u.muted),
            primary: rgb(u.primary),
            accent: rgb(u.accent),
        }
    }
}

pub fn rgb(c: Rgb) -> Color32 {
    let [r, g, b] = c.0;
    Color32::from_rgb(r, g, b)
}

const RADIUS: u8 = 3;

/// A heavier face for headings and primary buttons.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Uses macOS's own SF Pro and SF Mono. The files are memory-mapped, not
/// read: SF Pro is 8 MB, and only the pages for glyphs we draw get loaded.
/// Elsewhere, egui's bundled fonts stay.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::epaint::text::{FontData, FontDefinitions, FontTweak, VariationCoords};
    use std::sync::Arc;

    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let mut medium = fallback.clone();

    #[cfg(target_os = "macos")]
    if let (Some(sf), Some(sf_mono)) = (
        map_font("/System/Library/Fonts/SFNS.ttf"),
        map_font("/System/Library/Fonts/SFNSMono.ttf"),
    ) {
        let weight = |w: f32| FontTweak {
            coords: VariationCoords::new([(b"wght", w)]),
            ..Default::default()
        };
        let mut add = |name: &str, data: FontData| {
            fonts.font_data.insert(name.into(), Arc::new(data));
        };
        add("sf", FontData::from_static(sf).tweak(weight(400.0)));
        add("sf-medium", FontData::from_static(sf).tweak(weight(590.0)));
        add("sf-mono", FontData::from_static(sf_mono));
        let fams = &mut fonts.families;
        fams.get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "sf".into());
        fams.get_mut(&FontFamily::Monospace)
            .unwrap()
            .insert(0, "sf-mono".into());
        medium.insert(0, "sf-medium".into());
    }
    fonts
        .families
        .insert(FontFamily::Name("medium".into()), medium);
    ctx.set_fonts(fonts);
}

/// Maps a system font for the life of the process. Leaking the map is the
/// point: egui needs `'static` bytes, and fonts are used until exit.
#[cfg(target_os = "macos")]
fn map_font(path: &str) -> Option<&'static [u8]> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: a read-only system font that nothing rewrites while we run.
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    Some(&Box::leak(Box::new(map))[..])
}

/// egui's own widgets (checkbox, slider, menus) in the same palette.
pub fn apply(ctx: &egui::Context, pal: &Pal) {
    let mut v = if pal.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = pal.bg;
    v.window_fill = pal.surface;
    v.extreme_bg_color = pal.surface;
    v.faint_bg_color = pal.bg;
    v.override_text_color = Some(pal.text);
    v.weak_text_color = Some(pal.muted);
    v.selection.bg_fill = pal.accent;
    // Text on the highlighter stays dark in both modes.
    v.selection.stroke = Stroke::new(1.0, ink_on(pal.accent));
    v.window_stroke = Stroke::new(1.0, pal.line);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    let soft = egui::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(18),
    };
    v.window_shadow = soft;
    v.popup_shadow = soft;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
    for (w, fill) in [
        (&mut v.widgets.inactive, pal.surface),
        (&mut v.widgets.hovered, hover(pal, pal.bg)),
        (&mut v.widgets.active, pal.accent),
    ] {
        w.weak_bg_fill = fill;
        w.bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, pal.line);
        w.fg_stroke = Stroke::new(1.0, pal.text);
        w.corner_radius = CornerRadius::same(RADIUS);
        w.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        use egui::TextStyle::*;
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        for (style, size) in [(Body, 13.5), (Button, 13.5), (Small, 11.0), (Heading, 20.0)] {
            if let Some(f) = s.text_styles.get_mut(&style) {
                f.size = size;
            }
        }
        if let Some(f) = s.text_styles.get_mut(&Monospace) {
            f.size = 12.5;
        }
    });
}

/// A hovered version of a fill: nudged a little towards the text colour,
/// so it darkens in light mode and lightens in dark mode.
fn hover(pal: &Pal, c: Color32) -> Color32 {
    c.lerp_to_gamma(pal.text, 0.06)
}

/// Black or white, whichever reads better on `fill`.
pub fn ink_on(fill: Color32) -> Color32 {
    let [r, g, b, _] = fill.to_array();
    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    if luma > 150.0 {
        Color32::from_rgb(29, 29, 27)
    } else {
        Color32::WHITE
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// The one main action in a section: dark fill.
    Primary,
    /// Everything else: surface with a hairline.
    Secondary,
}

/// A button with an optional keyboard chip, e.g. `Start game [S]`.
pub fn button(ui: &mut Ui, pal: &Pal, text: &str, key: Option<&str>, kind: Kind) -> Response {
    let text_g = ui.painter().layout_no_wrap(
        text.into(),
        FontId::proportional(13.0),
        Color32::PLACEHOLDER,
    );
    let chip = key.map(|k| chip_galley(ui, k));
    let chip_w = chip.as_ref().map_or(0.0, |g| chip_width(g) + 8.0);
    let size = Vec2::new(text_g.size().x + 20.0 + chip_w, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let p = ui.painter();
    let (fill, ink, border) = match kind {
        Kind::Primary => (pal.primary, pal.on_primary, pal.primary),
        Kind::Secondary => (pal.surface, pal.text, pal.line),
    };
    let fill = if resp.hovered() && enabled {
        lift(pal, fill, kind)
    } else {
        fill
    };
    let ink = if enabled {
        ink
    } else {
        ink.gamma_multiply(0.45)
    };
    p.rect(
        rect,
        RADIUS,
        fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    p.galley(
        pos2(rect.left() + 10.0, rect.center().y - text_g.size().y / 2.0),
        text_g,
        ink,
    );
    if let Some(g) = chip {
        let r = Rect::from_min_size(
            pos2(rect.right() - 6.0 - chip_width(&g), rect.center().y - 8.0),
            Vec2::new(chip_width(&g), 16.0),
        );
        paint_chip(p, r, g, kind == Kind::Primary, pal);
    }
    resp
}

/// A full-width list row: `label  meta ............ [key]`, like Langfuse's
/// sidebar lists. Flat until hovered.
pub fn row(ui: &mut Ui, pal: &Pal, label: &str, meta: &str, key: Option<&str>) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect(
            rect,
            RADIUS,
            pal.surface,
            Stroke::new(1.0, pal.line),
            StrokeKind::Inside,
        );
    }
    let y = rect.center().y;
    p.text(
        pos2(rect.left() + 8.0, y),
        Align2::LEFT_CENTER,
        label,
        mono(13.0),
        pal.text,
    );
    p.text(
        pos2(rect.left() + 72.0, y),
        Align2::LEFT_CENTER,
        meta,
        FontId::proportional(13.0),
        pal.muted,
    );
    if let Some(k) = key {
        let g = chip_galley(ui, k);
        let r = Rect::from_min_size(
            pos2(rect.right() - 6.0 - chip_width(&g), y - 8.0),
            Vec2::new(chip_width(&g), 16.0),
        );
        paint_chip(ui.painter(), r, g, false, pal);
    }
    resp
}

/// A key chip on its own, e.g. next to a label.
pub fn kbd(ui: &mut Ui, pal: &Pal, key: &str) {
    let g = chip_galley(ui, key);
    let (r, _) = ui.allocate_exact_size(Vec2::new(chip_width(&g), 16.0), Sense::hover());
    paint_chip(ui.painter(), r, g, false, pal);
}

/// A section title, optionally with something on the right.
pub fn section(ui: &mut Ui, pal: &Pal, title: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(medium(13.5))
                .color(pal.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(2.0);
}

/// A full-width hairline between sections.
pub fn rule(ui: &mut Ui) {
    ui.add(egui::Separator::default().spacing(22.0));
}

pub fn muted(pal: &Pal, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).size(12.0).color(pal.muted)
}

fn chip_galley(ui: &Ui, key: &str) -> std::sync::Arc<egui::Galley> {
    ui.painter()
        .layout_no_wrap(key.into(), mono(10.5), Color32::PLACEHOLDER)
}

fn chip_width(g: &egui::Galley) -> f32 {
    (g.size().x + 8.0).max(16.0)
}

fn paint_chip(p: &Painter, r: Rect, g: std::sync::Arc<egui::Galley>, on_primary: bool, pal: &Pal) {
    let (border, ink, fill) = if on_primary {
        (
            pal.on_primary.gamma_multiply(0.35),
            pal.on_primary.gamma_multiply(0.85),
            Color32::TRANSPARENT,
        )
    } else {
        (pal.line, pal.muted, pal.bg)
    };
    p.rect(r, 2, fill, Stroke::new(1.0, border), StrokeKind::Inside);
    let pos = r.center() - g.size() / 2.0;
    p.galley(pos, g, ink);
}

fn lift(pal: &Pal, c: Color32, kind: Kind) -> Color32 {
    match kind {
        Kind::Primary => c.lerp_to_gamma(pal.bg, 0.15),
        Kind::Secondary => hover(pal, c),
    }
}

/// Fine diagonal hatching, for empty gutters (Langfuse's signature texture).
pub fn hatch(p: &Painter, rect: Rect, color: Color32) {
    let p = p.with_clip_rect(rect);
    let step = 7.0;
    let stroke = Stroke::new(1.0, color);
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        p.line_segment(
            [pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())],
            stroke,
        );
        x += step;
    }
}

/// How far crop marks overshoot the frame's corners.
pub const CROP: f32 = 5.0;

/// A hairline frame whose edges overshoot the corners slightly, like crop
/// marks. `rect` is the frame itself.
pub fn crop_frame(p: &Painter, rect: Rect, color: Color32) {
    let s = Stroke::new(1.0, color);
    let o = CROP;
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    p.line_segment([pos2(l - o, t), pos2(r + o, t)], s);
    p.line_segment([pos2(l - o, b), pos2(r + o, b)], s);
    p.line_segment([pos2(l, t - o), pos2(l, b + o)], s);
    p.line_segment([pos2(r, t - o), pos2(r, b + o)], s);
}
