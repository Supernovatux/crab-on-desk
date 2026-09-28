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
    ffi::{CString, NulError},
    num::{NonZero, NonZeroU32},
};

use glutin::{
    config::{Config, ConfigTemplateBuilder},
    context::{ContextAttributesBuilder, GlProfile, NotCurrentGlContext, PossiblyCurrentGlContext},
    display::{Display as gluDisplay, GlDisplay},
    surface::{GlSurface, SurfaceAttributesBuilder, WindowSurface},
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use snafu::{OptionExt, ResultExt, Snafu, ensure};

use crate::{
    renderer::{Renderer, RendererError},
    theme::Animation,
};

#[derive(Debug, Snafu)]
pub enum OpenglError {
    #[snafu(display("Glutin error with {thing}."))]
    Glutin {
        source: glutin::error::Error,
        thing: String,
    },
    #[snafu(display("No valid gl context found."))]
    Gl,
    #[snafu(display("Invalid Window Size."))]
    Size,
    #[snafu(display("Program link error {thing}"))]
    Program { thing: String },
    #[snafu(display("String error"))]
    #[snafu(context(false))]
    Cstring { source: NulError },
}
const VERTEX_SHADER: &str = "#version 330 core
uniform vec2 u_scale;
out vec2 v_uv;
void main() {
    vec2 corner = vec2(gl_VertexID & 1, gl_VertexID >> 1);
    v_uv = vec2(corner.x, 1.0 - corner.y);
    gl_Position = vec4((corner * 2.0 - 1.0) * u_scale, 0.0, 1.0);
}";

const FRAGMENT_SHADER: &str = "#version 330 core
uniform sampler2DArray u_frames;
uniform int u_frame;
in vec2 v_uv;
out vec4 frag_color;
void main() {
    frag_color = texture(u_frames, vec3(v_uv, float(u_frame)));
}";

pub struct Opengl {
    gl_display: gluDisplay,
    config: Config,
    gl_surface: glutin::surface::Surface<WindowSurface>,
    gl_context: glutin::context::PossiblyCurrentContext,
    program: gl::types::GLuint,
    vao: gl::types::GLuint,
    texture: gl::types::GLuint,
    scale_location: gl::types::GLint,
    frame_location: gl::types::GLint,
    frame_aspect: f32,
}

impl Renderer for Opengl {
    fn setup(
        display: RawDisplayHandle,
        window: RawWindowHandle,
        w: u32,
        h: u32,
    ) -> Result<Box<Self>, RendererError> {
        let gl_display = unsafe {
            gluDisplay::new(display, glutin::display::DisplayApiPreference::Egl).context(
                GlutinSnafu {
                    thing: "gl_display",
                },
            )?
        };
        let config = unsafe { gl_display.find_configs(ConfigTemplateBuilder::new().build()) }
            .context(GlutinSnafu {
                thing: "config iterator",
            })?
            .next()
            .ok_or(OpenglError::Gl)?;
        let context_attributes = ContextAttributesBuilder::new()
            .with_context_api(glutin::context::ContextApi::OpenGl(None))
            .with_profile(GlProfile::Core)
            .build(Some(window));
        let not_curr_context = unsafe { gl_display.create_context(&config, &context_attributes) }
            .context(GlutinSnafu {
            thing: "creating context",
        })?;
        let gl_surface = window_surface(&gl_display, &config, window, w, h)?;
        let gl_context = not_curr_context
            .make_current(&gl_surface)
            .context(GlutinSnafu { thing: "context" })?;
        gl::load_with(|symbol| {
            let symbol = unsafe { CString::new(symbol).unwrap_unchecked() };
            gl_display.get_proc_address(symbol.as_c_str()).cast()
        });
        let program = unsafe { link_program(VERTEX_SHADER, FRAGMENT_SHADER) }?;
        let scale_location = unsafe { uniform_location(program, "u_scale") }?;
        let frame_location = unsafe { uniform_location(program, "u_frame") }?;
        let sampler_location = unsafe { uniform_location(program, "u_frames") }?;
        let (vao, texture) = unsafe { setup_quad(program, sampler_location) };

        Ok(Box::new(Self {
            gl_display,
            config,
            gl_surface,
            gl_context,
            program,
            vao,
            texture,
            scale_location,
            frame_location,
            frame_aspect: 1.0,
        }))
    }
    fn set_animation(&mut self, animation: &Animation) -> Result<(), RendererError> {
        let size = i32::try_from(animation.blocks.len())
            .ok()
            .context(SizeSnafu)?;
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D_ARRAY, self.texture);
            gl::CompressedTexImage3D(
                gl::TEXTURE_2D_ARRAY,
                0,
                gl::COMPRESSED_RGBA_BPTC_UNORM,
                animation.width as i32,
                animation.height as i32,
                animation.frame_count as i32,
                0,
                size,
                animation.blocks.as_ptr().cast(),
            );
        }
        self.frame_aspect = (f64::from(animation.width) / f64::from(animation.height)) as f32;
        Ok(())
    }
    fn draw(&mut self, w: i32, h: i32, frame: u32, mirrored: bool) {
        let surface_aspect = (f64::from(w) / f64::from(h)) as f32;
        let direction = if mirrored { -1.0 } else { 1.0 };
        let scale_x = direction * (self.frame_aspect / surface_aspect).min(1.0);
        let scale_y = (surface_aspect / self.frame_aspect).min(1.0);
        unsafe {
            gl::Viewport(0, 0, w, h);
            gl::ClearColor(0.0, 0.0, 0.0, 0.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::UseProgram(self.program);
            gl::Uniform2f(self.scale_location, scale_x, scale_y);
            gl::Uniform1i(self.frame_location, frame as i32);
            gl::BindVertexArray(self.vao);
            gl::BindTexture(gl::TEXTURE_2D_ARRAY, self.texture);
            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        }
    }
    fn replace_window(
        &mut self,
        window: RawWindowHandle,
        w: u32,
        h: u32,
    ) -> Result<(), RendererError> {
        let gl_surface = window_surface(&self.gl_display, &self.config, window, w, h)?;
        self.gl_context
            .make_current(&gl_surface)
            .context(GlutinSnafu {
                thing: "switching surface",
            })?;
        self.gl_surface = gl_surface;
        Ok(())
    }
    fn swapbuffers(&self) -> Result<(), RendererError> {
        self.gl_surface
            .swap_buffers(&self.gl_context)
            .context(GlutinSnafu {
                thing: "unable to swap buffers",
            })?;
        Ok(())
    }
    fn resize(&self, w: u32, h: u32) -> Result<(), RendererError> {
        self.gl_surface.resize(
            &self.gl_context,
            NonZeroU32::new(w).ok_or(OpenglError::Size)?,
            NonZeroU32::new(h).ok_or(OpenglError::Size)?,
        );
        Ok(())
    }
}
fn window_surface(
    gl_display: &gluDisplay,
    config: &Config,
    window: RawWindowHandle,
    w: u32,
    h: u32,
) -> Result<glutin::surface::Surface<WindowSurface>, OpenglError> {
    let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        window,
        NonZero::new(w).ok_or(OpenglError::Size)?,
        NonZero::new(h).ok_or(OpenglError::Size)?,
    );
    unsafe { gl_display.create_window_surface(config, &surface_attributes) }.context(GlutinSnafu {
        thing: "glutin surface",
    })
}

unsafe fn compile_shader(
    src: &str,
    kind: gl::types::GLenum,
) -> Result<gl::types::GLuint, OpenglError> {
    unsafe {
        let shader = gl::CreateShader(kind);
        let c_src = CString::new(src)?;
        gl::ShaderSource(shader, 1, &c_src.as_ptr(), std::ptr::null());
        gl::CompileShader(shader);

        let mut success = gl::types::GLint::from(gl::FALSE);
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &raw mut success);
        if success != gl::types::GLint::from(gl::TRUE) {
            let mut len = 0;
            gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &raw mut len);
            let mut buf = vec![0u8; len as usize];
            gl::GetShaderInfoLog(shader, len, std::ptr::null_mut(), buf.as_mut_ptr().cast());
            Err(OpenglError::Program {
                thing: String::from_utf8_lossy_owned(buf),
            })?;
        }
        Ok(shader)
    }
}

unsafe fn link_program(
    vertex_src: &str,
    fragment_src: &str,
) -> Result<gl::types::GLuint, OpenglError> {
    unsafe {
        let vertex_shader = compile_shader(vertex_src, gl::VERTEX_SHADER)?;
        let fragment_shader = compile_shader(fragment_src, gl::FRAGMENT_SHADER)?;

        let program = gl::CreateProgram();
        gl::AttachShader(program, vertex_shader);
        gl::AttachShader(program, fragment_shader);
        gl::LinkProgram(program);

        let mut success = gl::types::GLint::from(gl::FALSE);
        gl::GetProgramiv(program, gl::LINK_STATUS, &raw mut success);
        if success != gl::types::GLint::from(gl::TRUE) {
            let mut len = 0;
            gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &raw mut len);
            let mut buf = vec![0u8; len as usize];
            gl::GetProgramInfoLog(program, len, std::ptr::null_mut(), buf.as_mut_ptr().cast());
            Err(OpenglError::Program {
                thing: String::from_utf8_lossy_owned(buf),
            })?;
        }

        gl::DeleteShader(vertex_shader);
        gl::DeleteShader(fragment_shader);
        Ok(program)
    }
}

unsafe fn uniform_location(
    program: gl::types::GLuint,
    name: &str,
) -> Result<gl::types::GLint, OpenglError> {
    let c_name = CString::new(name)?;
    let location = unsafe { gl::GetUniformLocation(program, c_name.as_ptr()) };
    ensure!(
        location >= 0,
        ProgramSnafu {
            thing: format!("missing uniform {name}"),
        }
    );
    Ok(location)
}

unsafe fn setup_quad(
    program: gl::types::GLuint,
    sampler_location: gl::types::GLint,
) -> (gl::types::GLuint, gl::types::GLuint) {
    unsafe {
        let mut vao = 0;
        let mut texture = 0;
        gl::GenVertexArrays(1, &raw mut vao);
        gl::GenTextures(1, &raw mut texture);

        gl::BindTexture(gl::TEXTURE_2D_ARRAY, texture);
        gl::TexParameteri(
            gl::TEXTURE_2D_ARRAY,
            gl::TEXTURE_MIN_FILTER,
            gl::LINEAR as i32,
        );
        gl::TexParameteri(
            gl::TEXTURE_2D_ARRAY,
            gl::TEXTURE_MAG_FILTER,
            gl::LINEAR as i32,
        );
        gl::TexParameteri(
            gl::TEXTURE_2D_ARRAY,
            gl::TEXTURE_WRAP_S,
            gl::CLAMP_TO_EDGE as i32,
        );
        gl::TexParameteri(
            gl::TEXTURE_2D_ARRAY,
            gl::TEXTURE_WRAP_T,
            gl::CLAMP_TO_EDGE as i32,
        );
        gl::TexParameteri(gl::TEXTURE_2D_ARRAY, gl::TEXTURE_MAX_LEVEL, 0);

        gl::UseProgram(program);
        gl::Uniform1i(sampler_location, 0);
        gl::ActiveTexture(gl::TEXTURE0);

        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::ONE, gl::ONE_MINUS_SRC_ALPHA);

        (vao, texture)
    }
}
