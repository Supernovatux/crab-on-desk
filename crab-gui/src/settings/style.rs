// crab-on-desk, a Rust based desktop pet for coding agents.
//     Copyright (C) 2026  Supernovatux thulashitharan.d@gmail.com
//
//     This program is free software: you can redistribute it and/or modify
//     it under the terms of the GNU Affero General Public License as
//     published by the Free Software Foundation, either version 3 of the
//     License, or (at your option) any later version.
//
//     This program is distributed in the hope that it will be useful,
//     but WITHOUT ANY WARRANTY; without even the implied warranty of
//     MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//     GNU Affero General Public License for more details.
//
//     You should have received a copy of the GNU Affero General Public License
//     along with this program.  If not, see <https://www.gnu.org/licenses/>.

use iced::{
    Background, Border, Color, Theme, color,
    theme::Palette,
    widget::{button, container, rule, text_input, toggler},
};

use crate::theme::{Appearance, BRAND, Scheme, outline};

const THEME_NAME: &str = "Settings";
const SIDEBAR_RADIUS: f32 = 7.0;
const SECTION_RADIUS: f32 = 10.0;
const THUMB_RADIUS: f32 = 7.0;
const BUTTON_RADIUS: f32 = 8.0;
const BADGE_RADIUS: f32 = 8.0;
const TOAST_RADIUS: f32 = 10.0;
const ACCENT_TINT: f32 = 0.1;
const ACCENT_EDGE: f32 = 0.24;
const DANGER_TINT: f32 = 0.08;
const DANGER_EDGE: f32 = 0.35;
const DISABLED_OPACITY: f32 = 0.5;
const SWITCH_PADDING_RATIO: f32 = 2.0 / 24.0;

#[derive(Debug)]
pub struct Colors {
    pub background: Color,
    pub panel: Color,
    pub sidebar: Color,
    pub sidebar_hover: Color,
    pub sidebar_active: Color,
    pub sidebar_active_text: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub border: Color,
    pub row_border: Color,
    pub rows: Color,
    pub switch_off: Color,
    pub switch_knob: Color,
    pub thumb: Color,
    pub danger: Color,
    pub toast: Color,
    pub toast_text: Color,
}

const DARK: Colors = Colors {
    background: color!(0x1c_1c_1f),
    panel: color!(0x23_23_27),
    sidebar: color!(0x18_18_1b),
    sidebar_hover: color!(0xff_ff_ff, 0.05),
    sidebar_active: color!(0x2e_2e_33),
    sidebar_active_text: color!(0xf5_f5_f7),
    text: color!(0xf4_f4_f5),
    text_secondary: color!(0xa1_a1_aa),
    text_tertiary: color!(0x71_71_7a),
    border: color!(0xff_ff_ff, 0.08),
    row_border: color!(0xff_ff_ff, 0.06),
    rows: color!(0x1a_1a_1d),
    switch_off: color!(0x3f_3f_46),
    switch_knob: color!(0xf4_f4_f5),
    thumb: color!(0xff_ff_ff, 0.04),
    danger: color!(0xb9_1c_1c),
    toast: color!(0x18_18_1b),
    toast_text: color!(0xf5_f5_f7),
};

const LIGHT: Colors = Colors {
    background: color!(0xf5_f5_f7),
    panel: color!(0xff_ff_ff),
    sidebar: color!(0xec_ec_ef),
    sidebar_hover: color!(0x00_00_00, 0.05),
    sidebar_active: color!(0xff_ff_ff),
    sidebar_active_text: color!(0x0a_0a_0c),
    text: color!(0x18_18_1b),
    text_secondary: color!(0x6b_6b_70),
    text_tertiary: color!(0x9b_9b_a0),
    border: color!(0x00_00_00, 0.08),
    row_border: color!(0x00_00_00, 0.06),
    rows: color!(0xf5_f5_f7),
    switch_off: color!(0xd4_d4_d8),
    switch_knob: color!(0xff_ff_ff),
    thumb: color!(0x00_00_00, 0.04),
    danger: color!(0xb9_1c_1c),
    toast: color!(0x18_18_1b),
    toast_text: color!(0xf5_f5_f7),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Accent,
    Danger,
}

#[must_use]
pub const fn colors(appearance: Appearance) -> &'static Colors {
    match appearance.scheme {
        Scheme::Dark => &DARK,
        Scheme::Light => &LIGHT,
    }
}

#[must_use]
pub fn theme(appearance: Appearance) -> Theme {
    let colors = colors(appearance);
    let base = match appearance.scheme {
        Scheme::Dark => Palette::DARK,
        Scheme::Light => Palette::LIGHT,
    };
    Theme::custom(
        THEME_NAME,
        Palette {
            background: colors.background,
            text: colors.text,
            primary: appearance.accent.unwrap_or(BRAND),
            ..base
        },
    )
}

#[must_use]
pub fn accent(theme: &Theme) -> Color {
    theme.palette().primary
}

#[must_use]
pub fn sidebar(colors: &Colors) -> container::Style {
    filled(colors.sidebar)
}

#[must_use]
pub fn panel(colors: &Colors) -> container::Style {
    filled(colors.panel)
}

#[must_use]
pub fn rows(colors: &Colors) -> container::Style {
    container::Style {
        background: Some(Background::Color(colors.rows)),
        border: outline(colors.border, SECTION_RADIUS),
        ..container::Style::default()
    }
}

#[must_use]
pub fn card(colors: &Colors, theme: &Theme, active: bool) -> container::Style {
    let edge = if active { accent(theme) } else { colors.border };
    container::Style {
        background: Some(Background::Color(colors.rows)),
        border: outline(edge, SECTION_RADIUS),
        ..container::Style::default()
    }
}

#[must_use]
pub fn thumb(colors: &Colors) -> container::Style {
    container::Style {
        background: Some(Background::Color(colors.thumb)),
        border: Border::default().rounded(THUMB_RADIUS),
        ..container::Style::default()
    }
}

#[must_use]
pub fn badge(colors: &Colors, theme: &Theme, tone: Tone) -> container::Style {
    let (text, background) = match tone {
        Tone::Accent => (accent(theme), tinted(accent(theme), ACCENT_TINT)),
        Tone::Danger => (colors.danger, tinted(colors.danger, DANGER_TINT)),
        Tone::Plain => (colors.text_tertiary, colors.row_border),
    };
    container::Style {
        text_color: Some(text),
        background: Some(Background::Color(background)),
        border: Border::default().rounded(BADGE_RADIUS),
        ..container::Style::default()
    }
}

#[must_use]
pub fn toast(colors: &Colors, error: bool) -> container::Style {
    container::Style {
        text_color: Some(colors.toast_text),
        background: Some(Background::Color(if error {
            colors.danger
        } else {
            colors.toast
        })),
        border: Border::default().rounded(TOAST_RADIUS),
        ..container::Style::default()
    }
}

#[must_use]
pub fn divider(colors: &Colors) -> rule::Style {
    rule::Style {
        color: colors.row_border,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

#[must_use]
pub fn sidebar_item(colors: &Colors, active: bool, status: button::Status) -> button::Style {
    let (background, text) = if active {
        (Some(colors.sidebar_active), colors.sidebar_active_text)
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        (Some(colors.sidebar_hover), colors.text_secondary)
    } else {
        (None, colors.text_secondary)
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: text,
        border: Border::default().rounded(SIDEBAR_RADIUS),
        ..button::Style::default()
    }
}

#[must_use]
pub fn soft(colors: &Colors, theme: &Theme, tone: Tone, status: button::Status) -> button::Style {
    let accent = accent(theme);
    let (text, background, edge) = match tone {
        Tone::Plain => (colors.text, colors.background, colors.border),
        Tone::Accent => (
            accent,
            tinted(accent, ACCENT_TINT),
            tinted(accent, ACCENT_EDGE),
        ),
        Tone::Danger => (
            colors.danger,
            tinted(colors.danger, DANGER_TINT),
            tinted(colors.danger, DANGER_EDGE),
        ),
    };
    let edge = match (tone, status) {
        (Tone::Danger, button::Status::Hovered | button::Status::Pressed) => colors.danger,
        (_, button::Status::Hovered | button::Status::Pressed) => accent,
        _ => edge,
    };
    let opacity = if status == button::Status::Disabled {
        DISABLED_OPACITY
    } else {
        1.0
    };
    button::Style {
        background: Some(Background::Color(faded(background, opacity))),
        text_color: faded(text, opacity),
        border: outline(faded(edge, opacity), BUTTON_RADIUS),
        ..button::Style::default()
    }
}

#[must_use]
pub fn switch(colors: &Colors, theme: &Theme, status: toggler::Status) -> toggler::Style {
    let on = matches!(
        status,
        toggler::Status::Active { is_toggled: true }
            | toggler::Status::Hovered { is_toggled: true }
            | toggler::Status::Disabled { is_toggled: true }
    );
    toggler::Style {
        background: Background::Color(if on { accent(theme) } else { colors.switch_off }),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: Background::Color(colors.switch_knob),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: SWITCH_PADDING_RATIO,
    }
}

#[must_use]
pub fn input(colors: &Colors, theme: &Theme, status: text_input::Status) -> text_input::Style {
    let edge = if matches!(status, text_input::Status::Focused { .. }) {
        accent(theme)
    } else {
        colors.border
    };
    text_input::Style {
        background: Background::Color(colors.background),
        border: outline(edge, BUTTON_RADIUS),
        icon: colors.text_tertiary,
        placeholder: colors.text_tertiary,
        value: colors.text,
        selection: tinted(accent(theme), ACCENT_EDGE),
    }
}

fn filled(color: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}

const fn faded(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity,
        ..color
    }
}

const fn tinted(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}
