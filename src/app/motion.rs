use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ratatui::buffer::Buffer;

use crate::animation::{
    Area, BLOOM, DIALOG, Edge, FOCUS, Glide, MENU, NOTICE, PANEL, PROGRESS, SCREEN, SPLASH_DRAW,
    SPLASH_EXIT, TAB, Tween,
};

use super::Panel;

/// How long a status message stays before it fades (`UX.md` §1.22).
pub const NOTICE_LIFE: Duration = Duration::from_secs(8);

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
    bloom: Tween,
    workspace: Workspace,
    tab: Option<(usize, PathBuf)>,
    previous: Buffer,
    /// A menu or a list of suggestions opens line by line (`MENU`).
    menu: Tween,
    menu_open: bool,
    /// The border of the panel with the focus is traced from its corner (`FOCUS`).
    focus: Tween,
    focused: Option<u8>,
    /// The status message: when it came, and its entrance (`NOTICE`).
    notice: Tween,
    notice_text: String,
    notice_since: Instant,
    /// A panel dropped somewhere else: where it was, then its way to the new place (`GLIDE`).
    glide_from: Option<(Panel, Area)>,
    glide: Option<(Panel, Glide)>,
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
            bloom: Tween::new(now, 1.0, BLOOM),
            workspace: Workspace::Editor,
            tab: None,
            previous: Buffer::default(),
            menu: Tween::new(now, 1.0, MENU),
            menu_open: false,
            focus: Tween::new(now, 1.0, FOCUS),
            focused: None,
            notice: Tween::new(now, 1.0, NOTICE),
            notice_text: String::new(),
            notice_since: now,
            glide_from: None,
            glide: None,
        }
    }

    pub fn now(&self) -> Instant {
        self.now
    }

    /// A menu that was closed opens from its first line.
    pub fn sync_menu(&mut self, open: bool) {
        if open && !self.menu_open {
            self.menu.restart(self.now, MENU);
        }
        self.menu_open = open;
    }

    /// How much of an open menu is drawn, from 0 to 1.
    pub fn menu(&self) -> f32 {
        self.menu.value(self.now)
    }

    /// Another panel takes the focus: its border is traced again. The first one is not.
    pub fn sync_focus(&mut self, panel: u8) {
        if self.focused.is_some_and(|focused| focused != panel) {
            self.focus.restart(self.now, FOCUS);
        }
        self.focused = Some(panel);
    }

    /// How much of the focused border is traced, from 0 to 1.
    pub fn focus(&self) -> f32 {
        self.focus.value(self.now)
    }

    /// A new status message comes in from the left.
    pub fn sync_notice(&mut self, text: &str) {
        if text != self.notice_text {
            self.notice_text = text.to_owned();
            self.notice_since = self.now;
            if !text.is_empty() {
                self.notice.restart(self.now, NOTICE);
            }
        }
    }

    /// A click on the status bar: the last message comes in again, for another while.
    pub fn replay_notice(&mut self) {
        self.notice_since = self.now;
        self.notice.restart(self.now, NOTICE);
    }

    /// The status message: how much has come in (0 to 1) and how much has faded (0 to 1,
    /// gone at 1) after `NOTICE_LIFE`.
    pub fn notice(&self) -> (f32, f32) {
        let age = self.now.saturating_duration_since(self.notice_since);
        let fading = age.checked_sub(NOTICE_LIFE).map_or(0.0, |late| {
            if NOTICE.close.is_zero() {
                1.0
            } else {
                (late.as_secs_f32() / NOTICE.close.as_secs_f32()).min(1.0)
            }
        });
        (self.notice.value(self.now), fading)
    }

    /// A panel is about to move: where it is now.
    pub fn glide_panel(&mut self, panel: Panel, from: Area) {
        self.glide_from = Some((panel, from));
    }

    /// Where `panel` was placed this frame; after a drop it glides there from where it was.
    pub fn place_panel(&mut self, panel: Panel, area: Area) {
        if let Some((moved, from)) = self.glide_from
            && moved == panel
        {
            self.glide_from = None;
            let mut glide = Glide::new(self.now, from);
            glide.set_target(area, self.now);
            self.glide = Some((panel, glide));
        }
    }

    /// The outline of a panel on its way to its new place, while it goes.
    pub fn gliding(&self) -> Option<Area> {
        let (_, glide) = self.glide.as_ref()?;
        (!glide.is_finished(self.now)).then(|| glide.value(self.now))
    }

    pub fn tick(&mut self, now: Instant) {
        self.now = now;
    }

    /// The panels where they are going, for the tests that click them.
    #[cfg(test)]
    pub fn settle_panels(&mut self, explorer: bool, bottom_terminal: bool, right_terminal: bool) {
        let settled = |shown: bool| Tween::new(self.now, f32::from(u8::from(shown)), PANEL);
        self.explorer = settled(explorer);
        self.bottom_terminal = settled(bottom_terminal);
        self.right_terminal = settled(right_terminal);
        self.overlay = Tween::new(self.now, self.overlay.target(), DIALOG);
        // A dropped panel is already in its place: no outline on its way (E76).
        self.glide = None;
        self.glide_from = None;
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

    /// How far the About flower has grown, from 0 to 1.
    pub fn bloom(&self) -> f32 {
        self.bloom.value(self.now)
    }

    pub fn restart_bloom(&mut self, now: Instant) {
        self.bloom.restart(now, BLOOM);
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
    fn menus_focus_messages_and_dropped_panels_move_and_settle() {
        use crate::animation::GLIDE;
        let start = Instant::now();
        let mut motion = AppMotion::new(start, false);
        motion.sync_menu(true);
        assert_eq!(motion.menu(), 0.0);
        motion.tick(start + MENU.open / 2);
        motion.sync_menu(true);
        assert!(motion.menu() > 0.0 && motion.menu() < 1.0);
        motion.tick(start + MENU.open);
        assert_eq!(motion.menu(), 1.0);

        // The first focus is not traced; a change of panel is.
        motion.sync_focus(1);
        assert_eq!(motion.focus(), 1.0);
        motion.sync_focus(2);
        assert_eq!(motion.focus(), 0.0);
        motion.tick(start + MENU.open + FOCUS.open);
        assert_eq!(motion.focus(), 1.0);

        let said = start + MENU.open + FOCUS.open;
        motion.sync_notice("Saved");
        assert_eq!(motion.notice(), (0.0, 0.0));
        motion.tick(said + NOTICE.open);
        assert_eq!(motion.notice(), (1.0, 0.0));
        motion.tick(said + NOTICE_LIFE + NOTICE.close / 2);
        let (_, fading) = motion.notice();
        assert!(fading > 0.0 && fading < 1.0);
        motion.tick(said + NOTICE_LIFE + NOTICE.close);
        assert_eq!(motion.notice().1, 1.0);
        // The same text again is not a new message.
        motion.sync_notice("Saved");
        assert_eq!(motion.notice().1, 1.0);

        let dropped = said + NOTICE_LIFE + NOTICE.close;
        let (from, to) = (
            Area {
                x: 0,
                y: 0,
                width: 20,
                height: 10,
            },
            Area {
                x: 60,
                y: 0,
                width: 20,
                height: 10,
            },
        );
        motion.place_panel(Panel::Explorer, to);
        assert!(motion.gliding().is_none());
        motion.glide_panel(Panel::Explorer, from);
        motion.place_panel(Panel::Editor, to);
        assert!(motion.gliding().is_none());
        motion.place_panel(Panel::Explorer, to);
        assert_eq!(motion.gliding(), Some(from));
        motion.tick(dropped + GLIDE.open / 2);
        assert_eq!(motion.gliding().unwrap().x, 30);
        motion.tick(dropped + GLIDE.open);
        assert!(motion.gliding().is_none());
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
