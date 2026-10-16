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

use crab_common::gnome::{self, GnomeError};

use crate::{
    desktop::{Desktop, DesktopError},
    outputs::Output,
};

pub struct Gnome;

impl Desktop for Gnome {
    fn focus(&self, pid: u32) -> Result<(), DesktopError> {
        Ok(gnome::reply(
            gnome::shell()?.activate(pid),
            "focusing the terminal",
        )?)
    }
}

pub fn outputs() -> Result<Vec<Output>, GnomeError> {
    let monitors = gnome::reply(gnome::shell()?.monitors(), "listing the monitors")?;
    Ok(monitors
        .into_iter()
        .map(|(name, (_, _, width, height))| Output {
            description: name.clone(),
            name,
            width: width as u32,
            height: height as u32,
        })
        .collect())
}
