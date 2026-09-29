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
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, Instant},
};

use crab_common::{
    agent::{Agent, AgentEvent, CompactTrigger, EventKind},
    atlas::{Animations, SleepMode, ThemeBehaviour, Tier},
};

use crate::{clicks::Gesture, window::Side};

const MAX_SESSIONS: usize = 20;
const BUSY_STALE: Duration = Duration::from_secs(300);
const UNWATCHED_STALE: Duration = Duration::from_secs(600);
const RECOVERY_LIMIT: Duration = Duration::from_secs(300);
const MINI_ENTER_FALLBACK: Duration = Duration::from_millis(3200);
const FIRST_ROAM: Duration = Duration::from_secs(8);
const BETWEEN_ROAMS: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Thinking,
    Working,
    Juggling,
    Carrying,
    Attention,
    Sweeping,
    Notification,
    Error,
    Roam,
    Dizzy,
    Yawning,
    Dozing,
    Collapsing,
    Sleeping,
    Waking,
    MiniEnter,
    MiniIdle,
    MiniPeek,
    MiniAlert,
    MiniHappy,
    MiniWorking,
}

impl State {
    const fn priority(self) -> u8 {
        match self {
            Self::Yawning
            | Self::Dozing
            | Self::Collapsing
            | Self::Sleeping
            | Self::Waking
            | Self::MiniEnter
            | Self::MiniIdle
            | Self::MiniPeek
            | Self::MiniAlert
            | Self::MiniHappy
            | Self::MiniWorking
            | Self::Dizzy => 0,
            Self::Idle | Self::Roam => 1,
            Self::Thinking => 2,
            Self::Working => 3,
            Self::Juggling | Self::Carrying => 4,
            Self::Attention => 5,
            Self::Sweeping => 6,
            Self::Notification => 7,
            Self::Error => 8,
        }
    }

    const fn is_oneshot(self) -> bool {
        matches!(
            self,
            Self::Carrying | Self::Attention | Self::Sweeping | Self::Notification | Self::Error
        )
    }

    const fn is_resting(self) -> bool {
        matches!(self, Self::Idle | Self::Roam)
    }

    const fn is_asleep(self) -> bool {
        matches!(
            self,
            Self::Yawning | Self::Dozing | Self::Collapsing | Self::Sleeping
        )
    }

    const fn is_busy(self) -> bool {
        matches!(self, Self::Thinking | Self::Working | Self::Juggling)
    }

    const fn animation(self) -> Animations {
        match self {
            Self::Idle => Animations::Idle,
            Self::Thinking => Animations::Thinking,
            Self::Working => Animations::Working,
            Self::Juggling => Animations::Juggling,
            Self::Carrying => Animations::Carrying,
            Self::Attention => Animations::Attention,
            Self::Sweeping => Animations::Sweeping,
            Self::Notification => Animations::Notification,
            Self::Error => Animations::Error,
            Self::Roam => Animations::Roam,
            Self::Dizzy => Animations::Dizzy,
            Self::Yawning => Animations::Yawning,
            Self::Dozing => Animations::Dozing,
            Self::Collapsing => Animations::Collapsing,
            Self::Sleeping => Animations::Sleeping,
            Self::Waking => Animations::Waking,
            Self::MiniEnter => Animations::MiniEnter,
            Self::MiniIdle => Animations::MiniIdle,
            Self::MiniPeek => Animations::MiniPeek,
            Self::MiniAlert => Animations::MiniAlert,
            Self::MiniHappy => Animations::MiniHappy,
            Self::MiniWorking => Animations::MiniWorking,
        }
    }
}

struct Session {
    state: State,
    subagents: BTreeSet<String>,
    spawning: bool,
    pid: Option<u32>,
    updated: Instant,
}

impl Session {
    fn subagent_count(&self) -> usize {
        self.subagents.len().max(usize::from(self.spawning))
    }

    fn clear_subagents(&mut self) {
        self.subagents.clear();
        self.spawning = false;
    }

    fn stale_at(&self) -> Option<Instant> {
        if self.state.is_busy() {
            Some(self.updated + BUSY_STALE)
        } else {
            self.pid.is_none().then(|| self.updated + UNWATCHED_STALE)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Off,
    Entering,
    Docked { hovered: bool },
}

struct Recovery {
    pids: BTreeSet<u32>,
    until: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    state: State,
    animation: Animations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Deadline {
    None,
    Pending { state: State, at: Instant },
    AutoReturn { at: Instant },
    Reaction { at: Instant },
    IdleAnimation { at: Instant },
}

pub struct StateMachine {
    behaviour: ThemeBehaviour,
    sessions: HashMap<(Agent, String), Session>,
    shown: Shown,
    shown_since: Instant,
    awake_since: Instant,
    user_idle_since: Option<Instant>,
    idle_animation_played: bool,
    recovery: Option<Recovery>,
    edge: Edge,
    clip_lengths: BTreeMap<Animations, Duration>,
    free_roam: bool,
    roam_at: Instant,
    overlay: Option<Animations>,
    deadline: Deadline,
}

impl StateMachine {
    #[must_use]
    pub fn new(
        behaviour: ThemeBehaviour,
        clip_lengths: BTreeMap<Animations, Duration>,
        free_roam: bool,
        now: Instant,
    ) -> Self {
        Self {
            behaviour,
            sessions: HashMap::new(),
            shown: Shown {
                state: State::Idle,
                animation: State::Idle.animation(),
            },
            shown_since: now,
            awake_since: now,
            user_idle_since: None,
            idle_animation_played: false,
            recovery: None,
            edge: Edge::Off,
            clip_lengths,
            free_roam,
            roam_at: now + FIRST_ROAM,
            overlay: None,
            deadline: Deadline::None,
        }
    }

    #[must_use]
    pub fn animation(&self) -> Animations {
        self.overlay.unwrap_or(self.shown.animation)
    }

    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        let display = match self.deadline {
            Deadline::None => None,
            Deadline::Pending { at, .. }
            | Deadline::AutoReturn { at }
            | Deadline::Reaction { at }
            | Deadline::IdleAnimation { at } => Some(at),
        };
        display
            .into_iter()
            .chain(self.rest_deadline())
            .chain(self.sessions.values().filter_map(Session::stale_at))
            .chain(self.recovery.as_ref().map(|recovery| recovery.until))
            .chain(self.can_roam().then_some(self.roam_at))
            .min()
    }

    pub fn recover(&mut self, pids: impl IntoIterator<Item = u32>, now: Instant) {
        let pids: BTreeSet<u32> = pids.into_iter().collect();
        self.recovery = (!pids.is_empty()).then(|| Recovery {
            pids,
            until: now + RECOVERY_LIMIT,
        });
    }

    pub fn dock(&mut self, now: Instant) -> Option<Animations> {
        if self.edge != Edge::Off {
            return None;
        }
        self.edge = Edge::Entering;
        self.apply(State::MiniEnter, now)
    }

    pub fn undock(&mut self, now: Instant) -> Option<Animations> {
        if self.edge == Edge::Off {
            return None;
        }
        self.edge = Edge::Off;
        self.apply(self.resolve(), now)
    }

    pub fn hover(&mut self, inside: bool, now: Instant) -> Option<Animations> {
        let Edge::Docked { .. } = self.edge else {
            return None;
        };
        self.edge = Edge::Docked { hovered: inside };
        match (inside, self.shown.state) {
            (true, State::MiniIdle) => self.apply(State::MiniPeek, now),
            (false, State::MiniPeek) => self.apply(State::MiniIdle, now),
            _ => None,
        }
    }

    const fn mini_rest(&self) -> State {
        match self.edge {
            Edge::Docked { hovered: true } => State::MiniPeek,
            Edge::Off | Edge::Entering | Edge::Docked { .. } => State::MiniIdle,
        }
    }

    fn mini_request(&self, state: State) -> Option<State> {
        match self.edge {
            Edge::Off => Some(state),
            Edge::Entering => None,
            Edge::Docked { .. } => match state {
                State::Notification => Some(State::MiniAlert),
                State::Attention => Some(State::MiniHappy),
                State::Thinking | State::Working | State::Juggling => Some(State::MiniWorking),
                _ => (self.shown.state == State::MiniWorking).then(|| self.mini_rest()),
            },
        }
    }

    pub fn on_process_exit(&mut self, pid: u32, now: Instant) -> Option<Animations> {
        if let Some(recovery) = &mut self.recovery {
            recovery.pids.remove(&pid);
            if recovery.pids.is_empty() {
                self.recovery = None;
            }
        }
        let count = self.sessions.len();
        self.sessions.retain(|_, session| session.pid != Some(pid));
        if self.sessions.len() == count {
            return None;
        }
        self.request(self.resolve(), now)
    }

    #[must_use]
    pub fn is_roaming(&self) -> bool {
        self.shown.state == State::Roam
    }

    pub fn end_roam(&mut self, now: Instant) -> Option<Animations> {
        if !self.is_roaming() {
            return None;
        }
        self.apply(self.resolve(), now)
    }

    #[must_use]
    pub fn dizzy_armed(&self) -> bool {
        self.is_calm()
            && self.user_idle_since.is_none()
            && self
                .behaviour
                .auto_return_ms
                .contains_key(&Animations::Dizzy)
    }

    pub fn dizzy(&mut self, now: Instant) -> Option<Animations> {
        if !self.dizzy_armed() {
            return None;
        }
        self.apply(State::Dizzy, now)
    }

    fn is_calm(&self) -> bool {
        self.shown.state == State::Idle && self.overlay.is_none() && self.edge == Edge::Off
    }

    fn can_roam(&self) -> bool {
        self.free_roam && self.is_calm()
    }

    pub fn react(&mut self, gesture: Gesture, roll: u64, now: Instant) -> Option<Animations> {
        if self.shown.state != State::Idle
            || self.overlay.is_some()
            || self.deadline != Deadline::None
        {
            return None;
        }
        let animation = self.reaction_for(gesture, roll)?;
        let duration = timing_of(&self.behaviour.reaction_ms, animation)?;
        self.overlay = Some(animation);
        self.deadline = Deadline::Reaction { at: now + duration };
        Some(animation)
    }

    pub const fn on_user_idle(&mut self, since: Instant) {
        self.user_idle_since = Some(since);
    }

    pub fn on_user_active(&mut self, now: Instant) -> Option<Animations> {
        let before = self.animation();
        self.user_idle_since = None;
        self.idle_animation_played = false;
        match self.shown.state {
            State::Yawning | State::Dozing => {
                self.apply(self.resolve(), now);
            }
            State::Collapsing | State::Sleeping
                if self.behaviour.sleep.mode == SleepMode::Direct =>
            {
                self.apply(self.resolve(), now);
            }
            State::Collapsing | State::Sleeping => {
                self.apply(State::Waking, now);
            }
            _ if matches!(self.deadline, Deadline::IdleAnimation { .. }) => {
                self.overlay = None;
                self.deadline = Deadline::None;
            }
            _ => {}
        }
        self.changed_from(before)
    }

    pub fn on_event(&mut self, event: &AgentEvent, now: Instant) -> Option<Animations> {
        self.recovery = None;
        match self.update_session(event, now) {
            Some(oneshot) => self.request(oneshot, now),
            None => self.request(self.resolve(), now),
        }
    }

    pub fn on_deadline(&mut self, now: Instant, roll: u64) -> Option<Animations> {
        let before = self.animation();
        self.on_display_deadline(now);
        self.on_stale_sessions(now);
        if self
            .recovery
            .as_ref()
            .is_some_and(|recovery| now >= recovery.until)
        {
            self.recovery = None;
        }
        self.on_rest_deadline(now, roll);
        if self.can_roam() && now >= self.roam_at {
            self.apply(State::Roam, now);
        }
        self.changed_from(before)
    }

    fn on_stale_sessions(&mut self, now: Instant) {
        let count = self.sessions.len();
        self.sessions.retain(|_, session| {
            !(session.state == State::Idle && session.stale_at().is_some_and(|at| now >= at))
        });
        let mut changed = self.sessions.len() != count;
        for session in self.sessions.values_mut() {
            if session.stale_at().is_some_and(|at| now >= at) {
                session.state = State::Idle;
                session.clear_subagents();
                session.updated = now;
                changed = true;
            }
        }
        if changed {
            self.request(self.resolve(), now);
        }
    }

    fn changed_from(&self, before: Animations) -> Option<Animations> {
        let animation = self.animation();
        (animation != before).then_some(animation)
    }

    fn on_display_deadline(&mut self, now: Instant) {
        match self.deadline {
            Deadline::None => {}
            Deadline::Pending { at, .. }
            | Deadline::AutoReturn { at }
            | Deadline::Reaction { at }
            | Deadline::IdleAnimation { at }
                if now < at => {}
            Deadline::Reaction { .. } | Deadline::IdleAnimation { .. } => {
                self.overlay = None;
                self.deadline = Deadline::None;
            }
            Deadline::Pending { state, .. } if state.is_oneshot() || self.edge != Edge::Off => {
                self.apply(state, now);
            }
            Deadline::Pending { .. } => {
                self.apply(self.resolve(), now);
            }
            Deadline::AutoReturn { .. } => {
                let next = match self.shown.state {
                    State::Yawning => State::Dozing,
                    State::Collapsing => State::Sleeping,
                    State::MiniEnter => {
                        self.edge = Edge::Docked { hovered: false };
                        State::MiniIdle
                    }
                    State::MiniPeek => State::MiniIdle,
                    State::MiniAlert | State::MiniHappy => self.mini_rest(),
                    _ => self.resolve(),
                };
                self.apply(next, now);
            }
        }
    }

    fn still_since(&self) -> Option<Instant> {
        if self.recovery.is_some() {
            return None;
        }
        self.user_idle_since
            .map(|since| since.max(self.awake_since))
    }

    fn rest_deadline(&self) -> Option<Instant> {
        let still = self.still_since()?;
        let sleep = &self.behaviour.sleep;
        match self.shown.state {
            State::Idle => {
                let yawn = still + millis(sleep.yawn_after_ms);
                let idle_animation = (self.overlay.is_none()
                    && !self.idle_animation_played
                    && !self.behaviour.idle_pool.is_empty())
                .then(|| still + millis(sleep.idle_after_ms));
                Some(idle_animation.map_or(yawn, |at| at.min(yawn)))
            }
            State::Dozing => Some(still + millis(sleep.deep_sleep_after_ms)),
            _ => None,
        }
    }

    fn on_rest_deadline(&mut self, now: Instant, roll: u64) {
        let Some(still) = self.still_since() else {
            return;
        };
        let sleep = self.behaviour.sleep;
        match self.shown.state {
            State::Idle if now >= still + millis(sleep.yawn_after_ms) => {
                let next = match sleep.mode {
                    SleepMode::Full => State::Yawning,
                    SleepMode::Direct => State::Sleeping,
                };
                self.apply(next, now);
            }
            State::Idle
                if self.overlay.is_none()
                    && !self.idle_animation_played
                    && now >= still + millis(sleep.idle_after_ms) =>
            {
                self.idle_animation_played = true;
                let pool = &self.behaviour.idle_pool;
                if let Some(idle) = roll
                    .checked_rem(pool.len() as u64)
                    .and_then(|index| pool.get(index as usize))
                {
                    self.overlay = Some(idle.animation);
                    self.deadline = Deadline::IdleAnimation {
                        at: now + millis(idle.duration_ms),
                    };
                }
            }
            State::Dozing if now >= still + millis(sleep.deep_sleep_after_ms) => {
                self.apply(State::Collapsing, now);
            }
            _ => {}
        }
    }

    fn update_session(&mut self, event: &AgentEvent, now: Instant) -> Option<State> {
        let key = (event.agent, event.session_id.clone());
        match &event.kind {
            EventKind::SessionEnd => {
                self.sessions.remove(&key);
                return None;
            }
            EventKind::SessionClear => {
                self.sessions.remove(&key);
                return Some(State::Sweeping);
            }
            _ => {}
        }
        if !self.sessions.contains_key(&key) {
            self.evict_oldest();
        }
        let session = self.sessions.entry(key).or_insert_with(|| Session {
            state: State::Idle,
            subagents: BTreeSet::new(),
            spawning: false,
            pid: None,
            updated: now,
        });
        session.updated = now;
        session.pid = event.agent_pid.or(event.source_pid).or(session.pid);
        let (state, oneshot) = match &event.kind {
            EventKind::SessionStart { .. }
            | EventKind::SessionEnd
            | EventKind::SessionClear
            | EventKind::CompactEnd {
                trigger: CompactTrigger::Manual,
            } => (State::Idle, None),
            EventKind::PromptSubmit => {
                session.clear_subagents();
                (State::Thinking, None)
            }
            EventKind::CompactEnd {
                trigger: CompactTrigger::Auto,
            } => (State::Thinking, None),
            EventKind::ToolStart | EventKind::ToolEnd => (State::Working, None),
            EventKind::SubagentStart { id } => {
                match id {
                    Some(id) => {
                        session.subagents.insert(id.clone());
                    }
                    None => session.spawning = true,
                }
                (State::Juggling, None)
            }
            EventKind::SubagentStop { id } => {
                if let Some(id) = id {
                    session.subagents.remove(id);
                }
                session.spawning = false;
                (State::Working, None)
            }
            EventKind::Stop => {
                session.clear_subagents();
                (State::Idle, Some(State::Attention))
            }
            EventKind::ToolFailure | EventKind::StopFailure => (State::Idle, Some(State::Error)),
            EventKind::CompactStart => (State::Idle, Some(State::Sweeping)),
            EventKind::Notification
            | EventKind::Elicitation
            | EventKind::PermissionRequest { .. } => (State::Idle, Some(State::Notification)),
            EventKind::WorktreeCreate => (State::Idle, Some(State::Carrying)),
        };
        session.state = if session.subagent_count() > 0 {
            State::Juggling
        } else {
            state
        };
        oneshot
    }

    fn evict_oldest(&mut self) {
        if self.sessions.len() < MAX_SESSIONS {
            return;
        }
        let oldest = self
            .sessions
            .iter()
            .min_by_key(|(_, session)| session.updated)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            self.sessions.remove(&key);
        }
    }

    fn resolve(&self) -> State {
        self.sessions
            .values()
            .map(|session| session.state)
            .fold(State::Idle, |best, state| {
                if state.priority() > best.priority() {
                    state
                } else {
                    best
                }
            })
    }

    fn request(&mut self, state: State, now: Instant) -> Option<Animations> {
        let state = self.mini_request(state)?;
        if state == State::Idle && self.shown.state == State::Roam {
            return None;
        }
        if state == self.shown.state && self.animation_for(state) == self.shown.animation {
            if self.deadline == Deadline::None {
                self.deadline = self.auto_return(state, now);
            }
            return None;
        }
        if let Deadline::Pending { state: pending, .. } = self.deadline
            && state.priority() < pending.priority()
        {
            return None;
        }
        let remaining = self
            .min_display(self.shown.state)
            .saturating_sub(now.saturating_duration_since(self.shown_since));
        if remaining.is_zero() {
            return self.apply(state, now);
        }
        self.deadline = Deadline::Pending {
            state,
            at: now + remaining,
        };
        None
    }

    fn apply(&mut self, state: State, now: Instant) -> Option<Animations> {
        let shown = Shown {
            state,
            animation: self.animation_for(state),
        };
        let before = self.animation();
        let still_resting = state.is_resting() && self.shown.state.is_resting();
        if shown.state != self.shown.state && !still_resting {
            self.idle_animation_played = false;
        }
        if !state.is_asleep() && !still_resting {
            self.awake_since = now;
        }
        match (self.shown.state, state) {
            (State::Roam, State::Idle) => self.roam_at = now + BETWEEN_ROAMS,
            (previous, State::Idle) if !previous.is_resting() => {
                self.roam_at = now + FIRST_ROAM;
            }
            _ => {}
        }
        self.shown = shown;
        self.shown_since = now;
        self.overlay = None;
        self.deadline = self.auto_return(state, now);
        self.changed_from(before)
    }

    fn reaction_for(&self, gesture: Gesture, roll: u64) -> Option<Animations> {
        let has = |animation| self.behaviour.reaction_ms.contains_key(&animation);
        let double = roll
            .checked_rem(self.behaviour.react_double.len() as u64)
            .and_then(|index| self.behaviour.react_double.get(index as usize).copied());
        let side = match (gesture, double) {
            (Gesture::Flail(_), Some(double)) => return Some(double),
            (Gesture::Poke(side) | Gesture::Flail(side), _) => side,
        };
        if roll.is_multiple_of(2) && has(Animations::ReactAnnoyed) {
            return Some(Animations::ReactAnnoyed);
        }
        let poke = match side {
            Side::Left => Animations::ReactLeft,
            Side::Right => Animations::ReactRight,
        };
        (has(Animations::ReactLeft) && has(Animations::ReactRight)).then_some(poke)
    }

    fn animation_for(&self, state: State) -> Animations {
        match state {
            State::Working => tier(
                &self.behaviour.working_tiers,
                self.sessions
                    .values()
                    .filter(|session| session.state.is_busy())
                    .count(),
                Animations::Working,
            ),
            State::Juggling => tier(
                &self.behaviour.juggling_tiers,
                self.sessions
                    .values()
                    .filter(|session| session.state == State::Juggling)
                    .map(Session::subagent_count)
                    .sum(),
                Animations::Juggling,
            ),
            other => other.animation(),
        }
    }

    fn min_display(&self, state: State) -> Duration {
        timing(&self.behaviour.min_display_ms, state).unwrap_or_default()
    }

    fn auto_return(&self, state: State, now: Instant) -> Deadline {
        let sleep = self.behaviour.sleep;
        let advance = match state {
            State::Yawning => Some(millis(sleep.yawn_ms)),
            State::Collapsing => sleep.collapse_ms.map(millis),
            State::Waking => Some(millis(sleep.wake_ms)),
            State::MiniEnter => Some(
                self.clip_lengths
                    .get(&Animations::MiniEnter)
                    .copied()
                    .unwrap_or(MINI_ENTER_FALLBACK),
            ),
            _ => None,
        };
        if let Some(delay) = advance {
            return Deadline::AutoReturn { at: now + delay };
        }
        timing(&self.behaviour.auto_return_ms, state).map_or(Deadline::None, |delay| {
            Deadline::AutoReturn { at: now + delay }
        })
    }
}

fn timing(timings: &BTreeMap<Animations, u32>, state: State) -> Option<Duration> {
    timing_of(timings, state.animation())
}

fn timing_of(timings: &BTreeMap<Animations, u32>, animation: Animations) -> Option<Duration> {
    timings.get(&animation).map(|ms| millis(*ms))
}

fn millis(ms: u32) -> Duration {
    Duration::from_millis(u64::from(ms))
}

fn tier(tiers: &[Tier], count: usize, fallback: Animations) -> Animations {
    tiers
        .iter()
        .filter(|tier| count >= tier.min_sessions as usize)
        .max_by_key(|tier| tier.min_sessions)
        .map_or(fallback, |tier| tier.animation)
}

#[cfg(test)]
mod tests {
    use crab_common::atlas::{IdleAnimation, SleepTimings};

    use super::*;

    fn behaviour() -> ThemeBehaviour {
        ThemeBehaviour {
            working_tiers: vec![
                Tier {
                    min_sessions: 1,
                    animation: Animations::Working,
                },
                Tier {
                    min_sessions: 2,
                    animation: Animations::Juggling,
                },
                Tier {
                    min_sessions: 3,
                    animation: Animations::WorkingTier3,
                },
            ],
            juggling_tiers: vec![
                Tier {
                    min_sessions: 1,
                    animation: Animations::Juggling,
                },
                Tier {
                    min_sessions: 2,
                    animation: Animations::JugglingTier2,
                },
            ],
            min_display_ms: BTreeMap::from([
                (Animations::Attention, 4000),
                (Animations::Error, 5000),
                (Animations::Working, 1000),
            ]),
            auto_return_ms: BTreeMap::from([
                (Animations::Attention, 4000),
                (Animations::Error, 5000),
            ]),
            react_double: vec![Animations::ReactDouble1, Animations::ReactDouble2],
            reaction_ms: BTreeMap::from([
                (Animations::ReactLeft, 2500),
                (Animations::ReactRight, 2500),
                (Animations::ReactAnnoyed, 3500),
                (Animations::ReactDouble1, 3500),
                (Animations::ReactDouble2, 3500),
            ]),
            ..ThemeBehaviour::default()
        }
    }

    fn event(session: &str, kind: EventKind) -> AgentEvent {
        AgentEvent {
            agent: Agent::Claude,
            session_id: session.to_owned(),
            kind,
            cwd: None,
            tool_name: None,
            source_pid: None,
            agent_pid: None,
        }
    }

    fn after(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn stop_shows_attention_then_returns_to_idle() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        assert_eq!(
            machine.on_event(&event("a", EventKind::PromptSubmit), start),
            Some(Animations::Thinking)
        );
        assert_eq!(
            machine.on_event(&event("a", EventKind::Stop), start),
            Some(Animations::Attention)
        );
        assert_eq!(machine.deadline(), Some(after(start, 4000)));
        assert_eq!(machine.on_deadline(after(start, 3999), 0), None);
        assert_eq!(
            machine.on_deadline(after(start, 4000), 0),
            Some(Animations::Idle)
        );
        assert_eq!(machine.deadline(), Some(after(start, 600_000)));
    }

    #[test]
    fn minimum_display_delays_the_next_state() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event("a", EventKind::ToolStart), start);
        assert_eq!(
            machine.on_event(&event("a", EventKind::ToolFailure), after(start, 200)),
            None
        );
        assert_eq!(machine.deadline(), Some(after(start, 1000)));
        assert_eq!(
            machine.on_deadline(after(start, 1000), 0),
            Some(Animations::Error)
        );
        assert_eq!(machine.deadline(), Some(after(start, 6000)));
    }

    #[test]
    fn lower_priority_does_not_replace_a_pending_state() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event("a", EventKind::ToolStart), start);
        machine.on_event(&event("a", EventKind::ToolFailure), start);
        machine.on_event(&event("b", EventKind::PromptSubmit), start);
        assert_eq!(
            machine.on_deadline(after(start, 1000), 0),
            Some(Animations::Error)
        );
    }

    #[test]
    fn higher_priority_session_wins() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event("a", EventKind::SubagentStart { id: None }), start);
        assert_eq!(
            machine.on_event(&event("b", EventKind::PromptSubmit), start),
            None
        );
        assert_eq!(machine.animation(), Animations::Juggling);
        assert_eq!(
            machine.on_event(&event("a", EventKind::SessionEnd), start),
            Some(Animations::Thinking)
        );
    }

    #[test]
    fn working_tier_follows_busy_sessions() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event("a", EventKind::ToolStart), start);
        machine.on_event(&event("b", EventKind::PromptSubmit), after(start, 1000));
        assert_eq!(machine.animation(), Animations::Juggling);
        assert_eq!(
            machine.on_event(&event("c", EventKind::ToolStart), after(start, 2000)),
            Some(Animations::WorkingTier3)
        );
    }

    #[test]
    fn juggling_tier_counts_subagents_once() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        let start_subagent = |id: Option<&str>| {
            event(
                "a",
                EventKind::SubagentStart {
                    id: id.map(str::to_owned),
                },
            )
        };
        machine.on_event(&start_subagent(None), start);
        machine.on_event(&start_subagent(Some("x")), start);
        assert_eq!(machine.animation(), Animations::Juggling);
        assert_eq!(
            machine.on_event(&start_subagent(Some("y")), start),
            Some(Animations::JugglingTier2)
        );
        let stop_x = event(
            "a",
            EventKind::SubagentStop {
                id: Some("x".to_owned()),
            },
        );
        assert_eq!(machine.on_event(&stop_x, start), Some(Animations::Juggling));
    }

    #[test]
    fn poke_plays_while_idle_then_returns() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        assert_eq!(
            machine.react(Gesture::Poke(Side::Left), 1, start),
            Some(Animations::ReactLeft)
        );
        assert_eq!(machine.react(Gesture::Poke(Side::Right), 1, start), None);
        assert_eq!(
            machine.on_deadline(after(start, 2500), 0),
            Some(Animations::Idle)
        );
        assert_eq!(
            machine.react(Gesture::Poke(Side::Right), 0, start),
            Some(Animations::ReactAnnoyed)
        );
    }

    #[test]
    fn flail_picks_a_double_reaction() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        assert_eq!(
            machine.react(Gesture::Flail(Side::Left), 3, start),
            Some(Animations::ReactDouble2)
        );
    }

    #[test]
    fn no_reaction_unless_idle() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event("a", EventKind::PromptSubmit), start);
        assert_eq!(machine.react(Gesture::Poke(Side::Left), 1, start), None);
    }

    #[test]
    fn state_change_cancels_the_reaction() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.react(Gesture::Poke(Side::Left), 1, start);
        assert_eq!(
            machine.on_event(&event("a", EventKind::SessionStart { source: None }), start),
            None
        );
        assert_eq!(machine.animation(), Animations::ReactLeft);
        assert_eq!(
            machine.on_event(&event("a", EventKind::PromptSubmit), start),
            Some(Animations::Thinking)
        );
        assert_eq!(machine.deadline(), Some(after(start, 300_000)));
    }

    fn sleepy() -> ThemeBehaviour {
        ThemeBehaviour {
            idle_pool: vec![
                IdleAnimation {
                    animation: Animations::IdlePool1,
                    duration_ms: 6500,
                },
                IdleAnimation {
                    animation: Animations::IdlePool2,
                    duration_ms: 13500,
                },
            ],
            sleep: SleepTimings {
                mode: SleepMode::Full,
                idle_after_ms: 20_000,
                yawn_after_ms: 60_000,
                deep_sleep_after_ms: 600_000,
                yawn_ms: 3000,
                collapse_ms: Some(1000),
                wake_ms: 1500,
            },
            ..behaviour()
        }
    }

    fn step(machine: &mut StateMachine, roll: u64) -> Option<(Instant, Option<Animations>)> {
        let at = machine.deadline()?;
        Some((at, machine.on_deadline(at, roll)))
    }

    #[test]
    fn idle_animation_plays_once_after_stillness() {
        let start = Instant::now();
        let mut machine = StateMachine::new(sleepy(), BTreeMap::new(), false, start);
        machine.on_user_idle(start);
        assert_eq!(
            step(&mut machine, 1),
            Some((after(start, 20_000), Some(Animations::IdlePool2)))
        );
        assert_eq!(
            step(&mut machine, 1),
            Some((after(start, 33_500), Some(Animations::Idle)))
        );
        assert_eq!(machine.deadline(), Some(after(start, 60_000)));
    }

    #[test]
    fn direct_sleep_skips_yawning_and_waking() {
        let start = Instant::now();
        let mut behaviour = sleepy();
        behaviour.idle_pool.clear();
        behaviour.sleep.mode = SleepMode::Direct;
        let mut machine = StateMachine::new(behaviour, BTreeMap::new(), false, start);
        machine.on_user_idle(start);
        assert_eq!(
            step(&mut machine, 0),
            Some((after(start, 60_000), Some(Animations::Sleeping)))
        );
        assert_eq!(machine.deadline(), None);
        assert_eq!(
            machine.on_user_active(after(start, 700_000)),
            Some(Animations::Idle)
        );
    }

    #[test]
    fn full_sleep_sequence_and_waking() {
        let start = Instant::now();
        let mut machine = StateMachine::new(sleepy(), BTreeMap::new(), false, start);
        machine.on_user_idle(start);
        let expected = [
            (20_000, Animations::IdlePool1),
            (26_500, Animations::Idle),
            (60_000, Animations::Yawning),
            (63_000, Animations::Dozing),
            (600_000, Animations::Collapsing),
            (601_000, Animations::Sleeping),
        ];
        for (ms, animation) in expected {
            assert_eq!(
                step(&mut machine, 0),
                Some((after(start, ms), Some(animation)))
            );
        }
        assert_eq!(machine.deadline(), None);
        assert_eq!(
            machine.on_user_active(after(start, 700_000)),
            Some(Animations::Waking)
        );
        assert_eq!(
            step(&mut machine, 0),
            Some((after(start, 701_500), Some(Animations::Idle)))
        );
    }

    #[test]
    fn activity_while_dozing_returns_to_idle() {
        let start = Instant::now();
        let mut machine = StateMachine::new(sleepy(), BTreeMap::new(), false, start);
        machine.on_user_idle(start);
        while machine.animation() != Animations::Dozing {
            if step(&mut machine, 0).is_none() {
                break;
            }
        }
        assert_eq!(machine.animation(), Animations::Dozing);
        assert_eq!(
            machine.on_user_active(after(start, 70_000)),
            Some(Animations::Idle)
        );
        assert_eq!(machine.deadline(), None);
    }

    #[test]
    fn stillness_counts_from_becoming_idle() {
        let start = Instant::now();
        let mut machine = StateMachine::new(sleepy(), BTreeMap::new(), false, start);
        machine.on_user_idle(start);
        machine.on_event(&event("a", EventKind::PromptSubmit), start);
        machine.on_event(&event("a", EventKind::Stop), after(start, 30_000));
        assert_eq!(
            step(&mut machine, 0),
            Some((after(start, 34_000), Some(Animations::Idle)))
        );
        assert_eq!(machine.deadline(), Some(after(start, 54_000)));
    }

    fn event_from(session: &str, kind: EventKind, pid: u32) -> AgentEvent {
        AgentEvent {
            agent_pid: Some(pid),
            ..event(session, kind)
        }
    }

    #[test]
    fn process_exit_drops_its_sessions() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event_from("a", EventKind::ToolStart, 10), start);
        machine.on_event(&event_from("b", EventKind::PromptSubmit, 11), start);
        machine.on_deadline(after(start, 1000), 0);
        assert_eq!(machine.on_process_exit(12, after(start, 2000)), None);
        assert_eq!(
            machine.on_process_exit(10, after(start, 2000)),
            Some(Animations::Thinking)
        );
        assert_eq!(
            machine.on_process_exit(11, after(start, 3000)),
            Some(Animations::Idle)
        );
    }

    #[test]
    fn quiet_busy_session_goes_idle() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event_from("a", EventKind::ToolStart, 10), start);
        assert_eq!(machine.deadline(), Some(after(start, 300_000)));
        assert_eq!(
            machine.on_deadline(after(start, 300_000), 0),
            Some(Animations::Idle)
        );
        assert_eq!(machine.deadline(), None);
    }

    #[test]
    fn unwatched_idle_session_is_dropped() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), false, start);
        machine.on_event(&event("a", EventKind::SessionStart { source: None }), start);
        assert_eq!(machine.deadline(), Some(after(start, 600_000)));
        machine.on_deadline(after(start, 600_000), 0);
        assert!(machine.sessions.is_empty());
        assert_eq!(machine.deadline(), None);
    }

    #[test]
    fn recovery_keeps_the_crab_awake_until_agents_exit() {
        let start = Instant::now();
        let mut machine = StateMachine::new(sleepy(), BTreeMap::new(), false, start);
        machine.recover([7, 8], start);
        machine.on_user_idle(start);
        assert_eq!(machine.deadline(), Some(after(start, 300_000)));
        machine.on_process_exit(7, after(start, 1000));
        assert_eq!(machine.deadline(), Some(after(start, 300_000)));
        machine.on_process_exit(8, after(start, 1000));
        assert_eq!(machine.deadline(), Some(after(start, 20_000)));
    }

    #[test]
    fn recovery_ends_at_the_limit() {
        let start = Instant::now();
        let mut machine = StateMachine::new(sleepy(), BTreeMap::new(), false, start);
        machine.recover([7], start);
        machine.on_user_idle(start);
        assert_eq!(
            machine.on_deadline(after(start, 300_000), 0),
            Some(Animations::Yawning)
        );
    }

    fn docked(start: Instant) -> StateMachine {
        let mut behaviour = behaviour();
        behaviour.min_display_ms.insert(Animations::MiniAlert, 4000);
        behaviour.auto_return_ms.insert(Animations::MiniAlert, 4000);
        let lengths = BTreeMap::from([(Animations::MiniEnter, Duration::from_millis(1000))]);
        let mut machine = StateMachine::new(behaviour, lengths, false, start);
        assert_eq!(machine.dock(start), Some(Animations::MiniEnter));
        machine
    }

    #[test]
    fn docking_enters_then_rests() {
        let start = Instant::now();
        let mut machine = docked(start);
        assert_eq!(
            machine.on_event(&event("a", EventKind::PromptSubmit), start),
            None
        );
        assert_eq!(
            step(&mut machine, 0),
            Some((after(start, 1000), Some(Animations::MiniIdle)))
        );
    }

    #[test]
    fn docked_states_map_to_mini_art() {
        let start = Instant::now();
        let mut machine = docked(start);
        step(&mut machine, 0);
        assert_eq!(
            machine.on_event(&event("a", EventKind::ToolStart), after(start, 1000)),
            Some(Animations::MiniWorking)
        );
        assert_eq!(
            machine.on_event(&event("a", EventKind::Notification), after(start, 2000)),
            Some(Animations::MiniAlert)
        );
        assert_eq!(
            machine.on_event(&event("a", EventKind::SessionEnd), after(start, 3000)),
            None
        );
        assert_eq!(
            machine.on_deadline(after(start, 6000), 0),
            Some(Animations::MiniIdle)
        );
    }

    #[test]
    fn hover_peeks_and_undock_resolves() {
        let start = Instant::now();
        let mut machine = docked(start);
        assert_eq!(machine.hover(true, start), None);
        step(&mut machine, 0);
        assert_eq!(
            machine.hover(true, after(start, 1000)),
            Some(Animations::MiniPeek)
        );
        assert_eq!(
            machine.hover(false, after(start, 1100)),
            Some(Animations::MiniIdle)
        );
        machine.on_event(&event("a", EventKind::PromptSubmit), after(start, 1200));
        assert_eq!(
            machine.undock(after(start, 1300)),
            Some(Animations::Thinking)
        );
    }

    #[test]
    fn roam_starts_after_idle_and_repeats() {
        let start = Instant::now();
        let mut machine = StateMachine::new(behaviour(), BTreeMap::new(), true, start);
        assert_eq!(
            step(&mut machine, 0),
            Some((after(start, 8000), Some(Animations::Roam)))
        );
        assert_eq!(
            machine.on_event(
                &event("a", EventKind::SessionStart { source: None }),
                after(start, 9000)
            ),
            None
        );
        assert!(machine.is_roaming());
        assert_eq!(
            machine.end_roam(after(start, 10_000)),
            Some(Animations::Idle)
        );
        assert_eq!(
            step(&mut machine, 0),
            Some((after(start, 14_000), Some(Animations::Roam)))
        );
        assert_eq!(
            machine.on_event(&event("a", EventKind::ToolStart), after(start, 15_000)),
            Some(Animations::Working)
        );
        assert!(!machine.is_roaming());
    }

    #[test]
    fn roaming_does_not_keep_the_crab_awake() {
        let start = Instant::now();
        let mut machine = StateMachine::new(sleepy(), BTreeMap::new(), true, start);
        machine.on_user_idle(start);
        let mut yawned_at = None;
        while let Some((at, animation)) = step(&mut machine, 0) {
            if machine.is_roaming() {
                machine.end_roam(at);
            }
            if animation == Some(Animations::Yawning) {
                yawned_at = Some(at);
                break;
            }
        }
        assert_eq!(yawned_at, Some(after(start, 60_000)));
    }

    #[test]
    fn dizzy_only_while_idle_and_active() {
        let start = Instant::now();
        let mut behaviour = behaviour();
        behaviour.auto_return_ms.insert(Animations::Dizzy, 6000);
        let mut machine = StateMachine::new(behaviour, BTreeMap::new(), false, start);
        assert!(machine.dizzy_armed());
        machine.on_user_idle(start);
        assert!(!machine.dizzy_armed());
        machine.on_user_active(start);
        assert_eq!(machine.dizzy(start), Some(Animations::Dizzy));
        assert_eq!(machine.dizzy(start), None);
        assert_eq!(
            step(&mut machine, 0),
            Some((after(start, 6000), Some(Animations::Idle)))
        );
    }
}
