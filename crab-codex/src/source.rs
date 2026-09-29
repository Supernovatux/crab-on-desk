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
    io::{self, Cursor, Read},
    path::{Component, Path, PathBuf},
};

use snafu::{OptionExt, ResultExt, Snafu, ensure};
use zip::{ZipArchive, result::ZipError};

const SITE: &str = "https://codex-pets.net";
const PET_PAGE_PREFIX: &str = "https://codex-pets.net/pets/";
const USER_AGENT: &str = concat!("crab-on-desk/", env!("CARGO_PKG_VERSION"));
const MANIFEST_FILE: &str = "pet.json";
const MAX_ZIP_BYTES: u64 = 25 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_SPRITESHEET_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Snafu)]
pub enum SourceError {
    #[snafu(display(
        "{input:?} is not a codex-pets.net link, an https .zip URL, a pet id or a local package"
    ))]
    Unrecognised { input: String },
    #[snafu(display("Unable to download {url}"))]
    Download { source: ureq::Error, url: String },
    #[snafu(display("Unable to read {path:?}"))]
    Read { source: io::Error, path: PathBuf },
    #[snafu(display("{thing} is larger than {limit} bytes"))]
    TooLarge { thing: String, limit: u64 },
    #[snafu(display("Invalid zip package"))]
    Zip { source: ZipError },
    #[snafu(display("Unable to read {name} from the zip package"))]
    Entry { source: io::Error, name: String },
    #[snafu(display(
        "The package must contain exactly one {MANIFEST_FILE} at the root or in one top-level folder"
    ))]
    Manifest,
    #[snafu(display("spritesheetPath {path:?} must be a relative path inside the package"))]
    UnsafePath { path: String },
    #[snafu(display("The package has no spritesheet {path:?}"))]
    NoSpritesheet { path: String },
}

pub struct Package {
    pub manifest: Vec<u8>,
    folder: PackageFolder,
}

enum PackageFolder {
    Zip {
        archive: ZipArchive<Cursor<Vec<u8>>>,
        prefix: String,
    },
    Directory(PathBuf),
}

pub fn fetch(input: &str) -> Result<Package, SourceError> {
    let path = Path::new(input);
    if path.is_dir() {
        let manifest = read_file(&path.join(MANIFEST_FILE), MAX_MANIFEST_BYTES)?;
        return Ok(Package {
            manifest,
            folder: PackageFolder::Directory(path.to_owned()),
        });
    }
    let zip = if path.is_file() {
        read_file(path, MAX_ZIP_BYTES)?
    } else {
        download(&download_url(input).context(UnrecognisedSnafu { input })?)?
    };
    open_zip(zip)
}

impl Package {
    pub fn spritesheet(&mut self, relative: &str) -> Result<Vec<u8>, SourceError> {
        let relative_path = Path::new(relative);
        ensure!(
            relative_path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
            UnsafePathSnafu { path: relative }
        );
        match &mut self.folder {
            PackageFolder::Directory(dir) => {
                read_file(&dir.join(relative_path), MAX_SPRITESHEET_BYTES)
            }
            PackageFolder::Zip { archive, prefix } => {
                let name = format!("{prefix}{relative}");
                ensure!(
                    archive.index_for_name(&name).is_some(),
                    NoSpritesheetSnafu { path: relative }
                );
                read_entry(archive, &name, MAX_SPRITESHEET_BYTES)
            }
        }
    }
}

fn download_url(input: &str) -> Option<String> {
    if let Some(page) = input.strip_prefix(PET_PAGE_PREFIX) {
        let id = page
            .split(['/', '?', '#'])
            .next()
            .filter(|id| !id.is_empty())?;
        return Some(pet_download_url(id));
    }
    if input.starts_with("https://") {
        return Some(input.to_owned());
    }
    input
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        .then(|| pet_download_url(input))
        .filter(|_| !input.is_empty())
}

fn pet_download_url(id: &str) -> String {
    format!("{SITE}/api/pets/{id}/download")
}

fn download(url: &str) -> Result<Vec<u8>, SourceError> {
    ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .and_then(|mut response| {
            response
                .body_mut()
                .with_config()
                .limit(MAX_ZIP_BYTES)
                .read_to_vec()
        })
        .context(DownloadSnafu { url })
}

fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, SourceError> {
    let file = fs::File::open(path).context(ReadSnafu { path })?;
    read_limited(file, limit, &path.display().to_string()).context(ReadSnafu { path })?
}

fn read_limited(
    reader: impl Read,
    limit: u64,
    thing: &str,
) -> io::Result<Result<Vec<u8>, SourceError>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    Ok(if bytes.len() as u64 > limit {
        TooLargeSnafu { thing, limit }.fail()
    } else {
        Ok(bytes)
    })
}

fn open_zip(zip: Vec<u8>) -> Result<Package, SourceError> {
    let mut archive = ZipArchive::new(Cursor::new(zip)).context(ZipSnafu)?;
    let manifests: Vec<String> = archive
        .file_names()
        .filter(|name| {
            let mut parts = name.split('/');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(file), None, None) => file == MANIFEST_FILE,
                (Some(folder), Some(file), None) => !folder.is_empty() && file == MANIFEST_FILE,
                _ => false,
            }
        })
        .map(str::to_owned)
        .collect();
    let [name] = manifests.as_slice() else {
        return ManifestSnafu.fail();
    };
    let name = name.clone();
    let prefix = name
        .strip_suffix(MANIFEST_FILE)
        .unwrap_or_default()
        .to_owned();
    let manifest = read_entry(&mut archive, &name, MAX_MANIFEST_BYTES)?;
    Ok(Package {
        manifest,
        folder: PackageFolder::Zip { archive, prefix },
    })
}

fn read_entry(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SourceError> {
    let entry = archive.by_name(name).context(ZipSnafu)?;
    ensure!(entry.size() <= limit, TooLargeSnafu { thing: name, limit });
    read_limited(entry, limit, name).context(EntrySnafu { name })?
}
