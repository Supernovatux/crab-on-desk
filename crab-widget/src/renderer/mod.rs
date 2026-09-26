use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use snafu::Snafu;

pub mod opengl;

#[derive(Debug, Snafu)]
pub enum RendererError {
    #[snafu(context(false))]
    OpenGL { source: opengl::OpenglError },
}

pub trait Renderer {
    fn setup(
        display: RawDisplayHandle,
        window: RawWindowHandle,
        w: u32,
        h: u32,
    ) -> Result<Box<Self>, RendererError>;
    fn draw(&mut self, w: i32, h: i32);
    fn swapbuffers(&self) -> Result<(), RendererError>;
    fn resize(&self, w: u32, h: u32) -> Result<(), RendererError>;
    // fn set_texture
}
