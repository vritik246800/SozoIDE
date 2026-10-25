use std::time::{Duration, Instant};

pub const PANEL_DURATION: Duration = Duration::from_millis(160);
pub const CONTENT_DURATION: Duration = Duration::from_millis(120);
pub const DIALOG_DURATION: Duration = Duration::from_millis(140);

#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    started_at: Instant,
    duration: Duration,
}

impl Tween {
    pub fn new(now: Instant, value: f32, duration: Duration) -> Self {
        let value = value.clamp(0.0, 1.0);
        Self {
            from: value,
            to: value,
            started_at: now,
            duration,
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        let progress = self.linear_progress(now);
        self.from + (self.to - self.from) * ease_out_cubic(progress)
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        self.from == self.to || self.linear_progress(now) >= 1.0
    }

    pub fn set_target(&mut self, target: f32, now: Instant) {
        let target = target.clamp(0.0, 1.0);
        if self.to == target {
            return;
        }
        self.from = self.value(now);
        self.to = target;
        self.started_at = now;
    }

    pub fn restart(&mut self, now: Instant) {
        self.from = 0.0;
        self.to = 1.0;
        self.started_at = now;
    }

    fn linear_progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.saturating_duration_since(self.started_at).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
    }
}

pub struct InterfaceMotion<T> {
    now: Instant,
    tab: T,
    content: Tween,
    dialog: Tween,
    operation_started: Option<Instant>,
}

impl<T: Copy + PartialEq> InterfaceMotion<T> {
    pub fn new(now: Instant, tab: T) -> Self {
        Self {
            now,
            tab,
            content: Tween::new(now, 1.0, CONTENT_DURATION),
            dialog: Tween::new(now, 0.0, DIALOG_DURATION),
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
            self.tab = tab;
            self.content.restart(self.now);
        }
    }

    pub fn content(&self) -> f32 {
        self.content.value(self.now)
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
        self.dialog.restart(now);
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
        const FRAMES: [&str; 4] = ["-", "\\", "|", "/"];
        let elapsed = self
            .operation_started
            .map(|start| self.now.saturating_duration_since(start))
            .unwrap_or_default();
        FRAMES[(elapsed.as_millis() / 80) as usize % FRAMES.len()]
    }
}

fn ease_out_cubic(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_has_bounded_initial_intermediate_and_final_values() {
        let start = Instant::now();
        let mut tween = Tween::new(start, 0.0, Duration::from_millis(100));
        tween.set_target(1.0, start);

        assert_eq!(tween.value(start), 0.0);
        let middle = tween.value(start + Duration::from_millis(50));
        assert!(middle > 0.0 && middle < 1.0);
        assert_eq!(tween.value(start + Duration::from_millis(100)), 1.0);
        assert_eq!(tween.value(start + Duration::from_millis(500)), 1.0);
        assert!(tween.is_finished(start + Duration::from_millis(100)));
    }

    #[test]
    fn reversing_starts_at_the_interrupted_value() {
        let start = Instant::now();
        let interrupted = start + Duration::from_millis(60);
        let mut tween = Tween::new(start, 0.0, Duration::from_millis(100));
        tween.set_target(1.0, start);
        let interrupted_value = tween.value(interrupted);

        tween.set_target(0.0, interrupted);

        assert_eq!(tween.value(interrupted), interrupted_value);
        let middle = tween.value(interrupted + Duration::from_millis(50));
        assert!(middle > 0.0 && middle < interrupted_value);
        assert_eq!(tween.value(interrupted + Duration::from_millis(100)), 0.0);
    }

    #[test]
    fn restart_reveals_new_content() {
        let start = Instant::now();
        let mut tween = Tween::new(start, 1.0, CONTENT_DURATION);
        tween.restart(start);

        assert_eq!(tween.value(start), 0.0);
        assert!(tween.value(start + CONTENT_DURATION / 2) > 0.0);
        assert_eq!(tween.value(start + CONTENT_DURATION), 1.0);
    }

    #[test]
    fn interface_motion_coordinates_tabs_dialogs_and_busy_frames() {
        let start = Instant::now();
        let mut motion = InterfaceMotion::new(start, 0);
        motion.tick(start, true);
        motion.sync_tab(1);
        assert_eq!(motion.content(), 0.0);
        assert_eq!(motion.spinner(), "-");

        motion.tick(start + Duration::from_millis(60), true);
        assert!(motion.content() > 0.0 && motion.content() < 1.0);
        motion.open_dialog(start + Duration::from_millis(60));
        assert_eq!(motion.dialog(), 0.0);

        motion.tick(start + Duration::from_millis(120), true);
        assert_eq!(motion.content(), 1.0);
        assert_eq!(motion.spinner(), "\\");
        assert!(motion.dialog() > 0.0 && motion.dialog() < 1.0);

        motion.close_dialog(start + Duration::from_millis(120));
        motion.tick(start + Duration::from_millis(120) + DIALOG_DURATION, false);
        assert!(motion.dialog_closed());
        assert_eq!(motion.dialog(), 0.0);
    }
}
