use std::collections::VecDeque;
use std::time::{Duration, Instant};

const RETRY_AFTER: Duration = Duration::from_secs(1);
const MAX_RETRIES: u8 = 2;
const MAX_QUEUED: usize = 64;

#[derive(Debug)]
pub(crate) struct StopAndWait<T> {
    queued: VecDeque<T>,
    in_flight: Option<InFlight<T>>,
}

#[derive(Debug)]
struct InFlight<T> {
    value: T,
    sent_at: Instant,
    retries: u8,
}

impl<T> Default for StopAndWait<T> {
    fn default() -> Self {
        Self {
            queued: VecDeque::new(),
            in_flight: None,
        }
    }
}

impl<T: Clone> StopAndWait<T> {
    pub fn push(&mut self, value: T, supersedes: impl Fn(&T) -> bool) {
        self.queued.retain(|queued| !supersedes(queued));
        if self.queued.len() == MAX_QUEUED {
            self.queued.pop_front();
        }
        self.queued.push_back(value);
    }

    pub fn complete(&mut self) -> bool {
        self.in_flight.take().is_some()
    }

    pub fn next(&mut self, now: Instant) -> Option<T> {
        if self.in_flight.is_some() {
            return None;
        }
        let value = self.queued.pop_front()?;
        self.in_flight = Some(InFlight {
            value: value.clone(),
            sent_at: now,
            retries: 0,
        });
        Some(value)
    }

    pub fn tick(&mut self, now: Instant) -> Option<T> {
        if let Some(in_flight) = &mut self.in_flight
            && now.saturating_duration_since(in_flight.sent_at) >= RETRY_AFTER
        {
            if in_flight.retries < MAX_RETRIES {
                in_flight.retries += 1;
                in_flight.sent_at = now;
                return Some(in_flight.value.clone());
            }
            self.in_flight = None;
        }
        self.next(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_retries_and_moves_on() {
        let now = Instant::now();
        let mut queue = StopAndWait::default();
        queue.push(1, |_| false);
        queue.push(2, |_| false);
        assert_eq!(queue.next(now), Some(1));
        assert_eq!(queue.next(now), None);
        assert_eq!(queue.tick(now + RETRY_AFTER), Some(1));
        assert_eq!(queue.tick(now + RETRY_AFTER * 2), Some(1));
        assert_eq!(queue.tick(now + RETRY_AFTER * 3), Some(2));
    }

    #[test]
    fn coalesces_pending_slider_changes() {
        let now = Instant::now();
        let mut queue = StopAndWait::default();
        queue.push(0, |_| false);
        assert_eq!(queue.next(now), Some(0));
        for value in [5, 10, 15] {
            queue.push(value, |_| true);
        }
        assert!(queue.complete());
        assert_eq!(queue.next(now), Some(15));
    }
}
