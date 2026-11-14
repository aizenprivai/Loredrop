//! Frame-time measurement for the performance overlay.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Rolling record of recent frame and tick times.
pub struct FrameStats {
    frames: VecDeque<(Instant, Duration)>,
    ticks: VecDeque<Instant>,
}

/// Summary of the last second.
#[derive(Debug, Clone, Copy, Default)]
pub struct StatsSummary {
    pub fps: u32,
    pub avg_ms: f32,
    pub worst_ms: f32,
    pub tps: u32,
}

impl FrameStats {
    pub fn new() -> Self {
        Self { frames: VecDeque::new(), ticks: VecDeque::new() }
    }

    pub fn record_frame(&mut self, now: Instant, dt: Duration) {
        self.frames.push_back((now, dt));
        Self::trim(&mut self.frames, now, |f| f.0);
    }

    pub fn record_tick(&mut self, now: Instant) {
        self.ticks.push_back(now);
        Self::trim(&mut self.ticks, now, |t| *t);
    }

    fn trim<T>(q: &mut VecDeque<T>, now: Instant, at: impl Fn(&T) -> Instant) {
        while q.front().is_some_and(|f| now.duration_since(at(f)) > Duration::from_secs(1)) {
            q.pop_front();
        }
    }

    pub fn summary(&self) -> StatsSummary {
        let n = self.frames.len().max(1) as f32;
        let total: f32 = self.frames.iter().map(|f| f.1.as_secs_f32()).sum();
        let worst = self.frames.iter().map(|f| f.1.as_secs_f32()).fold(0.0, f32::max);
        StatsSummary {
            fps: self.frames.len() as u32,
            avg_ms: total / n * 1000.0,
            worst_ms: worst * 1000.0,
            tps: self.ticks.len() as u32,
        }
    }
}
