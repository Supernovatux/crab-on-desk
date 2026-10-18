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

use std::{ptr::NonNull, sync::Arc};

use calloop::{
    EventLoop,
    channel::{self, Sender},
};
use calloop_wayland_source::WaylandSource;
use crab_common::gpu::GpuChoice;
use crab_common::{
    gnome::{self, ShellProxyBlocking},
    gui::WIDGET_APP_ID,
};
use raw_window_handle::{RawWindowHandle, WaylandWindowHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
        Shape, WpCursorShapeDeviceV1,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window as XdgWindow, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
};
use snafu::ResultExt;
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_callback, wl_output, wl_pointer, wl_seat, wl_surface},
};

use super::{
    CalloopSnafu, SIZE, Window, WindowError, WindowEvent,
    wayland::{self, WaylandError},
    widget::{
        Button, CursorShape, Output, OutputChange, Pointer, Position, Running, Surface, Widget,
    },
};
use crate::{
    backend::gnome::{self as backend, Signal},
    placement::Area,
    renderer::{Renderer, Target},
    theme::Theme,
};

const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;
const TITLE: &str = "Crab on Desk";

type State<R> = Widget<Gnome<R>, R>;

pub struct Gnome<R: Renderer + 'static> {
    window: XdgWindow,
    compositor: CompositorState,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    cursor_shapes: Option<CursorShapeManager>,
    pointer: Option<PointerDevice>,
    shell: ShellProxyBlocking<'static>,
    output: Option<Output>,
    placement: Placement,
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

pub fn open<R: Renderer + 'static>(
    theme: Arc<Theme>,
    events: Sender<WindowEvent>,
    gpu: &GpuChoice,
) -> Result<Box<dyn Window>, WindowError> {
    let conn = Connection::connect_to_env().map_err(WaylandError::from)?;
    let (globals, event_queue) = registry_queue_init(&conn).map_err(WaylandError::from)?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|source| WaylandError::Bind {
        source,
        thing: "wl_compositor".to_owned(),
    })?;
    let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|source| WaylandError::Bind {
        source,
        thing: "xdg_wm_base".to_owned(),
    })?;
    let size = (SIZE, SIZE);
    let window =
        xdg_shell.create_window(compositor.create_surface(&qh), WindowDecorations::None, &qh);
    window.set_app_id(WIDGET_APP_ID);
    window.set_title(TITLE);
    window.set_min_size(Some(size));
    window.set_max_size(Some(size));
    window.commit();
    let window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
        NonNull::new(window.wl_surface().id().as_ptr().cast()).ok_or(WaylandError::Window)?,
    ));
    let renderer = R::setup(Target {
        display: wayland::display_handle(&conn)?,
        window: window_handle,
        size,
        device: wayland::compositor_device(&conn, &globals),
        gpu: gpu.clone(),
        layers: theme.max_layers(),
    })?;
    let event_loop: EventLoop<State<R>> = EventLoop::try_new().context(CalloopSnafu {
        thing: "creating event loop",
    })?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| e.error)
        .context(CalloopSnafu {
            thing: "adding wayland source",
        })?;
    let shell = gnome::shell()?;
    let (signals, signal_channel) = channel::channel();
    event_loop
        .handle()
        .insert_source(signal_channel, |event, (), widget: &mut State<R>| {
            if let channel::Event::Msg(signal) = event {
                widget.signal(signal);
            }
        })
        .map_err(|e| e.error)
        .context(CalloopSnafu {
            thing: "adding extension channel",
        })?;
    backend::forward_moves(signals.clone())?;
    let sleep = theme.behaviour().sleep;
    backend::forward_idle(
        std::time::Duration::from_millis(u64::from(sleep.idle_after_ms.min(sleep.yawn_after_ms))),
        signals,
    )?;
    conn.flush().map_err(|_| WaylandError::Display)?;
    let (name, (x, y, width, height)) = gnome::reply(shell.register(), "registering the window")?;
    let surface = Gnome {
        window,
        compositor,
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
        pointer: None,
        shell,
        output: Some(Output {
            name: Some(name),
            area: Area {
                x,
                y,
                width,
                height,
            },
        }),
        placement: Placement::Anchored,
        size,
        qh,
        conn,
    };
    let widget = Widget::new(surface, renderer, size, theme, events, event_loop.handle());
    Ok(Box::new(Running { widget, event_loop }))
}

impl<R: Renderer + 'static> Gnome<R> {
    fn global(&self, (x, y): Position) -> Position {
        self.output
            .as_ref()
            .map_or((x, y), |output| (output.area.x + x, output.area.y + y))
    }

    fn commit_move(&mut self, target: Position) {
        if let Placement::Placed {
            applied,
            target: latest,
            ..
        } = self.placement
        {
            let (x, y) = self.global(target);
            if let Err(error) = gnome::reply(self.shell.move_window(x, y), "moving the crab") {
                eprintln!("{}", snafu::Report::from_error(error));
            }
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
}

impl<R: Renderer + 'static> Surface for Gnome<R> {
    fn output(&self) -> Option<Output> {
        self.output.clone()
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
        self.window
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
        _renderer: &mut T,
    ) -> Result<OutputChange, WindowError> {
        if self
            .output
            .as_ref()
            .and_then(|output| output.name.as_deref())
            == Some(name)
        {
            return Ok(OutputChange::Unchanged);
        }
        let monitors = gnome::reply(self.shell.monitors(), "listing the monitors")?;
        let Some((name, (x, y, width, height))) = monitors
            .into_iter()
            .find(|(connector, _)| connector == name)
        else {
            return Ok(OutputChange::Unchanged);
        };
        let home = (
            width - self.size.0 as i32,
            (height - self.size.1 as i32) / 2,
        );
        gnome::reply(
            self.shell.move_window(x + home.0, y + home.1),
            "moving the crab to another monitor",
        )?;
        self.output = Some(Output {
            name: Some(name),
            area: Area {
                x,
                y,
                width,
                height,
            },
        });
        self.placement = Placement::Placed {
            applied: home,
            in_flight: None,
            target: home,
        };
        Ok(OutputChange::Moved)
    }

    fn present<T: Renderer>(&mut self, renderer: &mut T) -> Result<bool, WindowError> {
        let surface = self.window.wl_surface();
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        renderer.swapbuffers()?;
        self.window.commit();
        Ok(true)
    }
}

impl<R: Renderer + 'static> State<R> {
    fn signal(&mut self, signal: Signal) {
        match signal {
            Signal::Moved((name, (x, y, width, height))) => {
                self.surface.output = Some(Output {
                    name: Some(name),
                    area: Area {
                        x,
                        y,
                        width,
                        height,
                    },
                });
                self.report_location();
            }
            Signal::UserIdle { since } => self.send(WindowEvent::UserIdle { since }),
            Signal::UserActive => self.send(WindowEvent::UserActive),
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
        if surface == self.surface.window.wl_surface() {
            self.frame_done();
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

impl<R: Renderer + 'static> WindowHandler for State<R> {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &XdgWindow) {
        self.close();
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &XdgWindow,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let (width, height) = configure.new_size;
        let size = (
            width.map_or(self.surface.size.0, std::num::NonZero::get),
            height.map_or(self.surface.size.1, std::num::NonZero::get),
        );
        if size != self.surface.size {
            self.surface.size = size;
            self.resized(size);
        }
        self.mapped();
    }
}

impl<R: Renderer + 'static> OutputHandler for State<R> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.surface.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
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
            if &event.surface != self.surface.window.wl_surface() {
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

smithay_client_toolkit::delegate_dispatch2!(@<R: Renderer + 'static> State<R>);
