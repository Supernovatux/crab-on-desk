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

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use snafu::Snafu;

use crate::theme::Animation;

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
    fn set_animation(&mut self, animation: &Animation) -> Result<(), RendererError>;
    fn draw(&mut self, w: i32, h: i32, frame: u32, mirrored: bool);
    fn replace_window(
        &mut self,
        window: RawWindowHandle,
        w: u32,
        h: u32,
    ) -> Result<(), RendererError>;
    fn swapbuffers(&self) -> Result<(), RendererError>;
    fn resize(&self, w: u32, h: u32) -> Result<(), RendererError>;
}
