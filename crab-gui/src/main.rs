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

use std::path::PathBuf;

use crab_common::{
    config::Config,
    dirs::{CommonError, get_config_file},
    gui::{DISPLAYS_PAGE, INIT_MODE, PERMISSION_MODE, SETTINGS_MODE},
};
use settings::Page;
use snafu::Snafu;

mod desktop;
mod outputs;
mod permission;
mod settings;
mod theme;

#[derive(Debug, Snafu)]
enum GuiError {
    #[snafu(context(false))]
    Permission { source: permission::PermissionError },
    #[snafu(context(false))]
    Settings { source: settings::SettingsError },
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display(
        "A valid config already exists at {path:?}; use `crab-gui {SETTINGS_MODE}` to change it"
    ))]
    Configured { path: PathBuf },
    #[snafu(display(
        "Usage: crab-gui [{SETTINGS_MODE} [{DISPLAYS_PAGE}] | {INIT_MODE} | {PERMISSION_MODE}]"
    ))]
    Usage,
}

#[snafu::report]
fn main() -> Result<(), GuiError> {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | [SETTINGS_MODE] => Ok(settings::run(Config::load().ok(), Page::General)?),
        [SETTINGS_MODE, DISPLAYS_PAGE] => Ok(settings::run(Config::load().ok(), Page::Displays)?),
        [INIT_MODE] => {
            if Config::load().is_ok() {
                return ConfiguredSnafu {
                    path: get_config_file()?,
                }
                .fail();
            }
            Ok(settings::run(None, Page::General)?)
        }
        [PERMISSION_MODE] => Ok(permission::run()?),
        _ => UsageSnafu.fail(),
    }
}
