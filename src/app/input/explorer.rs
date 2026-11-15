//! Keys of the Explorer.

use super::*;

impl App {
    pub(in crate::app) fn handle_explorer_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        // After `/` the letters go to the filter of the whole project (F01).
        if let Some(filter) = &mut self.explorer_filter {
            let changed = match key.code {
                KeyCode::Char(character) if !ctrl => {
                    filter.push(character);
                    true
                }
                KeyCode::Backspace => filter.pop().is_some(),
                KeyCode::Esc => {
                    self.explorer_filter = None;
                    true
                }
                _ => false,
            };
            if changed {
                self.apply_filter();
                return self.keep_selection_visible();
            }
        }
        match key.code {
            KeyCode::Char('/') if !ctrl => self.explorer_filter = Some(String::new()),
            KeyCode::Char('i' | 'I') if !ctrl => {
                self.dim_ignored = !self.dim_ignored;
                self.status = if self.dim_ignored {
                    "Ignored files are dim".into()
                } else {
                    "Ignored files look as the others".into()
                };
            }
            KeyCode::Esc if !self.marked.is_empty() => self.marked.clear(),
            KeyCode::Up if shift => self.mark_range(self.selected_entry.saturating_sub(1)),
            KeyCode::Down if shift => self.mark_range(self.selected_entry + 1),
            _ if self.keys.is(Action::Run, key) => {
                if let Some(entry) = self
                    .entries
                    .get(self.selected_entry)
                    .filter(|entry| !entry.is_dir)
                {
                    let path = entry.path.clone();
                    self.run_file(path);
                }
            }
            KeyCode::Char('z' | 'Z') if ctrl => self.undo_file_operation(),
            KeyCode::Char('c' | 'C') if ctrl => self.hold_selected_entry(false),
            KeyCode::Char('x' | 'X') if ctrl => self.hold_selected_entry(true),
            KeyCode::Char('v' | 'V') if ctrl => self.paste_selected_entry(),
            KeyCode::Up => self.selected_entry = self.selected_entry.saturating_sub(1),
            KeyCode::Down => {
                self.selected_entry =
                    (self.selected_entry + 1).min(self.entries.len().saturating_sub(1));
            }
            KeyCode::Home => self.selected_entry = 0,
            KeyCode::End => self.selected_entry = self.entries.len().saturating_sub(1),
            KeyCode::PageUp => self.explorer_page(false),
            KeyCode::PageDown => self.explorer_page(true),
            KeyCode::Left => self.explorer_left(),
            KeyCode::Right => self.explorer_right(),
            KeyCode::Enter => self.activate_selected(),
            KeyCode::F(2) => self.prompt_selected(EntryAction::Rename),
            KeyCode::Delete if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.prompt_selected(EntryAction::DeleteForever);
            }
            KeyCode::Delete => self.prompt_selected(EntryAction::Delete),
            KeyCode::Char('n') if !ctrl => self.prompt_selected(EntryAction::NewFile),
            KeyCode::Char('N') if !ctrl => self.prompt_selected(EntryAction::NewFolder),
            _ => {}
        }
        self.keep_selection_visible();
    }

    /// The width of the widest row of the Explorer, as `ui::editor` draws it.
    pub fn explorer_width_needed(&self) -> usize {
        self.entries
            .iter()
            .map(|entry| {
                // Indentation, the `+`/`-` marker, the icon and the spaces between them, and
                // Git's mark at the right edge, which the end of the name scrolls clear of.
                let mark = 2 * usize::from(self.git_change(&entry.path).is_some());
                2 * entry.depth + 4 + ratatui::text::Line::from(entry.name.as_str()).width() + mark
            })
            .max()
            .unwrap_or(0)
    }
}
