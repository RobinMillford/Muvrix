//! WP3 — explicit presentation state machine.
//!
//! The Phase 2.1 telemetry reported `state=presented` with
//! `unavailable_episodes=0` across a ~242 s gap in which no swap happened. That
//! defect exists because the reported state was a *label*, not a model: it was
//! never transitioned by the events that actually prove presentation.
//!
//! This module replaces the label with an explicit state machine driven by
//! authoritative events. Two rules matter more than the state list itself:
//!
//! * a state that has not transitioned is not stale by definition. A healthy
//!   `presenting` state can sit unchanged for minutes while frames keep
//!   arriving; `state_age_s` alone must never be read as failure. Freshness is
//!   therefore measured by *independent* ages: [`PresentationStateMachine::state_age_s`]
//!   (since the last transition), [`PresentationStateMachine::last_present_age_s`]
//!   (since the last successful compositor handoff),
//!   [`PresentationStateMachine::last_callback_age_s`] and
//!   [`PresentationStateMachine::last_swap_age_s`].
//! * a generic long frame gap does **not** prove a Wayland surface failure.
//!   [`PresentationState::Stalled`] is its own state and is never promoted to
//!   [`PresentationState::SurfaceUnavailable`] or
//!   [`PresentationState::DeviceLost`] without an explicit event saying so.
//!
//! Every method takes `now` as an argument. Nothing in this module reads the
//! wall clock, so all of it is deterministically testable without sleeping.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Default threshold after which a `presenting`/`awaiting_frame` state with no
/// successful handoff is reported as [`PresentationState::Stalled`].
pub const DEFAULT_STALL_THRESHOLD: Duration = Duration::from_millis(1_000);

/// Number of transitions retained for diagnostics. The full history is not
/// needed; the last few are enough to explain a stall from a CSV plus log.
const TRANSITION_HISTORY: usize = 32;

/// The seven states the Phase 2.2 contract requires (deliverable 04 §2, CSV
/// `state` column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationState {
    /// A frame was recently handed to the compositor and presentation is live.
    Presenting,
    /// The engine expects a frame and is waiting for it.
    AwaitingFrame,
    /// No frame is expected (paused, EOF, idle), and the reason is known.
    NoFrameExpected,
    /// Frames stopped progressing beyond the stall threshold. Not a failure
    /// classification: it is the absence of progress, with cause still unknown.
    Stalled,
    /// The surface/swap chain is known to be unavailable.
    SurfaceUnavailable,
    /// The GL/EGL device context is known to be lost.
    DeviceLost,
    /// Terminal failure for this run.
    Failed,
}

impl PresentationState {
    /// Stable token written to the `state` CSV column.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Presenting => "presenting",
            Self::AwaitingFrame => "awaiting_frame",
            Self::NoFrameExpected => "no_frame_expected",
            Self::Stalled => "stalled",
            Self::SurfaceUnavailable => "surface_unavailable",
            Self::DeviceLost => "device_lost",
            Self::Failed => "failed",
        }
    }

    /// Stable numeric token for lock-free publication to the telemetry worker.
    ///
    /// Must stay in sync with [`PresentationState::from_code`].
    pub fn as_code(self) -> u8 {
        match self {
            Self::Presenting => 0,
            Self::AwaitingFrame => 1,
            Self::NoFrameExpected => 2,
            Self::Stalled => 3,
            Self::SurfaceUnavailable => 4,
            Self::DeviceLost => 5,
            Self::Failed => 6,
        }
    }

    /// Inverse of [`PresentationState::as_code`]; unknown codes map to
    /// `AwaitingFrame` (the honest "no claim yet" state).
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Presenting,
            2 => Self::NoFrameExpected,
            3 => Self::Stalled,
            4 => Self::SurfaceUnavailable,
            5 => Self::DeviceLost,
            6 => Self::Failed,
            _ => Self::AwaitingFrame,
        }
    }

    /// Whether this state belongs to the *unavailable family* that drives
    /// `unavailable_episodes` / `unavailable_recoveries`.
    ///
    /// `Stalled` deliberately does not: an unattributed stall is not evidence of
    /// a surface or device failure.
    pub fn is_unavailable_family(self) -> bool {
        matches!(
            self,
            Self::SurfaceUnavailable | Self::DeviceLost | Self::Failed
        )
    }
}

/// An authoritative event that can move the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateEvent {
    PlaybackStarted,
    PlaybackPaused,
    PlaybackResumed,
    PlaybackEof,
    /// A frame is expected (engine signalled work, or a decode is in flight).
    FrameExpected,
    /// The mpv update callback fired.
    UpdateCallback,
    /// A frame was rendered into the working target.
    FrameRendered,
    /// `eglSwapBuffers` was called.
    SwapAttempted,
    /// `eglSwapBuffers` returned success: the buffer is the compositor's.
    SwapSucceeded,
    /// `eglSwapBuffers` returned failure.
    SwapFailed,
    /// The surface is known to be unavailable.
    SurfaceUnavailable,
    /// The GL/EGL device context is known to be lost.
    DeviceLost,
    /// Presentation was intentionally suspended by the windowing layer (for
    /// example the window became occluded). No frame is expected and the run is
    /// not stalled or failed.
    PresentationSuspended,
    /// Availability returned without a new presentation event.
    Recovery,
    /// Playback terminated (process teardown or fatal error).
    PlaybackTerminated,
}

/// An immutable view of the machine for one telemetry sample.
///
/// Carries the real `Instant` of every event, so a consumer that samples later
/// computes ages from actual event times rather than from its own sample
/// timestamp. Copying this is what lets an independent telemetry thread read
/// presentation state without touching the machine or blocking the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationSnapshot {
    pub state: PresentationState,
    pub state_since: Instant,
    pub last_present_at: Option<Instant>,
    pub last_swap_at: Option<Instant>,
    pub last_callback_at: Option<Instant>,
    pub unavailable_episodes: u64,
    pub unavailable_recoveries: u64,
    pub last_event_t_s: f64,
}

/// One recorded transition, with its monotonic timestamp and cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateTransition {
    pub at: Instant,
    pub from: PresentationState,
    pub to: PresentationState,
    pub cause: StateEvent,
}

impl PresentationSnapshot {
    /// Declares `Stalled` from time alone (Phase 2.5, Part 5).
    ///
    /// The state machine's `tick` needs a paint to run, so a frozen event loop
    /// stayed labelled `presenting` forever. This is the same rule expressed as a
    /// pure function of the snapshot plus a sampling instant, so the telemetry
    /// thread can apply it without GL, mpv or a paint.
    ///
    /// Only states that expect progress can become `stalled`: `paused`, `eof`,
    /// idle/no-media and the unavailable family are left untouched, because their
    /// absence of presentation is intentional or already classified.
    pub fn with_time_aware_state(self, now: Instant, threshold: Duration) -> Self {
        if !matches!(
            self.state,
            PresentationState::Presenting | PresentationState::AwaitingFrame
        ) {
            return self;
        }
        // Progress is measured from the last successful handoff; before any
        // handoff, from when the current state was entered.
        let since = self.last_present_at.unwrap_or(self.state_since);
        if now.saturating_duration_since(since) < threshold {
            return self;
        }
        Self {
            state: PresentationState::Stalled,
            // The stall started when the threshold was crossed, not when this
            // sample happened, so `state_age_s` is honest even if the sample is
            // late (the worker polls at 50 ms, but a blocked sample can be later).
            state_since: since + threshold,
            ..self
        }
    }
}

/// Ages derived from one [`PresentationSnapshot`] at a sampling instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapshotAges {
    pub state_age_s: f64,
    pub last_present_age_s: Option<f64>,
    pub last_swap_age_s: Option<f64>,
    pub last_callback_age_s: Option<f64>,
}

/// Explicit presentation state machine.
#[derive(Debug)]
pub struct PresentationStateMachine {
    state: PresentationState,
    state_since: Instant,
    last_present_at: Option<Instant>,
    last_callback_at: Option<Instant>,
    last_swap_at: Option<Instant>,
    unavailable_episodes: u64,
    unavailable_recoveries: u64,
    rendered_frames_total: u64,
    presented_frames_total: u64,
    update_callbacks_total: u64,
    stall_threshold: Duration,
    transitions: VecDeque<StateTransition>,
    /// `t`-style seconds since construction, used for `state_last_event_ts`.
    started: Instant,
    last_event_t_s: f64,
}

impl PresentationStateMachine {
    /// Creates a machine in [`PresentationState::AwaitingFrame`].
    ///
    /// A freshly constructed engine has not presented anything yet, so
    /// `presenting` would be a claim it cannot support; `awaiting_frame` is the
    /// honest initial state until the first successful swap.
    pub fn new(now: Instant) -> Self {
        Self::with_stall_threshold(now, DEFAULT_STALL_THRESHOLD)
    }

    pub fn with_stall_threshold(now: Instant, stall_threshold: Duration) -> Self {
        Self {
            state: PresentationState::AwaitingFrame,
            state_since: now,
            last_present_at: None,
            last_callback_at: None,
            last_swap_at: None,
            unavailable_episodes: 0,
            unavailable_recoveries: 0,
            rendered_frames_total: 0,
            presented_frames_total: 0,
            update_callbacks_total: 0,
            stall_threshold,
            transitions: VecDeque::with_capacity(TRANSITION_HISTORY),
            started: now,
            last_event_t_s: 0.0,
        }
    }

    pub fn state(&self) -> PresentationState {
        self.state
    }

    pub fn stall_threshold(&self) -> Duration {
        self.stall_threshold
    }

    pub fn unavailable_episodes(&self) -> u64 {
        self.unavailable_episodes
    }

    pub fn unavailable_recoveries(&self) -> u64 {
        self.unavailable_recoveries
    }

    pub fn rendered_frames_total(&self) -> u64 {
        self.rendered_frames_total
    }

    pub fn presented_frames_total(&self) -> u64 {
        self.presented_frames_total
    }

    pub fn update_callbacks_total(&self) -> u64 {
        self.update_callbacks_total
    }

    pub fn last_present_at(&self) -> Option<Instant> {
        self.last_present_at
    }

    pub fn last_callback_at(&self) -> Option<Instant> {
        self.last_callback_at
    }

    pub fn last_swap_at(&self) -> Option<Instant> {
        self.last_swap_at
    }

    pub fn transitions(&self) -> impl Iterator<Item = &StateTransition> {
        self.transitions.iter()
    }

    /// Point-in-time snapshot for telemetry. Never blocks and never mutates.
    pub fn snapshot(&self) -> PresentationSnapshot {
        PresentationSnapshot {
            state: self.state,
            state_since: self.state_since,
            last_present_at: self.last_present_at,
            last_swap_at: self.last_swap_at,
            last_callback_at: self.last_callback_at,
            unavailable_episodes: self.unavailable_episodes,
            unavailable_recoveries: self.unavailable_recoveries,
            last_event_t_s: self.last_event_t_s,
        }
    }

    /// Age helpers that operate on a snapshot rather than on the live machine.
    pub fn snapshot_ages(snapshot: &PresentationSnapshot, now: Instant) -> SnapshotAges {
        let age =
            |at: Option<Instant>| at.map(|at| now.saturating_duration_since(at).as_secs_f64());
        SnapshotAges {
            state_age_s: now
                .saturating_duration_since(snapshot.state_since)
                .as_secs_f64(),
            last_present_age_s: age(snapshot.last_present_at),
            last_swap_age_s: age(snapshot.last_swap_at),
            last_callback_age_s: age(snapshot.last_callback_at),
        }
    }

    /// Seconds since the current state was entered (`state_age_s`).
    pub fn state_age_s(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.state_since)
            .as_secs_f64()
    }

    /// Seconds since the last successful compositor handoff
    /// (`last_present_age_s`). `None` when nothing has been presented yet — a
    /// genuinely unavailable measurement, not zero.
    pub fn last_present_age_s(&self, now: Instant) -> Option<f64> {
        self.last_present_at
            .map(|at| now.saturating_duration_since(at).as_secs_f64())
    }

    pub fn last_callback_age_s(&self, now: Instant) -> Option<f64> {
        self.last_callback_at
            .map(|at| now.saturating_duration_since(at).as_secs_f64())
    }

    pub fn last_swap_age_s(&self, now: Instant) -> Option<f64> {
        self.last_swap_at
            .map(|at| now.saturating_duration_since(at).as_secs_f64())
    }

    /// `t` coordinate of the most recent state transition, in seconds since
    /// construction (CSV `state_last_event_ts`).
    pub fn state_last_event_ts(&self) -> f64 {
        self.last_event_t_s
    }

    /// Applies an authoritative event.
    pub fn on_event(&mut self, event: StateEvent, now: Instant) {
        match event {
            StateEvent::UpdateCallback => {
                self.last_callback_at = Some(now);
                self.update_callbacks_total = self.update_callbacks_total.saturating_add(1);
                // A callback means the engine has work. From a no-frame state
                // that is a real expectation; from `presenting` it is business
                // as usual and must not churn the state.
                if matches!(
                    self.state,
                    PresentationState::NoFrameExpected | PresentationState::Stalled
                ) {
                    self.transition(PresentationState::AwaitingFrame, event, now);
                }
            }
            StateEvent::PlaybackStarted => {
                self.transition(PresentationState::AwaitingFrame, event, now);
            }
            StateEvent::PlaybackResumed => {
                self.transition(PresentationState::AwaitingFrame, event, now);
            }
            StateEvent::PlaybackPaused
            | StateEvent::PlaybackEof
            | StateEvent::PresentationSuspended => {
                // Pause, EOF and occlusion are *expected* absences of frames.
                // They must not be reported as stalls, and they must not start
                // an unavailable episode.
                self.transition(PresentationState::NoFrameExpected, event, now);
            }
            StateEvent::FrameExpected => {
                if matches!(
                    self.state,
                    PresentationState::NoFrameExpected
                        | PresentationState::Stalled
                        | PresentationState::AwaitingFrame
                ) {
                    self.transition(PresentationState::AwaitingFrame, event, now);
                }
            }
            StateEvent::FrameRendered => {
                self.rendered_frames_total = self.rendered_frames_total.saturating_add(1);
            }
            StateEvent::SwapAttempted => {
                self.last_swap_at = Some(now);
            }
            StateEvent::SwapSucceeded => {
                self.last_swap_at = Some(now);
                self.last_present_at = Some(now);
                self.presented_frames_total = self.presented_frames_total.saturating_add(1);
                self.recover_or_present(event, now);
            }
            StateEvent::SwapFailed => {
                self.last_swap_at = Some(now);
                self.enter_unavailable(PresentationState::Failed, event, now);
            }
            StateEvent::SurfaceUnavailable => {
                self.enter_unavailable(PresentationState::SurfaceUnavailable, event, now);
            }
            StateEvent::DeviceLost => {
                self.enter_unavailable(PresentationState::DeviceLost, event, now);
            }
            StateEvent::Recovery => {
                if self.state.is_unavailable_family() {
                    self.unavailable_recoveries = self.unavailable_recoveries.saturating_add(1);
                    self.transition(PresentationState::Presenting, event, now);
                }
            }
            StateEvent::PlaybackTerminated => {
                // `Failed` is unavailable-family: entering it starts an episode
                // (and repeated termination events stay in the same episode).
                self.enter_unavailable(PresentationState::Failed, event, now);
            }
        }
    }

    /// Re-evaluates staleness. Call once per telemetry tick (and cheap enough to
    /// call per frame).
    ///
    /// A stall is declared only from a state that *expects* progress. Paused and
    /// EOF (`no_frame_expected`) never become `stalled`, which is what keeps a
    /// pause from inventing a stall episode.
    pub fn tick(&mut self, now: Instant) {
        if !matches!(
            self.state,
            PresentationState::Presenting | PresentationState::AwaitingFrame
        ) {
            return;
        }
        // The reference point for "progress" is the last successful handoff. If
        // nothing has ever been presented, fall back to when the current state
        // was entered so a never-presenting run still surfaces as stalled.
        let since = self.last_present_at.unwrap_or(self.state_since);
        if now.saturating_duration_since(since) >= self.stall_threshold {
            self.transition(PresentationState::Stalled, StateEvent::FrameExpected, now);
        }
    }

    fn recover_or_present(&mut self, event: StateEvent, now: Instant) {
        if self.state.is_unavailable_family() {
            self.unavailable_recoveries = self.unavailable_recoveries.saturating_add(1);
        }
        self.transition(PresentationState::Presenting, event, now);
    }

    fn enter_unavailable(&mut self, next: PresentationState, event: StateEvent, now: Instant) {
        // Increment on entrance only. Repeated failures of the same kind are the
        // same episode, not new ones.
        if !self.state.is_unavailable_family() {
            self.unavailable_episodes = self.unavailable_episodes.saturating_add(1);
        }
        self.transition(next, event, now);
    }

    fn transition(&mut self, next: PresentationState, cause: StateEvent, now: Instant) {
        let from = self.state;
        self.last_event_t_s = now.saturating_duration_since(self.started).as_secs_f64();
        if from == next {
            return;
        }
        self.state = next;
        self.state_since = now;
        if self.transitions.len() == TRANSITION_HISTORY {
            self.transitions.pop_front();
        }
        self.transitions.push_back(StateTransition {
            at: now,
            from,
            to: next,
            cause,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn machine() -> (Instant, PresentationStateMachine) {
        let base = Instant::now();
        (
            base,
            PresentationStateMachine::with_stall_threshold(base, Duration::from_millis(500)),
        )
    }

    #[test]
    fn normal_presenting() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackStarted, at(base, 0));
        m.on_event(StateEvent::SwapSucceeded, at(base, 16));
        assert_eq!(m.state(), PresentationState::Presenting);
        assert_eq!(m.presented_frames_total(), 1);
        assert_eq!(m.unavailable_episodes(), 0);
        assert!(m.last_present_age_s(at(base, 16)).unwrap() < 0.001);
    }

    #[test]
    fn pause_never_becomes_a_stall() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackStarted, at(base, 0));
        m.on_event(StateEvent::SwapSucceeded, at(base, 16));
        m.on_event(StateEvent::PlaybackPaused, at(base, 100));
        assert_eq!(m.state(), PresentationState::NoFrameExpected);
        // Way past the stall threshold: still not a stall, no episode.
        m.tick(at(base, 60_000));
        assert_eq!(m.state(), PresentationState::NoFrameExpected);
        assert_eq!(m.unavailable_episodes(), 0);
    }

    #[test]
    fn resume_returns_to_presenting() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackPaused, at(base, 10));
        m.on_event(StateEvent::PlaybackResumed, at(base, 20));
        m.on_event(StateEvent::SwapSucceeded, at(base, 30));
        assert_eq!(m.state(), PresentationState::Presenting);
    }

    #[test]
    fn eof_is_no_frame_expected() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackStarted, at(base, 0));
        m.on_event(StateEvent::PlaybackEof, at(base, 50));
        assert_eq!(m.state(), PresentationState::NoFrameExpected);
        m.tick(at(base, 10_000));
        assert_eq!(m.state(), PresentationState::NoFrameExpected);
    }

    #[test]
    fn awaiting_frame_after_callback_from_idle() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackPaused, at(base, 0));
        m.on_event(StateEvent::UpdateCallback, at(base, 10));
        assert_eq!(m.state(), PresentationState::AwaitingFrame);
        assert_eq!(m.update_callbacks_total(), 1);
    }

    #[test]
    fn transition_to_stalled_after_threshold() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackStarted, at(base, 0));
        m.on_event(StateEvent::SwapSucceeded, at(base, 10));
        m.tick(at(base, 400));
        assert_eq!(m.state(), PresentationState::Presenting);
        m.tick(at(base, 600));
        assert_eq!(m.state(), PresentationState::Stalled);
        // A stall is not an unavailable-family state.
        assert_eq!(m.unavailable_episodes(), 0);
    }

    #[test]
    fn surface_unavailable_episode_is_counted_once() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::SurfaceUnavailable, at(base, 10));
        m.on_event(StateEvent::SurfaceUnavailable, at(base, 20));
        assert_eq!(m.state(), PresentationState::SurfaceUnavailable);
        assert_eq!(m.unavailable_episodes(), 1);
        assert_eq!(m.unavailable_recoveries(), 0);
    }

    #[test]
    fn device_lost_enters_unavailable_family() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::DeviceLost, at(base, 10));
        assert_eq!(m.state(), PresentationState::DeviceLost);
        assert!(m.state().is_unavailable_family());
        assert_eq!(m.unavailable_episodes(), 1);
    }

    #[test]
    fn failure_is_terminal_unavailable_family() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackTerminated, at(base, 10));
        assert_eq!(m.state(), PresentationState::Failed);
        assert_eq!(m.unavailable_episodes(), 1);
    }

    #[test]
    fn recovery_increments_recoveries_not_episodes() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::SurfaceUnavailable, at(base, 10));
        m.on_event(StateEvent::Recovery, at(base, 20));
        assert_eq!(m.state(), PresentationState::Presenting);
        assert_eq!(m.unavailable_episodes(), 1);
        assert_eq!(m.unavailable_recoveries(), 1);
    }

    #[test]
    fn multiple_unavailable_episodes() {
        let (base, mut m) = machine();
        for (i, ms) in [10u64, 100, 200, 300, 400, 500].iter().enumerate() {
            if i % 2 == 0 {
                m.on_event(StateEvent::DeviceLost, at(base, *ms));
            } else {
                m.on_event(StateEvent::Recovery, at(base, *ms));
            }
        }
        assert_eq!(m.unavailable_episodes(), 3);
        assert_eq!(m.unavailable_recoveries(), 3);
    }

    #[test]
    fn state_age_is_independent_of_last_present_age() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackStarted, at(base, 0));
        // Keep presenting for 5 s: state does not transition again, but
        // presentation stays fresh.
        for ms in (16..=5_000).step_by(16) {
            m.on_event(StateEvent::SwapSucceeded, at(base, ms));
        }
        m.on_event(StateEvent::SwapSucceeded, at(base, 5_000));
        let state_age = m.state_age_s(at(base, 5_000));
        let present_age = m.last_present_age_s(at(base, 5_000)).unwrap();
        assert!(state_age >= 4.9, "state_age={state_age}");
        assert!(present_age < 0.001, "present_age={present_age}");
        assert_eq!(m.state(), PresentationState::Presenting);
    }

    #[test]
    fn unpresented_run_stalls_instead_of_claiming_presentation() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackStarted, at(base, 0));
        assert_eq!(m.state(), PresentationState::AwaitingFrame);
        assert!(m.last_present_age_s(at(base, 0)).is_none());
        m.tick(at(base, 1_000));
        assert_eq!(m.state(), PresentationState::Stalled);
        assert_eq!(m.unavailable_episodes(), 0);
    }

    #[test]
    fn transitions_record_cause_and_timestamp() {
        let (base, mut m) = machine();
        m.on_event(StateEvent::PlaybackStarted, at(base, 0));
        m.on_event(StateEvent::SwapSucceeded, at(base, 16));
        let last = *m.transitions().last().unwrap();
        assert_eq!(last.from, PresentationState::AwaitingFrame);
        assert_eq!(last.to, PresentationState::Presenting);
        assert_eq!(last.cause, StateEvent::SwapSucceeded);
        assert_eq!(last.at, at(base, 16));
    }

    // --- Phase 2.5, Part 5: time-aware stall without a paint -----------------

    fn snapshot(
        state: PresentationState,
        base: Instant,
        presented_at: Option<Instant>,
    ) -> PresentationSnapshot {
        PresentationSnapshot {
            state,
            state_since: base,
            last_present_at: presented_at,
            last_swap_at: presented_at,
            last_callback_at: presented_at,
            unavailable_episodes: 0,
            unavailable_recoveries: 0,
            last_event_t_s: 0.0,
        }
    }

    #[test]
    fn healthy_presentation_is_not_reclassified() {
        let base = Instant::now();
        let s = snapshot(PresentationState::Presenting, base, Some(at(base, 1000)))
            .with_time_aware_state(at(base, 1016), Duration::from_millis(500));
        assert_eq!(s.state, PresentationState::Presenting);
    }

    #[test]
    fn presenting_becomes_stalled_without_any_paint() {
        let base = Instant::now();
        let s = snapshot(PresentationState::Presenting, base, Some(base))
            .with_time_aware_state(at(base, 900), Duration::from_millis(500));
        assert_eq!(s.state, PresentationState::Stalled);
        // The episode started at the threshold crossing, not at sample time.
        assert_eq!(s.state_since, at(base, 500));
        assert_eq!(s.last_present_at, Some(base));
    }

    #[test]
    fn never_presented_falls_back_to_state_entry() {
        let base = Instant::now();
        let s = snapshot(PresentationState::AwaitingFrame, base, None)
            .with_time_aware_state(at(base, 700), Duration::from_millis(500));
        assert_eq!(s.state, PresentationState::Stalled);
        assert_eq!(s.state_since, at(base, 500));
    }

    #[test]
    fn intentional_states_are_never_stalled() {
        let base = Instant::now();
        // `NoFrameExpected` is how paused/EOF/idle are represented: their
        // absence of presentation is intentional and must never read as a stall.
        for state in [
            PresentationState::NoFrameExpected,
            PresentationState::SurfaceUnavailable,
            PresentationState::DeviceLost,
            PresentationState::Failed,
        ] {
            let s = snapshot(state, base, Some(base))
                .with_time_aware_state(at(base, 60_000), Duration::from_millis(500));
            assert_eq!(s.state, state, "{state:?} must stay {state:?}");
        }
    }

    #[test]
    fn recovery_after_a_present_is_immediate() {
        let base = Instant::now();
        let stalled = snapshot(PresentationState::Presenting, base, Some(base))
            .with_time_aware_state(at(base, 900), Duration::from_millis(500));
        assert_eq!(stalled.state, PresentationState::Stalled);
        // A new successful present produces a fresh snapshot; time-aware
        // classification must then report presenting again.
        let recovered = snapshot(
            PresentationState::Presenting,
            at(base, 905),
            Some(at(base, 905)),
        )
        .with_time_aware_state(at(base, 910), Duration::from_millis(500));
        assert_eq!(recovered.state, PresentationState::Presenting);
    }
}
