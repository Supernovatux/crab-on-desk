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
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use crab_common::{
    atlas::{
        Animations, MANIFEST_FILE, SleepMode, SleepTimings, THUMBNAIL_FILE, ThemeBehaviour,
        ThemeManifest, Tier, premultiply_alpha,
    },
    dirs::{CommonError, get_animation_texture, get_user_themes},
};
use image::{ImageError, RgbaImage, imageops};
use snafu::{ResultExt, Snafu};

use crab_settings::atlas::{AtlasError, encode_frames};

use crate::pet::{CELL_HEIGHT, CELL_WIDTH, Pet, Row};

const CANVAS: u32 = 200;
const VISIBLE_HEIGHT_RATIO: f64 = 0.58;
const BASELINE_BOTTOM_RATIO: f64 = 0.05;
const CENTER_X_RATIO: f64 = 0.5;
const STAGING_SUFFIX: &str = ".partial";

#[derive(Debug, Snafu)]
pub enum ThemeError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to write {path:?}"))]
    Write { source: io::Error, path: PathBuf },
    #[snafu(display("Unable to encode the manifest"))]
    Encode { source: toml::ser::Error },
    #[snafu(display("Unable to encode {animation:?}"))]
    Texture {
        source: AtlasError,
        animation: Animations,
    },
    #[snafu(display("Unable to write the thumbnail {path:?}"))]
    Thumbnail { source: ImageError, path: PathBuf },
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Loop,
    Once,
    Hold,
}

const DRAG: &[Row] = &[
    Row::RunningRight,
    Row::RunningRight,
    Row::RunningLeft,
    Row::RunningLeft,
];

const CLIPS: [(Animations, &[Row], Mode); 13] = [
    (Animations::Idle, &[Row::Idle], Mode::Loop),
    (Animations::Thinking, &[Row::Review], Mode::Loop),
    (Animations::Working, &[Row::Running], Mode::Loop),
    (Animations::Sweeping, &[Row::Running], Mode::Loop),
    (Animations::Carrying, &[Row::Running], Mode::Loop),
    (Animations::Notification, &[Row::Waiting], Mode::Loop),
    (Animations::Attention, &[Row::Jumping], Mode::Loop),
    (Animations::Error, &[Row::Failed], Mode::Loop),
    (Animations::Sleeping, &[Row::Idle], Mode::Hold),
    (Animations::ReactDrag, DRAG, Mode::Loop),
    (Animations::ReactLeft, &[Row::Jumping], Mode::Once),
    (Animations::ReactRight, &[Row::Jumping], Mode::Once),
    (Animations::ReactDouble1, &[Row::Waving], Mode::Once),
];

pub fn install(pet: &Pet) -> Result<PathBuf, ThemeError> {
    let root = get_user_themes()?;
    let target = root.join(&pet.theme);
    let staging = root.join(format!("{}{STAGING_SUFFIX}", pet.theme));
    remove(&staging)?;
    fs::create_dir_all(&staging).context(WriteSnafu { path: &staging })?;
    write_theme(pet, &staging)?;
    remove(&target)?;
    fs::rename(&staging, &target).context(WriteSnafu { path: &target })?;
    Ok(target)
}

fn write_theme(pet: &Pet, dir: &Path) -> Result<(), ThemeError> {
    let mut animations = BTreeMap::new();
    for (animation, rows, mode) in CLIPS {
        let (layers, delays) = clip(pet, rows, mode);
        let path = get_animation_texture(dir, animation)?;
        let info = encode_frames(
            CANVAS,
            CANVAS,
            &layers,
            delays,
            matches!(mode, Mode::Loop),
            &path,
        )
        .context(TextureSnafu { animation })?;
        animations.insert(animation, info);
    }
    let manifest = ThemeManifest {
        behaviour: behaviour(),
        animations,
    };
    let path = dir.join(MANIFEST_FILE);
    fs::write(
        &path,
        toml::to_string_pretty(&manifest).context(EncodeSnafu)?,
    )
    .context(WriteSnafu { path })?;
    let path = dir.join(THUMBNAIL_FILE);
    if let Some(cell) = pet.frames(Row::Idle).first() {
        cell.save(&path).context(ThumbnailSnafu { path })?;
    }
    Ok(())
}

fn clip(pet: &Pet, rows: &[Row], mode: Mode) -> (Vec<Vec<u8>>, Vec<u32>) {
    let frames = rows.iter().flat_map(|&row| {
        pet.frames(row)
            .into_iter()
            .zip(row.durations_ms().iter().copied())
    });
    let (frames, delays): (Vec<RgbaImage>, Vec<u32>) = match mode {
        Mode::Hold => frames.take(1).unzip(),
        Mode::Loop | Mode::Once => frames.unzip(),
    };
    (frames.into_iter().map(place).collect(), delays)
}

fn place(cell: RgbaImage) -> Vec<u8> {
    let unit = VISIBLE_HEIGHT_RATIO * f64::from(CANVAS) / f64::from(CELL_HEIGHT);
    let width = (f64::from(CELL_WIDTH) * unit).round() as u32;
    let height = (f64::from(CELL_HEIGHT) * unit).round() as u32;
    let left = (f64::from(CELL_WIDTH) / 2.0).mul_add(-unit, CENTER_X_RATIO * f64::from(CANVAS));
    let top = f64::from(CANVAS).mul_add(1.0 - BASELINE_BOTTOM_RATIO, -f64::from(height));
    let mut premultiplied = cell;
    premultiply_alpha(&mut premultiplied);
    let scaled = imageops::resize(
        &premultiplied,
        width,
        height,
        imageops::FilterType::CatmullRom,
    );
    let mut canvas = RgbaImage::new(CANVAS, CANVAS);
    imageops::replace(
        &mut canvas,
        &scaled,
        left.round() as i64,
        top.round() as i64,
    );
    canvas.into_raw()
}

fn behaviour() -> ThemeBehaviour {
    ThemeBehaviour {
        react_double: vec![Animations::ReactDouble1],
        working_tiers: vec![Tier {
            min_sessions: 1,
            animation: Animations::Working,
        }],
        juggling_tiers: vec![Tier {
            min_sessions: 1,
            animation: Animations::Working,
        }],
        min_display_ms: BTreeMap::from([
            (Animations::Attention, 4000),
            (Animations::Error, 5000),
            (Animations::Sweeping, 5500),
            (Animations::Notification, 2500),
            (Animations::Carrying, 3000),
            (Animations::Working, 1000),
            (Animations::Thinking, 1000),
        ]),
        auto_return_ms: BTreeMap::from([
            (Animations::Attention, 4000),
            (Animations::Error, 5000),
            (Animations::Sweeping, 300_000),
            (Animations::Notification, 2500),
            (Animations::Carrying, 3000),
        ]),
        reaction_ms: BTreeMap::from([
            (Animations::ReactLeft, 840),
            (Animations::ReactRight, 840),
            (Animations::ReactDouble1, 700),
        ]),
        sleep: SleepTimings {
            mode: SleepMode::Direct,
            idle_after_ms: 20_000,
            yawn_after_ms: 60_000,
            deep_sleep_after_ms: 600_000,
            yawn_ms: 3000,
            collapse_ms: None,
            wake_ms: 1500,
        },
        ..ThemeBehaviour::default()
    }
}

fn remove(path: &Path) -> Result<(), ThemeError> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            Err(error).context(WriteSnafu { path })
        }
        _ => Ok(()),
    }
}
