//! Small custom-painted controls matching the design system: flat, square, high contrast.

use crate::theme::{self, Icon, Palette};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Color32, CursorIcon, FontId, Galley, Id, Image, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2, vec2,
};
use std::sync::Arc;

/// Run `add` inside `rect`, clipped to it.
pub fn region<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        add(ui)
    })
    .inner
}

pub fn cut_top(r: &mut Rect, h: f32) -> Rect {
    let out = Rect::from_min_max(r.min, Pos2::new(r.max.x, (r.min.y + h).min(r.max.y)));
    r.min.y = out.max.y;
    out
}

pub fn cut_bottom(r: &mut Rect, h: f32) -> Rect {
    let out = Rect::from_min_max(Pos2::new(r.min.x, (r.max.y - h).max(r.min.y)), r.max);
    r.max.y = out.min.y;
    out
}

pub fn cut_right(r: &mut Rect, w: f32) -> Rect {
    let out = Rect::from_min_max(Pos2::new((r.max.x - w).max(r.min.x), r.min.y), r.max);
    r.max.x = out.min.x;
    out
}

// ───────────────────────────── text ─────────────────────────────

pub fn label(ui: &mut Ui, text: impl Into<String>, font: FontId, color: Color32) -> Response {
    ui.add(egui::Label::new(RichText::new(text).font(font).color(color)))
}

/// Small upper-case, letter-spaced heading.
pub fn caps(text: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        TextFormat { extra_letter_spacing: size * 0.08, ..TextFormat::simple(theme::semi(size), color) },
    );
    job
}

/// A run of differently styled pieces on one line.
pub fn spans(parts: &[(&str, FontId, Color32)]) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (text, font, color) in parts {
        job.append(text, 0.0, TextFormat { valign: egui::Align::Center, ..TextFormat::simple(font.clone(), *color) });

    }
    job
}

/// One line, cut off with an ellipsis.
pub fn elided(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    wrapped(ui, text, font, color, max_width, 1)
}

/// Wraps anywhere (paths have no spaces to break at); at most `max_rows` lines.
pub fn wrapped(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32, max_rows: usize) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping {
        max_width: max_width.max(8.0),
        max_rows,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

// ───────────────────────────── icons ─────────────────────────────

pub fn paint_icon(ui: &Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    Image::from_uri(icon.uri()).tint(color).paint_at(ui, rect);
}

pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_icon(ui, icon, rect.center(), size, color);
    resp
}

/// A borderless square button holding one icon.
pub fn icon_btn(ui: &mut Ui, ic: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    paint_icon_btn(ui, rect, &resp, ic, color);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Same, at a fixed place (for hand-laid-out rows).
pub fn icon_btn_at(ui: &Ui, rect: Rect, id: Id, ic: Icon, color: Color32) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    paint_icon_btn(ui, rect, &resp, ic, color);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

fn paint_icon_btn(ui: &Ui, rect: Rect, resp: &Response, ic: Icon, color: Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 0.0, color.gamma_multiply(0.22));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, color.gamma_multiply(0.12));
    }
    let icon_size = if rect.width() < 30.0 { 14.0 } else { 16.0 };
    paint_icon(ui, ic, rect.center(), icon_size, color);
}

// ───────────────────────────── buttons ─────────────────────────────

#[derive(Clone, Copy)]
pub struct BtnStyle {
    pub fill: Color32,
    pub fill_hover: Color32,
    pub fill_down: Color32,
    pub text: Color32,
    pub border: Option<Color32>,
}

impl BtnStyle {
    pub fn primary(p: &Palette) -> Self {
        BtnStyle {
            fill: p.accent,
            fill_hover: theme::mix(p.accent, Color32::BLACK, 0.10),
            fill_down: theme::mix(p.accent, Color32::BLACK, 0.26),
            text: p.on_accent,
            border: None,
        }
    }

    pub fn secondary(p: &Palette) -> Self {
        Self::outline(p.text, p.divider)
    }

    /// Transparent with a border; works on any background.
    pub fn outline(fg: Color32, border: Color32) -> Self {
        BtnStyle {
            fill: Color32::TRANSPARENT,
            fill_hover: fg.gamma_multiply(0.09),
            fill_down: fg.gamma_multiply(0.18),
            text: fg,
            border: Some(border),
        }
    }

    pub fn solid(fill: Color32, text: Color32) -> Self {
        BtnStyle {
            fill,
            fill_hover: theme::mix(fill, text, 0.10),
            fill_down: theme::mix(fill, text, 0.20),
            text,
            border: None,
        }
    }
}

pub struct Btn<'a> {
    label: &'a str,
    icon: Option<Icon>,
    style: BtnStyle,
    height: f32,
    font: f32,
    pad_x: f32,
    enabled: bool,
}

impl<'a> Btn<'a> {
    pub fn new(label: &'a str, style: BtnStyle) -> Self {
        Btn { label, icon: None, style, height: 36.0, font: 14.0, pad_x: 14.0, enabled: true }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The compact variant used inside bars and list rows.
    pub fn small(mut self) -> Self {
        self.height = 32.0;
        self.font = 13.0;
        self.pad_x = 10.0;
        self
    }

    pub fn tiny(mut self) -> Self {
        self.height = 28.0;
        self.font = 12.0;
        self.pad_x = 10.0;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let fade = if self.enabled { 1.0 } else { 0.45 };
        let text_color = self.style.text.gamma_multiply(fade);
        let galley = ui.painter().layout_no_wrap(self.label.to_owned(), theme::heavy(self.font), text_color);
        let icon_w = if self.icon.is_some() { 16.0 + 6.0 } else { 0.0 };
        let size = vec2((self.pad_x * 2.0 + icon_w + galley.size().x).ceil(), self.height);
        let sense = if self.enabled { Sense::click() } else { Sense::hover() };
        let (rect, resp) = ui.allocate_exact_size(size, sense);
        if ui.is_rect_visible(rect) {
            let fill = if !self.enabled {
                self.style.fill.gamma_multiply(fade)
            } else if resp.is_pointer_button_down_on() {
                self.style.fill_down
            } else if resp.hovered() {
                self.style.fill_hover
            } else {
                self.style.fill
            };
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, fill);
            if let Some(border) = self.style.border {
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0, border.gamma_multiply(fade)), StrokeKind::Inside);
            }
            let mut x = rect.left() + self.pad_x;
            if let Some(ic) = self.icon {
                paint_icon(ui, ic, Pos2::new(x + 8.0, rect.center().y), 16.0, text_color);
                x += 22.0;
            }
            let y = rect.center().y - galley.size().y / 2.0;
            ui.painter().galley(Pos2::new(x, y), galley, text_color);
        }
        if self.enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
    }
}

/// Segmented control. Returns the option clicked this frame, if any.
pub fn seg(ui: &mut Ui, p: &Palette, id_salt: &str, labels: &[&str], selected: usize) -> Option<usize> {
    const H: f32 = 34.0;
    let galleys: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let (font, color) =
                if i == selected { (theme::semi(13.0), p.bg) } else { (theme::body(13.0), p.text) };
            ui.painter().layout_no_wrap((*l).to_owned(), font, color)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| (g.size().x + 24.0).ceil()).collect();
    let (rect, _) = ui.allocate_exact_size(vec2(widths.iter().sum::<f32>() + 2.0, H), Sense::hover());
    let base = ui.scope_id().with(id_salt);
    let mut clicked = None;
    let mut x = rect.left() + 1.0;
    for (i, galley) in galleys.into_iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + 1.0), vec2(widths[i], H - 2.0));
        let resp = ui.interact(r, base.with(i), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        if i == selected {
            ui.painter().rect_filled(r, 0.0, p.text);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, p.text.gamma_multiply(0.07));
        }
        if i > 0 {
            ui.painter().vline(x, r.y_range(), Stroke::new(1.0, p.divider));
        }
        let pos = Pos2::new(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, p.text);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, p.divider), StrokeKind::Inside);
    clicked
}

// ───────────────────────────── tags ─────────────────────────────

#[derive(Clone, Copy)]
pub enum TagKind {
    Neutral,
    Accent,
    Outline,
}

const TAG_H: f32 = 20.0;

fn tag_colors(p: &Palette, kind: TagKind) -> (Color32, Color32, Option<Color32>) {
    match kind {
        TagKind::Neutral => (p.n300, p.muted(0.92), None),
        TagKind::Accent if p.dark => (p.a100, p.a800, None),
        TagKind::Accent => (hex_accent_200(), p.a800, None),
        TagKind::Outline => (Color32::TRANSPARENT, p.a600, Some(p.a600)),
    }
}

fn hex_accent_200() -> Color32 {
    theme::hex(0xffe0d9)
}

/// Paint a tag with its top-left at `pos`; returns the rect it covers.
pub fn paint_tag(ui: &Ui, p: &Palette, pos: Pos2, text: &str, kind: TagKind) -> Rect {
    let (fill, fg, outline) = tag_colors(p, kind);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::body(11.0), fg);
    let rect = Rect::from_min_size(pos, vec2((galley.size().x + 20.0).ceil(), TAG_H));
    ui.painter().rect_filled(rect, 0.0, fill);
    if let Some(c) = outline {
        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, c), StrokeKind::Inside);
    }
    let at = Pos2::new(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, fg);
    rect
}

pub fn tag(ui: &mut Ui, p: &Palette, text: &str, kind: TagKind) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::body(11.0), p.text);
    let (rect, resp) = ui.allocate_exact_size(vec2((galley.size().x + 20.0).ceil(), TAG_H), Sense::hover());
    paint_tag(ui, p, rect.min, text, kind);
    resp
}

// ───────────────────────────── inputs ─────────────────────────────

fn paint_check(ui: &Ui, p: &Palette, rect: Rect, checked: bool, hovered: bool) {
    if checked {
        ui.painter().rect_filled(rect, 0.0, p.accent);
        paint_icon(ui, Icon::Check, rect.center(), 12.0, Color32::WHITE);
    } else {
        let border = if hovered { p.accent } else { p.divider };
        ui.painter().rect(rect, 0.0, p.surface, Stroke::new(1.5, border), StrokeKind::Inside);
    }
}

/// A bare 16px check box at `rect`. Returns true when clicked.
pub fn checkbox_at(ui: &Ui, p: &Palette, rect: Rect, id: Id, checked: bool) -> bool {
    let resp = ui.interact(rect.expand(4.0), id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    paint_check(ui, p, rect, checked, resp.hovered());
    resp.clicked()
}

/// Check box followed by a label; clicking either toggles. Returns true when toggled.
pub fn check_label(ui: &mut Ui, p: &Palette, checked: &mut bool, text: LayoutJob) -> bool {
    let galley = ui.painter().layout_job(text);
    let size = vec2(16.0 + 10.0 + galley.size().x, galley.size().y.max(18.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let box_rect = Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - 8.0), Vec2::splat(16.0));
    paint_check(ui, p, box_rect, *checked, resp.hovered());
    let at = Pos2::new(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, p.text);
    if resp.clicked() {
        *checked = !*checked;
    }
    resp.clicked()
}

/// Single-line text field, `width` wide overall.
pub fn text_input(ui: &mut Ui, p: &Palette, text: &mut String, hint: &str, width: f32, font: FontId) -> Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(RichText::new(hint).font(font.clone()).color(p.muted(0.5)))
            .font(font)
            .text_color(p.text)
            .margin(egui::Margin::symmetric(10, 9))
            .desired_width(width),
    )
}

/// The label above a form field.
pub fn field_label(ui: &mut Ui, p: &Palette, text: &str) {
    label(ui, text, theme::body(12.0), p.muted(0.7));
    ui.add_space(-3.0);
}
