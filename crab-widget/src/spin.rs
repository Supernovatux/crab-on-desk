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
    f64::consts::{PI, TAU},
    time::{Duration, Instant},
};

const THRESHOLD: f64 = 2.0 * TAU;
const STALL: Duration = Duration::from_millis(500);
const MIN_RADIUS: f64 = 24.0;
const COOLDOWN: Duration = Duration::from_secs(12);

#[derive(Debug, Default)]
pub struct Spin {
    last: Option<(f64, Instant)>,
    turned: f64,
    cooldown_until: Option<Instant>,
}

impl Spin {
    pub fn moved(&mut self, (dx, dy): (f64, f64), now: Instant) -> bool {
        if dx.hypot(dy) < MIN_RADIUS {
            self.reset();
            return false;
        }
        let angle = dy.atan2(dx);
        match self.last {
            Some((last, at)) if now.saturating_duration_since(at) <= STALL => {
                self.turned += wrap(angle - last);
            }
            _ => self.turned = 0.0,
        }
        self.last = Some((angle, now));
        let cooled = self.cooldown_until.is_none_or(|until| now > until);
        if self.turned.abs() < THRESHOLD || !cooled {
            return false;
        }
        self.reset();
        self.cooldown_until = Some(now + COOLDOWN);
        true
    }

    pub const fn reset(&mut self) {
        self.last = None;
        self.turned = 0.0;
    }
}

fn wrap(delta: f64) -> f64 {
    if delta > PI {
        delta - TAU
    } else if delta < -PI {
        delta + TAU
    } else {
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(spin: &mut Spin, start: Instant, turns: u32, radius: f64) -> Option<u32> {
        (0..turns * 16).find(|step| {
            let angle = f64::from(*step) * TAU / 16.0;
            let now = start + Duration::from_millis(u64::from(*step) * 50);
            spin.moved((radius * angle.cos(), radius * angle.sin()), now)
        })
    }

    #[test]
    fn two_circles_trigger_once() {
        let start = Instant::now();
        let mut spin = Spin::default();
        assert_eq!(circle(&mut spin, start, 3, 100.0), Some(32));
        assert_eq!(
            circle(&mut spin, start + Duration::from_secs(3), 3, 100.0),
            None
        );
    }

    #[test]
    fn close_or_slow_circles_do_not_count() {
        let start = Instant::now();
        let mut spin = Spin::default();
        assert_eq!(circle(&mut spin, start, 3, 10.0), None);
        let slow = (0..48_u32).any(|step| {
            let angle = f64::from(step) * TAU / 16.0;
            let step = u64::from(step);
            let now = start + Duration::from_millis(step * 600);
            spin.moved((100.0 * angle.cos(), 100.0 * angle.sin()), now)
        });
        assert!(!slow);
    }
}
