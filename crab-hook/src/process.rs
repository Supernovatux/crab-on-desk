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

use std::{fs, os::unix::process::parent_id};

#[derive(Debug, Default, Clone, Copy)]
pub struct Pids {
    pub agent: Option<u32>,
    pub source: Option<u32>,
}

struct Stat {
    comm: String,
    parent: u32,
    has_terminal: bool,
}

impl Stat {
    fn read(pid: u32) -> Option<Self> {
        let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, rest) = text.split_once('(')?;
        let (comm, fields) = rest.rsplit_once(')')?;
        let mut fields = fields.split_whitespace();
        let parent = fields.nth(1)?.parse().ok()?;
        let terminal: i32 = fields.nth(2)?.parse().ok()?;
        Some(Self {
            comm: comm.to_owned(),
            parent,
            has_terminal: terminal != 0,
        })
    }
}

pub fn find(agent_process: &str) -> Pids {
    let mut pids = Pids::default();
    let mut inside_terminal = false;
    let mut pid = parent_id();
    while pid > 1 {
        let Some(stat) = Stat::read(pid) else {
            break;
        };
        if pids.agent.is_none() && stat.comm == agent_process {
            pids.agent = Some(pid);
        }
        if stat.has_terminal {
            inside_terminal = true;
        } else if inside_terminal {
            pids.source = Some(pid);
            break;
        }
        if stat.parent == pid {
            break;
        }
        pid = stat.parent;
    }
    pids
}
