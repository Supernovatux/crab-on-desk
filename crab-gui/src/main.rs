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

use crab_common::gui::{PERMISSION_MODE, SETTINGS_MODE};
use snafu::Snafu;

mod desktop;
mod permission;
mod settings;
mod theme;

#[derive(Debug, Snafu)]
enum GuiError {
    #[snafu(context(false))]
    Permission { source: permission::PermissionError },
    #[snafu(context(false))]
    Settings { source: settings::SettingsError },
    #[snafu(display("Usage: crab-gui [{SETTINGS_MODE} | {PERMISSION_MODE}]"))]
    Usage,
}

#[snafu::report]
fn main() -> Result<(), GuiError> {
    match env::args().nth(1).as_deref() {
        None | Some(SETTINGS_MODE) => Ok(settings::run()?),
        Some(PERMISSION_MODE) => Ok(permission::run()?),
        Some(_) => UsageSnafu.fail(),
    }
}
