//! One progress bar and a time estimate for the whole import, built from the
//! per-phase events. The phases run one after the other, so each phase alone
//! would restart the bar.

use std::time::{Duration, Instant};

use tauri_plugin_import_notify::{Overall, Phase};

use crate::scan::ingest::LEAD_WINDOW;

/// Items the library holds, counted after the walk. They give the phases that
/// have not started an expected size, so the bar does not fill early.
#[derive(Clone, Copy, Default)]
pub struct Expect {
    pub photos: u64,
    pub videos: u64,
}

/// Phases that report a total, in run order.
const COUNTED: [Phase; 5] = [
    Phase::Metadata,
    Phase::FirstThumbnails,
    Phase::Previews,
    Phase::Thumbnails,
    Phase::Videos,
];

/// Time of one item, relative to one metadata read. A preview decodes a whole
/// image, and a video poster starts a decoder.
fn weight(phase: Phase) -> u64 {
    match phase {
        Phase::Metadata => 1,
        Phase::FirstThumbnails | Phase::Thumbnails => 3,
        Phase::Previews => 10,
        Phase::Videos => 40,
        Phase::Finding => 0,
    }
}

/// The bar never shows full: the notification ends when the import ends, and a
/// phase without an estimate (thumbnail stragglers) can finish early on paper.
const MAX_PERMILLE: u16 = 990;
/// A video poster costs this many previews, until its own speed is measured.
const VIDEO_COST_FACTOR: f64 = 4.0;
/// The estimate needs a phase that ran this long and finished this many items.
const MIN_MEASURED: Duration = Duration::from_secs(15);
const MIN_ITEMS: u64 = 10;
/// A speed sample spans at least this long, so burst updates do not skew it.
const RATE_WINDOW: Duration = Duration::from_secs(2);
const RATE_SMOOTHING: f64 = 0.3;

#[derive(Default)]
struct Slot {
    done: u64,
    total: Option<u64>,
    started: Option<Instant>,
    sample: Option<(Instant, u64)>,
    /// Items per second, smoothed.
    rate: Option<f64>,
}

impl Slot {
    fn record(&mut self, done: u64, total: Option<u64>, now: Instant) {
        // A new pass restarts at 0 or has another total. Metadata reports from
        // parallel workers, so a smaller count with the same total is only late.
        let new_pass = (done == 0 && self.done > 0) || self.total.is_some_and(|t| Some(t) != total);
        if new_pass {
            *self = Slot::default();
        } else if done < self.done {
            return;
        }
        self.done = done;
        self.total = total;
        self.started.get_or_insert(now);
        let Some((at, was)) = self.sample else {
            self.sample = Some((now, done));
            return;
        };
        let span = now.duration_since(at);
        if span >= RATE_WINDOW {
            let rate = (done - was) as f64 / span.as_secs_f64();
            self.rate = Some(
                self.rate
                    .map_or(rate, |old| old + RATE_SMOOTHING * (rate - old)),
            );
            self.sample = Some((now, done));
        }
    }

    /// Seconds per item, once the speed is steady enough to trust.
    fn seconds_per_item(&self, now: Instant) -> Option<f64> {
        let ran = self
            .started
            .is_some_and(|s| now.duration_since(s) >= MIN_MEASURED);
        self.rate
            .filter(|&rate| ran && self.done >= MIN_ITEMS && rate > 0.0)
            .map(|rate| 1.0 / rate)
    }
}

#[derive(Default)]
pub struct Tracker {
    slots: [Slot; 5],
    expect: Option<Expect>,
    /// What the bar and the estimate showed last. They only move forward, or by a full step.
    shown_permille: u16,
    shown_minutes: Option<u32>,
}

fn slot_index(phase: Phase) -> Option<usize> {
    COUNTED.iter().position(|&p| p == phase)
}

impl Tracker {
    pub fn expect(&mut self, expect: Expect) {
        self.expect = Some(expect);
    }

    pub fn update(&mut self, phase: Phase, done: u64, total: Option<u64>, now: Instant) -> Overall {
        let Some(index) = slot_index(phase) else {
            return Overall::default();
        };
        self.slots[index].record(done, total, now);
        let minutes = self
            .seconds_left(phase, now)
            .map(|secs| self.settle(secs / 60.0));
        if minutes.is_none() {
            self.shown_minutes = None;
        }
        Overall {
            permille: self.permille(),
            eta_minutes: minutes,
        }
    }

    fn slot(&self, phase: Phase) -> &Slot {
        &self.slots[slot_index(phase).expect("a counted phase")]
    }

    /// Size of a phase: the real total once the phase reports it.
    fn total(&self, phase: Phase) -> u64 {
        let expect = self.expect.unwrap_or_default();
        self.slot(phase).total.unwrap_or(match phase {
            Phase::FirstThumbnails => expect.photos.min(LEAD_WINDOW as u64),
            Phase::Previews => expect.photos,
            Phase::Videos => expect.videos,
            _ => 0,
        })
    }

    fn remaining(&self, phase: Phase) -> u64 {
        self.total(phase).saturating_sub(self.slot(phase).done)
    }

    fn permille(&mut self) -> Option<u16> {
        self.expect?;
        let (mut done, mut all) = (0, 0);
        for phase in COUNTED {
            let total = self.total(phase);
            done += self.slot(phase).done.min(total) * weight(phase);
            all += total * weight(phase);
        }
        if all == 0 {
            return None;
        }
        // A total that grows or a pass that restarts must not pull the bar back.
        let now = ((done * 1000 / all) as u16).min(MAX_PERMILLE);
        self.shown_permille = self.shown_permille.max(now);
        Some(self.shown_permille)
    }

    /// Only the preview and video phases take long enough to estimate.
    fn seconds_left(&self, phase: Phase, now: Instant) -> Option<f64> {
        let preview = self.slot(Phase::Previews).seconds_per_item(now);
        let video = self
            .slot(Phase::Videos)
            .seconds_per_item(now)
            .or(preview.map(|secs| secs * VIDEO_COST_FACTOR));
        let videos = match self.remaining(Phase::Videos) {
            0 => 0.0,
            left => left as f64 * video?,
        };
        match phase {
            Phase::Previews => Some(self.remaining(Phase::Previews) as f64 * preview? + videos),
            Phase::Videos => Some(videos),
            _ => None,
        }
    }

    /// Rounds to a coarse step and keeps the shown value until the estimate
    /// moves by a full step, so the text does not flicker.
    fn settle(&mut self, minutes: f64) -> u32 {
        let step = |shown: u32| match shown {
            0..=9 => 1.0,
            10..=59 => 5.0,
            _ => 60.0,
        };
        let rounded = match minutes {
            m if m < 1.0 => 0,
            m if m < 10.0 => m.round() as u32,
            m if m < 60.0 => (m / 5.0).round() as u32 * 5,
            m => (m / 60.0).round() as u32 * 60,
        };
        let shown = match self.shown_minutes {
            Some(shown) if (minutes - f64::from(shown)).abs() < step(shown) => shown,
            _ => rounded,
        };
        self.shown_minutes = Some(shown);
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(photos: u64, videos: u64) -> (Tracker, Instant) {
        let mut tracker = Tracker::default();
        tracker.expect(Expect { photos, videos });
        (tracker, Instant::now())
    }

    /// Runs a phase at a steady speed and returns the last answer.
    fn run(
        tracker: &mut Tracker,
        phase: Phase,
        start: Instant,
        per_second: u64,
        seconds: u64,
        total: u64,
    ) -> Overall {
        let mut last = Overall::default();
        for s in 0..=seconds {
            let at = start + Duration::from_secs(s);
            last = tracker.update(phase, (per_second * s).min(total), Some(total), at);
        }
        last
    }

    #[test]
    fn the_walk_has_no_bar_and_no_estimate() {
        let (mut tracker, now) = fresh(100, 0);
        let overall = tracker.update(Phase::Finding, 40, None, now);
        assert_eq!((overall.permille, overall.eta_minutes), (None, None));
    }

    #[test]
    fn a_phase_that_has_not_started_keeps_the_bar_short() {
        let (mut tracker, now) = fresh(100, 0);
        let permille = tracker
            .update(Phase::Metadata, 100, Some(100), now)
            .permille;
        // 100 of 100 + 60 * 3 + 100 * 10 cost units.
        assert!(matches!(permille, Some(1..=300)), "{permille:?}");
    }

    #[test]
    fn the_bar_never_goes_back() {
        let (mut tracker, now) = fresh(100, 0);
        let mut last = 0;
        let steps = [
            (Phase::Metadata, 100, 100),
            (Phase::Previews, 0, 100),
            (Phase::Previews, 100, 100),
            // A second preview pass starts over and a straggler pass finds more.
            (Phase::Previews, 0, 300),
            (Phase::Thumbnails, 0, 500),
        ];
        for (phase, done, total) in steps {
            let permille = tracker
                .update(phase, done, Some(total), now)
                .permille
                .unwrap();
            assert!(
                permille >= last,
                "{phase:?} {done}/{total}: {permille} < {last}"
            );
            last = permille;
        }
    }

    #[test]
    fn the_bar_stays_below_full_when_every_phase_is_complete() {
        let (mut tracker, now) = fresh(100, 5);
        let mut permille = None;
        for phase in COUNTED {
            permille = tracker.update(phase, 100, Some(100), now).permille;
        }
        assert_eq!(permille, Some(990));
    }

    #[test]
    fn a_late_count_does_not_start_a_new_pass() {
        let (mut tracker, now) = fresh(100, 0);
        tracker.update(Phase::Metadata, 60, Some(100), now);
        tracker.update(Phase::Metadata, 40, Some(100), now);
        assert_eq!(tracker.slot(Phase::Metadata).done, 60);
    }

    #[test]
    fn a_restart_or_a_new_total_starts_a_new_pass() {
        let (mut tracker, now) = fresh(100, 0);
        tracker.update(Phase::Previews, 60, Some(100), now);
        tracker.update(Phase::Previews, 0, Some(100), now);
        assert_eq!(tracker.slot(Phase::Previews).done, 0);
        tracker.update(Phase::Previews, 30, Some(100), now);
        tracker.update(Phase::Previews, 10, Some(50), now);
        assert_eq!(tracker.slot(Phase::Previews).done, 10);
    }

    #[test]
    fn no_estimate_before_the_preview_speed_is_known() {
        let (mut tracker, now) = fresh(1000, 0);
        let early = run(&mut tracker, Phase::Previews, now, 2, 14, 1000);
        assert_eq!(early.eta_minutes, None, "too short");
        let (mut idle, now) = fresh(1000, 0);
        let stalled = run(&mut idle, Phase::Previews, now, 0, 20, 1000);
        assert_eq!(stalled.eta_minutes, None, "no new items, rate 0");
    }

    #[test]
    fn no_estimate_in_the_short_phases() {
        let (mut tracker, now) = fresh(1000, 0);
        let overall = run(&mut tracker, Phase::Metadata, now, 5, 60, 1000);
        assert_eq!(overall.eta_minutes, None);
        let overall = run(&mut tracker, Phase::FirstThumbnails, now, 1, 60, 60);
        assert_eq!(overall.eta_minutes, None);
    }

    #[test]
    fn the_estimate_adds_the_videos_that_wait() {
        let (mut plain, now) = fresh(1000, 0);
        // 2 per second, 940 left: 470 s.
        let overall = run(&mut plain, Phase::Previews, now, 2, 30, 1000);
        assert_eq!(overall.eta_minutes, Some(8));
        let (mut with_videos, now) = fresh(1000, 30);
        // Each poster costs 4 previews, 2 s: 60 s more.
        let overall = run(&mut with_videos, Phase::Previews, now, 2, 30, 1000);
        assert_eq!(overall.eta_minutes, Some(9));
    }

    #[test]
    fn the_estimate_stays_while_it_moves_less_than_a_step() {
        let (mut tracker, _) = fresh(1, 0);
        assert_eq!(tracker.settle(8.4), 8);
        assert_eq!(tracker.settle(8.9), 8);
        assert_eq!(tracker.settle(9.1), 9);
        assert_eq!(tracker.settle(0.4), 0);
        assert_eq!(tracker.settle(25.0), 25);
        assert_eq!(tracker.settle(27.0), 25);
        assert_eq!(tracker.settle(95.0), 120);
    }
}
