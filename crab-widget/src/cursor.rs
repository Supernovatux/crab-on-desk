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

use crab_common::desktop;
use snafu::Snafu;

pub type Position = (i32, i32);

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum CursorError {
    #[snafu(display("No cursor source for this desktop"))]
    Unsupported,
    #[cfg(feature = "hyprland")]
    #[snafu(display("Hyprland did not report the cursor"))]
    Hyprland { source: hyprland::error::HyprError },
    #[cfg(feature = "kde")]
    #[snafu(context(false))]
    KWin {
        source: crate::backend::kwin::KWinError,
    },
    #[cfg(feature = "x11")]
    #[snafu(context(false))]
    X11 {
        source: crate::backend::x11::X11Error,
    },
}

pub trait CursorSource {
    fn position(&mut self) -> Result<Option<Position>, CursorError>;
}

pub fn open() -> Result<Box<dyn CursorSource>, CursorError> {
    match desktop::detect() {
        #[cfg(feature = "hyprland")]
        Some(desktop::Session::Wayland(Some(desktop::Compositor::Hyprland))) => {
            Ok(Box::new(crate::backend::hyprland::Cursor))
        }
        #[cfg(feature = "kde")]
        Some(desktop::Session::Wayland(Some(desktop::Compositor::KWin))) => {
            Ok(Box::new(crate::backend::kwin::Cursor::open()?))
        }
        #[cfg(feature = "x11")]
        Some(desktop::Session::X11) => Ok(Box::new(crate::backend::x11::Cursor::open()?)),
        _ => UnsupportedSnafu.fail(),
    }
}
