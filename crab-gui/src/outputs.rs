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

use crate::backend;

#[derive(Debug, Snafu)]
pub enum OutputsError {
    #[snafu(display("No screen listing for this desktop"))]
    Unsupported,
    #[cfg(feature = "wayland")]
    #[snafu(context(false))]
    Wayland {
        source: backend::wayland::WaylandError,
    },
    #[cfg(feature = "x11")]
    #[snafu(context(false))]
    X11 { source: backend::x11::X11Error },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
}

pub fn list() -> Result<Vec<Output>, OutputsError> {
    match desktop::detect() {
        #[cfg(feature = "wayland")]
        Some(desktop::Session::Wayland(_)) => Ok(backend::wayland::outputs()?),
        #[cfg(feature = "x11")]
        Some(desktop::Session::X11) => Ok(backend::x11::outputs()?),
        _ => UnsupportedSnafu.fail(),
    }
}
