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

use image::{ImageError, RgbaImage, imageops};
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu, ensure};

use crate::source::{Package, SourceError};

pub const CELL_WIDTH: u32 = 192;
pub const CELL_HEIGHT: u32 = 208;
const ATLAS_WIDTH: u32 = 1536;
const DEFAULT_SPRITESHEET: &str = "spritesheet.webp";
const DEFAULT_SPRITE_VERSION: u32 = 1;
const THEME_PREFIX: &str = "codex-pet-";

#[derive(Debug, Snafu)]
pub enum PetError {
    #[snafu(context(false))]
    Source { source: SourceError },
    #[snafu(display("Invalid pet.json"))]
    Manifest { source: serde_json::Error },
    #[snafu(display("pet.json spriteVersionNumber must be 1 or 2, not {version}"))]
    Version { version: u32 },
    #[snafu(display("pet id {id:?} has no ASCII letters or digits to name the theme after"))]
    Id { id: String },
    #[snafu(display("Unable to decode the spritesheet"))]
    Decode { source: ImageError },
    #[snafu(display(
        "The spritesheet is {width}x{height}, expected {expected_width}x{expected_height}"
    ))]
    Size {
        width: u32,
        height: u32,
        expected_width: u32,
        expected_height: u32,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    id: String,
    display_name: Option<String>,
    spritesheet_path: Option<String>,
    sprite_version_number: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Idle,
    Waving,
    Jumping,
    Failed,
    Waiting,
    Running,
    Review,
}

impl Row {
    const fn index(self) -> u32 {
        match self {
            Self::Idle => 0,
            Self::Waving => 3,
            Self::Jumping => 4,
            Self::Failed => 5,
            Self::Waiting => 6,
            Self::Running => 7,
            Self::Review => 8,
        }
    }

    pub const fn durations_ms(self) -> &'static [u32] {
        match self {
            Self::Idle => &[280, 110, 110, 140, 140, 320],
            Self::Waving => &[140, 140, 140, 280],
            Self::Jumping => &[140, 140, 140, 140, 280],
            Self::Failed => &[140, 140, 140, 140, 140, 140, 140, 240],
            Self::Waiting => &[150, 150, 150, 150, 150, 260],
            Self::Running => &[120, 120, 120, 120, 120, 220],
            Self::Review => &[150, 150, 150, 150, 150, 280],
        }
    }
}

pub struct Pet {
    pub theme: String,
    pub display_name: String,
    sheet: RgbaImage,
}

impl Pet {
    pub fn load(mut package: Package) -> Result<Self, PetError> {
        let manifest: Manifest =
            serde_json::from_slice(&package.manifest).context(ManifestSnafu)?;
        let version = manifest
            .sprite_version_number
            .unwrap_or(DEFAULT_SPRITE_VERSION);
        let rows = match version {
            1 => 9,
            2 => 11,
            _ => return VersionSnafu { version }.fail(),
        };
        let spritesheet = package.spritesheet(
            manifest
                .spritesheet_path
                .as_deref()
                .unwrap_or(DEFAULT_SPRITESHEET),
        )?;
        let sheet = image::load_from_memory(&spritesheet)
            .context(DecodeSnafu)?
            .into_rgba8();
        let (width, height) = sheet.dimensions();
        let expected_height = rows * CELL_HEIGHT;
        ensure!(
            width == ATLAS_WIDTH && height == expected_height,
            SizeSnafu {
                width,
                height,
                expected_width: ATLAS_WIDTH,
                expected_height,
            }
        );
        let theme = theme_name(&manifest.id).context(IdSnafu { id: &manifest.id })?;
        Ok(Self {
            theme,
            display_name: manifest.display_name.unwrap_or(manifest.id),
            sheet,
        })
    }

    pub fn frames(&self, row: Row) -> Vec<RgbaImage> {
        (0..row.durations_ms().len() as u32)
            .map(|column| {
                imageops::crop_imm(
                    &self.sheet,
                    column * CELL_WIDTH,
                    row.index() * CELL_HEIGHT,
                    CELL_WIDTH,
                    CELL_HEIGHT,
                )
                .to_image()
            })
            .collect()
    }
}

fn theme_name(id: &str) -> Option<String> {
    let slug = id
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    (!slug.is_empty()).then(|| format!("{THEME_PREFIX}{slug}"))
}
