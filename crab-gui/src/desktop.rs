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

use crab_common::desktop::{self, Compositor};
use hyprland::{
    dispatch::{Dispatch, DispatchType, WindowIdentifier},
    error::HyprError,
};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum DesktopError {
    #[snafu(display("Hyprland could not focus the window of process {pid}"))]
    Focus { source: HyprError, pid: u32 },
}

pub trait Desktop {
    fn focus(&self, pid: u32) -> Result<(), DesktopError>;
}

struct Hyprland;

impl Desktop for Hyprland {
    fn focus(&self, pid: u32) -> Result<(), DesktopError> {
        Dispatch::call(DispatchType::FocusWindow(WindowIdentifier::ProcessId(pid)))
            .context(FocusSnafu { pid })
    }
}

#[must_use]
pub fn detect() -> Option<Box<dyn Desktop>> {
    match desktop::detect()? {
        Compositor::Hyprland => Some(Box::new(Hyprland)),
        Compositor::KWin => None,
    }
}
