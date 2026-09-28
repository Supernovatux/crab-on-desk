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

use crab_common::{
    config::Config,
    dirs::{CommonError, get_config_file},
    toml_file::{self, TomlFileError},
};
use snafu::Snafu;

pub const DEFAULT_THEME: &str = "clawd";

#[derive(Debug, Snafu)]
pub enum ConfigError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(context(false))]
    Toml { source: TomlFileError },
}

pub fn write_default_if_missing() -> Result<(), ConfigError> {
    let path = get_config_file()?;
    if path.exists() {
        return Ok(());
    }
    let config = Config {
        default_theme: DEFAULT_THEME.to_owned(),
        free_roam: true,
    };
    Ok(toml_file::write(path, &config)?)
}
