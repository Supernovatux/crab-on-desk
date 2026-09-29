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

use std::path::Path;

use crab_common::{
    atlas::THUMBNAIL_FILE,
    dirs::{CommonError, get_theme_dirs},
};
use iced::widget::image::Handle;
use image::ImageReader;

#[derive(Debug, Clone)]
pub struct ThemeEntry {
    pub name: String,
    pub thumbnail: Option<Handle>,
}

pub fn list() -> Result<Vec<ThemeEntry>, CommonError> {
    Ok(get_theme_dirs()?
        .into_iter()
        .map(|(name, dir)| ThemeEntry {
            thumbnail: thumbnail(&dir.join(THUMBNAIL_FILE)),
            name,
        })
        .collect())
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
