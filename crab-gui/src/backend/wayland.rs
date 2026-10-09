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

use smithay_client_toolkit::{
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
};
use snafu::Snafu;
use wayland_client::{
    ConnectError, Connection, DispatchError, QueueHandle,
    globals::{GlobalError, registry_queue_init},
    protocol::wl_output,
};

use crate::outputs::Output;

#[derive(Debug, Snafu)]
pub enum WaylandError {
    #[snafu(context(false), display("Unable to connect to the Wayland display"))]
    Connect { source: ConnectError },
    #[snafu(context(false), display("Unable to read the Wayland globals"))]
    Globals { source: GlobalError },
    #[snafu(context(false), display("Unable to receive the screens from Wayland"))]
    Dispatch { source: DispatchError },
}

struct Listing {
    registry_state: RegistryState,
    output_state: OutputState,
}

pub fn outputs() -> Result<Vec<Output>, WaylandError> {
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<Listing>(&connection)?;
    let handle = queue.handle();
    let mut listing = Listing {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &handle),
    };
    queue.roundtrip(&mut listing)?;
    queue.roundtrip(&mut listing)?;
    Ok(listing
        .output_state
        .outputs()
        .filter_map(|output| listing.output_state.info(&output))
        .filter_map(|info| {
            let (width, height) = info.logical_size.or_else(|| {
                info.modes
                    .iter()
                    .find(|mode| mode.current)
                    .map(|mode| mode.dimensions)
            })?;
            Some(Output {
                name: info.name?,
                description: info.description.unwrap_or_default(),
                width: width as u32,
                height: height as u32,
            })
        })
        .collect())
}

impl OutputHandler for Listing {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

delegate_dispatch2!(Listing);
delegate_registry!(Listing);

impl ProvidesRegistryState for Listing {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
