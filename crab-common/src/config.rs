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

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, Snafu};

use crate::{
    dirs::{CommonError, get_config_file, get_theme_dir},
    toml_file::{self, TomlFileError},
};

pub const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Snafu)]
pub enum ConfigError {
    #[snafu(display("Unable to locate the config"))]
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("No valid config"))]
    #[snafu(context(false))]
    Toml { source: TomlFileError },
    #[snafu(display("The configured theme {theme:?} is not installed"))]
    ThemeMissing { theme: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_theme: String,
    #[serde(default)]
    pub free_roam: bool,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config: Self = toml_file::read(get_config_file()?)?;
        config.theme_dir()?;
        Ok(config)
    }

    pub fn theme_dir(&self) -> Result<PathBuf, ConfigError> {
        get_theme_dir(&self.default_theme)?.context(ThemeMissingSnafu {
            theme: &self.default_theme,
        })
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        Ok(toml_file::write(get_config_file()?, self)?)
    }
}
