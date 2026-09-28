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

use std::env;

use hyprland::{
    data::Monitors,
    dispatch::{Dispatch, DispatchType, WindowIdentifier},
    error::HyprError,
    shared::HyprData,
};
use snafu::{ResultExt, Snafu};

const HYPRLAND_INSTANCE: &str = "HYPRLAND_INSTANCE_SIGNATURE";

#[derive(Debug, Snafu)]
pub enum DesktopError {
    #[snafu(display("Hyprland could not focus the window of process {pid}"))]
    Focus { source: HyprError, pid: u32 },
    #[snafu(display("Hyprland could not list the monitors"))]
    Outputs { source: HyprError },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
}

pub trait Desktop {
    fn focus(&self, pid: u32) -> Result<(), DesktopError>;
    fn outputs(&self) -> Result<Vec<Output>, DesktopError>;
}

struct Hyprland;

impl Desktop for Hyprland {
    fn focus(&self, pid: u32) -> Result<(), DesktopError> {
        Dispatch::call(DispatchType::FocusWindow(WindowIdentifier::ProcessId(pid)))
            .context(FocusSnafu { pid })
    }

    fn outputs(&self) -> Result<Vec<Output>, DesktopError> {
        Ok(Monitors::get()
            .context(OutputsSnafu)?
            .into_iter()
            .map(|monitor| Output {
                name: monitor.name,
                description: monitor.description,
                width: u32::from(monitor.width),
                height: u32::from(monitor.height),
            })
            .collect())
    }
}

#[must_use]
pub fn detect() -> Option<Box<dyn Desktop>> {
    env::var_os(HYPRLAND_INSTANCE).map(|_| Box::new(Hyprland) as Box<dyn Desktop>)
}
