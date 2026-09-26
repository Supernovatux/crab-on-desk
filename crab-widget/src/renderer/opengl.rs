use std::{
    ffi::{CString, NulError},
    num::{NonZero, NonZeroU32},
};

use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextAttributesBuilder, GlProfile, NotCurrentGlContext},
    display::{Display as gluDisplay, GlDisplay},
    surface::{GlSurface, SurfaceAttributesBuilder, WindowSurface},
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use snafu::{ResultExt, Snafu};

use crate::renderer::{Renderer, RendererError};

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
const VERTICES: [f32; 15] = [
    // x,    y,     r,   g,   b
    0.0, 0.5, 1.0, 0.0, 0.0, //
    -0.5, -0.5, 0.0, 1.0, 0.0, //
    0.5, -0.5, 0.0, 0.0, 1.0,
];

const VERTEX_SHADER: &str = "#version 330 core
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec3 a_color;
out vec3 v_color;
void main() {
    gl_Position = vec4(a_pos, 0.0, 1.0);
    v_color = a_color;
}";

const FRAGMENT_SHADER: &str = "#version 330 core
in vec3 v_color;
out vec4 frag_color;
void main() {
    frag_color = vec4(v_color, 1.0);
}";
pub struct Opengl {
    gl_surface: glutin::surface::Surface<WindowSurface>,
    gl_context: glutin::context::PossiblyCurrentContext,
    program: gl::types::GLuint,
    vao: gl::types::GLuint,
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
        let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            window,
            NonZero::new(w).ok_or(OpenglError::Size)?,
            NonZero::new(h).ok_or(OpenglError::Size)?,
        );
        let gl_surface = unsafe { gl_display.create_window_surface(&config, &surface_attributes) }
            .context(GlutinSnafu {
                thing: "glutin surface",
            })?;
        let gl_context = not_curr_context
            .make_current(&gl_surface)
            .context(GlutinSnafu { thing: "context" })?;
        gl::load_with(|symbol| {
            let symbol = unsafe { CString::new(symbol).unwrap_unchecked() };
            gl_display.get_proc_address(symbol.as_c_str()).cast()
        });
        let program = unsafe { link_program(VERTEX_SHADER, FRAGMENT_SHADER) }?;
        let vao = unsafe { setup_triangle() };

        Ok(Box::new(Self {
            gl_surface,
            gl_context,
            program,
            vao,
        }))
    }
    fn draw(&mut self, w: i32, h: i32) {
        unsafe {
            gl::Viewport(0, 0, w, h);
            gl::ClearColor(0.0, 0.0, 0.0, 0.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::UseProgram(self.program);
            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLES, 0, 3);
        }
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

unsafe fn setup_triangle() -> gl::types::GLuint {
    unsafe {
        let mut vao = 0;
        let mut vbo = 0;
        gl::GenVertexArrays(1, &raw mut vao);
        gl::GenBuffers(1, &raw mut vbo);

        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
        gl::BufferData(
            gl::ARRAY_BUFFER,
            (VERTICES.len() * size_of::<f32>()) as isize,
            VERTICES.as_ptr().cast(),
            gl::STATIC_DRAW,
        );

        let stride = (5 * size_of::<f32>()) as i32;
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
        gl::EnableVertexAttribArray(0);
        gl::VertexAttribPointer(
            1,
            3,
            gl::FLOAT,
            gl::FALSE,
            stride,
            (2 * size_of::<f32>()) as *const _,
        );
        gl::EnableVertexAttribArray(1);

        vao
    }
}
