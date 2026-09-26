use crab_settings::{
    atlas::{AtlasError, gen_atlas},
    themes::{ThemeError, Themes},
};
use snafu::{self, Snafu};

#[derive(Debug, Snafu)]
enum MainError {
    #[snafu(context(false))]
    Theme { source: ThemeError },

    #[snafu(context(false))]
    Atlas { source: AtlasError },
}

#[snafu::report]
fn main() -> Result<(), MainError> {
    let themes = Themes::new()?;
    for theme in themes.themes.keys() {
        println!("generating {theme}");
        gen_atlas(theme, &themes)?;
    }
    Ok(())
}
