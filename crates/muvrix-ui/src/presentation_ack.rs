//! Token-ordered presentation acknowledgement tracking.
//!
//! `mpv_render_context_report_swap()` is a presentation acknowledgement, not an
//! EGL-submit notification. On Wayland, `eglSwapBuffers()` returns before the
//! compositor presents, so acknowledging immediately is premature. This module
//! maps rendered-frame tokens to `wp_presentation_feedback` results and keeps
//! the acknowledgement in strict FIFO order.
//!
//! It is deliberately platform-neutral so the ordering, discard, timeout and
//! backlog rules can be tested without a compositor. The Wayland runner is the
//! only production owner.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A frame whose buffer has been committed but whose presentation has not yet
/// been acknowledged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingAck {
    token: u64,
    queued_at: Instant,
}

/// What the caller must do with a retired token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckAction {
    /// The frame reached compositor presentation. Call `report_swap()` once for
    /// this token.
    Report { token: u64 },
    /// The frame was discarded, failed to swap, timed out, or was evicted by the
    /// in-flight cap. Do not call `report_swap()`.
    Drop { token: u64 },
}

/// Bounded FIFO of unpresented committed frames.
///
/// Rules:
/// * `enqueue` adds a newly committed frame and evicts the oldest token when the
///   in-flight cap is reached.
/// * `presented` retires all tokens from the head through the matching token in
///   strict order. This is the only path that emits [`AckAction::Report`].
/// * `discarded`/`failed` remove the matching token without reporting it.
/// * `expire` retires timed-out head entries without reporting them, keeping the
///   watchdog below libmpv's 200 ms flip timeout.
#[derive(Debug, Clone)]
pub struct PresentationAckQueue {
    pending: VecDeque<PendingAck>,
    max_in_flight: usize,
    timeout: Duration,
}

impl PresentationAckQueue {
    /// Creates a queue. A zero cap is treated as one so an accidental
    /// configuration cannot disable the guard.
    pub fn new(max_in_flight: usize, timeout: Duration) -> Self {
        Self {
            pending: VecDeque::with_capacity(max_in_flight.max(1)),
            max_in_flight: max_in_flight.max(1),
            timeout,
        }
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Enqueues a newly committed frame token.
    ///
    /// Duplicate tokens are ignored (a caller can only commit one frame per
    /// token). When the cap is full, the oldest frame is retired with
    /// [`AckAction::Drop`] before the new one is added.
    pub fn enqueue(&mut self, token: u64, now: Instant) -> Vec<AckAction> {
        if self.pending.iter().any(|p| p.token == token) {
            return Vec::new();
        }
        let mut actions = Vec::new();
        while self.pending.len() >= self.max_in_flight {
            if let Some(old) = self.pending.pop_front() {
                actions.push(AckAction::Drop { token: old.token });
            }
        }
        self.pending.push_back(PendingAck {
            token,
            queued_at: now,
        });
        actions
    }

    /// A compositor `presented` event for `token`.
    ///
    /// Every pending entry up to and including the matching token is retired
    /// with [`AckAction::Report`], in FIFO order. Entries newer than the match
    /// remain pending. An unknown token is a late/duplicate feedback event and
    /// produces no action.
    pub fn presented(&mut self, token: u64) -> Vec<AckAction> {
        let Some(pos) = self.pending.iter().position(|p| p.token == token) else {
            return Vec::new();
        };
        let mut actions = Vec::with_capacity(pos + 1);
        for _ in 0..=pos {
            if let Some(old) = self.pending.pop_front() {
                actions.push(AckAction::Report { token: old.token });
            }
        }
        actions
    }

    /// A compositor `discarded` event for `token`.
    ///
    /// The discarded frame is retired without a presentation report. Older
    /// pending frames are left in place; they can still be presented later.
    pub fn discarded(&mut self, token: u64) -> Vec<AckAction> {
        self.remove_without_report(token)
    }

    /// An EGL swap failed: the frame was never committed, so drop its token
    /// without reporting.
    pub fn failed(&mut self, token: u64) -> Vec<AckAction> {
        self.remove_without_report(token)
    }

    /// Retires all timed-out entries at the head.
    ///
    /// The queue is FIFO by queued time, so once the head is fresh every later
    /// entry is fresher. Timed-out frames are dropped, never reported: there is
    /// no evidence they reached presentation.
    pub fn expire(&mut self, now: Instant) -> Vec<AckAction> {
        let mut actions = Vec::new();
        while let Some(front) = self.pending.front() {
            let Some(deadline) = front.queued_at.checked_add(self.timeout) else {
                break;
            };
            if now < deadline {
                break;
            }
            if let Some(old) = self.pending.pop_front() {
                actions.push(AckAction::Drop { token: old.token });
            }
        }
        actions
    }

    /// Earliest watchdog deadline, if any frame is pending.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending
            .front()
            .and_then(|p| p.queued_at.checked_add(self.timeout))
    }

    /// Retires every pending token without reporting it (teardown).
    pub fn clear(&mut self) -> Vec<AckAction> {
        let mut actions = Vec::with_capacity(self.pending.len());
        while let Some(old) = self.pending.pop_front() {
            actions.push(AckAction::Drop { token: old.token });
        }
        actions
    }

    fn remove_without_report(&mut self, token: u64) -> Vec<AckAction> {
        let Some(pos) = self.pending.iter().position(|p| p.token == token) else {
            return Vec::new();
        };
        self.pending.remove(pos);
        vec![AckAction::Drop { token }]
    }
}

impl Default for PresentationAckQueue {
    fn default() -> Self {
        // Four frames is ~66 ms at 60 Hz: enough for normal compositor feedback
        // latency, short enough to bound a stalled compositor. The timeout is
        // deliberately below libmpv's 200 ms VO flip timeout.
        Self::new(4, Duration::from_millis(150))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn queue() -> PresentationAckQueue {
        PresentationAckQueue::new(4, Duration::from_millis(150))
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn presented_retires_older_tokens_in_strict_fifo_order() {
        let base = Instant::now();
        let mut q = queue();
        assert!(q.enqueue(10, at(base, 0)).is_empty());
        assert!(q.enqueue(11, at(base, 10)).is_empty());
        assert!(q.enqueue(12, at(base, 20)).is_empty());

        let actions = q.presented(11);
        assert_eq!(
            actions,
            vec![
                AckAction::Report { token: 10 },
                AckAction::Report { token: 11 }
            ]
        );
        assert_eq!(q.len(), 1);
        assert_eq!(q.presented(12), vec![AckAction::Report { token: 12 }]);
        assert!(q.is_empty());
    }

    #[test]
    fn out_of_order_feedback_still_retires_in_fifo_order() {
        let base = Instant::now();
        let mut q = queue();
        for token in 1..=4 {
            q.enqueue(token, at(base, token));
        }
        let actions = q.presented(4);
        assert_eq!(
            actions,
            vec![
                AckAction::Report { token: 1 },
                AckAction::Report { token: 2 },
                AckAction::Report { token: 3 },
                AckAction::Report { token: 4 }
            ]
        );
        assert!(q.is_empty());
    }

    #[test]
    fn discarded_token_is_not_reported_and_does_not_retire_older_frames() {
        let base = Instant::now();
        let mut q = queue();
        q.enqueue(20, at(base, 0));
        q.enqueue(21, at(base, 10));
        q.enqueue(22, at(base, 20));

        assert_eq!(q.discarded(21), vec![AckAction::Drop { token: 21 }]);
        assert_eq!(q.len(), 2);
        assert_eq!(
            q.presented(22),
            vec![
                AckAction::Report { token: 20 },
                AckAction::Report { token: 22 }
            ]
        );
    }

    #[test]
    fn failed_swap_drops_only_its_token() {
        let base = Instant::now();
        let mut q = queue();
        q.enqueue(30, at(base, 0));
        q.enqueue(31, at(base, 10));
        assert_eq!(q.failed(31), vec![AckAction::Drop { token: 31 }]);
        assert_eq!(q.presented(30), vec![AckAction::Report { token: 30 }]);
        assert!(q.is_empty());
    }

    #[test]
    fn timeout_drops_without_reporting() {
        let base = Instant::now();
        let mut q = PresentationAckQueue::new(4, Duration::from_millis(150));
        q.enqueue(40, at(base, 0));
        q.enqueue(41, at(base, 100));

        assert!(q.expire(at(base, 149)).is_empty());
        assert_eq!(q.expire(at(base, 150)), vec![AckAction::Drop { token: 40 }]);
        assert_eq!(q.len(), 1);
        assert_eq!(q.expire(at(base, 250)), vec![AckAction::Drop { token: 41 }]);
        assert!(q.is_empty());
    }

    #[test]
    fn in_flight_cap_evicts_oldest_without_reporting() {
        let base = Instant::now();
        let mut q = PresentationAckQueue::new(2, Duration::from_millis(150));
        q.enqueue(50, at(base, 0));
        q.enqueue(51, at(base, 1));
        let actions = q.enqueue(52, at(base, 2));
        assert_eq!(actions, vec![AckAction::Drop { token: 50 }]);
        assert_eq!(q.len(), 2);
        assert_eq!(
            q.presented(52),
            vec![
                AckAction::Report { token: 51 },
                AckAction::Report { token: 52 }
            ]
        );
    }

    #[test]
    fn duplicate_tokens_are_ignored() {
        let base = Instant::now();
        let mut q = queue();
        q.enqueue(60, at(base, 0));
        assert!(q.enqueue(60, at(base, 1)).is_empty());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn unknown_feedback_is_a_no_op() {
        let base = Instant::now();
        let mut q = queue();
        q.enqueue(70, at(base, 0));
        assert!(q.presented(999).is_empty());
        assert!(q.discarded(999).is_empty());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn next_deadline_is_the_head_timeout() {
        let base = Instant::now();
        let mut q = queue();
        assert_eq!(q.next_deadline(), None);
        q.enqueue(80, at(base, 10));
        q.enqueue(81, at(base, 20));
        assert_eq!(q.next_deadline(), Some(at(base, 160)));
    }

    #[test]
    fn clear_retires_everything_without_reporting() {
        let base = Instant::now();
        let mut q = queue();
        q.enqueue(90, at(base, 0));
        q.enqueue(91, at(base, 1));
        assert_eq!(
            q.clear(),
            vec![AckAction::Drop { token: 90 }, AckAction::Drop { token: 91 }]
        );
        assert!(q.is_empty());
    }
}
