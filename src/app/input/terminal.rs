//! The mouse in the terminals: what goes to the program in them and what chooses text.

use super::*;
use crate::app::BOTTOM_TERMINALS;
use std::process::Stdio;

impl App {
    /// The area of the terminal panel `panel` names.
    pub(in crate::app) fn terminal_area(&self, panel: Focus) -> Rect {
        if panel == Focus::RightTerminal {
            self.regions.right_terminal
        } else {
            self.regions.bottom_terminal
        }
    }

    /// The terminal drawn under `point`, inside its border.
    pub(in crate::app) fn terminal_at(&self, point: (u16, u16)) -> Option<Focus> {
        if self.overlay != Overlay::None {
            return None;
        }
        [Focus::BottomTerminal, Focus::RightTerminal]
            .into_iter()
            .find(|panel| contains(inner_area(self.terminal_area(*panel)), point))
    }

    /// Whether the program in the terminal under `point` asked for the mouse (F37).
    pub(in crate::app) fn terminal_wants_mouse(&self, point: (u16, u16)) -> bool {
        self.terminal_at(point).is_some_and(|panel| {
            let terminals = if panel == Focus::RightTerminal {
                &self.right_terminal
            } else {
                &self.bottom_terminal
            };
            terminals.current().wants_mouse()
        })
    }

    pub(in crate::app) fn mouse_to_terminal(
        &mut self,
        panel: Focus,
        action: MouseAction,
        mouse: MouseEvent,
    ) {
        let cell = clamped_cell(self.terminal_area(panel), (mouse.column, mouse.row));
        self.terminals(panel)
            .current_mut()
            .send_mouse(action, cell, mouse.modifiers);
    }

    /// A left press in a terminal: to the program that asked for the mouse, unless Shift says
    /// to choose text, as terminals do (F37, Q57).
    pub(in crate::app) fn press_terminal(
        &mut self,
        panel: Focus,
        mouse: MouseEvent,
        point: (u16, u16),
    ) {
        self.focus = panel;
        if mouse.modifiers.contains(KeyModifiers::CONTROL) && self.open_terminal_link(panel, point)
        {
            return;
        }
        if self.terminal_wants_mouse(point) && !mouse.modifiers.contains(KeyModifiers::SHIFT) {
            self.terminal_mouse = Some((panel, 0));
            self.mouse_to_terminal(panel, MouseAction::Press(0), mouse);
        } else {
            self.start_terminal_selection(panel, point);
        }
    }

    /// A press inside a terminal's border starts choosing its text (Q57).
    pub(in crate::app) fn start_terminal_selection(&mut self, panel: Focus, point: (u16, u16)) {
        let inner = inner_area(self.terminal_area(panel));
        if contains(inner, point) {
            let cell = (point.1 - inner.y, point.0 - inner.x);
            self.terminal_selection = Some(TerminalSelection {
                panel,
                anchor: cell,
                end: cell,
            });
            self.terminal_dragging = true;
        }
    }

    /// Letting go after a drag in a terminal copies the text it chose; a click chooses nothing.
    pub(in crate::app) fn copy_terminal_selection(&mut self) {
        let Some(selection) = self.terminal_selection else {
            return;
        };
        if selection.anchor == selection.end {
            self.terminal_selection = None;
            return;
        }
        let text = self
            .terminals(selection.panel)
            .current()
            .text_between(selection.anchor, selection.end);
        // The empty rows under the output are not copied.
        self.copy(text.trim_end_matches('\n').to_string());
    }

    /// `+` on a terminal: another shell in a tab of its own, up to three (F04, TODO 5.2.4).
    pub(in crate::app) fn add_terminal(&mut self, panel: Focus) {
        let root = self.root.clone();
        let terminals = self.terminals(panel);
        if terminals.tabs.len() < BOTTOM_TERMINALS {
            terminals.add(&root);
        }
        if panel == Focus::RightTerminal {
            self.show_right_terminal = true;
        } else {
            self.show_bottom_terminal = true;
        }
        self.focus = panel;
        self.terminal_selection = None;
    }

    /// `x` on a tab closes it with its shell; the last one stays.
    pub(in crate::app) fn close_terminal(&mut self, panel: Focus, index: usize) {
        self.terminals(panel).close(index);
        self.focus = panel;
        self.terminal_selection = None;
    }

    /// `Shift+F6` or the `|` button: the shell on screen and the next side by side, or one again.
    pub(in crate::app) fn split_terminal(&mut self, panel: Focus) {
        let root = self.root.clone();
        let terminals = self.terminals(panel);
        if !terminals.split && terminals.tabs.len() < 2 {
            terminals.add(&root);
            terminals.step(false);
        }
        terminals.split = !terminals.split;
        self.focus = panel;
    }

    /// `F3`: the search of the history, from what was typed before.
    pub(in crate::app) fn open_terminal_search(&mut self) {
        let terminals = self.terminals(self.focus);
        terminals.renaming = None;
        terminals.search.get_or_insert_with(Default::default);
    }

    /// A right click on a terminal's tab: its name, typed in place (TODO 5.2.4).
    pub(in crate::app) fn rename_terminal(&mut self, panel: Focus, index: usize) {
        let terminals = self.terminals(panel);
        terminals.select(index);
        terminals.search = None;
        terminals.renaming = Some(terminals.tabs[index].name.clone().unwrap_or_default());
        self.focus = panel;
    }

    /// The keys of the search bar or of a tab's name while one is open; true when taken.
    pub(in crate::app) fn terminal_box_key(&mut self, key: KeyEvent) -> bool {
        let terminals = self.terminals(self.focus);
        if let Some(name) = &mut terminals.renaming {
            match key.code {
                KeyCode::Esc => terminals.renaming = None,
                KeyCode::Enter => {
                    let name = terminals.renaming.take().unwrap_or_default();
                    let name: String = name.chars().filter(|c| !c.is_control()).take(30).collect();
                    terminals.current_mut().name = (!name.trim().is_empty()).then_some(name);
                }
                KeyCode::Backspace => {
                    name.pop();
                }
                KeyCode::Char(character) if name.chars().count() < 30 => name.push(character),
                _ => {}
            }
            return true;
        }
        let Some(search) = &mut terminals.search else {
            return false;
        };
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Esc => {
                terminals.search = None;
                terminals.current().scroll_to_live();
                return true;
            }
            // Enter goes up to older lines, Shift+Enter down to newer ones.
            KeyCode::Enter | KeyCode::Up | KeyCode::Down if !search.matches.is_empty() => {
                let count = search.matches.len();
                let older =
                    matches!(key.code, KeyCode::Up) || (key.code == KeyCode::Enter && !shift);
                search.index = if older {
                    (search.index + count - 1) % count
                } else {
                    (search.index + 1) % count
                };
            }
            KeyCode::Backspace => {
                search.query.pop();
                search.matches.clear();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                search.query.push(character);
                search.matches.clear();
            }
            _ => return true,
        }
        // A query changed: every line again, the newest match first.
        if search.matches.is_empty() {
            let query = search.query.clone();
            let found = crate::terminal::find_in(&terminals.current().lines(), &query);
            let search = terminals.search.as_mut().expect("open");
            search.index = found.len().saturating_sub(1);
            search.matches = found;
        }
        let shown = terminals
            .search
            .as_ref()
            .and_then(|search| search.matches.get(search.index).copied());
        match shown {
            Some((line, ..)) => terminals.current().show_line(line),
            None => terminals.current().scroll_to_live(),
        }
        true
    }

    /// `Ctrl`+click in a terminal: the address in the browser, or the file at its line
    /// (TODO 5.1.3). Only `http`, `https` and files of the project.
    pub(in crate::app) fn open_terminal_link(&mut self, panel: Focus, point: (u16, u16)) -> bool {
        let cell = clamped_cell(self.terminal_area(panel), point);
        let root = self.root.clone();
        let Some(link) = self.terminals(panel).current().link_at(cell, &root) else {
            return false;
        };
        match link {
            crate::terminal::Link::Url(url) => {
                self.status = if cfg!(test) {
                    format!("Opened {url}")
                } else {
                    match crate::app::opener(std::env::consts::OS, &url)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(_) => format!("Opened {url}"),
                        Err(error) => format!("Could not open the browser: {error}"),
                    }
                };
            }
            crate::terminal::Link::File(path, line, column) => match self.open_file(path) {
                Ok(()) => {
                    self.go_to(&format!("{line}:{column}"));
                    self.focus = Focus::Editor;
                }
                Err(error) => self.status = error.to_string(),
            },
        }
        true
    }
}
