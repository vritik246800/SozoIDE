use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ratatui::buffer::Buffer;

use crate::animation::{
    DIALOG, Edge, PANEL, PROGRESS, SCREEN, SPLASH_DRAW, SPLASH_EXIT, TAB, Tween,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Editor,
    Git,
    Database,
}

pub struct AppMotion {
    now: Instant,
    started: Instant,
    explorer: Tween,
    bottom_terminal: Tween,
    right_terminal: Tween,
    content: Tween,
    edge: Edge,
    overlay: Tween,
    search: Tween,
    splash_draw: Tween,
    splash_exit: Tween,
    download: Tween,
    workspace: Workspace,
    tab: Option<(usize, PathBuf)>,
    previous: Buffer,
}

impl AppMotion {
    pub fn new(now: Instant, splash: bool) -> Self {
        let mut splash_draw = Tween::new(now, 1.0, SPLASH_DRAW);
        if splash {
            splash_draw.restart(now, SPLASH_DRAW);
        }
        Self {
            now,
            started: now,
            explorer: Tween::new(now, 1.0, PANEL),
            bottom_terminal: Tween::new(now, 0.0, PANEL),
            right_terminal: Tween::new(now, 0.0, PANEL),
            content: Tween::new(now, 1.0, SCREEN),
            edge: Edge::Top,
            overlay: Tween::new(now, 0.0, DIALOG),
            search: Tween::new(now, 0.0, DIALOG),
            splash_draw,
            splash_exit: Tween::new(now, 0.0, SPLASH_EXIT),
            download: Tween::new(now, 0.0, PROGRESS),
            workspace: Workspace::Editor,
            tab: None,
            previous: Buffer::default(),
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

    /// Another workspace is drawn line by line from the top; another file comes in from the
    /// side its tab is on (`DESIGN.md` §1.3).
    pub fn sync_content(&mut self, git: bool, database: bool, tab: Option<(usize, &Path)>) {
        let workspace = if database {
            Workspace::Database
        } else if git {
            Workspace::Git
        } else {
            Workspace::Editor
        };
        let edge = if self.workspace != workspace {
            Edge::Top
        } else {
            match (&mut self.tab, tab) {
                // Closing a tab on its left moves it without changing what is on screen.
                (Some((index, old)), Some((to, new))) if old.as_path() == new => {
                    *index = to;
                    return;
                }
                (Some((from, _)), Some((to, _))) => Edge::between(*from, to),
                (None, None) => return,
                _ => Edge::Top,
            }
        };
        self.edge = edge;
        self.content
            .restart(self.now, if edge == Edge::Top { SCREEN } else { TAB });
        self.workspace = workspace;
        self.tab = tab.map(|(index, path)| (index, path.to_path_buf()));
    }

    pub fn sync_download(&mut self, ratio: f32) {
        self.download.set_target(ratio, self.now);
    }

    /// Keeps the frame just drawn: transitions show it wherever the new content has not
    /// arrived yet.
    pub fn remember(&mut self, frame: &Buffer) {
        self.previous.clone_from(frame);
    }

    pub fn previous(&self) -> &Buffer {
        &self.previous
    }

    pub fn elapsed(&self) -> Duration {
        self.now.saturating_duration_since(self.started)
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

    pub fn content_edge(&self) -> Edge {
        self.edge
    }

    pub fn overlay(&self) -> f32 {
        self.overlay.value(self.now)
    }

    pub fn search(&self) -> f32 {
        self.search.value(self.now)
    }

    pub fn download(&self) -> f32 {
        self.download.value(self.now)
    }

    /// How much of the splash is printed, line by line.
    pub fn splash_draw(&self) -> f32 {
        self.splash_draw.value(self.now)
    }

    /// How much of the IDE is drawn over the splash, line by line from the top.
    pub fn splash_exit(&self) -> f32 {
        self.splash_exit.value(self.now)
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
        self.splash_exit.set_target(1.0, now);
    }

    pub fn splash_closing(&self) -> bool {
        self.splash_exit.target() == 1.0
    }

    pub fn splash_closed(&self) -> bool {
        self.splash_closing() && self.splash_exit.is_finished(self.now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_comes_in_from_the_workspace_top_or_the_side_of_its_tab() {
        let start = Instant::now();
        let mut motion = AppMotion::new(start, false);
        let (first, second) = (Path::new("a.rs"), Path::new("b.rs"));

        motion.sync_content(false, false, Some((0, first)));
        assert_eq!((motion.content(), motion.content_edge()), (0.0, Edge::Top));
        motion.tick(start + SCREEN.open);
        assert_eq!(motion.content(), 1.0);

        motion.sync_content(false, false, Some((1, second)));
        assert_eq!(
            (motion.content(), motion.content_edge()),
            (0.0, Edge::Right)
        );
        motion.tick(start + SCREEN.open + TAB.open);
        assert_eq!(motion.content(), 1.0);

        motion.sync_content(false, false, Some((0, second)));
        assert_eq!(motion.content(), 1.0);

        motion.sync_content(false, false, Some((1, first)));
        motion.sync_content(false, false, Some((0, second)));
        assert_eq!(motion.content_edge(), Edge::Left);

        motion.sync_content(true, false, Some((0, second)));
        assert_eq!((motion.content(), motion.content_edge()), (0.0, Edge::Top));
    }

    #[test]
    fn the_splash_prints_then_the_ide_is_drawn_over_it() {
        let start = Instant::now();
        let mut motion = AppMotion::new(start, true);
        assert_eq!(motion.splash_draw(), 0.0);
        motion.tick(start + SPLASH_DRAW.open / 2);
        assert!((motion.splash_draw() - 0.5).abs() < 0.01);
        motion.tick(start + SPLASH_DRAW.open);
        assert_eq!(motion.splash_draw(), 1.0);
        assert_eq!(motion.splash_exit(), 0.0);

        let closing = start + SPLASH_DRAW.open;
        motion.close_splash(closing);
        assert!(motion.splash_closing() && !motion.splash_closed());
        motion.tick(closing + SPLASH_EXIT.open / 2);
        assert!((motion.splash_exit() - 0.5).abs() < 0.01);
        motion.tick(closing + SPLASH_EXIT.open);
        assert!(motion.splash_closed());
        assert_eq!(AppMotion::new(start, false).splash_draw(), 1.0);
    }

    #[test]
    fn the_download_bar_follows_the_progress() {
        let start = Instant::now();
        let mut motion = AppMotion::new(start, false);
        motion.sync_download(0.5);
        assert_eq!(motion.download(), 0.0);
        motion.tick(start + PROGRESS.open / 2);
        let halfway = motion.download();
        assert!(halfway > 0.25 && halfway < 0.5);
        motion.sync_download(0.8);
        assert_eq!(motion.download(), halfway);
        motion.tick(start + PROGRESS.open / 2 + PROGRESS.open);
        assert_eq!(motion.download(), 0.8);
    }
}
