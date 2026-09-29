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
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
};

use crab_common::atlas::{Animations, SOURCE_EXTENSION};
use snafu::{OptionExt, ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum ThemeError {
    #[snafu(display("Unable to list {path:?}"))]
    List { source: io::Error, path: PathBuf },
    #[snafu(display("{path:?} is not a UTF-8 file name"))]
    FileName { path: PathBuf },
    #[snafu(display("{path:?} is not named after an animation"))]
    AnimName {
        source: serde_plain::Error,
        path: PathBuf,
    },
}

#[derive(Debug)]
pub struct Themes {
    pub themes: HashMap<String, BTreeMap<Animations, PathBuf>>,
}

impl Themes {
    pub fn new(sources: &Path) -> Result<Self, ThemeError> {
        let mut themes = HashMap::new();
        for dir in list(sources)? {
            if dir.is_dir() {
                themes.insert(utf8_name(dir.file_name(), &dir)?, animations(&dir)?);
            }
        }
        Ok(Self { themes })
    }
}

fn animations(dir: &Path) -> Result<BTreeMap<Animations, PathBuf>, ThemeError> {
    let mut animations = BTreeMap::new();
    for path in list(dir)? {
        if path.extension().is_some_and(|ext| ext == SOURCE_EXTENSION) {
            let stem = utf8_name(path.file_stem(), &path)?;
            let animation = stem.parse().context(AnimNameSnafu { path: path.clone() })?;
            animations.insert(animation, path);
        }
    }
    Ok(animations)
}

fn list(dir: &Path) -> Result<Vec<PathBuf>, ThemeError> {
    fs::read_dir(dir)
        .and_then(|entries| entries.map(|entry| entry.map(|e| e.path())).collect())
        .context(ListSnafu { path: dir })
}

fn utf8_name(name: Option<&std::ffi::OsStr>, path: &Path) -> Result<String, ThemeError> {
    name.and_then(|name| name.to_str())
        .map(str::to_owned)
        .context(FileNameSnafu { path })
}
