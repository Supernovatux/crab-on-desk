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

use std::{collections::BTreeMap, fs::File, io, path::PathBuf, time::Duration};

use crab_common::{
    atlas::{Animations, MANIFEST_FILE, Rect, ThemeBehaviour, ThemeManifest},
    dirs::{CommonError, get_animation_texture},
    toml_file::{self, TomlFileError},
};
use ktx2::{Format, Reader};
use memmap2::Mmap;
use snafu::{OptionExt, ResultExt, Snafu, ensure};

const BC7_BLOCK_BYTES: u64 = 16;

#[derive(Debug, Snafu)]
pub enum ThemeError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(context(false))]
    Toml { source: TomlFileError },
    #[snafu(display("Theme has no {animation:?} animation"))]
    Missing { animation: Animations },
    #[snafu(display("Unable to read {path:?}"))]
    Read { source: io::Error, path: PathBuf },
    #[snafu(display("Invalid ktx2 {path:?}"))]
    Ktx2 {
        source: ktx2::ParseError,
        path: PathBuf,
    },
    #[snafu(display("{path:?} is not layered raw BC7 matching the manifest"))]
    Layout { path: PathBuf },
}

pub struct Theme {
    dir: PathBuf,
    manifest: ThemeManifest,
}

pub struct Animation {
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub frame_delays: Vec<Duration>,
    pub loops: bool,
    pub hitbox: Rect,
    texture: Reader<Mmap>,
}

impl Animation {
    #[must_use]
    pub fn blocks(&self) -> &[u8] {
        self.texture.levels().next().map_or(&[], |level| level.data)
    }
}

impl Theme {
    pub fn load(dir: PathBuf) -> Result<Self, ThemeError> {
        let manifest = toml_file::read(dir.join(MANIFEST_FILE))?;
        Ok(Self { dir, manifest })
    }

    #[must_use]
    pub const fn behaviour(&self) -> &ThemeBehaviour {
        &self.manifest.behaviour
    }

    #[must_use]
    pub fn clip_lengths(&self) -> BTreeMap<Animations, Duration> {
        self.manifest
            .animations
            .iter()
            .map(|(animation, info)| {
                let total: u64 = info.frame_delays_ms.iter().copied().map(u64::from).sum();
                (*animation, Duration::from_millis(total))
            })
            .collect()
    }

    #[must_use]
    pub fn contains(&self, animation: Animations) -> bool {
        self.manifest.animations.contains_key(&animation)
    }

    pub fn animation(&self, animation: Animations) -> Result<Animation, ThemeError> {
        let info = self
            .manifest
            .animations
            .get(&animation)
            .context(MissingSnafu { animation })?;
        let path = get_animation_texture(&self.dir, animation)?;
        let mapping = File::open(&path)
            .and_then(|file| unsafe { Mmap::map(&file) })
            .context(ReadSnafu { path: path.clone() })?;
        let reader = Reader::new(mapping).context(Ktx2Snafu { path: path.clone() })?;
        let header = reader.header();
        let level = reader
            .levels()
            .next()
            .context(LayoutSnafu { path: path.clone() })?;
        let frame_bytes = u64::from(info.width.div_ceil(4))
            * u64::from(info.height.div_ceil(4))
            * BC7_BLOCK_BYTES;
        ensure!(
            info.width > 0
                && info.height > 0
                && within(info.hitbox.x, info.hitbox.width, info.width)
                && within(info.hitbox.y, info.hitbox.height, info.height)
                && header.format == Some(Format::BC7_SRGB_BLOCK)
                && header.supercompression_scheme.is_none()
                && header.pixel_width == info.width
                && header.pixel_height == info.height
                && header.layer_count.max(1) == info.frame_count
                && header.face_count == 1
                && header.level_count == 1
                && level.data.len() as u64 == frame_bytes * u64::from(info.frame_count)
                && info.frame_delays_ms.len() == info.frame_count as usize
                && info.frame_delays_ms.iter().all(|delay| *delay > 0),
            LayoutSnafu { path }
        );
        Ok(Animation {
            width: info.width,
            height: info.height,
            frame_count: info.frame_count,
            frame_delays: info
                .frame_delays_ms
                .iter()
                .map(|delay| Duration::from_millis(u64::from(*delay)))
                .collect(),
            loops: info.loops,
            hitbox: info.hitbox,
            texture: reader,
        })
    }
}

fn within(start: u32, length: u32, limit: u32) -> bool {
    start.checked_add(length).is_some_and(|end| end <= limit)
}
