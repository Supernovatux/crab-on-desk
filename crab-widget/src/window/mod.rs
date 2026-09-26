use snafu::Snafu;

mod wayland;

#[derive(Debug, Snafu)]
pub enum WindowError {
    #[snafu(context(false))]
    Wayland { source: wayland::WaylandError },
}

pub trait Window {
    fn new() -> Result<Box<Self>, WindowError>
    where
        Self: Sized;
    fn run(self: Box<Self>) -> Result<(), WindowError>; // Send a default animation along with this
    // fn set_animation changes current animation
}

pub fn create() -> Result<Box<dyn Window>, WindowError> {
    #[cfg(target_os = "linux")]
    {
        use crate::{renderer::opengl::Opengl, window::wayland::WaylandWindow};

        let window = WaylandWindow::<Opengl>::new()?;
        Ok(window)
    }
}
