//! The layout from the keyboard (TODO 5.2.3, F39): one panel over the whole window and back
//! (`Ctrl+M`, Q94), the layout SozoIDE came with (`Alt+0`), three layouts kept with a name
//! (`Alt+L`), and the focus moved to the panel beside with `Ctrl+Alt` and an arrow.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use serde::{Deserialize, Serialize};

use super::{App, Focus, Panel, Tile};

/// One layout kept: its name, the tree, which panels show and their lengths.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Layout {
    pub name: String,
    tiles: Tile,
    shown: [bool; 3],
    lengths: [u16; 6],
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct LayoutsFile {
    #[serde(default)]
    layouts: Vec<Option<Layout>>,
}

/// The Layouts dialog: the row chosen and, while one is named, the name typed.
#[derive(Clone, Debug, Default)]
pub struct LayoutMenu {
    pub selected: usize,
    pub naming: Option<String>,
    pub layouts: Vec<Option<Layout>>,
}

/// How many layouts are kept.
pub const LAYOUTS: usize = 3;

/// `layouts.toml` beside `keys.toml`; debug builds (tests, `cargo run`) keep none.
fn layouts_path() -> Option<PathBuf> {
    if cfg!(debug_assertions) || cfg!(test) {
        return None;
    }
    directories::ProjectDirs::from("", "", "SozoIDE")
        .map(|directories| directories.config_dir().join("layouts.toml"))
}

fn read_layouts(path: Option<&std::path::Path>) -> Vec<Option<Layout>> {
    let mut layouts = path
        .map(|path| crate::storage::read::<LayoutsFile>(path).layouts)
        .unwrap_or_default();
    layouts.resize(LAYOUTS, None);
    // A file changed by hand that breaks the tree is left out.
    for layout in &mut layouts {
        if layout
            .as_ref()
            .is_some_and(|layout| !layout.tiles.is_complete())
        {
            *layout = None;
        }
    }
    layouts
}

impl App {
    /// `Ctrl+M`: the panel with the focus over the whole window, or the layout again.
    pub(super) fn toggle_maximized(&mut self) {
        let panel = match self.focus {
            Focus::Explorer => Panel::Explorer,
            Focus::BottomTerminal => Panel::BottomTerminal,
            Focus::RightTerminal => Panel::RightTerminal,
            _ => Panel::Editor,
        };
        self.maximized = match self.maximized {
            Some(_) => None,
            None => Some(panel),
        };
        self.status = match self.maximized {
            Some(panel) => format!(
                "{} fills the window: Ctrl+M brings the others back",
                panel.name()
            ),
            None => "The layout is back".into(),
        };
    }

    /// The tree placed this frame in `area`: one panel when one is maximized; in a small
    /// window the panels give way in order (TODO 5.3.2): the right terminal under 80 columns,
    /// the bottom one under 24 rows, the Explorer under 60 columns. The editor always stays.
    pub fn placed_tiles(&self, area: ratatui::layout::Rect) -> Tile {
        if let Some(panel) = self.maximized {
            return Tile::Panel(panel);
        }
        let mut tiles = self.tiles.clone();
        for (panel, small) in [
            (Panel::RightTerminal, area.width < 80),
            (Panel::BottomTerminal, area.height < 24),
            (Panel::Explorer, area.width < 60),
        ] {
            if small {
                tiles = tiles.leaving_out(panel);
            }
        }
        tiles
    }

    /// `Alt+0`: the layout SozoIDE came with, every panel where it started.
    pub(super) fn reset_layout(&mut self) {
        self.tiles = Tile::default();
        self.maximized = None;
        (
            self.explorer_width,
            self.right_terminal_width,
            self.bottom_terminal_height,
        ) = (30, 50, 12);
        self.status = "The layout SozoIDE came with".into();
    }

    fn current_layout(&self, name: String) -> Layout {
        Layout {
            name,
            tiles: self.tiles.clone(),
            shown: [
                self.show_explorer,
                self.show_bottom_terminal,
                self.show_right_terminal,
            ],
            lengths: [
                self.explorer_width,
                self.explorer_height,
                self.right_terminal_width,
                self.right_terminal_height,
                self.bottom_terminal_width,
                self.bottom_terminal_height,
            ],
        }
    }

    fn apply_layout(&mut self, layout: &Layout) {
        self.tiles = layout.tiles.clone();
        self.maximized = None;
        [
            self.show_explorer,
            self.show_bottom_terminal,
            self.show_right_terminal,
        ] = layout.shown;
        let [
            explorer_width,
            explorer_height,
            right_width,
            right_height,
            bottom_width,
            bottom_height,
        ] = layout.lengths;
        self.explorer_width = explorer_width.clamp(10, 400);
        self.explorer_height = explorer_height.clamp(3, 200);
        self.right_terminal_width = right_width.clamp(10, 400);
        self.right_terminal_height = right_height.clamp(3, 200);
        self.bottom_terminal_width = bottom_width.clamp(10, 400);
        self.bottom_terminal_height = bottom_height.clamp(3, 200);
        self.status = format!("Layout {}", layout.name);
    }

    /// `Alt+L`: the dialog of the kept layouts.
    pub(super) fn open_layouts(&mut self) {
        self.layout_menu = Some(LayoutMenu {
            selected: 0,
            naming: None,
            layouts: read_layouts(layouts_path().as_deref()),
        });
    }

    /// The keys of the Layouts dialog: `↑`/`↓` choose, `Enter` or `1`–`3` use one, `S` keeps
    /// the layout on screen there under a name typed, `Delete` forgets it, `Esc` closes.
    pub(super) fn handle_layout_key(&mut self, key: KeyEvent) {
        let Some(menu) = &mut self.layout_menu else {
            return;
        };
        if let Some(name) = &mut menu.naming {
            match key.code {
                KeyCode::Esc => menu.naming = None,
                KeyCode::Backspace => {
                    name.pop();
                }
                KeyCode::Char(character)
                    if !character.is_control() && name.chars().count() < 30 =>
                {
                    name.push(character)
                }
                KeyCode::Enter => {
                    let name = menu.naming.take().unwrap_or_default();
                    let name = if name.trim().is_empty() {
                        format!("Layout {}", menu.selected + 1)
                    } else {
                        name.trim().to_owned()
                    };
                    let selected = menu.selected;
                    let layout = self.current_layout(name);
                    let Some(menu) = &mut self.layout_menu else {
                        return;
                    };
                    menu.layouts[selected] = Some(layout);
                    let layouts = menu.layouts.clone();
                    self.status = match layouts_path() {
                        Some(path) => match toml::to_string_pretty(&LayoutsFile { layouts }) {
                            Ok(text) => match crate::storage::write(&path, &text) {
                                Ok(()) => "Layout kept".into(),
                                Err(error) => format!("Could not keep the layout: {error}"),
                            },
                            Err(error) => format!("Could not keep the layout: {error}"),
                        },
                        None => "Layout kept for this run".into(),
                    };
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.layout_menu = None,
            KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
            KeyCode::Down => menu.selected = (menu.selected + 1).min(LAYOUTS - 1),
            KeyCode::Char('s' | 'S') => {
                let name = menu.layouts[menu.selected]
                    .as_ref()
                    .map(|layout| layout.name.clone())
                    .unwrap_or_default();
                menu.naming = Some(name);
            }
            KeyCode::Delete => menu.layouts[menu.selected] = None,
            KeyCode::Enter | KeyCode::Char('1'..='3') => {
                let index = match key.code {
                    KeyCode::Char(digit) => digit as usize - '1' as usize,
                    _ => menu.selected,
                };
                match menu.layouts.get(index).cloned().flatten() {
                    Some(layout) => {
                        self.layout_menu = None;
                        self.apply_layout(&layout);
                    }
                    None => {
                        self.status = "Nothing kept there yet: S keeps the layout on screen".into()
                    }
                }
            }
            _ => {}
        }
    }

    /// `Ctrl+Alt` and an arrow: the focus to the panel on that side, by the middle of the
    /// one with the focus now.
    pub(super) fn focus_beside(&mut self, (x, y): (i32, i32)) {
        let areas = [
            (Focus::Explorer, self.regions.explorer),
            (Focus::Editor, self.regions.editor),
            (Focus::BottomTerminal, self.regions.bottom_terminal),
            (Focus::RightTerminal, self.regions.right_terminal),
        ];
        let Some((_, from)) = areas.iter().find(|(focus, _)| *focus == self.focus) else {
            return;
        };
        let middle = |area: ratatui::layout::Rect| {
            (
                i32::from(area.x) + i32::from(area.width) / 2,
                i32::from(area.y) + i32::from(area.height) / 2,
            )
        };
        let (fx, fy) = middle(*from);
        let best = areas
            .iter()
            .filter(|(focus, area)| *focus != self.focus && !area.is_empty())
            .filter_map(|(focus, area)| {
                let (ax, ay) = middle(*area);
                let (dx, dy) = (ax - fx, ay - fy);
                // On that side, and nearer along it than across it.
                let along = dx * x + dy * y;
                (along > 0).then_some((along + (dx * y + dy * x).abs() * 2, *focus))
            })
            .min_by_key(|(score, _)| *score);
        if let Some((_, focus)) = best {
            self.focus = focus;
        }
    }
}

impl App {
    /// `Shift+F10`: the menu a right click opens, for the entry or the text with the focus,
    /// beside it (TODO 5.2.7).
    pub(super) fn context_menu(&mut self) {
        match self.focus {
            Focus::Explorer if self.show_explorer => {
                let area = self.regions.explorer;
                let row = self.selected_entry.saturating_sub(self.explorer_scroll) as u16;
                let point = (
                    area.x + 2,
                    (area.y + 1 + row).min(area.bottom().saturating_sub(1)),
                );
                self.open_entry_menu(
                    (self.selected_entry < self.entries.len()).then_some(self.selected_entry),
                    point,
                );
            }
            Focus::Editor => {
                let Some(file) = self.active_file().filter(|file| file.is_text()) else {
                    return;
                };
                let body = self.regions.editor_body;
                let row = file.cursor_row.saturating_sub(usize::from(file.scroll)) as u16;
                let point = (
                    body.x + 4,
                    (body.y + row + 1).min(body.bottom().saturating_sub(1)),
                );
                self.open_editor_menu(point);
            }
            _ => {}
        }
    }

    /// `Ctrl+Alt+Shift` and an arrow: the panel with the focus to that edge of the window, as
    /// a drag of its title to the border does (F39, TODO 5.2.7).
    pub(super) fn move_panel(&mut self, zone: super::DropZone) {
        let panel = match self.focus {
            Focus::Explorer => Panel::Explorer,
            Focus::BottomTerminal => Panel::BottomTerminal,
            Focus::RightTerminal => Panel::RightTerminal,
            _ => Panel::Editor,
        };
        let moved = self.tiles.clone().moved(panel, None, zone);
        if moved.is_complete() {
            self.tiles = moved;
            self.maximized = None;
            self.status = format!("{} moved", panel.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn a_layout_is_kept_used_maximized_and_reset_and_a_broken_file_is_left_out() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        let key = |app: &mut App, code: KeyCode| {
            app.handle_layout_key(KeyEvent::new(code, KeyModifiers::NONE))
        };
        // Keep the layout under a name, change it, then bring it back.
        app.show_right_terminal = false;
        app.explorer_width = 44;
        app.open_layouts();
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char('s'));
        for character in "Review".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.layout_menu.as_ref().unwrap().layouts[1]
                .as_ref()
                .unwrap()
                .name,
            "Review"
        );
        key(&mut app, KeyCode::Esc);
        app.tiles = Tile::default().moved(
            Panel::Explorer,
            Some(Panel::Editor),
            super::super::DropZone::Bottom,
        );
        app.show_right_terminal = true;
        app.explorer_width = 20;
        app.open_layouts();
        // Debug builds keep no file: what the dialog holds is the run's.
        app.layout_menu.as_mut().unwrap().layouts[1] = Some(app.current_layout("Review".into()));
        app.layout_menu.as_mut().unwrap().layouts[1]
            .as_mut()
            .unwrap()
            .shown[2] = false;
        app.layout_menu.as_mut().unwrap().layouts[1]
            .as_mut()
            .unwrap()
            .lengths[0] = 44;
        key(&mut app, KeyCode::Char('2'));
        assert!(app.layout_menu.is_none());
        assert_eq!((app.show_right_terminal, app.explorer_width), (false, 44));
        // An empty slot says so.
        app.open_layouts();
        key(&mut app, KeyCode::Char('3'));
        assert!(app.status.starts_with("Nothing kept there yet"));
        // Maximize and back; reset.
        app.focus = Focus::BottomTerminal;
        app.toggle_maximized();
        assert_eq!(
            app.placed_tiles(ratatui::layout::Rect::new(0, 0, 200, 60)),
            Tile::Panel(Panel::BottomTerminal)
        );
        app.toggle_maximized();
        assert_eq!(
            app.placed_tiles(ratatui::layout::Rect::new(0, 0, 200, 60)),
            app.tiles
        );
        app.reset_layout();
        assert_eq!(app.tiles, Tile::default());
        // A file with a tree that misses a panel is left out, a broken one too.
        let path = directory.path().join("layouts.toml");
        let broken = Layout {
            name: "x".into(),
            tiles: Tile::Panel(Panel::Editor),
            shown: [true; 3],
            lengths: [1; 6],
        };
        std::fs::write(
            &path,
            toml::to_string(&LayoutsFile {
                layouts: vec![Some(broken)],
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(read_layouts(Some(&path)), vec![None, None, None]);
        std::fs::write(&path, "layouts = [").unwrap();
        assert_eq!(read_layouts(Some(&path)).len(), LAYOUTS);
        assert_eq!(read_layouts(None).len(), LAYOUTS);
    }

    #[test]
    fn ctrl_alt_arrows_move_the_focus_to_the_panel_on_that_side() {
        use ratatui::layout::Rect;
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        // Explorer on the left, the editor over the bottom terminal, the right one on the right.
        app.regions.explorer = Rect::new(0, 0, 20, 40);
        app.regions.editor = Rect::new(20, 0, 60, 28);
        app.regions.bottom_terminal = Rect::new(20, 28, 60, 12);
        app.regions.right_terminal = Rect::new(80, 0, 40, 40);
        app.focus = Focus::Editor;
        app.focus_beside((0, 1));
        assert_eq!(app.focus, Focus::BottomTerminal);
        app.focus_beside((1, 0));
        assert_eq!(app.focus, Focus::RightTerminal);
        app.focus_beside((-1, 0));
        assert_eq!(app.focus, Focus::Editor, "the nearest on the left");
        app.focus_beside((-1, 0));
        assert_eq!(app.focus, Focus::Explorer);
        app.focus_beside((-1, 0));
        assert_eq!(app.focus, Focus::Explorer, "nothing further left");
        // A hidden panel is skipped.
        app.regions.right_terminal = Rect::default();
        app.focus = Focus::Editor;
        app.focus_beside((1, 0));
        assert_eq!(app.focus, Focus::Editor);
    }

    #[test]
    fn shift_f10_opens_the_menus_a_panel_moves_by_keys_and_colours_become_attributes() {
        use ratatui::style::{Color, Modifier, Style};
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.txt"), "hello\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        app.show_explorer = true;
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let press = |app: &mut App, code: KeyCode, modifiers: KeyModifiers| {
            app.handle_key(KeyEvent::new(code, modifiers))
        };
        // In the Explorer, Shift+F10 is the entry's menu; in the editor, the text's.
        app.focus = Focus::Explorer;
        press(&mut app, KeyCode::F(10), KeyModifiers::SHIFT);
        assert!(app.entry_menu.as_ref().is_some_and(|menu| !menu.editor));
        app.entry_menu = None;
        app.open_file(directory.path().join("a.txt")).unwrap();
        app.focus = Focus::Editor;
        press(&mut app, KeyCode::F(10), KeyModifiers::SHIFT);
        assert!(app.entry_menu.as_ref().is_some_and(|menu| menu.editor));
        app.entry_menu = None;
        // Ctrl+Alt+Shift+Up: the bottom terminal to the top edge.
        app.focus = Focus::BottomTerminal;
        press(
            &mut app,
            KeyCode::Up,
            KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(
            app.tiles,
            Tile::default().moved(Panel::BottomTerminal, None, super::super::DropZone::Top)
        );
        // Without colour: a background is reverse video, the accent bold, red underlined.
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 3, 1));
        buffer[(0, 0)].set_style(Style::default().bg(Color::Yellow).fg(Color::Black));
        buffer[(1, 0)].set_style(Style::default().fg(Color::Cyan));
        buffer[(2, 0)].set_style(Style::default().fg(Color::Red));
        crate::ui::without_colour(&mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
        );
        assert!(buffer[(0, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buffer[(1, 0)].modifier.contains(Modifier::BOLD));
        assert!(buffer[(2, 0)].modifier.contains(Modifier::UNDERLINED));
    }
}
