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
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use crab_common::dirs::{CommonError, get_cache, get_theme_sources};
use snafu::{OptionExt, ResultExt, Snafu, ensure};

const RENDER_SCRIPT: &str = include_str!("../import/render.js");
const RENDER_PAGE: &str = include_str!("../import/page.html");
const RENDER_SCRIPT_FILE: &str = "render.js";
const RENDER_PAGE_FILE: &str = "page.html";
const IMPORTER_DIR: &str = "importer";
const ELECTRON: &str = "electron";
const REFERENCE_THEMES_DIR: &str = "themes";
const REFERENCE_MANIFEST: &str = "theme.json";
const RENDER_SIZE: u32 = 200;

#[derive(Debug, Snafu)]
pub enum ImportError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("No electron or electronNN executable found on PATH"))]
    NoElectron,
    #[snafu(display("Unable to list {path:?}"))]
    List { source: io::Error, path: PathBuf },
    #[snafu(display("No themes found in {path:?}"))]
    NoThemes { path: PathBuf },
    #[snafu(display("Unable to write the importer to {path:?}"))]
    Write { source: io::Error, path: PathBuf },
    #[snafu(display("Unable to run {program:?}"))]
    Run { source: io::Error, program: PathBuf },
    #[snafu(display("The importer failed"))]
    Failed,
}

pub fn import(reference: &Path, themes: Vec<String>) -> Result<Vec<String>, ImportError> {
    let themes = if themes.is_empty() {
        reference_themes(reference)?
    } else {
        themes
    };
    ensure!(!themes.is_empty(), NoThemesSnafu { path: reference });
    let electron = find_electron().context(NoElectronSnafu)?;
    let script = write_importer()?;
    let output = get_theme_sources()?;
    fs::create_dir_all(&output).context(WriteSnafu { path: &output })?;
    let status = Command::new(&electron)
        .arg(script)
        .arg(reference)
        .arg(output)
        .arg(RENDER_SIZE.to_string())
        .args(&themes)
        .status()
        .context(RunSnafu { program: electron })?;
    ensure!(status.success(), FailedSnafu);
    Ok(themes)
}

fn reference_themes(reference: &Path) -> Result<Vec<String>, ImportError> {
    let dir = reference.join(REFERENCE_THEMES_DIR);
    let mut themes: Vec<String> = fs::read_dir(&dir)
        .context(ListSnafu { path: &dir })?
        .flatten()
        .filter(|entry| entry.path().join(REFERENCE_MANIFEST).is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    themes.sort();
    Ok(themes)
}

fn write_importer() -> Result<PathBuf, ImportError> {
    let dir = get_cache()?.join(IMPORTER_DIR);
    fs::create_dir_all(&dir).context(WriteSnafu { path: &dir })?;
    let page = dir.join(RENDER_PAGE_FILE);
    fs::write(&page, RENDER_PAGE).context(WriteSnafu { path: page })?;
    let script = dir.join(RENDER_SCRIPT_FILE);
    fs::write(&script, RENDER_SCRIPT).context(WriteSnafu { path: &script })?;
    Ok(script)
}

fn find_electron() -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let version = name.strip_prefix(ELECTRON)?;
            let version: u32 = if version.is_empty() {
                0
            } else {
                version.parse().ok()?
            };
            Some((version, entry.path()))
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, path)| path)
}
