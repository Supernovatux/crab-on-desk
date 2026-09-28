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
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use crab_common::{
    atlas::Animations,
    dirs::{
        CommonError, get_animation_source, get_theme_cache, get_theme_manifest, get_theme_source,
        get_theme_sources,
    },
};
use iced::widget::image::Handle;
use image::ImageReader;
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum CatalogError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to list {path:?}"))]
    List { source: io::Error, path: PathBuf },
    #[snafu(display("Unable to delete {path:?}"))]
    Delete { source: io::Error, path: PathBuf },
}

#[derive(Debug, Clone)]
pub struct ThemeEntry {
    pub name: String,
    pub generated: bool,
    pub thumbnail: Option<Handle>,
}

pub fn list() -> Result<Vec<ThemeEntry>, CatalogError> {
    let dir = get_theme_sources()?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context(ListSnafu { path: dir }),
    };
    let mut themes = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .map(entry)
        .collect::<Result<Vec<_>, _>>()?;
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(themes)
}

pub fn delete(theme: &str) -> Result<(), CatalogError> {
    remove(&get_theme_source(theme)?)?;
    remove(&get_theme_cache(theme)?)
}

fn entry(name: String) -> Result<ThemeEntry, CatalogError> {
    Ok(ThemeEntry {
        generated: get_theme_manifest(&name)?.is_file(),
        thumbnail: thumbnail(&get_animation_source(&name, Animations::default())?),
        name,
    })
}

fn thumbnail(path: &Path) -> Option<Handle> {
    let image = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .into_rgba8();
    Some(Handle::from_rgba(
        image.width(),
        image.height(),
        image.into_raw(),
    ))
}

fn remove(path: &Path) -> Result<(), CatalogError> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            Err(error).context(DeleteSnafu { path })
        }
        _ => Ok(()),
    }
}
