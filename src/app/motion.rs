use std::time::Instant;

use crate::animation::{CONTENT_DURATION, DIALOG_DURATION, PANEL_DURATION, Tween};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Editor,
    Git,
    Database,
}

pub struct AppMotion {
    now: Instant,
    explorer: Tween,
    bottom_terminal: Tween,
    right_terminal: Tween,
    content: Tween,
    overlay: Tween,
    search: Tween,
    splash: Tween,
    workspace: Workspace,
    active_tab: Option<usize>,
}

impl AppMotion {
    pub fn new(now: Instant, splash: bool) -> Self {
        let mut splash_motion = Tween::new(now, 0.0, DIALOG_DURATION);
        if splash {
            splash_motion.set_target(1.0, now);
        }
        Self {
            now,
            explorer: Tween::new(now, 1.0, PANEL_DURATION),
            bottom_terminal: Tween::new(now, 0.0, PANEL_DURATION),
            right_terminal: Tween::new(now, 0.0, PANEL_DURATION),
            content: Tween::new(now, 1.0, CONTENT_DURATION),
            overlay: Tween::new(now, 0.0, DIALOG_DURATION),
            search: Tween::new(now, 0.0, DIALOG_DURATION),
            splash: splash_motion,
            workspace: Workspace::Editor,
            active_tab: None,
        }
    }

    pub fn tick(&mut self, now: Instant) {
        self.now = now;
    }

    pub fn sync_panels(&mut self, explorer: bool, bottom_terminal: bool, right_terminal: bool) {
        let now = self.now;
        self.explorer
            .set_target(if explorer { 1.0 } else { 0.0 }, now);
        self.bottom_terminal
            .set_target(if bottom_terminal { 1.0 } else { 0.0 }, now);
        self.right_terminal
            .set_target(if right_terminal { 1.0 } else { 0.0 }, now);
    }

    pub fn sync_content(&mut self, git: bool, database: bool, active_tab: Option<usize>) {
        let workspace = if database {
            Workspace::Database
        } else if git {
            Workspace::Git
        } else {
            Workspace::Editor
        };
        if self.workspace != workspace || self.active_tab != active_tab {
            self.content.restart(self.now);
            self.workspace = workspace;
            self.active_tab = active_tab;
        }
    }

    pub fn explorer(&self) -> f32 {
        self.explorer.value(self.now)
    }

    pub fn bottom_terminal(&self) -> f32 {
        self.bottom_terminal.value(self.now)
    }

    pub fn right_terminal(&self) -> f32 {
        self.right_terminal.value(self.now)
    }

    pub fn content(&self) -> f32 {
        self.content.value(self.now)
    }

    pub fn overlay(&self) -> f32 {
        self.overlay.value(self.now)
    }

    pub fn search(&self) -> f32 {
        self.search.value(self.now)
    }

    pub fn splash(&self) -> f32 {
        self.splash.value(self.now)
    }

    pub fn bottom_terminal_stable(&self) -> bool {
        self.bottom_terminal.target() == 1.0 && self.bottom_terminal.is_finished(self.now)
    }

    pub fn right_terminal_stable(&self) -> bool {
        self.right_terminal.target() == 1.0 && self.right_terminal.is_finished(self.now)
    }

    pub fn open_overlay(&mut self, now: Instant) {
        self.tick(now);
        self.overlay.set_target(1.0, now);
    }

    pub fn close_overlay(&mut self, now: Instant) {
        self.tick(now);
        self.overlay.set_target(0.0, now);
    }

    pub fn overlay_closing(&self) -> bool {
        self.overlay.target() == 0.0
    }

    pub fn overlay_closed(&self) -> bool {
        self.overlay_closing() && self.overlay.is_finished(self.now)
    }

    pub fn open_search(&mut self, now: Instant) {
        self.tick(now);
        self.search.set_target(1.0, now);
    }

    pub fn close_search(&mut self, now: Instant) {
        self.tick(now);
        self.search.set_target(0.0, now);
    }

    pub fn search_closing(&self) -> bool {
        self.search.target() == 0.0
    }

    pub fn search_closed(&self) -> bool {
        self.search_closing() && self.search.is_finished(self.now)
    }

    pub fn close_splash(&mut self, now: Instant) {
        self.tick(now);
        self.splash.set_target(0.0, now);
    }

    pub fn splash_closing(&self) -> bool {
        self.splash.target() == 0.0
    }

    pub fn splash_closed(&self) -> bool {
        self.splash_closing() && self.splash.is_finished(self.now)
    }
}
