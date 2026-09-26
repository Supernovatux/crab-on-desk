use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::PathBuf,
    str::FromStr,
};

use crab_common::{
    atlas::Animations,
    dirs::{CommonError, get_config},
};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum ThemeError {
    #[snafu(context(false))]
    CheckFS {
        source: CommonError,
    },
    #[snafu(display("Path {:?}", thing))]
    IO {
        thing: PathBuf,
        source: io::Error,
    },
    FileName,
    #[snafu(context(false))]
    AnimName {
        source: serde_plain::Error,
    },
}

#[derive(Debug)]
pub struct Themes {
    pub themes: HashMap<String, BTreeMap<Animations, PathBuf>>,
}

impl Themes {
    fn list_themes_files() -> Result<Vec<PathBuf>, ThemeError> {
        let mut path = get_config()?;
        let pathc = path.clone();
        path.push("themes");
        let ls = fs::read_dir(path).context(IOSnafu { thing: pathc })?;
        let mut names = vec![];
        for i in ls {
            let i = i.map_err(|_| ThemeError::FileName)?;
            names.push(i.path());
        }
        Ok(names)
    }

    pub fn new() -> Result<Self, ThemeError> {
        let files = Self::list_themes_files()?;
        let mut themes = HashMap::new();
        for i in files {
            let filename = i
                .file_stem()
                .ok_or(ThemeError::FileName)?
                .to_os_string()
                .into_string()
                .map_err(|_| ThemeError::FileName)?;
            let (name, anim) = filename.split_once('-').ok_or(ThemeError::FileName)?;
            let anim = Animations::from_str(anim)?;
            themes
                .entry(name.to_owned())
                .and_modify(|v: &mut BTreeMap<Animations, PathBuf>| {
                    v.insert(anim.clone(), i.clone());
                })
                .or_insert_with(|| BTreeMap::from([(anim, i)]));
        }
        Ok(Self { themes })
    }
}
