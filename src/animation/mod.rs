use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// How long a tween takes to open and to close, and how it eases (`DESIGN.md` §1.3).
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub open: Duration,
    pub close: Duration,
    ease: fn(f32) -> f32,
}

const fn timing(open: u64, close: u64, ease: fn(f32) -> f32) -> Timing {
    Timing {
        open: Duration::from_millis(open),
        close: Duration::from_millis(close),
        ease,
    }
}

pub const PANEL: Timing = timing(180, 120, ease_out_cubic);
pub const DIALOG: Timing = timing(180, 120, ease_out_cubic);
pub const TAB: Timing = timing(160, 160, ease_out_cubic);
pub const SCREEN: Timing = timing(220, 220, linear);
pub const SPLASH_DRAW: Timing = timing(420, 420, linear);
pub const SPLASH_EXIT: Timing = timing(260, 260, linear);
/// The flower of the About dialog grows from its centre (`DESIGN.md` §1.3).
pub const BLOOM: Timing = timing(1400, 1400, ease_out_cubic);
pub const PROGRESS: Timing = timing(160, 160, ease_out_cubic);
pub const SPINNER_FRAME: Duration = Duration::from_millis(80);
pub const SWEEP_CYCLE: Duration = Duration::from_millis(1200);

/// The side new content comes in from when it replaces what was on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// Line by line from the top: another screen.
    Top,
    /// Column by column from the left: a tab to the left of the previous one.
    Left,
    /// Column by column from the right: a tab to the right of the previous one.
    Right,
}

impl Edge {
    pub fn between<T: PartialOrd>(from: T, to: T) -> Self {
        if to < from { Self::Left } else { Self::Right }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    started_at: Instant,
    duration: Duration,
    timing: Timing,
}

impl Tween {
    pub fn new(now: Instant, value: f32, timing: Timing) -> Self {
        let value = value.clamp(0.0, 1.0);
        Self {
            from: value,
            to: value,
            started_at: now,
            duration: Duration::ZERO,
            timing,
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        let progress = self.linear_progress(now);
        if progress >= 1.0 {
            return self.to;
        }
        self.from + (self.to - self.from) * (self.timing.ease)(progress)
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        self.from == self.to || self.linear_progress(now) >= 1.0
    }

    /// Moves to `target` from wherever the tween is now, so interrupting never jumps.
    pub fn set_target(&mut self, target: f32, now: Instant) {
        let target = target.clamp(0.0, 1.0);
        if self.to == target {
            return;
        }
        self.from = self.value(now);
        self.to = target;
        self.started_at = now;
        self.duration = motion(if target > self.from {
            self.timing.open
        } else {
            self.timing.close
        });
    }

    /// Plays from 0 to 1 again with `timing`.
    pub fn restart(&mut self, now: Instant, timing: Timing) {
        self.timing = timing;
        self.from = 0.0;
        self.to = 1.0;
        self.started_at = now;
        self.duration = motion(timing.open);
    }

    fn linear_progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.saturating_duration_since(self.started_at).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
    }
}

/// `SOZO_REDUCED_MOTION` turns every transition into a cut; spinners and bars keep moving
/// because they show state (`UX.md` §1.3).
fn motion(duration: Duration) -> Duration {
    static REDUCED: OnceLock<bool> = OnceLock::new();
    if *REDUCED.get_or_init(|| std::env::var_os("SOZO_REDUCED_MOTION").is_some()) {
        Duration::ZERO
    } else {
        duration
    }
}

pub struct InterfaceMotion<T> {
    now: Instant,
    tab: T,
    content: Tween,
    edge: Edge,
    dialog: Tween,
    operation_started: Option<Instant>,
}

impl<T: Copy + PartialOrd> InterfaceMotion<T> {
    pub fn new(now: Instant, tab: T) -> Self {
        Self {
            now,
            tab,
            content: Tween::new(now, 1.0, TAB),
            edge: Edge::Right,
            dialog: Tween::new(now, 0.0, DIALOG),
            operation_started: None,
        }
    }

    pub fn tick(&mut self, now: Instant, operation: bool) {
        self.now = now;
        match (operation, self.operation_started) {
            (true, None) => self.operation_started = Some(now),
            (false, Some(_)) => self.operation_started = None,
            _ => {}
        }
    }

    pub fn sync_tab(&mut self, tab: T) {
        if self.tab != tab {
            self.edge = Edge::between(self.tab, tab);
            self.tab = tab;
            self.content.restart(self.now, TAB);
        }
    }

    pub fn content(&self) -> f32 {
        self.content.value(self.now)
    }

    pub fn edge(&self) -> Edge {
        self.edge
    }

    pub fn dialog(&self) -> f32 {
        self.dialog.value(self.now)
    }

    pub fn open_dialog(&mut self, now: Instant) {
        self.now = now;
        self.dialog.set_target(1.0, now);
    }

    pub fn restart_dialog(&mut self, now: Instant) {
        self.now = now;
        self.dialog.restart(now, DIALOG);
    }

    pub fn close_dialog(&mut self, now: Instant) {
        self.now = now;
        self.dialog.set_target(0.0, now);
    }

    pub fn dialog_closing(&self) -> bool {
        self.dialog.target() == 0.0
    }

    pub fn dialog_closed(&self) -> bool {
        self.dialog_closing() && self.dialog.is_finished(self.now)
    }

    pub fn spinner(&self) -> &'static str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let elapsed = self
            .operation_started
            .map(|start| self.now.saturating_duration_since(start))
            .unwrap_or_default();
        FRAMES[(elapsed.as_millis() / SPINNER_FRAME.as_millis()) as usize % FRAMES.len()]
    }
}

/// Where a motion that loops every `period` is after `elapsed`: 0 to 1, eased in and out.
pub fn cycle(elapsed: Duration, period: Duration) -> f32 {
    let phase = (elapsed.as_secs_f32() / period.as_secs_f32()).fract();
    (1.0 - (phase * std::f32::consts::PI).cos()) / 2.0
}

fn ease_out_cubic(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

fn linear(progress: f32) -> f32 {
    progress
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: Timing = timing(100, 100, ease_out_cubic);

    #[test]
    fn tween_has_bounded_initial_intermediate_and_final_values() {
        let start = Instant::now();
        let mut tween = Tween::new(start, 0.0, ONE);
        tween.set_target(1.0, start);

        assert_eq!(tween.value(start), 0.0);
        let middle = tween.value(start + Duration::from_millis(50));
        assert!(middle > 0.5 && middle < 1.0);
        assert_eq!(tween.value(start + Duration::from_millis(100)), 1.0);
        assert_eq!(tween.value(start + Duration::from_millis(500)), 1.0);
        assert!(tween.is_finished(start + Duration::from_millis(100)));
    }

    #[test]
    fn reversing_starts_at_the_interrupted_value() {
        let start = Instant::now();
        let interrupted = start + Duration::from_millis(60);
        let mut tween = Tween::new(start, 0.0, ONE);
        tween.set_target(1.0, start);
        let interrupted_value = tween.value(interrupted);

        tween.set_target(0.0, interrupted);

        assert_eq!(tween.value(interrupted), interrupted_value);
        let middle = tween.value(interrupted + Duration::from_millis(50));
        assert!(middle > 0.0 && middle < interrupted_value);
        assert_eq!(tween.value(interrupted + Duration::from_millis(100)), 0.0);
    }

    #[test]
    fn closing_is_shorter_than_opening() {
        let start = Instant::now();
        let mut tween = Tween::new(start, 0.0, DIALOG);
        tween.set_target(1.0, start);
        assert!(!tween.is_finished(start + DIALOG.close));
        let opened = start + DIALOG.open;
        assert_eq!(tween.value(opened), 1.0);

        tween.set_target(0.0, opened);
        assert!(tween.value(opened + DIALOG.close / 2) > 0.0);
        assert!(tween.is_finished(opened + DIALOG.close));
        assert_eq!(tween.value(opened + DIALOG.close), 0.0);
        assert!(DIALOG.close < DIALOG.open && PANEL.close < PANEL.open);
    }

    #[test]
    fn restart_reveals_new_content_with_its_own_curve() {
        let start = Instant::now();
        let mut tween = Tween::new(start, 1.0, TAB);
        tween.restart(start, TAB);
        assert_eq!(tween.value(start), 0.0);
        assert!(tween.value(start + TAB.open / 2) > 0.5);
        assert_eq!(tween.value(start + TAB.open), 1.0);

        tween.restart(start, SCREEN);
        assert!((tween.value(start + SCREEN.open / 2) - 0.5).abs() < 0.01);
        assert_eq!(tween.value(start + SCREEN.open), 1.0);
    }

    #[test]
    fn tabs_come_in_from_the_side_they_are_on() {
        assert_eq!(Edge::between(2, 5), Edge::Right);
        assert_eq!(Edge::between(5, 2), Edge::Left);
        assert_eq!(Edge::between(3, 3), Edge::Right);
    }

    #[test]
    fn interface_motion_coordinates_tabs_dialogs_and_busy_frames() {
        let start = Instant::now();
        let mut motion = InterfaceMotion::new(start, 1);
        motion.tick(start, true);
        motion.sync_tab(2);
        assert_eq!(motion.content(), 0.0);
        assert_eq!(motion.edge(), Edge::Right);
        assert_eq!(motion.spinner(), "⠋");

        motion.tick(start + TAB.open / 2, true);
        assert!(motion.content() > 0.0 && motion.content() < 1.0);
        assert_eq!(motion.spinner(), "⠙");
        motion.open_dialog(start + TAB.open / 2);
        assert_eq!(motion.dialog(), 0.0);

        motion.tick(start + TAB.open, true);
        assert_eq!(motion.content(), 1.0);
        assert!(motion.dialog() > 0.0 && motion.dialog() < 1.0);

        motion.sync_tab(0);
        assert_eq!(motion.edge(), Edge::Left);

        let closing = start + TAB.open + DIALOG.open;
        motion.close_dialog(closing);
        motion.tick(closing + DIALOG.close, false);
        assert!(motion.dialog_closed());
        assert_eq!(motion.dialog(), 0.0);
    }

    #[test]
    fn spinner_turns_through_the_braille_frames() {
        let start = Instant::now();
        let mut motion = InterfaceMotion::new(start, 0);
        motion.tick(start, true);
        let frames = (0..11)
            .map(|frame| {
                motion.tick(start + SPINNER_FRAME * frame, true);
                motion.spinner()
            })
            .collect::<String>();
        assert_eq!(frames, "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏⠋");
    }

    #[test]
    fn cycle_eases_across_each_period_and_starts_again() {
        assert_eq!(cycle(Duration::ZERO, SWEEP_CYCLE), 0.0);
        assert!((cycle(SWEEP_CYCLE / 2, SWEEP_CYCLE) - 0.5).abs() < 0.001);
        assert!(cycle(SWEEP_CYCLE / 4, SWEEP_CYCLE) < 0.25);
        assert!(cycle(SWEEP_CYCLE * 3 / 4, SWEEP_CYCLE) > 0.75);
        assert!(cycle(SWEEP_CYCLE + SWEEP_CYCLE / 10, SWEEP_CYCLE) < 0.1);
    }
}
