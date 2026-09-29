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

use std::{
    env,
    path::{Path, PathBuf},
};

use crab_common::{
    gui::{GENERATE_COMMAND, INSTALL_HOOKS_COMMAND, UNINSTALL_HOOKS_COMMAND},
    hooks::{self, HooksError},
};
use crab_settings::{
    atlas::{AtlasError, gen_atlas},
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
    Hooks { source: HooksError },

    #[snafu(display(
        "Usage: crab-settings {GENERATE_COMMAND} <apng-dir> <out-dir> [theme...] | {INSTALL_HOOKS_COMMAND} | {UNINSTALL_HOOKS_COMMAND}"
    ))]
    Usage,
}

#[snafu::report]
fn main() -> Result<(), MainError> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some(GENERATE_COMMAND) => {
            let sources = PathBuf::from(args.next().context(UsageSnafu)?);
            let out = PathBuf::from(args.next().context(UsageSnafu)?);
            generate(&sources, &out, &args.collect::<Vec<_>>())
        }
        Some(INSTALL_HOOKS_COMMAND) => Ok(hooks::install()?),
        Some(UNINSTALL_HOOKS_COMMAND) => Ok(hooks::uninstall()?),
        _ => Err(MainError::Usage),
    }
}

fn generate(sources: &Path, out: &Path, selected: &[String]) -> Result<(), MainError> {
    let themes = Themes::new(sources)?;
    let mut names: Vec<&String> = themes
        .themes
        .keys()
        .filter(|theme| selected.is_empty() || selected.contains(theme))
        .collect();
    names.sort();
    for theme in names {
        println!("generating {theme}");
        gen_atlas(theme, &themes, sources, out)?;
    }
    Ok(())
}
