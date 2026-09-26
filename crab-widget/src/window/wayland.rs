use std::ptr::NonNull;

use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use snafu::{ResultExt, Snafu};
use wayland_client::{
    ConnectError, Connection, DispatchError, EventQueue, Proxy, QueueHandle,
    globals::{BindError, GlobalError, registry_queue_init},
    protocol::{wl_output, wl_surface},
};
use wayland_egl::WlEglSurface;

use crate::renderer::{Renderer, RendererError};

#[derive(Debug, Snafu)]
pub enum WaylandError {
    #[snafu(display("Wayland connection error"))]
    #[snafu(context(false))]
    Connection { source: ConnectError },
    #[snafu(display("Wayland global error"))]
    #[snafu(context(false))]
    Global { source: GlobalError },
    #[snafu(display("{thing} not available."))]
    Bind { source: BindError, thing: String },
    #[snafu(display("Display handle not available."))]
    Display,
    #[snafu(display("Error during dispatch"))]
    #[snafu(context(false))]
    Dispatch { source: DispatchError },
    #[snafu(display("Window handle not available."))]
    Window,
    #[snafu(display("Wayland error while {thing}."))]
    Wayland {
        source: wayland_egl::Error,
        thing: String,
    },
    #[snafu(display("Renderer error"))]
    #[snafu(context(false))]
    Renderer { source: RendererError },
    #[snafu(display("Calloop Error at {thing}"))]
    Calloop {
        source: calloop::Error,
        thing: String,
    },
}

pub struct WaylandWindow<T: Renderer> {
    window: _WaylandWindow<T>,
    queue: EventQueue<_WaylandWindow<T>>,
}
impl<T: Renderer + 'static> super::Window for WaylandWindow<T> {
    fn new() -> Result<Box<Self>, super::WindowError> {
        let (window, queue) = _WaylandWindow::new()?;
        Ok(Box::new(Self { window, queue }))
    }
    fn run(mut self: Box<Self>) -> Result<(), super::WindowError> {
        self.window.run(self.queue)?;
        Ok(())
    }
}

pub struct _WaylandWindow<T: Renderer> {
    conn: Connection,
    layer: LayerSurface,
    renderer: Box<T>,
    registry_state: RegistryState,
    output_state: OutputState,
    exit: bool,
    error: Option<WaylandError>,
    width: u32,
    height: u32,
    first: bool,
    egl_window: WlEglSurface,
}

impl<T: Renderer + 'static> _WaylandWindow<T> {
    pub fn new() -> Result<(Self, EventQueue<Self>), WaylandError> {
        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init(&conn)?;
        let qh = event_queue.handle();

        let compositor = CompositorState::bind(&globals, &qh).context(BindSnafu {
            thing: "wl_compositor",
        })?;
        let wlr_shell =
            LayerShell::bind(&globals, &qh).context(BindSnafu { thing: "wlr-shell" })?;
        let surface = compositor.create_surface(&qh);
        let width = 200;
        let height = 200;
        let layer =
            wlr_shell.create_layer_surface(&qh, surface, Layer::Top, Some("carb-on-desk"), None);
        layer.set_anchor(Anchor::RIGHT);
        layer.set_size(width, height);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_margin(0, 0, 0, 0);
        layer.commit();
        let egl_window = WlEglSurface::new(layer.wl_surface().id(), width as i32, height as i32)
            .context(WaylandSnafu {
                thing: "egl_window",
            })?;

        let display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(conn.backend().display_ptr().cast()).ok_or(WaylandError::Display)?,
        ));
        let window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new((layer.wl_surface()).id().as_ptr().cast()).ok_or(WaylandError::Window)?,
        ));
        let renderer = T::setup(display_handle, window_handle, width, height)?;
        Ok((
            Self {
                conn,
                layer,
                renderer,
                registry_state: RegistryState::new(&globals),
                output_state: OutputState::new(&globals, &qh),
                exit: false,
                error: None,
                width,
                height,
                first: true,
                egl_window,
            },
            event_queue,
        ))
    }
    pub fn run(&mut self, event_queue: EventQueue<Self>) -> Result<(), WaylandError> {
        let mut event_loop: EventLoop<Self> = EventLoop::try_new().context(CalloopSnafu {
            thing: "creating event loop",
        })?;
        WaylandSource::new(self.conn.clone(), event_queue)
            .insert(event_loop.handle())
            .map_err(|e| e.error)
            .context(CalloopSnafu {
                thing: "adding wayland source",
            })?;
        let loop_signal = event_loop.get_signal();

        event_loop
            .run(None, self, move |app| {
                if app.exit {
                    loop_signal.stop();
                }
            })
            .context(CalloopSnafu {
                thing: "event loop failed",
            })?;
        Ok(())
    }
    fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<(), WaylandError> {
        self.renderer.draw(self.width as i32, self.height as i32);
        self.layer
            .wl_surface()
            .frame(qh, FrameCallbackData(self.layer.wl_surface().clone()));
        self.renderer.swapbuffers()?;
        self.layer.commit();
        Ok(())
    }
}

impl<T: Renderer + 'static> CompositorHandler for _WaylandWindow<T> {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if let Err(e) = self.draw(qh) {
            self.error = Some(e);
            self.exit = true;
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}
impl<T: Renderer> OutputHandler for _WaylandWindow<T> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl<T: Renderer + 'static> LayerShellHandler for _WaylandWindow<T> {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (mut new_width, mut new_height) = configure.new_size;
        if new_width == 0 {
            new_width = self.width;
        }
        if new_height == 0 {
            new_height = self.height;
        }

        if new_width != self.width || new_height != self.height {
            self.width = new_width;
            self.height = new_height;
            self.egl_window
                .resize(self.width as i32, self.height as i32, 0, 0);
            if let Err(e) = self.renderer.resize(self.width, self.height) {
                self.error = Some(WaylandError::Renderer { source: e });
                self.exit = true;
            }
        }

        if self.first {
            self.first = false;
            if let Err(e) = self.draw(qh) {
                self.error = Some(e);
                self.exit = true;
            }
        }
    }
}

delegate_registry!(@<T: Renderer+'static>_WaylandWindow<T>);

impl<T: Renderer + 'static> ProvidesRegistryState for _WaylandWindow<T> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(@<T:Renderer> _WaylandWindow<T>);
