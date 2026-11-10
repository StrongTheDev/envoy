//! The "Modernist" look from the design: palette, fonts, icons.

use egui::{Color32, FontFamily, FontId, Stroke};
use std::sync::Arc;

pub const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// `a` with `t` of `b` mixed in (0 = all `a`, 1 = all `b`).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub accent: Color32,
    /// Text on an accent-filled button.
    pub on_accent: Color32,
    pub divider: Color32,
    pub n300: Color32,
    pub n400: Color32,
    pub n600: Color32,
    pub a100: Color32,
    pub a400: Color32,
    pub a600: Color32,
    pub a700: Color32,
    pub a800: Color32,
    pub a900: Color32,
}

impl Palette {
    pub fn light() -> Self {
        let (bg, text) = (hex(0xf3f2f2), hex(0x201e1d));
        Palette {
            dark: false,
            bg,
            surface: hex(0xeae9e9),
            text,
            accent: hex(0xec3013),
            on_accent: bg,
            divider: mix(bg, text, 0.40),
            n300: hex(0xd7d3d3),
            n400: hex(0xbab6b6),
            n600: hex(0x7d7979),
            a100: hex(0xfff2ef),
            a400: hex(0xff9783),
            a600: hex(0xdd2b0f),
            a700: hex(0xae1800),
            a800: hex(0x7c1405),
            a900: hex(0x4d170e),
        }
    }

    pub fn dark() -> Self {
        // Deeper page, lifted panels, softer text and hairline dividers: less glare,
        // more depth. The accent family is unchanged.
        let (bg, text) = (hex(0x151312), hex(0xe9e5e3));
        Palette {
            dark: true,
            bg,
            surface: hex(0x211e1c),
            text,
            accent: hex(0xf03a1c),
            on_accent: Color32::WHITE,
            divider: mix(bg, text, 0.20),
            n300: hex(0x3a3736),
            n400: hex(0x575352),
            n600: hex(0x9c9795),
            a100: hex(0x341610),
            a400: hex(0xff9783),
            a600: hex(0xff563c),
            a700: hex(0xff9783),
            a800: hex(0xffc4b8),
            a900: hex(0xffe0d9),
        }
    }

    pub fn of(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// Text at reduced emphasis, pre-blended against the page background.
    pub fn muted(&self, opacity: f32) -> Color32 {
        mix(self.bg, self.text, opacity)
    }

}

// ───────────────────────────── fonts ─────────────────────────────

pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semi".into()))
}
pub fn heavy(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("heavy".into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
pub fn mono_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("mono-bold".into()))
}

pub fn install_fonts(ctx: &egui::Context) {
    use egui::{epaint::text::{FontTweak, VariationCoords}, FontData, FontDefinitions};
    let mut defs = FontDefinitions::default();
    let fallback_prop = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let fallback_mono = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    let mut add = |name: &str, data: FontData, family: FontFamily, fallback: &[String]| {
        defs.font_data.insert(name.to_owned(), Arc::new(data));
        let mut list = vec![name.to_owned()];
        list.extend(fallback.iter().cloned());
        defs.families.insert(family, list);
    };

    // Google Sans is one variable font; each family is the same file at a different weight.
    let sans = |wght: f32| {
        FontData::from_static(include_bytes!("../assets/fonts/GoogleSans-VariableFont_GRAD,opsz,wght.ttf"))
            .tweak(FontTweak { coords: VariationCoords::new([(b"wght", wght)]), ..Default::default() })
    };
    add("sans", sans(400.0), FontFamily::Proportional, &fallback_prop);
    add("sans-semi", sans(600.0), FontFamily::Name("semi".into()), &fallback_prop);
    add("sans-mono", sans(400.0), FontFamily::Monospace, &fallback_mono);
    add("sans-mono-b", sans(600.0), FontFamily::Name("mono-bold".into()), &fallback_mono);

    add("sans-heavy", sans(700.0), FontFamily::Name("heavy".into()), &fallback_prop);
    ctx.set_fonts(defs);
}

/// Push the palette into egui's own widgets (text fields, scroll bars, combo boxes, tooltips).
pub fn apply(ctx: &egui::Context, p: &Palette) {
    use egui::{CornerRadius, Shadow, Theme};
    ctx.set_theme(if p.dark { Theme::Dark } else { Theme::Light });
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 7.0);
        s.spacing.interact_size.y = 32.0;
        s.spacing.scroll = egui::style::ScrollStyle::solid();
        s.spacing.scroll.bar_width = 10.0;
        s.spacing.scroll.bar_inner_margin = 0.0;
        s.spacing.scroll.bar_outer_margin = 0.0;
        s.interaction.selectable_labels = false;
        s.interaction.tooltip_delay = 0.35;
        for (style, font) in [
            (egui::TextStyle::Body, body(14.0)),
            (egui::TextStyle::Button, body(14.0)),
            (egui::TextStyle::Small, body(12.0)),
            (egui::TextStyle::Heading, heavy(20.0)),
            (egui::TextStyle::Monospace, mono(13.0)),
        ] {
            s.text_styles.insert(style, font);
        }

        let v = &mut s.visuals;
        v.dark_mode = p.dark;
        v.override_text_color = Some(p.text);
        v.panel_fill = p.bg;
        v.window_fill = p.surface;
        v.window_stroke = Stroke::new(1.0, p.divider);
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = CornerRadius::ZERO;
        v.extreme_bg_color = p.surface;
        v.text_edit_bg_color = Some(p.surface);
        v.faint_bg_color = p.surface;
        v.code_bg_color = p.surface;
        v.hyperlink_color = p.a700;
        v.selection.bg_fill = mix(p.surface, p.accent, 0.30);
        v.selection.stroke = Stroke::new(1.0, p.accent);
        v.text_cursor.stroke = Stroke::new(2.0, p.accent);
        let shadow = Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 140 } else { 56 }),
        };
        v.popup_shadow = Shadow { offset: [0, 3], blur: 10, ..shadow };
        v.window_shadow = shadow;

        let w = &mut v.widgets;
        for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            wv.corner_radius = CornerRadius::ZERO;
            wv.expansion = 0.0;
            wv.fg_stroke = Stroke::new(1.0, p.text);
            wv.bg_stroke = Stroke::new(1.0, p.divider);
        }
        w.noninteractive.bg_fill = p.surface;
        w.noninteractive.weak_bg_fill = p.surface;
        w.inactive.bg_fill = p.n400;
        w.inactive.weak_bg_fill = p.surface;
        w.hovered.bg_fill = p.n600;
        w.hovered.weak_bg_fill = mix(p.surface, p.text, 0.07);
        w.hovered.bg_stroke = Stroke::new(1.0, mix(p.bg, p.text, 0.6));
        w.active.bg_fill = p.n600;
        w.active.weak_bg_fill = mix(p.surface, p.text, 0.14);
        w.active.bg_stroke = Stroke::new(1.0, p.accent);
        w.open.bg_fill = p.surface;
        w.open.weak_bg_fill = p.surface;
    });
}

// ───────────────────────────── icons ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Sun,
    Moon,
    User,
    Shield,
    Conflict,
    Search,
    History,
    Transfer,
    Plus,
    Warn,
    Eye,
    EyeOff,
    Edit,
    Trash,
    Close,
    Grip,
    MoveOut,
    Check,
    Up,
    Down,
    Refresh,
    Folder,
}

impl Icon {
    const ALL: [Icon; 22] = [
        Icon::Sun,
        Icon::Moon,
        Icon::User,
        Icon::Shield,
        Icon::Conflict,
        Icon::Search,
        Icon::History,
        Icon::Transfer,
        Icon::Plus,
        Icon::Warn,
        Icon::Eye,
        Icon::EyeOff,
        Icon::Edit,
        Icon::Trash,
        Icon::Close,
        Icon::Grip,
        Icon::MoveOut,
        Icon::Check,
        Icon::Up,
        Icon::Down,
        Icon::Refresh,
        Icon::Folder,
    ];

    /// (stroke width, SVG body). Stroke width 0 means "filled".
    fn def(self) -> (f32, &'static str) {
        match self {
            Icon::Sun => (2.0, r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#),
            Icon::Moon => (2.0, r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#),
            Icon::User => (2.0, r#"<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>"#),
            Icon::Shield => (2.0, r#"<path d="M20 13c0 5-3.5 7.5-7.7 8.9a1 1 0 0 1-.6 0C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.2-2.7a1.2 1.2 0 0 1 1.6 0C14.5 3.8 17 5 19 5a1 1 0 0 1 1 1z"/>"#),
            Icon::Conflict => (2.0, r#"<path d="M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5"/>"#),
            Icon::Search => (2.0, r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#),
            Icon::History => (2.0, r#"<path d="M3 12a9 9 0 1 0 9-9 9.8 9.8 0 0 0-6.7 2.7L3 8"/><path d="M3 3v5h5M12 7v5l4 2"/>"#),
            Icon::Transfer => (2.0, r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12"/>"#),
            Icon::Plus => (2.0, r#"<path d="M5 12h14M12 5v14"/>"#),
            Icon::Warn => (2.2, r#"<path d="m21.7 18-8-14a2 2 0 0 0-3.5 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.7-3"/><path d="M12 9v4M12 17h.01"/>"#),
            Icon::Eye => (2.0, r#"<path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7"/><circle cx="12" cy="12" r="3"/>"#),
            Icon::EyeOff => (2.0, r#"<path d="M9.9 5.2A10.6 10.6 0 0 1 12 5c7 0 10 7 10 7a13.2 13.2 0 0 1-1.7 2.7M6.6 6.6C3.6 8.4 2 12 2 12s3 7 10 7a9.7 9.7 0 0 0 5.4-1.6M2 2l20 20M9.9 9.9a3 3 0 1 0 4.2 4.2"/>"#),
            Icon::Edit => (2.0, r#"<path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/>"#),
            Icon::Trash => (2.0, r#"<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#),
            Icon::Close => (2.0, r#"<path d="M18 6 6 18M6 6l12 12"/>"#),
            Icon::Grip => (0.0, r#"<circle cx="9" cy="6" r="1.3"/><circle cx="15" cy="6" r="1.3"/><circle cx="9" cy="12" r="1.3"/><circle cx="15" cy="12" r="1.3"/><circle cx="9" cy="18" r="1.3"/><circle cx="15" cy="18" r="1.3"/>"#),
            Icon::MoveOut => (2.0, r#"<path d="m15 10 5 5-5 5"/><path d="M4 4v7a4 4 0 0 0 4 4h12"/>"#),
            Icon::Check => (2.4, r#"<path d="M20 6 9 17l-5-5"/>"#),
            Icon::Up => (2.0, r#"<path d="m18 15-6-6-6 6"/>"#),
            Icon::Down => (2.0, r#"<path d="m6 9 6 6 6-6"/>"#),
            Icon::Refresh => (2.0, r#"<path d="M3 12a9 9 0 0 1 15-6.7L21 8M21 3v5h-5M21 12a9 9 0 0 1-15 6.7L3 16M3 21v-5h5"/>"#),
            Icon::Folder => (2.0, r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.6 3.9A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#),
        }
    }

    pub fn uri(self) -> String {
        format!("bytes://envoy-icon/{self:?}.svg")
    }

    fn svg(self) -> String {
        let (stroke, body) = self.def();
        let paint = if stroke == 0.0 {
            r#"fill="white""#.to_owned()
        } else {
            format!(r#"fill="none" stroke="white" stroke-width="{stroke}""#)
        };
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" {paint}>{body}</svg>"#)
    }
}

/// Icons are drawn white and tinted at paint time.
pub fn install_icons(ctx: &egui::Context) {
    for icon in Icon::ALL {
        ctx.include_bytes(icon.uri(), icon.svg().into_bytes());
    }
}
