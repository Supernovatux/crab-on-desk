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

use std::time::Duration;

use iced::{Border, Color, Font, color, font::Weight};
use mundy::{Interest, Preferences, Srgba};

const PREFERENCES_TIMEOUT: Duration = Duration::from_millis(500);
const BORDER_WIDTH: f32 = 1.0;

pub const BRAND: Color = color!(0xd9_77_57);
pub const MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..Font::DEFAULT
};
pub const SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..Font::DEFAULT
};
pub const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    pub scheme: Scheme,
    pub accent: Option<Color>,
}

#[must_use]
pub fn system() -> Appearance {
    let preferences = Preferences::once_blocking(
        Interest::ColorScheme | Interest::AccentColor,
        PREFERENCES_TIMEOUT,
    )
    .unwrap_or_default();
    Appearance {
        scheme: if preferences.color_scheme.is_light() {
            Scheme::Light
        } else {
            Scheme::Dark
        },
        accent: preferences.accent_color.0.map(color),
    }
}

const fn color(accent: Srgba) -> Color {
    Color::from_rgba(
        accent.red as f32,
        accent.green as f32,
        accent.blue as f32,
        accent.alpha as f32,
    )
}

#[must_use]
pub fn outline(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: BORDER_WIDTH,
        radius: radius.into(),
    }
}
