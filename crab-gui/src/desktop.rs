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

use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum DesktopError {
    #[cfg(feature = "hyprland")]
    #[snafu(display("Hyprland could not focus the window of process {pid}"))]
    Hyprland {
        source: hyprland::error::HyprError,
        pid: u32,
    },
    #[cfg(feature = "x11")]
    #[snafu(context(false))]
    X11 {
        source: crate::backend::x11::X11Error,
    },
}

pub trait Desktop {
    fn focus(&self, pid: u32) -> Result<(), DesktopError>;
}

#[must_use]
pub fn detect() -> Option<Box<dyn Desktop>> {
    use crab_common::desktop;

    match desktop::detect()? {
        #[cfg(feature = "hyprland")]
        desktop::Session::Wayland(Some(desktop::Compositor::Hyprland)) => {
            Some(Box::new(crate::backend::hyprland::Hyprland))
        }
        #[cfg(feature = "x11")]
        desktop::Session::X11 => Some(Box::new(crate::backend::x11::X11)),
        _ => None,
    }
}
