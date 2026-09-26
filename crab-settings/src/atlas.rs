use std::{
    collections::BTreeMap,
    fs::{self, File, create_dir_all},
    io::{self, BufReader},
    num::NonZero,
    path::Path,
    thread,
};

use crab_common::{
    atlas::{AnimationInfo, MANIFEST_FILE, ThemeManifest},
    dirs::{CommonError, get_theme_cache},
};
use image::{AnimationDecoder, ImageError, RgbaImage, codecs::gif::GifDecoder};
use ktx2_rw::{BasisCompressionParams, Ktx2Texture, TranscodeFormat, VkFormat};
use snafu::{ResultExt, Snafu};

use crate::themes::Themes;

#[derive(Debug, Snafu)]
pub enum AtlasError {
    NotFound,
    #[snafu(context(false))]
    IO {
        source: io::Error,
    },
    #[snafu(context(false))]
    Gif {
        source: ImageError,
    },
    InvalidGif,
    #[snafu(context(false))]
    Dir {
        source: CommonError,
    },
    #[snafu(context(false))]
    Toml {
        source: toml::ser::Error,
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
    let dir = get_theme_cache(theme)?;
    create_dir_all(&dir)?;
    let mut animations = BTreeMap::new();
    for (anim, path) in theme_files {
        let info = encode_animation(path, &dir.join(anim.texture_file()?))?;
        animations.insert(anim.clone(), info);
    }
    fs::write(
        dir.join(MANIFEST_FILE),
        toml::to_string_pretty(&ThemeManifest { animations })?,
    )?;
    Ok(())
}

fn encode_animation(gif: &Path, out: &Path) -> Result<AnimationInfo, AtlasError> {
    let frames: Vec<RgbaImage> = GifDecoder::new(BufReader::new(File::open(gif)?))?
        .into_frames()
        .collect_frames()?
        .into_iter()
        .map(image::Frame::into_buffer)
        .collect();
    let (width, height) = frames.first().ok_or(AtlasError::InvalidGif)?.dimensions();
    let frame_count = frames.len() as u32;

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
