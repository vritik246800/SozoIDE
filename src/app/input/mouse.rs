//! The mouse: clicks, drags and the wheel over every panel.

use super::*;

/// Clicks closer than this in the same cell are a double or a triple click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl App {
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.splash_visible && !self.motion.splash_closing() {
            if mouse.kind != MouseEventKind::Moved {
                self.dismiss_splash();
            }
            return;
        }
        let point = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                self.selecting = false;
                self.terminal_selection = None;
                // A click in a hover or Peek box keeps it; anywhere else closes it (F31).
                if self.peek.is_some() || self.hover.is_some() {
                    if contains(self.regions.peek, point) || contains(self.regions.hover, point) {
                        return;
                    }
                    self.peek = None;
                    self.hover = None;
                }
                if self.name_prompt.is_some() {
                    return;
                }
                if self.properties.is_some() {
                    if !contains(self.regions.properties, point) {
                        self.properties = None;
                    }
                    return;
                }
                // The command dialog takes Enter or Esc; a project is chosen with a click.
                if self.confirm.is_some() {
                    return;
                }
                if self.project_choice.is_some() {
                    match self
                        .regions
                        .choice_rows
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                    {
                        Some((index, _)) => self.choose_project(Some(*index)),
                        None => self.project_choice = None,
                    }
                    return;
                }
                if self.entry_menu.is_some() {
                    if let Some(row) = self
                        .regions
                        .preview_rows
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .map(|(row, _)| *row)
                    {
                        self.choose_preview_row(row);
                        return;
                    }
                    let action = self
                        .regions
                        .entry_menu_items
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .map(|(action, _)| *action);
                    match action {
                        Some(action) => self.choose_entry_action(action),
                        None => self.entry_menu = None,
                    }
                    return;
                }
                if !contains(self.regions.editor, point) {
                    self.flush_active_file();
                }
                if let Some(divider) = self
                    .regions
                    .tiling
                    .dividers
                    .iter()
                    .find(|divider| contains(divider.handle, point))
                    .cloned()
                    .filter(|_| self.overlay == Overlay::None)
                {
                    self.drag = Some(divider);
                    return;
                }
                if contains(self.regions.header_help, point) {
                    self.open_overlay(Overlay::Help);
                    return;
                }
                if contains(self.regions.header_about, point) {
                    self.open_overlay(Overlay::About);
                    return;
                }
                // The branch opens Source Control; the message shows the last one again.
                if contains(self.regions.footer_branch, point) {
                    self.show_git = true;
                    self.show_database = false;
                    self.git.open();
                    self.focus = Focus::Git;
                    return;
                }
                if contains(self.regions.footer_message, point) {
                    self.motion.replay_notice();
                    return;
                }
                // Ln, Col opens Go to line; the line end and the indentation change (2.3.3).
                if contains(self.regions.footer_place, point) {
                    return self.open_go_to_line();
                }
                if let Some(target) = self
                    .regions
                    .debug_rows
                    .iter()
                    .find(|(_, area)| contains(*area, point))
                    .map(|(target, _)| *target)
                {
                    return self.debug_click(target);
                }
                if contains(self.regions.footer_problems, point) {
                    return self.open_finder(super::super::FinderMode::Problems);
                }
                if let Some(active) = self.active_tab {
                    if contains(self.regions.footer_line_end, point) {
                        return self.open_files[active].toggle_line_end();
                    }
                    if contains(self.regions.footer_indent, point) {
                        return self.open_files[active].toggle_indent_style();
                    }
                }
                if self.search.is_some() {
                    if self.motion.search_closing() {
                        return;
                    }
                    if contains(self.regions.search_previous, point) {
                        self.step_match(false);
                        return;
                    }
                    if contains(self.regions.search_next, point) {
                        self.step_match(true);
                        return;
                    }
                    if let Some((index, _)) = self
                        .regions
                        .search_match_items
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                    {
                        if let Some(search) = &mut self.search {
                            search.selected = *index;
                        }
                        self.handle_search_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    }
                    return;
                }
                if self.overlay == Overlay::Update && !self.motion.overlay_closing() {
                    if contains(self.regions.update_install, point) {
                        self.handle_update_key(KeyCode::Enter);
                    } else if contains(self.regions.update_skip, point) {
                        self.handle_update_key(KeyCode::Char('s'));
                    }
                    return;
                }
                if self.overlay == Overlay::Help && !self.motion.overlay_closing() {
                    // The tabs, then the Dependencies rows (F33).
                    if let Some((tab, _)) = self
                        .regions
                        .help_tabs
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                    {
                        match tab {
                            HelpTab::Shortcuts => self.help_tab = tab,
                            HelpTab::Dependencies => self.open_dependencies(),
                        }
                        return;
                    }
                    if self.help_tab == HelpTab::Dependencies {
                        let row = self
                            .regions
                            .dependency_rows
                            .iter()
                            .find(|(_, area)| contains(*area, point))
                            .map(|(row, _)| *row);
                        if let (Some(row), Some(dependencies)) = (row, &mut self.dependencies) {
                            // A click chooses the row; a click on the chosen one asks.
                            if dependencies.selected == row {
                                self.ask_to_install(Some(row));
                            } else {
                                dependencies.selected = row;
                            }
                        }
                        return;
                    }
                    // A command's key waits for new keys; Reset asks first (F29).
                    if contains(self.regions.help_reset, point) {
                        self.ask_to_reset_shortcuts();
                    } else if let Some((action, _)) = self
                        .regions
                        .help_keys
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                    {
                        self.choose_shortcut(action);
                    } else {
                        self.key_editor.capturing = false;
                        self.key_editor.confirm_reset = false;
                    }
                    return;
                }
                if self.overlay == Overlay::WhatsNew
                    && !self.motion.overlay_closing()
                    && let Some((filter, _)) = self
                        .regions
                        .notes_filters
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                {
                    self.filter_notes(filter);
                    return;
                }
                if self.overlay == Overlay::Setup
                    && !self.motion.overlay_closing()
                    && let Some((row, _)) = self
                        .regions
                        .dependency_rows
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                {
                    self.click_setup_row(row);
                    return;
                }
                if self.overlay != Overlay::None && !self.motion.overlay_closing() {
                    let regions = &self.regions;
                    let key = if contains(regions.about_repository, point) {
                        Some(KeyCode::Char('g'))
                    } else if contains(regions.about_update, point) {
                        Some(KeyCode::Char('u'))
                    } else if contains(regions.about_notes, point) {
                        Some(KeyCode::Char('n'))
                    } else if contains(regions.dialog_next, point) {
                        Some(KeyCode::Enter)
                    } else if contains(regions.dialog_skip, point) {
                        Some(KeyCode::Esc)
                    } else {
                        None
                    };
                    if let Some(key) = key {
                        self.handle_overlay_key(KeyEvent::from(key));
                    }
                    return;
                }
                if self.overlay != Overlay::None {
                    return;
                }
                if let Some(divider) = self.divider_beside(point) {
                    self.drag = Some(divider);
                    return;
                }
                if let Some(action) = self
                    .regions
                    .explorer_actions
                    .iter()
                    .find(|(_, area)| contains(*area, point))
                    .map(|(action, _)| *action)
                {
                    self.focus = Focus::Explorer;
                    return self.explorer_action(action);
                }
                if let Some(panel) = self.panel_handle_at(point) {
                    self.focus = match panel {
                        Panel::Explorer => Focus::Explorer,
                        Panel::Editor if self.show_git => Focus::Git,
                        Panel::Editor if self.show_database => Focus::Database,
                        Panel::Editor => Focus::Editor,
                        Panel::BottomTerminal => Focus::BottomTerminal,
                        Panel::RightTerminal => Focus::RightTerminal,
                    };
                    self.panel_drag = Some(PanelDrag {
                        panel,
                        from: point,
                        zone: None,
                    });
                    return;
                }
                if self.show_database && contains(self.database.regions.area, point) {
                    self.focus = Focus::Database;
                    self.database.click(point);
                    return;
                }
                if self.show_git && contains(self.git.regions.area, point) {
                    self.focus = Focus::Git;
                    self.git.click(point);
                    return;
                }
                if self.show_explorer && contains(self.regions.explorer, point) {
                    self.focus = Focus::Explorer;
                    let row = mouse.row.saturating_sub(self.regions.explorer.y + 1) as usize;
                    let index = self.explorer_scroll + row;
                    if index < self.entries.len() {
                        // Shift and Ctrl mark more than one entry (F01); a plain click opens.
                        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                            self.mark_range(index);
                        } else if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                            self.toggle_mark(index);
                        } else {
                            self.entry_drag = Some(index);
                            self.selected_entry = index;
                            self.activate_selected();
                        }
                    }
                } else if let Some((pane, right, _)) = self
                    .regions
                    .tab_arrows
                    .iter()
                    .find(|(_, _, area)| contains(*area, point))
                    .copied()
                {
                    self.scroll_tabs(pane, right);
                } else if let Some((index, tab, close)) = self
                    .regions
                    .tab_items
                    .iter()
                    .find(|(_, tab, close)| contains(*tab, point) || contains(*close, point))
                    .cloned()
                {
                    self.focus = Focus::Editor;
                    if contains(close, point) {
                        self.close_tab(index);
                    } else if contains(tab, point) {
                        self.tab_drag = Some(TabDrag {
                            tab: index,
                            previous: self.active_tab,
                            zone: None,
                        });
                        self.select_tab(index);
                    }
                } else if let Some((panel, index, _, close)) = self
                    .regions
                    .terminal_tabs
                    .iter()
                    .find(|(_, _, tab, close)| contains(*tab, point) || contains(*close, point))
                    .copied()
                {
                    // A terminal's tab shows its shell; its `x` closes it (F04).
                    if contains(close, point) {
                        self.close_terminal(panel, index);
                    } else {
                        self.terminals(panel).select(index);
                        self.focus = panel;
                    }
                } else if let Some((panel, _)) = self
                    .regions
                    .terminal_add
                    .iter()
                    .find(|(_, area)| contains(*area, point))
                    .copied()
                {
                    self.add_terminal(panel);
                } else if let Some((panel, _)) = self
                    .regions
                    .terminal_split
                    .iter()
                    .find(|(_, area)| contains(*area, point))
                    .copied()
                {
                    self.split_terminal(panel);
                } else if self.show_bottom_terminal && contains(self.regions.bottom_terminal, point)
                {
                    self.press_terminal(Focus::BottomTerminal, mouse, point);
                } else if self.show_right_terminal && contains(self.regions.right_terminal, point) {
                    self.press_terminal(Focus::RightTerminal, mouse, point);
                } else if contains(self.regions.editor_body, point) {
                    self.focus = Focus::Editor;
                    if self.show_git || self.show_database {
                        return;
                    }
                    let Some(pane) = self.pane_under(point) else {
                        return;
                    };
                    if pane != self.active_pane()
                        && let Some(split) = self.split
                    {
                        self.flush_active_file();
                        self.select_tab(split.other);
                    }
                    let area = self.regions.pane_bodies[pane];
                    if let Some(index) = self.active_tab.filter(|index| {
                        self.open_files
                            .get(*index)
                            .and_then(|file| file.media.as_ref())
                            .is_some_and(|media| media.video().is_some())
                    }) {
                        return self.click_video(index, area, point);
                    }
                    if self
                        .active_tab
                        .and_then(|index| self.open_files.get(index))
                        .is_some_and(|file| file.media.is_some())
                    {
                        self.turning = Some(point);
                        return;
                    }
                    if let Some(file) = self
                        .active_tab
                        .and_then(|index| self.open_files.get_mut(index))
                        && file.shows_text()
                        && contains(area, point)
                    {
                        // A click starts a selection that a drag makes longer; Shift extends
                        // the one there is.
                        let extend = mouse.modifiers.contains(KeyModifiers::SHIFT);
                        let (row, visual_column) = text_position(file, area, point);
                        let column = file.cursor_column_for_click(row, visual_column);
                        // A `[[ ]]` opens its document (F40), at its heading (TODO 5.1.9).
                        if mouse.modifiers.is_empty()
                            && let Some(target) = file.wiki_link_at(row, column)
                        {
                            let heading = file.wiki_heading_at(row, column);
                            self.open_wiki_link(&target);
                            return self.go_to_heading(heading.as_deref());
                        }
                        // A task box ticks; a link or a note goes where it points.
                        if mouse.modifiers.is_empty() && file.toggle_task_at(row, column) {
                            return;
                        }
                        if mouse.modifiers.is_empty()
                            && let Some(pointer) = file.pointer_at(row, column)
                        {
                            return self.follow_pointer(pointer);
                        }
                        file.mark(extend);
                        file.move_cursor_to(row, column);
                        if !extend {
                            file.anchor = Some((file.cursor_row, file.cursor_column));
                        }
                        // Two clicks in the same place select the word, three the line
                        // (TODO 2.2.4).
                        let now = Instant::now();
                        let clicks = match self.last_click {
                            Some((at, place, count))
                                if place == point && now.duration_since(at) < DOUBLE_CLICK =>
                            {
                                count % 3 + 1
                            }
                            _ => 1,
                        };
                        self.last_click = Some((now, point, clicks));
                        match clicks {
                            2 if !extend => file.select_word(),
                            3 if !extend => file.select_line(),
                            _ => {}
                        }
                        // Ctrl+click goes to the definition, where the terminal sends the Ctrl
                        // with the click (F31).
                        if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                            file.anchor = None;
                            self.ask_places(Goal::Definition);
                        } else {
                            self.selecting = clicks == 1;
                        }
                    }
                }
            }
            MouseEventKind::Down(button @ (MouseButton::Right | MouseButton::Middle))
                if self.terminal_wants_mouse(point) =>
            {
                let number = u8::from(button == MouseButton::Right) + 1;
                if let Some(panel) = self.terminal_at(point) {
                    self.focus = panel;
                    self.terminal_mouse = Some((panel, number));
                    self.mouse_to_terminal(panel, MouseAction::Press(number), mouse);
                }
            }
            // A right click on a terminal's tab names it (TODO 5.2.4).
            MouseEventKind::Down(crossterm::event::MouseButton::Right)
                if self
                    .regions
                    .terminal_tabs
                    .iter()
                    .any(|(_, _, tab, _)| contains(*tab, point)) =>
            {
                if let Some((panel, index, _, _)) = self
                    .regions
                    .terminal_tabs
                    .iter()
                    .find(|(_, _, tab, _)| contains(*tab, point))
                    .copied()
                {
                    self.rename_terminal(panel, index);
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                let explorer = self.regions.explorer;
                let free = self.overlay == Overlay::None
                    && self.search.is_none()
                    && self.name_prompt.is_none()
                    && self.properties.is_none()
                    && !self.splash_visible;
                if free && self.show_explorer && contains(explorer, point) && point.1 > explorer.y {
                    self.focus = Focus::Explorer;
                    let index = self.explorer_scroll + usize::from(point.1 - explorer.y - 1);
                    self.open_entry_menu((index < self.entries.len()).then_some(index), point);
                } else if free
                    && let Some(index) = self
                        .pane_at(point)
                        .filter(|index| self.open_files[*index].is_text())
                {
                    if Some(index) != self.active_tab {
                        self.flush_active_file();
                        self.select_tab(index);
                    }
                    self.focus = Focus::Editor;
                    // Without a selection the click puts the cursor on the word the menu's
                    // questions are about (F31).
                    if let Some((_, row, column)) = self.text_under(point) {
                        let file = &mut self.open_files[index];
                        if file.selection().is_none() {
                            file.move_cursor_to(row, column);
                        }
                    }
                    self.open_editor_menu(point);
                } else {
                    self.entry_menu = None;
                }
            }
            MouseEventKind::Drag(_) if self.terminal_mouse.is_some() => {
                if let Some((panel, button)) = self.terminal_mouse {
                    self.mouse_to_terminal(panel, MouseAction::Motion(Some(button)), mouse);
                }
            }
            MouseEventKind::Up(_) if self.terminal_mouse.is_some() => {
                if let Some((panel, button)) = self.terminal_mouse.take() {
                    self.mouse_to_terminal(panel, MouseAction::Release(button), mouse);
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left)
                if self.panel_drag.is_some() =>
            {
                self.drag_panel(point);
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.scrubbing => {
                self.scrub_video(point, false);
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.turning.is_some() => {
                let (column, row) = self.turning.replace(point).unwrap_or(point);
                if let Some(media) = self
                    .active_tab
                    .and_then(|index| self.open_files.get_mut(index))
                    .and_then(|file| file.media.as_mut())
                {
                    let columns = i32::from(point.0) - i32::from(column);
                    media.drag(columns, i32::from(point.1) - i32::from(row));
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.terminal_dragging => {
                if let Some(selection) = self.terminal_selection {
                    let area = self.terminal_area(selection.panel);
                    let end = clamped_cell(area, point);
                    self.terminal_selection = Some(TerminalSelection { end, ..selection });
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.selecting => {
                let area = self.regions.pane_bodies[self.active_pane()];
                if let Some(file) = self
                    .active_tab
                    .and_then(|index| self.open_files.get_mut(index))
                    && file.shows_text()
                {
                    // Past the top or the bottom of the text the view follows the selection.
                    let (row, visual_column) = if point.1 <= area.y {
                        let display = (file.scroll as usize).saturating_sub(1);
                        (file.source_row(display).unwrap_or(0), 0)
                    } else {
                        text_position(file, area, point)
                    };
                    let column = file.cursor_column_for_click(row, visual_column);
                    file.move_cursor_to(row, column);
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left)
                if self.drag.is_none() && self.tab_drag.is_some() =>
            {
                let body = self.regions.editor_body;
                let editor = !self.show_git && !self.show_database;
                let pane = (0..2).find(|pane| contains(self.regions.pane_areas[*pane], point));
                let several = self.open_files.len() > 1;
                if let Some(drag) = &mut self.tab_drag {
                    let from = self.open_files.get(drag.tab).map(|file| file.pane);
                    drag.zone = match self.split {
                        // Split: another pane takes the tab.
                        Some(_) => pane
                            .filter(|pane| editor && Some(*pane) != from)
                            .map(DropZone::Pane),
                        None => (editor && several && contains(body, point))
                            .then(|| DropZone::at(body, point)),
                    };
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => self.drag_divider(point),
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                if self.scrubbing {
                    self.scrub_video(point, true);
                }
                if let Some(drag) = self.panel_drag.take()
                    && let Some((target, zone)) = drag.zone
                {
                    let from = self.regions.tiling.area(drag.panel);
                    self.motion.glide_panel(
                        drag.panel,
                        crate::animation::Area {
                            x: from.x,
                            y: from.y,
                            width: from.width,
                            height: from.height,
                        },
                    );
                    self.tiles = self.tiles.clone().moved(drag.panel, target, zone);
                }
                // A drag of an Explorer row ended on another row: the question to move it.
                if let Some(from) = self.entry_drag.take()
                    && self.show_explorer
                    && contains(self.regions.explorer, point)
                {
                    let row = mouse.row.saturating_sub(self.regions.explorer.y + 1) as usize;
                    let onto = self.explorer_scroll + row;
                    if onto != from && onto < self.entries.len() {
                        self.drop_entries(from, onto);
                    }
                }
                self.drag = None;
                self.selecting = false;
                self.turning = None;
                if std::mem::take(&mut self.terminal_dragging) {
                    self.copy_terminal_selection();
                }
                if let Some(drag) = self.tab_drag.take()
                    && let Some(zone) = drag.zone
                {
                    self.drop_tab(drag, zone);
                }
            }
            MouseEventKind::Moved if self.entry_menu.is_some() => self.menu_mouse(point),
            MouseEventKind::Moved => {
                if let Some(panel) = self.terminal_at(point) {
                    self.mouse_to_terminal(panel, MouseAction::Motion(None), mouse);
                }
                self.hover(point);
                let at = self.text_under(point);
                let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
                self.mark_link(at, ctrl);
                self.rest_mouse(point, at, ctrl);
            }
            // Shift turns the wheel sideways, for terminals without a horizontal wheel.
            MouseEventKind::ScrollUp => self.scroll_at(point, true, sideways(mouse)),
            MouseEventKind::ScrollDown => self.scroll_at(point, false, sideways(mouse)),
            MouseEventKind::ScrollLeft => self.scroll_at(point, true, true),
            MouseEventKind::ScrollRight => self.scroll_at(point, false, true),
            _ => {}
        }
    }

    pub(in crate::app) fn scroll_at(&mut self, point: (u16, u16), up: bool, sideways: bool) {
        // The hover and Peek boxes scroll under the mouse (F31).
        if let Some(hover) = self
            .hover
            .as_mut()
            .filter(|_| contains(self.regions.hover, point))
        {
            hover.scroll = if up {
                hover.scroll.saturating_sub(1)
            } else {
                (hover.scroll + 1).min(hover.overflow)
            };
            return;
        }
        if let Some(peek) = self
            .peek
            .as_mut()
            .filter(|_| contains(self.regions.peek, point))
        {
            peek.top = if up {
                peek.top.saturating_sub(3)
            } else {
                (peek.top + 3).min(peek.lines.len().saturating_sub(1))
            };
            return;
        }
        if matches!(self.overlay, Overlay::Help | Overlay::WhatsNew) {
            self.overlay_scroll = if up {
                self.overlay_scroll.saturating_sub(3)
            } else {
                self.overlay_scroll.saturating_add(3)
            };
            return;
        }
        if let Some(pane) = (0..2).find(|pane| contains(self.regions.tab_bars[*pane], point)) {
            self.scroll_tabs(pane, !up);
        } else if self.show_database && contains(self.database.regions.area, point) {
            self.database.scroll(point, up, sideways);
        } else if self.show_git && contains(self.git.regions.content, point) {
            self.git.scroll(up);
        } else if contains(self.regions.explorer, point) && sideways {
            // Long names scroll sideways, as far as the widest one (F01).
            let room = usize::from(self.regions.explorer.width.saturating_sub(2));
            let most = self.explorer_width_needed().saturating_sub(room);
            self.explorer_scroll_x = if up {
                self.explorer_scroll_x.saturating_sub(6)
            } else {
                self.explorer_scroll_x.saturating_add(6).min(most)
            };
        } else if contains(self.regions.explorer, point) {
            self.explorer_scroll = if up {
                self.explorer_scroll.saturating_sub(3)
            } else {
                self.explorer_scroll
                    .saturating_add(3)
                    .min(self.entries.len().saturating_sub(1))
            };
        } else if let Some(panel) = [Focus::BottomTerminal, Focus::RightTerminal]
            .into_iter()
            .find(|panel| contains(self.terminal_area(*panel), point))
        {
            self.terminal_selection = None;
            let cell = clamped_cell(self.terminal_area(panel), point);
            self.terminals(panel)
                .current_mut()
                .wheel(up, cell, KeyModifiers::NONE);
        } else if let Some(index) = self.pane_at(point) {
            // The wheel zooms a model; pictures and videos do not scroll.
            if let Some(media) = &mut self.open_files[index].media {
                media.wheel(up);
                return;
            }
            let file = &self.open_files[index];
            // A page of text scrolls first and turns at its top or its end (F21).
            let height = self.regions.pane_bodies[file.pane.min(1)]
                .height
                .saturating_sub(2);
            let turns = file.image.is_some()
                || if up {
                    file.scroll == 0
                } else {
                    usize::from(file.scroll) + usize::from(height) >= file.display_rows()
                };
            if let Some((page, _)) = file.page().filter(|_| turns && !sideways) {
                // One page per turn of the wheel, however many events it sends.
                let now = Instant::now();
                if self
                    .page_turned
                    .is_some_and(|last| now.duration_since(last) < Duration::from_millis(250))
                {
                    return;
                }
                let page = if up { page.saturating_sub(1) } else { page + 1 };
                self.open_files[index].show_page(page, &mut self.picker);
                // The gesture counts from when the page is on screen: drawing a page can take
                // longer than the gesture itself on a slow machine (E27).
                self.page_turned = Some(Instant::now());
                return;
            }
            // The editor clamps both scrolls to the text when it draws.
            let file = &mut self.open_files[index];
            let scroll = if sideways {
                &mut file.scroll_x
            } else {
                &mut file.scroll
            };
            let step = if sideways { 6 } else { 3 };
            *scroll = if up {
                scroll.saturating_sub(step)
            } else {
                scroll.saturating_add(step)
            };
        }
    }

    /// A click on a video (Q52): a button does what it says, the progress bar starts a seek,
    /// the picture plays or pauses.
    pub(in crate::app) fn click_video(&mut self, index: usize, area: Rect, point: (u16, u16)) {
        let [picture, progress, buttons] = video_areas(area);
        let Some(video) = self.open_files[index]
            .media
            .as_ref()
            .and_then(|media| media.video())
        else {
            return;
        };
        if contains(progress, point) {
            if video
                .fraction_at(progress.width, point.0 - progress.x)
                .is_some()
            {
                self.scrubbing = true;
                self.scrub_video(point, false);
            }
        } else if contains(buttons, point) {
            if let Some(control) = video.button_at(point.0 - buttons.x) {
                self.control_video(index, control);
            }
        } else if contains(picture, point) {
            self.control_video(index, Control::Play);
        }
    }

    /// The mouse holds the progress bar at `point`, kept on the bar; letting go seeks there.
    pub(in crate::app) fn scrub_video(&mut self, point: (u16, u16), release: bool) {
        self.scrubbing = !release;
        let Some(index) = self.active_tab else {
            return;
        };
        let pane = self.open_files[index].pane.min(1);
        let [_, progress, _] = video_areas(self.regions.pane_bodies[pane]);
        if let Some(media) = self.open_files[index].media.as_mut()
            && let Some(video) = media.video()
        {
            let (_, start, length, _, _) = video.progress(progress.width);
            let column = point
                .0
                .saturating_sub(progress.x)
                .clamp(start, start + length.saturating_sub(1));
            if let Some(fraction) = video.fraction_at(progress.width, column) {
                media.scrub(fraction, release);
            }
        }
    }

    /// Whether a tab, an arrow or a button of a tab bar is under `point`.
    pub(in crate::app) fn tab_bar_button_at(&self, point: (u16, u16)) -> bool {
        let regions = &self.regions;
        regions
            .tab_items
            .iter()
            .any(|(_, tab, close)| contains(*tab, point) || contains(*close, point))
            || regions
                .tab_arrows
                .iter()
                .any(|(_, _, area)| contains(*area, point))
            || regions
                .terminal_tabs
                .iter()
                .any(|(_, _, tab, close)| contains(*tab, point) || contains(*close, point))
            || regions
                .terminal_add
                .iter()
                .chain(&regions.terminal_split)
                .any(|(_, area)| contains(*area, point))
    }

    /// The panel that can be taken at `point` (F39): by the title on its top border, by a
    /// corner of its border, or, for the editor, by the free part of its tab bar.
    pub(in crate::app) fn panel_handle_at(&self, point: (u16, u16)) -> Option<Panel> {
        if self.tab_bar_button_at(point) {
            return None;
        }
        self.regions.tiling.panels.iter().find_map(|(panel, area)| {
            let rows = if *panel == Panel::Editor { 2 } else { 1 };
            let title = Rect::new(area.x, area.y, area.width, area.height.min(rows));
            let bottom_corner =
                point.1 + 1 == area.bottom() && (point.0 == area.x || point.0 + 1 == area.right());
            (contains(title, point) || (contains(*area, point) && bottom_corner)).then_some(*panel)
        })
    }

    /// The divider next to the side or bottom border of a panel under `point`: those borders
    /// resize as the divider does (F39). The corners and the top border move the panel.
    pub(in crate::app) fn divider_beside(
        &self,
        point: (u16, u16),
    ) -> Option<crate::app::layout::Divider> {
        if self.tab_bar_button_at(point) || self.panel_handle_at(point).is_some() {
            return None;
        }
        let regions = &self.regions;
        let (_, area) = regions
            .tiling
            .panels
            .iter()
            .find(|(_, area)| contains(*area, point))?;
        let beside = |handle: Rect| {
            let (x, y) = point;
            let in_rows = y >= handle.y && y < handle.bottom();
            let in_columns = x >= handle.x && x < handle.right();
            (x == area.x && handle.right() == area.x && in_rows)
                || (x + 1 == area.right() && handle.x == area.right() && in_rows)
                || (y + 1 == area.bottom() && handle.y == area.bottom() && in_columns)
        };
        regions
            .tiling
            .dividers
            .iter()
            .find(|divider| beside(divider.handle))
            .cloned()
    }

    /// Where the panel held would end up, in the layout it would make (F39).
    pub fn panel_landing(&self) -> Option<Rect> {
        let drag = self.panel_drag?;
        let (target, zone) = drag.zone?;
        let tiles = self.tiles.clone().moved(drag.panel, target, zone);
        let length = |panel, direction| self.panel_length(panel, direction);
        Some(tiles.place(self.regions.body, &length).area(drag.panel))
    }

    /// Where a panel held at `point` lands: an edge of the window takes that whole edge, a
    /// side of another panel shares its place, its centre changes places with it.
    pub(in crate::app) fn panel_drop(
        &self,
        panel: Panel,
        point: (u16, u16),
    ) -> Option<(Option<Panel>, DropZone)> {
        let body = self.regions.body;
        if !contains(body, point) {
            return None;
        }
        let edge = if point.0 == body.x {
            Some(DropZone::Left)
        } else if point.0 + 1 == body.right() {
            Some(DropZone::Right)
        } else if point.1 == body.y {
            Some(DropZone::Top)
        } else if point.1 + 1 == body.bottom() {
            Some(DropZone::Bottom)
        } else {
            None
        };
        if let Some(zone) = edge {
            return Some((None, zone));
        }
        let (under, area) = self
            .regions
            .tiling
            .panels
            .iter()
            .find(|(_, area)| contains(*area, point))?;
        (*under != panel).then(|| (Some(*under), DropZone::at(*area, point)))
    }

    pub(in crate::app) fn drag_panel(&mut self, point: (u16, u16)) {
        let Some(mut drag) = self.panel_drag else {
            return;
        };
        if point != drag.from {
            drag.zone = self.panel_drop(drag.panel, point);
            self.status = format!(
                "{}: drop on a side of a panel, its centre or the edge of the window",
                drag.panel.name()
            );
        }
        self.panel_drag = Some(drag);
    }

    /// A divider held with the mouse: its fixed side takes the length to the pointer.
    pub(in crate::app) fn drag_divider(&mut self, point: (u16, u16)) {
        let Some(divider) = self.drag.clone() else {
            return;
        };
        let Some(direction) = divider.direction else {
            return;
        };
        let (position, start, end) = match direction {
            SplitDirection::Columns => (point.0, divider.area.x, divider.area.right()),
            SplitDirection::Rows => (point.1, divider.area.y, divider.area.bottom()),
        };
        let value = if divider.first {
            position.saturating_sub(start)
        } else {
            end.saturating_sub(position + 1)
        };
        let length = |panel, direction| self.panel_length(panel, direction);
        let sized = self.tiles.resized(&divider, value, &length);
        for (panel, value) in sized {
            self.set_panel_length(panel, direction, value);
        }
    }

    /// Moves a tab bar by one tab; it stops at the last one.
    pub(in crate::app) fn scroll_tabs(&mut self, pane: usize, right: bool) {
        let count = self.pane_tabs(pane).len();
        let bar = &mut self.tab_bars[pane];
        bar.scroll = if right {
            (bar.scroll + 1).min(count.saturating_sub(1))
        } else {
            bar.scroll.saturating_sub(1)
        };
    }

    /// The editor pane whose text is under `point`.
    pub(in crate::app) fn pane_under(&self, point: (u16, u16)) -> Option<usize> {
        (0..2).find(|pane| contains(self.regions.pane_bodies[*pane], point))
    }

    /// The file drawn under `point`, in either pane.
    pub(in crate::app) fn pane_at(&self, point: (u16, u16)) -> Option<usize> {
        if self.show_git || self.show_database {
            return None;
        }
        self.pane_under(point)
            .and_then(|pane| self.shown_tab(pane))
            .filter(|index| *index < self.open_files.len())
    }
}
