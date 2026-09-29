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
    fs::{self, File, create_dir_all},
    io::{self, BufReader},
    num::NonZero,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use crab_common::{
    atlas::{
        AnimationInfo, Animations, BEHAVIOUR_FILE, MANIFEST_FILE, THUMBNAIL_FILE, ThemeBehaviour,
        ThemeManifest, opaque_bounds, premultiply_alpha,
    },
    toml_file::{self, TomlFileError},
};
use image::{
    AnimationDecoder, ImageError, ImageReader, RgbaImage, codecs::png::PngDecoder,
    metadata::LoopCount,
};
use ktx2_rw::{BasisCompressionParams, Ktx2Texture, TranscodeFormat, VkFormat};
use snafu::{OptionExt, ResultExt, Snafu};

use crate::themes::Themes;

#[derive(Debug, Snafu)]
pub enum AtlasError {
    NotFound,
    #[snafu(context(false))]
    IO {
        source: io::Error,
    },
    #[snafu(display("Unable to decode {path:?}"))]
    Decode {
        source: ImageError,
        path: PathBuf,
    },
    #[snafu(display("Unable to write the thumbnail {path:?}"))]
    Thumbnail {
        source: ImageError,
        path: PathBuf,
    },
    #[snafu(display("{path:?} has no frames"))]
    Empty {
        path: PathBuf,
    },
    #[snafu(context(false))]
    Toml {
        source: toml::ser::Error,
    },
    #[snafu(context(false))]
    Behaviour {
        source: TomlFileError,
    },
    #[snafu(display("{theme} behaviour refers to {animation:?}, which has no source"))]
    Unsourced {
        theme: String,
        animation: Animations,
    },
    #[snafu(context(false))]
    AnimName {
        source: serde_plain::Error,
    },
    #[snafu(display("Ktx2 error while {thing}."))]
    Ktx2 {
        source: ktx2_rw::Error,
        thing: String,
    },
}

pub fn gen_atlas(
    theme: &str,
    themes: &Themes,
    sources: &Path,
    out: &Path,
) -> Result<(), AtlasError> {
    let theme_files = themes.themes.get(theme).ok_or(AtlasError::NotFound)?;
    let behaviour: ThemeBehaviour = toml_file::read(sources.join(theme).join(BEHAVIOUR_FILE))?;
    if let Some(animation) = behaviour
        .animations()
        .find(|animation| !theme_files.contains_key(animation))
    {
        return UnsourcedSnafu { theme, animation }.fail();
    }
    let dir = out.join(theme);
    create_dir_all(&dir)?;
    let mut animations = BTreeMap::new();
    for (anim, path) in theme_files {
        let info = encode_animation(path, &dir.join(anim.texture_file()?))?;
        animations.insert(*anim, info);
    }
    if let Some(idle) = theme_files.get(&Animations::default()) {
        write_thumbnail(idle, &dir.join(THUMBNAIL_FILE))?;
    }
    fs::write(
        dir.join(MANIFEST_FILE),
        toml::to_string_pretty(&ThemeManifest {
            behaviour,
            animations,
        })?,
    )?;
    Ok(())
}

fn write_thumbnail(source: &Path, out: &Path) -> Result<(), AtlasError> {
    ImageReader::open(source)?
        .with_guessed_format()?
        .decode()
        .context(DecodeSnafu { path: source })?
        .save(out)
        .context(ThumbnailSnafu { path: out })
}

fn encode_animation(source: &Path, out: &Path) -> Result<AnimationInfo, AtlasError> {
    let decoder = PngDecoder::new(BufReader::new(File::open(source)?))
        .and_then(PngDecoder::apng)
        .context(DecodeSnafu { path: source })?;
    let loops = matches!(decoder.loop_count(), LoopCount::Infinite);
    let (frame_delays_ms, frames): (Vec<u32>, Vec<RgbaImage>) = decoder
        .into_frames()
        .collect_frames()
        .context(DecodeSnafu { path: source })?
        .into_iter()
        .map(|frame| {
            (
                Duration::from(frame.delay()).as_millis() as u32,
                frame.into_buffer(),
            )
        })
        .unzip();
    let (width, height) = frames
        .first()
        .context(EmptySnafu { path: source })?
        .dimensions();
    let layers = frames
        .into_iter()
        .map(|frame| {
            let mut data = frame.into_raw();
            premultiply_alpha(&mut data);
            data
        })
        .collect::<Vec<_>>();
    encode_frames(width, height, &layers, frame_delays_ms, loops, out)
}

pub fn encode_frames(
    width: u32,
    height: u32,
    premultiplied: &[Vec<u8>],
    frame_delays_ms: Vec<u32>,
    loops: bool,
    out: &Path,
) -> Result<AnimationInfo, AtlasError> {
    let frame_count = premultiplied.len() as u32;
    let hitbox = opaque_bounds(width, premultiplied.iter().map(Vec::as_slice));
    let mut texture =
        Ktx2Texture::create(width, height, 1, frame_count, 1, 1, VkFormat::R8G8B8A8_SRGB).context(
            Ktx2Snafu {
                thing: "creating texture",
            },
        )?;
    for (layer, data) in premultiplied.iter().enumerate() {
        texture
            .set_image_data(0, layer as u32, 0, data)
            .context(Ktx2Snafu {
                thing: "writing frame",
            })?;
    }

    let params = BasisCompressionParams::builder()
        .uastc(true)
        .thread_count(thread::available_parallelism().map_or(1, NonZero::get) as u32)
        .build();
    texture.compress_basis(&params).context(Ktx2Snafu {
        thing: "compressing to uastc",
    })?;
    texture
        .transcode_basis(TranscodeFormat::Bc7Rgba)
        .context(Ktx2Snafu {
            thing: "transcoding to bc7",
        })?;
    texture.write_to_file(out).context(Ktx2Snafu {
        thing: "writing texture",
    })?;

    Ok(AnimationInfo {
        width,
        height,
        frame_count,
        frame_delays_ms,
        loops,
        hitbox,
    })
}
