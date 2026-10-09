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

use std::{ptr::NonNull, sync::Arc, time::Instant};

use calloop::{EventLoop, channel::Sender};
use calloop_wayland_source::WaylandSource;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    dispatch2::Dispatch2,
    error::GlobalError as SctkGlobalError,
    output::{OutputHandler, OutputState},
    reexports::protocols::{
        ext::idle_notify::v1::client::{
            ext_idle_notification_v1::{self, ExtIdleNotificationV1},
            ext_idle_notifier_v1::ExtIdleNotifierV1,
        },
        wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatError, SeatHandler, SeatState,
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
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
    ConnectError, Connection, DispatchError, Proxy, QueueHandle,
    globals::{BindError, GlobalError, registry_queue_init},
    protocol::{wl_callback, wl_output, wl_pointer, wl_seat, wl_surface},
};
use wayland_egl::WlEglSurface;

use super::{
    CalloopSnafu, NAMESPACE, SIZE, Window, WindowError, WindowEvent,
    widget::{
        Button, CursorShape, Output, OutputChange, Pointer, Position, Running, Surface, Widget,
    },
};
use crate::{placement::Area, renderer::Renderer, theme::Theme};

const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;
const INPUT_IDLE_VERSION: u32 = 2;

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
    #[snafu(display("Unable to create the input region"))]
    #[snafu(context(false))]
    Region { source: SctkGlobalError },
    #[snafu(display("Unable to use the pointer"))]
    #[snafu(context(false))]
    Seat { source: SeatError },
}

type State<R> = Widget<Wayland<R>, R>;

pub struct Wayland<R: Renderer + 'static> {
    layer: LayerSurface,
    compositor: CompositorState,
    layer_shell: LayerShell,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    cursor_shapes: Option<CursorShapeManager>,
    pointer: Option<PointerDevice>,
    idle_notifier: Option<ExtIdleNotifierV1>,
    idle_notification: Option<ExtIdleNotificationV1>,
    output: Option<wl_output::WlOutput>,
    placement: Placement,
    egl_window: WlEglSurface,
    size: (u32, u32),
    qh: QueueHandle<State<R>>,
    conn: Connection,
}

struct PointerDevice {
    device: wl_pointer::WlPointer,
    cursor: Option<WpCursorShapeDeviceV1>,
    enter_serial: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    Anchored,
    Placed {
        applied: Position,
        in_flight: Option<Position>,
        target: Position,
    },
}

struct MoveAck;

struct IdleNotifierData;

struct IdleWatch {
    timeout: std::time::Duration,
}

pub fn open<R: Renderer + 'static>(
    theme: Arc<Theme>,
    events: Sender<WindowEvent>,
) -> Result<Box<dyn Window>, WindowError> {
    let conn = Connection::connect_to_env().map_err(WaylandError::from)?;
    let (globals, event_queue) = registry_queue_init(&conn).map_err(WaylandError::from)?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context(BindSnafu {
        thing: "wl_compositor",
    })?;
    let layer_shell = LayerShell::bind(&globals, &qh).context(BindSnafu { thing: "wlr-shell" })?;
    let size = (SIZE, SIZE);
    let (layer, egl_window, window_handle) =
        create_layer(&compositor, &layer_shell, &qh, None, size)?;
    let display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
        NonNull::new(conn.backend().display_ptr().cast()).ok_or(WaylandError::Display)?,
    ));
    let renderer = R::setup(display_handle, window_handle, size.0, size.1)?;
    let event_loop: EventLoop<State<R>> = EventLoop::try_new().context(CalloopSnafu {
        thing: "creating event loop",
    })?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| e.error)
        .context(CalloopSnafu {
            thing: "adding wayland source",
        })?;
    let surface = Wayland {
        layer,
        compositor,
        layer_shell,
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
        pointer: None,
        idle_notifier: globals.bind(&qh, 1..=2, IdleNotifierData).ok(),
        idle_notification: None,
        output: None,
        placement: Placement::Anchored,
        egl_window,
        size,
        qh,
        conn,
    };
    let widget = Widget::new(surface, renderer, size, theme, events, event_loop.handle());
    Ok(Box::new(Running { widget, event_loop }))
}

impl<R: Renderer + 'static> Wayland<R> {
    fn commit_move(&mut self, target: Position) {
        if let Placement::Placed {
            applied,
            target: latest,
            ..
        } = self.placement
        {
            self.layer.set_margin(target.1, 0, 0, target.0);
            self.layer.commit();
            self.conn.display().sync(&self.qh, MoveAck);
            self.placement = Placement::Placed {
                applied,
                in_flight: Some(target),
                target: latest,
            };
        }
    }

    fn acknowledge_move(&mut self) -> bool {
        let Placement::Placed {
            in_flight: Some(applied),
            target,
            ..
        } = self.placement
        else {
            return false;
        };
        self.placement = Placement::Placed {
            applied,
            in_flight: None,
            target,
        };
        if target != applied {
            self.commit_move(target);
        }
        true
    }

    fn watch_idle(&mut self, seat: &wl_seat::WlSeat, timeout: std::time::Duration) {
        let Some(notifier) = &self.idle_notifier else {
            return;
        };
        if self.idle_notification.is_some() {
            return;
        }
        let timeout_ms = timeout.as_millis() as u32;
        let watch = IdleWatch { timeout };
        self.idle_notification = Some(if notifier.version() >= INPUT_IDLE_VERSION {
            notifier.get_input_idle_notification(timeout_ms, seat, &self.qh, watch)
        } else {
            notifier.get_idle_notification(timeout_ms, seat, &self.qh, watch)
        });
    }
}

impl<R: Renderer + 'static> Surface for Wayland<R> {
    fn output(&self) -> Option<Output> {
        let info = self
            .output
            .as_ref()
            .and_then(|output| self.output_state.info(output))?;
        let ((x, y), (width, height)) = (info.logical_position?, info.logical_size?);
        Some(Output {
            name: info.name,
            area: Area {
                x,
                y,
                width,
                height,
            },
        })
    }

    fn origin(&self) -> Option<Position> {
        match self.placement {
            Placement::Anchored => None,
            Placement::Placed { applied, .. } => Some(applied),
        }
    }

    fn target(&self) -> Option<Position> {
        match self.placement {
            Placement::Anchored => None,
            Placement::Placed { target, .. } => Some(target),
        }
    }

    fn place(&mut self, home: Position) {
        if self.placement == Placement::Anchored {
            self.layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            self.placement = Placement::Placed {
                applied: home,
                in_flight: None,
                target: home,
            };
        }
    }

    fn move_to(&mut self, target: Position) -> bool {
        if let Placement::Placed {
            applied, in_flight, ..
        } = self.placement
        {
            self.placement = Placement::Placed {
                applied,
                in_flight,
                target,
            };
            if in_flight.is_none() && target != applied {
                self.commit_move(target);
            }
        }
        false
    }

    fn set_input_region(&mut self, region: Area) -> Result<(), WindowError> {
        let input = Region::new(&self.compositor).map_err(WaylandError::from)?;
        input.add(region.x, region.y, region.width, region.height);
        self.layer
            .wl_surface()
            .set_input_region(Some(input.wl_region()));
        Ok(())
    }

    fn set_cursor(&mut self, shape: CursorShape) {
        if let Some(PointerDevice {
            cursor: Some(cursor),
            enter_serial: Some(serial),
            ..
        }) = &self.pointer
        {
            cursor.set_shape(
                *serial,
                match shape {
                    CursorShape::Default => Shape::Default,
                    CursorShape::Grabbing => Shape::Grabbing,
                },
            );
        }
    }

    fn move_to_output<T: Renderer>(
        &mut self,
        name: &str,
        renderer: &mut T,
    ) -> Result<OutputChange, WindowError> {
        let Some(output) = self.output_state.outputs().find(|output| {
            self.output_state
                .info(output)
                .and_then(|info| info.name)
                .is_some_and(|output_name| output_name == name)
        }) else {
            return Ok(OutputChange::Unchanged);
        };
        if self.output.as_ref() == Some(&output) {
            return Ok(OutputChange::Unchanged);
        }
        let (layer, egl_window, window_handle) = create_layer(
            &self.compositor,
            &self.layer_shell,
            &self.qh,
            Some(&output),
            self.size,
        )?;
        renderer.replace_window(window_handle, self.size.0, self.size.1)?;
        self.egl_window = egl_window;
        self.layer = layer;
        self.output = Some(output);
        self.placement = Placement::Anchored;
        Ok(OutputChange::Remapped)
    }

    fn present<T: Renderer>(&mut self, renderer: &T) -> Result<bool, WindowError> {
        let surface = self.layer.wl_surface();
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        renderer.swapbuffers()?;
        self.layer.commit();
        Ok(true)
    }
}

impl<R: Renderer + 'static> State<R> {
    fn output_changed(&self, output: &wl_output::WlOutput) {
        if self.surface.output.as_ref() == Some(output) {
            self.report_location();
        }
    }
}

fn pointer(event: &PointerEvent) -> Option<Pointer> {
    let button = |button| match button {
        BUTTON_LEFT => Some(Button::Left),
        BUTTON_RIGHT => Some(Button::Right),
        _ => None,
    };
    match event.kind {
        PointerEventKind::Enter { .. } => Some(Pointer::Enter),
        PointerEventKind::Leave { .. } => Some(Pointer::Leave),
        PointerEventKind::Motion { .. } => Some(Pointer::Motion(event.position)),
        PointerEventKind::Press { button: code, .. } => {
            Some(Pointer::Press(button(code)?, event.position))
        }
        PointerEventKind::Release { button: code, .. } => {
            Some(Pointer::Release(button(code)?, event.position))
        }
        PointerEventKind::Axis { .. } => None,
    }
}

impl<R: Renderer + 'static> CompositorHandler for State<R> {
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
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if surface == self.surface.layer.wl_surface() {
            self.frame_done();
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if self.surface.output.is_none() {
            self.surface.output = Some(output.clone());
        }
        self.report_location();
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

impl<R: Renderer + 'static> OutputHandler for State<R> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.surface.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.output_changed(&output);
    }

    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.output_changed(&output);
    }

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl<R: Renderer + 'static> LayerShellHandler for State<R> {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.close();
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (width, height) = configure.new_size;
        let size = (
            if width == 0 {
                self.surface.size.0
            } else {
                width
            },
            if height == 0 {
                self.surface.size.1
            } else {
                height
            },
        );
        if size != self.surface.size {
            self.surface.size = size;
            self.surface
                .egl_window
                .resize(size.0 as i32, size.1 as i32, 0, 0);
            self.resized(size);
        }
        self.mapped();
    }
}

delegate_registry!(@<R: Renderer + 'static> State<R>);

impl<R: Renderer + 'static> ProvidesRegistryState for State<R> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.surface.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl<R: Renderer + 'static> SeatHandler for State<R> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.surface.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        let timeout = self.idle_timeout();
        self.surface.watch_idle(&seat, timeout);
        if capability != Capability::Pointer || self.surface.pointer.is_some() {
            return;
        }
        match self.surface.seat_state.get_pointer(qh, &seat) {
            Ok(pointer) => {
                let cursor = self
                    .surface
                    .cursor_shapes
                    .as_ref()
                    .map(|shapes| shapes.get_shape_device(&pointer, qh));
                self.surface.pointer = Some(PointerDevice {
                    device: pointer,
                    cursor,
                    enter_serial: None,
                });
            }
            Err(e) => self.fail(WaylandError::from(e)),
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability != Capability::Pointer {
            return;
        }
        if let Some(PointerDevice { device, cursor, .. }) = self.surface.pointer.take() {
            if let Some(cursor) = cursor {
                cursor.destroy();
            }
            device.release();
        }
        self.pointer_lost();
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl<R: Renderer + 'static> PointerHandler for State<R> {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.surface.layer.wl_surface() {
                continue;
            }
            if let PointerEventKind::Enter { serial } = event.kind
                && let Some(device) = &mut self.surface.pointer
            {
                device.enter_serial = Some(serial);
            }
            if let Some(event) = pointer(event) {
                self.pointer(event);
            }
        }
    }
}

impl<R: Renderer + 'static> Dispatch2<wl_callback::WlCallback, State<R>> for MoveAck {
    fn event(
        &self,
        state: &mut State<R>,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &Connection,
        _: &QueueHandle<State<R>>,
    ) {
        if let wl_callback::Event::Done { .. } = event
            && state.surface.acknowledge_move()
        {
            state.moved();
        }
    }
}

impl<R: Renderer + 'static> Dispatch2<ExtIdleNotifierV1, State<R>> for IdleNotifierData {
    fn event(
        &self,
        _: &mut State<R>,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<State<R>>,
    ) {
    }
}

impl<R: Renderer + 'static> Dispatch2<ExtIdleNotificationV1, State<R>> for IdleWatch {
    fn event(
        &self,
        state: &mut State<R>,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &Connection,
        _: &QueueHandle<State<R>>,
    ) {
        let event = match event {
            ext_idle_notification_v1::Event::Idled => {
                let now = Instant::now();
                WindowEvent::UserIdle {
                    since: now.checked_sub(self.timeout).unwrap_or(now),
                }
            }
            ext_idle_notification_v1::Event::Resumed => WindowEvent::UserActive,
            _ => return,
        };
        state.send(event);
    }
}

fn create_layer<R: Renderer + 'static>(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<State<R>>,
    output: Option<&wl_output::WlOutput>,
    (width, height): (u32, u32),
) -> Result<(LayerSurface, WlEglSurface, RawWindowHandle), WaylandError> {
    let surface = compositor.create_surface(qh);
    let layer = layer_shell.create_layer_surface(qh, surface, Layer::Top, Some(NAMESPACE), output);
    layer.set_anchor(Anchor::RIGHT);
    layer.set_size(width, height);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_margin(0, 0, 0, 0);
    layer.commit();
    let egl_window = WlEglSurface::new(layer.wl_surface().id(), width as i32, height as i32)
        .context(WaylandSnafu {
            thing: "egl_window",
        })?;
    let window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
        NonNull::new(layer.wl_surface().id().as_ptr().cast()).ok_or(WaylandError::Window)?,
    ));
    Ok((layer, egl_window, window_handle))
}

smithay_client_toolkit::delegate_dispatch2!(@<R: Renderer + 'static> State<R>);
