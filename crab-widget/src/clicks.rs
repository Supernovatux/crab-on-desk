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

use std::time::{Duration, Instant};

use crate::window::Side;

const CLICK_WINDOW: Duration = Duration::from_millis(400);
const FLAIL_CLICKS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Poke(Side),
    Flail(Side),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clicks {
    #[default]
    None,
    Counting {
        count: u32,
        side: Side,
        until: Instant,
    },
}

impl Clicks {
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        match self {
            Self::None => None,
            Self::Counting { until, .. } => Some(*until),
        }
    }

    pub fn click(&mut self, side: Side, now: Instant) -> Option<Gesture> {
        let (count, side) = match *self {
            Self::None => (1, side),
            Self::Counting { count, side, .. } => (count + 1, side),
        };
        if count >= FLAIL_CLICKS {
            *self = Self::None;
            return Some(Gesture::Flail(side));
        }
        *self = Self::Counting {
            count,
            side,
            until: now + CLICK_WINDOW,
        };
        None
    }

    pub fn expire(&mut self, now: Instant) -> Option<Gesture> {
        match *self {
            Self::Counting { count, side, until } if until <= now => {
                *self = Self::None;
                (count >= 2).then_some(Gesture::Poke(side))
            }
            Self::None | Self::Counting { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_click_does_nothing() {
        let start = Instant::now();
        let mut clicks = Clicks::default();
        assert_eq!(clicks.click(Side::Left, start), None);
        assert_eq!(clicks.expire(start + CLICK_WINDOW), None);
        assert_eq!(clicks, Clicks::None);
    }

    #[test]
    fn double_click_pokes_the_first_side_after_the_window() {
        let start = Instant::now();
        let mut clicks = Clicks::default();
        clicks.click(Side::Right, start);
        clicks.click(Side::Left, start + Duration::from_millis(300));
        assert_eq!(clicks.expire(start + CLICK_WINDOW), None);
        assert_eq!(
            clicks.expire(start + Duration::from_millis(700)),
            Some(Gesture::Poke(Side::Right))
        );
    }

    #[test]
    fn fourth_click_flails_at_once() {
        let start = Instant::now();
        let mut clicks = Clicks::default();
        for _ in 0..3 {
            assert_eq!(clicks.click(Side::Left, start), None);
        }
        assert_eq!(
            clicks.click(Side::Right, start),
            Some(Gesture::Flail(Side::Left))
        );
        assert_eq!(clicks.deadline(), None);
    }
}
