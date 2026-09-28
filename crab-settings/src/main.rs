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

use std::{env, path::PathBuf};

use crab_common::{
    gui::{GENERATE_COMMAND, IMPORT_COMMAND, INSTALL_HOOKS_COMMAND, UNINSTALL_HOOKS_COMMAND},
    hooks::{self, HooksError},
};
use crab_settings::{
    atlas::{AtlasError, gen_atlas},
    config::{ConfigError, write_default_if_missing},
    import::{ImportError, import},
    themes::{ThemeError, Themes},
};
use snafu::{self, OptionExt, Snafu};

#[derive(Debug, Snafu)]
enum MainError {
    #[snafu(context(false))]
    Theme { source: ThemeError },

    #[snafu(context(false))]
    Atlas { source: AtlasError },

    #[snafu(context(false))]
    Config { source: ConfigError },

    #[snafu(context(false))]
    Hooks { source: HooksError },

    #[snafu(context(false))]
    Import { source: ImportError },

    #[snafu(display(
        "Usage: crab-settings [{GENERATE_COMMAND} [theme...] | {IMPORT_COMMAND} <reference-repo> [theme...] | {INSTALL_HOOKS_COMMAND} | {UNINSTALL_HOOKS_COMMAND}]"
    ))]
    Usage,
}

#[snafu::report]
fn main() -> Result<(), MainError> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None => generate(&[]),
        Some(GENERATE_COMMAND) => generate(&args.collect::<Vec<_>>()),
        Some(IMPORT_COMMAND) => {
            let reference = PathBuf::from(args.next().context(UsageSnafu)?);
            let themes = import(&reference, args.collect())?;
            generate(&themes)
        }
        Some(INSTALL_HOOKS_COMMAND) => Ok(hooks::install()?),
        Some(UNINSTALL_HOOKS_COMMAND) => Ok(hooks::uninstall()?),
        Some(_) => Err(MainError::Usage),
    }
}

fn generate(selected: &[String]) -> Result<(), MainError> {
    let themes = Themes::new()?;
    let mut names: Vec<&String> = themes
        .themes
        .keys()
        .filter(|theme| selected.is_empty() || selected.contains(theme))
        .collect();
    names.sort();
    for theme in names {
        println!("generating {theme}");
        gen_atlas(theme, &themes)?;
    }
    write_default_if_missing()?;
    Ok(())
}
