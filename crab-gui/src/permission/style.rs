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
    widget::{button, container},
};

use crate::theme::{Appearance, BRAND, Scheme, outline};

const THEME_NAME: &str = "Permission";
const DEFAULT_TOOL: Color = color!(0x52_52_5b);
const TOOL_COLORS: [(&str, Color); 8] = [
    ("Bash", color!(0xd9_77_57)),
    ("Shell", color!(0xd9_77_57)),
    ("Edit", color!(0x5b_8d_d9)),
    ("Write", color!(0x8b_7e_c7)),
    ("Read", color!(0x5a_9e_6f)),
    ("Glob", color!(0x5a_9e_ab)),
    ("Grep", color!(0x5a_9e_ab)),
    ("Agent", color!(0xc4_7a_9a)),
];
const BUTTON_RADIUS: f32 = 8.0;
const BLOCK_RADIUS: f32 = 10.0;
const PILL_RADIUS: f32 = 6.0;
const TAG_OPACITY: f32 = 0.7;

#[derive(Debug)]
pub struct Colors {
    pub card: Color,
    pub text: Color,
    pub header: Color,
    pub code_background: Color,
    pub code_border: Color,
    pub code_text: Color,
    pub deny_background: Color,
    pub deny_text: Color,
    pub deny_border: Color,
    pub deny_hover_background: Color,
    pub deny_hover_border: Color,
    pub secondary_background: Color,
    pub secondary_text: Color,
    pub secondary_border: Color,
    pub secondary_hover_background: Color,
    pub secondary_hover_border: Color,
    pub secondary_hover_text: Color,
    pub warning: Color,
}

const DARK: Colors = Colors {
    card: color!(0x18_18_1b),
    text: color!(0xf4_f4_f5),
    header: color!(0xe4_e4_e7),
    code_background: color!(0x09_09_0b),
    code_border: color!(0xff_ff_ff, 0.06),
    code_text: color!(0xa1_a1_aa),
    deny_background: color!(0xff_ff_ff, 0.05),
    deny_text: color!(0xe4_e4_e7),
    deny_border: color!(0xff_ff_ff, 0.1),
    deny_hover_background: color!(0xff_ff_ff, 0.1),
    deny_hover_border: color!(0xff_ff_ff, 0.15),
    secondary_background: color!(0xff_ff_ff, 0.03),
    secondary_text: color!(0xa1_a1_aa),
    secondary_border: color!(0xff_ff_ff, 0.06),
    secondary_hover_background: color!(0xff_ff_ff, 0.08),
    secondary_hover_border: color!(0xff_ff_ff, 0.15),
    secondary_hover_text: color!(0xe4_e4_e7),
    warning: color!(0xff_ca_5f),
};

const LIGHT: Colors = Colors {
    card: color!(0xff_ff_ff),
    text: color!(0x18_18_1b),
    header: color!(0x37_41_51),
    code_background: color!(0xf4_f4_f5),
    code_border: color!(0x00_00_00, 0.06),
    code_text: color!(0x37_41_51),
    deny_background: color!(0xff_ff_ff),
    deny_text: color!(0x52_52_5b),
    deny_border: color!(0xd1_d5_db),
    deny_hover_background: color!(0xf9_fa_fb),
    deny_hover_border: color!(0x9c_a3_af),
    secondary_background: color!(0xff_ff_ff),
    secondary_text: color!(0x71_71_7a),
    secondary_border: color!(0xe5_e7_eb),
    secondary_hover_background: color!(0xf9_fa_fb),
    secondary_hover_border: color!(0xd1_d5_db),
    secondary_hover_text: color!(0x37_41_51),
    warning: color!(0x8a_5a_00),
};

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
            background: colors.card,
            text: colors.text,
            primary: appearance.accent.unwrap_or(BRAND),
            ..base
        },
    )
}

#[must_use]
pub fn tool_color(tool: &str) -> Color {
    TOOL_COLORS
        .iter()
        .find(|(name, _)| *name == tool)
        .map_or(DEFAULT_TOOL, |(_, color)| *color)
}

#[must_use]
pub const fn faded(color: Color) -> Color {
    Color {
        a: color.a * TAG_OPACITY,
        ..color
    }
}

#[must_use]
pub fn pill(background: Color) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(background)),
        border: Border::default().rounded(PILL_RADIUS),
        ..container::Style::default()
    }
}

#[must_use]
pub fn code_block(colors: &Colors) -> container::Style {
    container::Style {
        text_color: Some(colors.code_text),
        background: Some(Background::Color(colors.code_background)),
        border: outline(colors.code_border, BLOCK_RADIUS),
        ..container::Style::default()
    }
}

#[must_use]
pub fn allow(theme: &Theme, status: button::Status) -> button::Style {
    let primary = theme.extended_palette().primary;
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => primary.strong,
        button::Status::Active | button::Status::Disabled => primary.base,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border::default().rounded(BUTTON_RADIUS),
        ..button::Style::default()
    }
}

#[must_use]
pub fn deny(colors: &Colors, status: button::Status) -> button::Style {
    let (background, border) = if hovered(status) {
        (colors.deny_hover_background, colors.deny_hover_border)
    } else {
        (colors.deny_background, colors.deny_border)
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: colors.deny_text,
        border: outline(border, BUTTON_RADIUS),
        ..button::Style::default()
    }
}

#[must_use]
pub fn secondary(colors: &Colors, text: Color, status: button::Status) -> button::Style {
    let (background, border, text) = if hovered(status) {
        (
            colors.secondary_hover_background,
            colors.secondary_hover_border,
            colors.secondary_hover_text,
        )
    } else {
        (colors.secondary_background, colors.secondary_border, text)
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: text,
        border: outline(border, BUTTON_RADIUS),
        ..button::Style::default()
    }
}

#[must_use]
pub fn plain(colors: &Colors, status: button::Status) -> button::Style {
    button::Style {
        background: hovered(status).then_some(Background::Color(colors.secondary_hover_background)),
        text_color: colors.code_text,
        border: Border::default().rounded(BUTTON_RADIUS),
        ..button::Style::default()
    }
}

const fn hovered(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}
