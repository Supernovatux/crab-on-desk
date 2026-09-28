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
    atlas::{AnimationInfo, Animations, Rect, ThemeBehaviour, ThemeManifest},
    dirs::{
        CommonError, get_animation_texture, get_theme_behaviour, get_theme_cache,
        get_theme_manifest,
    },
    toml_file::{self, TomlFileError},
};
use image::{
    AnimationDecoder, ImageError, RgbaImage, codecs::png::PngDecoder, metadata::LoopCount,
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
    #[snafu(display("{path:?} has no frames"))]
    Empty {
        path: PathBuf,
    },
    #[snafu(context(false))]
    Dir {
        source: CommonError,
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

pub fn gen_atlas(theme: &str, themes: &Themes) -> Result<(), AtlasError> {
    let theme_files = themes.themes.get(theme).ok_or(AtlasError::NotFound)?;
    let behaviour: ThemeBehaviour = toml_file::read(get_theme_behaviour(theme)?)?;
    if let Some(animation) = behaviour
        .animations()
        .find(|animation| !theme_files.contains_key(animation))
    {
        return UnsourcedSnafu { theme, animation }.fail();
    }
    let dir = get_theme_cache(theme)?;
    create_dir_all(&dir)?;
    let mut animations = BTreeMap::new();
    for (anim, path) in theme_files {
        let info = encode_animation(path, &get_animation_texture(theme, *anim)?)?;
        animations.insert(*anim, info);
    }
    fs::write(
        get_theme_manifest(theme)?,
        toml::to_string_pretty(&ThemeManifest {
            behaviour,
            animations,
        })?,
    )?;
    Ok(())
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
    let frame_count = frames.len() as u32;
    let hitbox = opaque_bounds(&frames);

    let mut texture =
        Ktx2Texture::create(width, height, 1, frame_count, 1, 1, VkFormat::R8G8B8A8_SRGB).context(
            Ktx2Snafu {
                thing: "creating texture",
            },
        )?;
    for (layer, frame) in frames.into_iter().enumerate() {
        let mut data = frame.into_raw();
        premultiply_alpha(&mut data);
        texture
            .set_image_data(0, layer as u32, 0, &data)
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

fn opaque_bounds(frames: &[RgbaImage]) -> Rect {
    let opaque = frames.iter().flat_map(|frame| {
        frame
            .enumerate_pixels()
            .filter(|(_, _, pixel)| matches!(pixel.0, [.., alpha] if alpha > 0))
            .map(|(x, y, _)| (x, y))
    });
    let bounds = opaque.fold(None, |bounds: Option<(u32, u32, u32, u32)>, (x, y)| {
        Some(bounds.map_or((x, y, x, y), |(left, top, right, bottom)| {
            (left.min(x), top.min(y), right.max(x), bottom.max(y))
        }))
    });
    bounds.map_or_else(Rect::default, |(left, top, right, bottom)| Rect {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

fn premultiply_alpha(pixels: &mut [u8]) {
    let (pixels, _) = pixels.as_chunks_mut::<4>();
    for [r, g, b, a] in pixels {
        let alpha = u32::from(*a);
        *r = (u32::from(*r) * alpha / 255) as u8;
        *g = (u32::from(*g) * alpha / 255) as u8;
        *b = (u32::from(*b) * alpha / 255) as u8;
    }
}
