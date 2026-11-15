use std::{sync::Arc, time::Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use tokio::{runtime::Runtime, sync::Mutex, task::JoinHandle};

use crate::{
    animation::InterfaceMotion,
    database::{
        ConnectionField, ConnectionForm, DatabasePanel, DatabaseTab, Operator, QueryCancellation,
        QueryKind, Sort, Total, cycle,
        more::{Export, Statement, export},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseAction {
    Tab(DatabaseTab),
    AddConnection,
    SaveConnection,
    TestConnection,
    CancelForm,
    SelectConnection(usize),
    UseConnection,
    RunQuery,
    CancelQuery,
    ClearResults,
    /// A table or view of the list (its index in the catalog): in or out of the diagram.
    Object(usize),
    /// A column of a table in the diagram (catalog indexes): in or out of the builder.
    Column(usize, usize),
    /// A cell of the builder grid: row and column.
    Cell(usize, GridColumn),
    /// The SQL to the clipboard.
    Copy,
    /// The SQL made again from the builder, over what was typed.
    Rebuild,
    /// The plan of the SQL (`Alt+E`).
    Explain,
    /// A page more of the results (`L`).
    LoadMore,
    /// How many rows the last query gives (`#`).
    Count,
    /// A new SQL tab (`Alt+T`), or the tab `n`.
    NewSqlTab,
    SqlTab(usize),
}

/// The part of the workspace the keys go to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DatabaseFocus {
    #[default]
    Objects,
    Content,
    Results,
}

/// The columns of the builder grid that change: the table and column are fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridColumn {
    Show,
    Total,
    Sort,
    Filter,
    Value,
}

impl GridColumn {
    pub const ALL: [Self; 5] = [
        Self::Show,
        Self::Total,
        Self::Sort,
        Self::Filter,
        Self::Value,
    ];
}

#[derive(Default)]
pub struct DatabaseRegions {
    pub area: Rect,
    pub content: Rect,
    pub objects: Rect,
    pub results: Rect,
    pub actions: Vec<(DatabaseAction, Rect)>,
    pub form_fields: Vec<(ConnectionField, Rect)>,
}

enum TaskKind {
    Test(ConnectionForm),
    Save(ConnectionForm),
    Catalog,
    Run(QueryCancellation, QueryKind),
    Edit(Statement),
}

pub struct DatabaseController {
    pub panel: Arc<Mutex<DatabasePanel>>,
    pub form: Option<ConnectionForm>,
    pub regions: DatabaseRegions,
    pub motion: InterfaceMotion<DatabaseTab>,
    pub operation: bool,
    pub focus: DatabaseFocus,
    /// The table or view under the cursor in the list (catalog index).
    pub object_cursor: usize,
    /// The column under the cursor in the diagram, counted across its tables.
    pub diagram_cursor: usize,
    pub builder_row: usize,
    pub builder_column: GridColumn,
    pub result_scroll: usize,
    /// Characters of the result rows scrolled off on the left.
    pub result_horizontal_scroll: usize,
    /// SQL that Copy put aside for the app, which owns the clipboard.
    pub copy_request: Option<String>,
    /// Results exported, for the app: to the clipboard, or to a new file of that extension.
    pub export_request: Option<(String, Option<&'static str>)>,
    /// The cell of the results under the cursor: row and column.
    pub cell: (usize, usize),
    /// The value typed for that cell, before its `UPDATE` is shown.
    pub cell_edit: Option<String>,
    /// A statement shown that runs only after `Enter` (TODO 4.1.11).
    pub confirm: Option<Statement>,
    /// The whole value of the cell in a box (`Space`).
    pub cell_popup: bool,
    /// The cell moved: the view scrolls the results to show it once.
    pub follow_cell: bool,
    runtime: Runtime,
    task: Option<JoinHandle<()>>,
    cancellation: Option<QueryCancellation>,
}

impl DatabaseController {
    pub fn new() -> Result<Self> {
        Self::with_panel(DatabasePanel::new()?)
    }

    #[cfg(test)]
    pub fn with_config_path(path: std::path::PathBuf) -> Result<Self> {
        Self::with_panel(DatabasePanel::with_config_path(path)?)
    }

    fn with_panel(panel: DatabasePanel) -> Result<Self> {
        Ok(Self {
            panel: Arc::new(Mutex::new(panel)),
            form: None,
            regions: DatabaseRegions::default(),
            motion: InterfaceMotion::new(Instant::now(), DatabaseTab::Connections),
            operation: false,
            focus: DatabaseFocus::Objects,
            object_cursor: 0,
            diagram_cursor: 0,
            builder_row: 0,
            builder_column: GridColumn::Show,
            result_scroll: 0,
            result_horizontal_scroll: 0,
            copy_request: None,
            export_request: None,
            cell: (0, 0),
            cell_edit: None,
            confirm: None,
            cell_popup: false,
            follow_cell: false,
            runtime: Runtime::new()?,
            task: None,
            cancellation: None,
        })
    }

    #[cfg(test)]
    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    pub fn tick_at(&mut self, now: Instant) {
        if self.task.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(task) = self.task.take() {
                let _ = self.runtime.block_on(task);
            }
            self.operation = false;
            self.cancellation = None;
        }
        self.motion.tick(now, self.operation);
        if let Ok(panel) = self.panel.try_lock() {
            self.motion.sync_tab(panel.tab);
        }
        if self.form.is_some() && self.motion.dialog_closed() {
            self.form = None;
        }
    }

    pub fn shutdown(&self) {
        if let Ok(mut panel) = self.panel.try_lock() {
            self.runtime.block_on(panel.close());
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.form.is_some() && !self.motion.dialog_closing() {
            self.handle_form_key(key);
            return true;
        }
        if self.overlay_key(key) {
            return true;
        }
        if key.code == KeyCode::Esc {
            return false;
        }
        let Some(tab) = self.panel.try_lock().map(|panel| panel.tab).ok() else {
            return true;
        };
        if tab != DatabaseTab::Connections && self.workspace_key(key) {
            return true;
        }
        let run = key.code == KeyCode::F(5)
            || (key.code == KeyCode::Enter
                && (key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)));
        let completing = self
            .panel
            .try_lock()
            .is_ok_and(|panel| !panel.completions.is_empty());
        match key.code {
            KeyCode::Tab if completing => {
                self.sql_key(key);
                return true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let next = if key.code == KeyCode::Tab {
                    tab.next()
                } else {
                    tab.previous()
                };
                self.action(DatabaseAction::Tab(next));
                return true;
            }
            KeyCode::F(6) if tab != DatabaseTab::Connections => {
                self.focus = match self.focus {
                    DatabaseFocus::Objects => DatabaseFocus::Content,
                    DatabaseFocus::Content => DatabaseFocus::Results,
                    DatabaseFocus::Results => DatabaseFocus::Objects,
                };
                return true;
            }
            _ if run && tab != DatabaseTab::Connections => {
                self.action(DatabaseAction::RunQuery);
                return true;
            }
            _ => {}
        }
        if tab == DatabaseTab::Connections {
            match key.code {
                KeyCode::Up => self.with_panel_mut(DatabasePanel::select_previous),
                KeyCode::Down => self.with_panel_mut(DatabasePanel::select_next),
                KeyCode::Enter => self.action(DatabaseAction::UseConnection),
                KeyCode::Char('n' | 'N') => self.action(DatabaseAction::AddConnection),
                _ => {}
            }
            return true;
        }
        match self.focus {
            DatabaseFocus::Objects => self.objects_key(key.code),
            DatabaseFocus::Results => self.results_key(key.code),
            DatabaseFocus::Content => match tab {
                DatabaseTab::Diagram => self.diagram_key(key.code),
                DatabaseTab::Builder => self.builder_key(key),
                DatabaseTab::Sql => self.sql_key(key),
                DatabaseTab::Connections => {}
            },
        }
        true
    }

    fn with_panel_mut(&mut self, change: impl FnOnce(&mut DatabasePanel)) {
        if let Ok(mut panel) = self.panel.try_lock() {
            change(&mut panel);
        }
    }

    fn objects_key(&mut self, code: KeyCode) {
        let count = self
            .panel
            .try_lock()
            .ok()
            .and_then(|panel| panel.catalog.as_ref().map(|catalog| catalog.objects.len()))
            .unwrap_or(0);
        match code {
            KeyCode::Up => self.object_cursor = self.object_cursor.saturating_sub(1),
            KeyCode::Down => {
                self.object_cursor = (self.object_cursor + 1).min(count.saturating_sub(1));
            }
            KeyCode::Enter | KeyCode::Char(' ') if count > 0 => {
                self.action(DatabaseAction::Object(self.object_cursor));
            }
            KeyCode::Right => self.focus = DatabaseFocus::Content,
            _ => {}
        }
    }

    /// The columns of the diagram in order: (catalog object, column).
    pub fn diagram_columns(panel: &DatabasePanel) -> Vec<(usize, usize)> {
        let Some(catalog) = &panel.catalog else {
            return Vec::new();
        };
        panel
            .design
            .objects
            .iter()
            .filter_map(|name| {
                catalog
                    .objects
                    .iter()
                    .position(|object| &object.name == name)
            })
            .flat_map(|object| {
                (0..catalog.objects[object].columns.len()).map(move |column| (object, column))
            })
            .collect()
    }

    fn diagram_key(&mut self, code: KeyCode) {
        let columns = self
            .panel
            .try_lock()
            .map(|panel| Self::diagram_columns(&panel))
            .unwrap_or_default();
        match code {
            KeyCode::Up => self.diagram_cursor = self.diagram_cursor.saturating_sub(1),
            KeyCode::Down => {
                self.diagram_cursor =
                    (self.diagram_cursor + 1).min(columns.len().saturating_sub(1));
            }
            KeyCode::Left => self.focus = DatabaseFocus::Objects,
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(&(object, column)) = columns.get(self.diagram_cursor) {
                    self.action(DatabaseAction::Column(object, column));
                }
            }
            _ => {}
        }
    }

    fn builder_key(&mut self, key: KeyEvent) {
        let rows = self
            .panel
            .try_lock()
            .map(|panel| panel.design.fields.len())
            .unwrap_or(0);
        let column = GridColumn::ALL
            .iter()
            .position(|column| *column == self.builder_column)
            .unwrap_or(0);
        let value = self.builder_column == GridColumn::Value;
        match key.code {
            KeyCode::Up => self.builder_row = self.builder_row.saturating_sub(1),
            KeyCode::Down => self.builder_row = (self.builder_row + 1).min(rows.saturating_sub(1)),
            KeyCode::Left if column == 0 => self.focus = DatabaseFocus::Objects,
            KeyCode::Left => self.builder_column = GridColumn::ALL[column - 1],
            KeyCode::Right => {
                self.builder_column = GridColumn::ALL[(column + 1).min(GridColumn::ALL.len() - 1)];
            }
            KeyCode::Delete if rows > 0 => {
                let row = self.builder_row;
                self.with_panel_mut(|panel| {
                    panel.change_design(|design| {
                        if row < design.fields.len() {
                            design.fields.remove(row);
                        }
                        Ok(())
                    });
                });
                self.builder_row = self.builder_row.min(rows.saturating_sub(2));
            }
            KeyCode::Backspace if value => self.edit_value(|text| {
                text.pop();
            }),
            KeyCode::Char(character) if value && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.edit_value(|text| text.push(character));
            }
            KeyCode::Enter | KeyCode::Char(' ') if rows > 0 => {
                self.action(DatabaseAction::Cell(self.builder_row, self.builder_column));
            }
            _ => {}
        }
    }

    fn edit_value(&mut self, change: impl FnOnce(&mut String)) {
        let row = self.builder_row;
        self.with_panel_mut(|panel| {
            panel.change_design(|design| {
                if let Some(field) = design.fields.get_mut(row) {
                    change(&mut field.value);
                }
                Ok(())
            });
        });
    }

    /// The boxes over the panel take every key while open.
    fn overlay_key(&mut self, key: KeyEvent) -> bool {
        if self.confirm.is_some() {
            match key.code {
                KeyCode::Enter => {
                    if let Some(statement) = self.confirm.take()
                        && !self.operation
                    {
                        self.spawn(TaskKind::Edit(statement));
                    }
                }
                KeyCode::Esc => self.confirm = None,
                _ => {}
            }
            return true;
        }
        if let Some(text) = &mut self.cell_edit {
            match key.code {
                KeyCode::Esc => self.cell_edit = None,
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text.push(character);
                }
                KeyCode::Enter => {
                    let typed = self.cell_edit.take().unwrap_or_default();
                    let (row, column) = self.cell;
                    self.prepare(|panel| panel.edit_statement(row, column, &typed));
                }
                _ => {}
            }
            return true;
        }
        if self.cell_popup {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ')) {
                self.cell_popup = false;
            }
            return true;
        }
        let shown = self
            .panel
            .try_lock()
            .is_ok_and(|panel| !panel.completions.is_empty());
        if shown && key.code == KeyCode::Esc {
            self.with_panel_mut(|panel| panel.completions.clear());
            return true;
        }
        false
    }

    /// The statement of an edit, shown for `Enter`; or why it cannot be made.
    fn prepare(&mut self, make: impl FnOnce(&DatabasePanel) -> anyhow::Result<Statement>) {
        let Ok(mut panel) = self.panel.try_lock() else {
            return;
        };
        match make(&panel) {
            Ok(statement) => self.confirm = Some(statement),
            Err(error) => panel.error = Some(error.to_string()),
        }
    }

    /// The keys of the connected workspace in any of its parts: plan and SQL tabs.
    fn workspace_key(&mut self, key: KeyEvent) -> bool {
        if !key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        match key.code {
            KeyCode::Char('e' | 'E') => self.action(DatabaseAction::Explain),
            KeyCode::Char('t' | 'T') => self.action(DatabaseAction::NewSqlTab),
            KeyCode::Char('w' | 'W') => self.with_panel_mut(DatabasePanel::close_sql_tab),
            KeyCode::Char(digit @ '1'..='9') => {
                self.action(DatabaseAction::SqlTab(digit as usize - '1' as usize));
            }
            _ => return false,
        }
        true
    }

    fn sql_key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        self.with_panel_mut(|panel| {
            if !panel.completions.is_empty() {
                if matches!(key.code, KeyCode::Tab | KeyCode::Enter) && panel.accept_completion() {
                    return;
                }
                if !(control && key.code == KeyCode::Char(' ')) {
                    panel.completions.clear();
                }
            }
            match key.code {
                KeyCode::Char(' ') if control => panel.complete(),
                KeyCode::Up if alt => panel.walk_history(true),
                KeyCode::Down if alt => panel.walk_history(false),
                _ => sql_edit(panel, key.code, control),
            }
        });
    }
}

fn sql_edit(panel: &mut DatabasePanel, code: KeyCode, control: bool) {
    {
        match code {
            KeyCode::Enter => panel.insert_query("\n"),
            KeyCode::Backspace => panel.query_backspace(),
            KeyCode::Delete => panel.query_delete(),
            KeyCode::Left => panel.query_left(),
            KeyCode::Right => panel.query_right(),
            KeyCode::Up => panel.query_vertical(false),
            KeyCode::Down => panel.query_vertical(true),
            KeyCode::Home => panel.query_home(),
            KeyCode::End => panel.query_end(),
            KeyCode::Char(character) if !control => panel.insert_query(&character.to_string()),
            _ => {}
        }
    }
}

impl DatabaseController {
    /// The results: a cell moves with the arrows; `Enter` edits it, `Delete` deletes its row,
    /// `+` copies it as a new row (each after `Enter` on its statement), `Space` shows it whole,
    /// `C`/`J`/`M` copy the results as CSV, JSON or Markdown (with `Shift`, to a file), `L`
    /// loads a page more, `#` counts them all.
    fn results_key(&mut self, code: KeyCode) {
        let (rows, columns) = self
            .panel
            .try_lock()
            .ok()
            .and_then(|panel| {
                let result = panel.results.as_ref()?;
                Some((result.rows.len(), result.columns.len()))
            })
            .unwrap_or((0, 0));
        let last = |count: usize| count.saturating_sub(1);
        self.follow_cell = true;
        let (row, column) = &mut self.cell;
        match code {
            KeyCode::Up => *row = row.saturating_sub(1),
            KeyCode::Down => *row = (*row + 1).min(last(rows)),
            KeyCode::PageUp => *row = row.saturating_sub(10),
            KeyCode::PageDown => *row = (*row + 10).min(last(rows)),
            KeyCode::Left => *column = column.saturating_sub(1),
            KeyCode::Right => *column = (*column + 1).min(last(columns)),
            KeyCode::Home => {
                self.cell = (0, 0);
                (self.result_scroll, self.result_horizontal_scroll) = (0, 0);
            }
            KeyCode::Enter if rows > 0 => {
                let value = self.panel.try_lock().ok().and_then(|panel| {
                    panel
                        .results
                        .as_ref()?
                        .rows
                        .get(self.cell.0)?
                        .get(self.cell.1)
                        .map(|value| match value {
                            crate::database::DatabaseValue::Text(text) => text.clone(),
                            other => other.to_string(),
                        })
                });
                self.cell_edit = value;
            }
            KeyCode::Delete if rows > 0 => {
                let row = self.cell.0;
                self.prepare(|panel| panel.delete_statement(row));
            }
            KeyCode::Char('+') | KeyCode::Insert if rows > 0 => {
                let row = self.cell.0;
                self.prepare(|panel| panel.insert_statement(row));
            }
            KeyCode::Char(' ') if rows > 0 => self.cell_popup = true,
            KeyCode::Char(letter @ ('c' | 'C' | 'j' | 'J' | 'm' | 'M')) => {
                let format = match letter.to_ascii_lowercase() {
                    'c' => Export::Csv,
                    'j' => Export::Json,
                    _ => Export::Markdown,
                };
                let to_file = letter.is_ascii_uppercase();
                self.export_request = self.panel.try_lock().ok().and_then(|panel| {
                    let text = export(panel.results.as_ref()?, format);
                    Some((text, to_file.then_some(format.extension())))
                });
            }
            KeyCode::Char('l' | 'L') => self.action(DatabaseAction::LoadMore),
            KeyCode::Char('#') => self.action(DatabaseAction::Count),
            _ => {}
        }
    }

    fn handle_form_key(&mut self, key: KeyEvent) {
        let Some(form) = &mut self.form else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_form(),
            KeyCode::Tab => form.next_field(),
            KeyCode::BackTab => form.previous_field(),
            KeyCode::Enter => self.action(DatabaseAction::SaveConnection),
            KeyCode::F(5) => self.action(DatabaseAction::TestConnection),
            KeyCode::Left if form.field == ConnectionField::Driver => form.cycle_driver(false),
            KeyCode::Right if form.field == ConnectionField::Driver => form.cycle_driver(true),
            KeyCode::Left if form.field == ConnectionField::Tls => form.cycle_tls(false),
            KeyCode::Right | KeyCode::Char(_) if form.field == ConnectionField::Tls => {
                form.cycle_tls(true);
            }
            KeyCode::Left => form.move_left(),
            KeyCode::Right => form.move_right(),
            KeyCode::Home => form.move_home(),
            KeyCode::End => form.move_end(),
            KeyCode::Backspace => form.backspace(),
            KeyCode::Delete => form.delete(),
            KeyCode::Char(_) if form.field == ConnectionField::Driver => form.cycle_driver(true),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.insert(&character.to_string());
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.form.is_some() && !self.motion.dialog_closing() {
            if let Some(form) = &mut self.form {
                form.insert(text);
            }
            return;
        }
        let tab = self.panel.try_lock().map(|panel| panel.tab).ok();
        if tab == Some(DatabaseTab::Builder) && self.builder_column == GridColumn::Value {
            self.edit_value(|value| value.push_str(text));
        } else if tab == Some(DatabaseTab::Sql) || tab == Some(DatabaseTab::Builder) {
            self.with_panel_mut(|panel| panel.insert_query(text));
        }
    }

    pub fn click(&mut self, point: (u16, u16)) {
        if self.form.is_some() && self.motion.dialog_closing() {
            return;
        }
        if let Some((field, _)) = self
            .regions
            .form_fields
            .iter()
            .find(|(_, area)| contains(*area, point))
        {
            if let Some(form) = &mut self.form {
                form.select_field(*field);
            }
            return;
        }
        if contains(self.regions.objects, point) {
            self.focus = DatabaseFocus::Objects;
        } else if contains(self.regions.results, point) {
            self.focus = DatabaseFocus::Results;
        } else if contains(self.regions.content, point) {
            self.focus = DatabaseFocus::Content;
        }
        if let Some((action, _)) = self
            .regions
            .actions
            .iter()
            .find(|(_, area)| contains(*area, point))
            .cloned()
        {
            self.action(action);
        }
    }

    /// The wheel over the results scrolls them, sideways with Shift; elsewhere it moves the
    /// cursor of what is under it.
    pub fn scroll(&mut self, point: (u16, u16), up: bool, sideways: bool) {
        if contains(self.regions.results, point) {
            if sideways {
                self.result_horizontal_scroll = if up {
                    self.result_horizontal_scroll.saturating_sub(4)
                } else {
                    self.result_horizontal_scroll + 4
                };
            } else if up {
                self.result_scroll = self.result_scroll.saturating_sub(3);
            } else {
                self.result_scroll += 3;
            }
            return;
        }
        let tab = self.panel.try_lock().map(|panel| panel.tab).ok();
        let code = if up { KeyCode::Up } else { KeyCode::Down };
        match tab {
            Some(DatabaseTab::Connections) => {
                self.with_panel_mut(|panel| {
                    if up {
                        panel.select_previous();
                    } else {
                        panel.select_next();
                    }
                });
            }
            _ if contains(self.regions.objects, point) => self.objects_key(code),
            Some(DatabaseTab::Diagram) => self.diagram_key(code),
            Some(DatabaseTab::Builder) => self.builder_key(KeyEvent::new(code, KeyModifiers::NONE)),
            _ => {}
        }
    }

    pub fn action(&mut self, action: DatabaseAction) {
        match action {
            DatabaseAction::Tab(tab) => self.with_panel_mut(|panel| panel.tab = tab),
            DatabaseAction::AddConnection => self.open_form(),
            DatabaseAction::CancelForm => self.close_form(),
            DatabaseAction::SaveConnection => {
                if !self.operation
                    && let Some(form) = self.form.clone()
                {
                    self.close_form();
                    self.spawn(TaskKind::Save(form));
                }
            }
            DatabaseAction::TestConnection => {
                if !self.operation
                    && let Some(form) = self.form.clone()
                {
                    self.spawn(TaskKind::Test(form));
                }
            }
            DatabaseAction::SelectConnection(index) => self.with_panel_mut(|panel| {
                if index < panel.connections.len() {
                    panel.selection = index;
                }
            }),
            DatabaseAction::UseConnection => {
                let mut used = false;
                self.with_panel_mut(|panel| {
                    let selected = panel
                        .selected_config()
                        .map(|connection| connection.id.clone());
                    let result = selected.map_or_else(
                        || Err(anyhow::anyhow!("No connection is selected")),
                        |id| panel.set_active_connection(&id),
                    );
                    match result {
                        Ok(()) => used = true,
                        Err(error) => panel.error = Some(error.to_string()),
                    }
                });
                if used {
                    self.spawn(TaskKind::Catalog);
                }
            }
            DatabaseAction::RunQuery => {
                // The SQL of a builder that cannot make it is the old one: say why instead.
                let stale = self.panel.try_lock().ok().and_then(|mut panel| {
                    let error = panel
                        .builder_error
                        .clone()
                        .filter(|_| !panel.sql_edited && panel.tab != DatabaseTab::Connections)?;
                    panel.error = Some(error.clone());
                    Some(error)
                });
                if !self.operation && stale.is_none() {
                    self.run(QueryKind::Run);
                    self.focus = DatabaseFocus::Results;
                    (self.result_scroll, self.result_horizontal_scroll) = (0, 0);
                    self.cell = (0, 0);
                }
            }
            DatabaseAction::Explain => {
                if !self.operation {
                    self.run(QueryKind::Explain);
                    self.focus = DatabaseFocus::Results;
                    self.cell = (0, 0);
                }
            }
            DatabaseAction::LoadMore => {
                let more = self
                    .panel
                    .try_lock()
                    .is_ok_and(|panel| panel.results.as_ref().is_some_and(|result| result.more));
                if more && !self.operation {
                    self.run(QueryKind::More);
                }
            }
            DatabaseAction::Count => {
                if !self.operation {
                    self.run(QueryKind::Count);
                }
            }
            DatabaseAction::NewSqlTab => {
                self.with_panel_mut(|panel| {
                    panel.new_sql_tab();
                    panel.tab = DatabaseTab::Sql;
                });
                self.focus = DatabaseFocus::Content;
            }
            DatabaseAction::SqlTab(index) => {
                self.with_panel_mut(|panel| {
                    panel.open_sql_tab(index);
                    panel.tab = DatabaseTab::Sql;
                });
                self.focus = DatabaseFocus::Content;
            }
            DatabaseAction::CancelQuery => {
                if let Some(cancellation) = &self.cancellation {
                    cancellation.cancel();
                } else if let Ok(panel) = self.panel.try_lock() {
                    panel.cancel_query();
                }
            }
            DatabaseAction::ClearResults => {
                self.with_panel_mut(|panel| panel.results = None);
                (self.result_scroll, self.result_horizontal_scroll) = (0, 0);
            }
            DatabaseAction::Object(index) => {
                self.object_cursor = index;
                self.focus = DatabaseFocus::Objects;
                self.with_panel_mut(|panel| {
                    let Some(name) = panel
                        .catalog
                        .as_ref()
                        .and_then(|catalog| catalog.objects.get(index))
                        .map(|object| object.name.clone())
                    else {
                        return;
                    };
                    panel.change_design(|design| design.toggle_object(&name));
                    if panel.tab == DatabaseTab::Connections {
                        panel.tab = DatabaseTab::Diagram;
                    }
                });
            }
            DatabaseAction::Column(object, column) => {
                self.focus = DatabaseFocus::Content;
                let mut cursor = None;
                self.with_panel_mut(|panel| {
                    let Some((name, column_name)) = panel.catalog.as_ref().and_then(|catalog| {
                        let object = catalog.objects.get(object)?;
                        Some((
                            object.name.clone(),
                            object.columns.get(column)?.name.clone(),
                        ))
                    }) else {
                        return;
                    };
                    panel.change_design(|design| {
                        design.toggle_field(&name, &column_name);
                        Ok(())
                    });
                    cursor = Self::diagram_columns(panel)
                        .iter()
                        .position(|place| *place == (object, column));
                });
                self.diagram_cursor = cursor.unwrap_or(self.diagram_cursor);
            }
            DatabaseAction::Cell(row, column) => {
                self.focus = DatabaseFocus::Content;
                (self.builder_row, self.builder_column) = (row, column);
                // The value cell is typed into; the others change at each click or Enter.
                if column != GridColumn::Value {
                    self.with_panel_mut(|panel| {
                        panel.change_design(|design| {
                            if let Some(field) = design.fields.get_mut(row) {
                                match column {
                                    GridColumn::Show => field.show = !field.show,
                                    GridColumn::Total => {
                                        field.total = cycle(&Total::ALL, field.total, true);
                                    }
                                    GridColumn::Sort => {
                                        field.sort = cycle(&Sort::ALL, field.sort, true)
                                    }
                                    GridColumn::Filter => {
                                        field.operator =
                                            cycle(&Operator::ALL, field.operator, true);
                                    }
                                    GridColumn::Value => {}
                                }
                            }
                            Ok(())
                        });
                    });
                }
            }
            DatabaseAction::Copy => {
                self.copy_request = self
                    .panel
                    .try_lock()
                    .ok()
                    .map(|panel| panel.query_text.clone())
                    .filter(|text| !text.trim().is_empty());
            }
            DatabaseAction::Rebuild => self.with_panel_mut(|panel| panel.rebuild(true)),
        }
    }

    fn run(&mut self, kind: QueryKind) {
        let cancellation = QueryCancellation::new();
        self.cancellation = Some(cancellation.clone());
        self.spawn(TaskKind::Run(cancellation, kind));
    }

    fn open_form(&mut self) {
        self.form = Some(ConnectionForm::default());
        self.motion.open_dialog(Instant::now());
    }

    fn close_form(&mut self) {
        self.motion.close_dialog(Instant::now());
    }

    fn spawn(&mut self, kind: TaskKind) {
        let panel = Arc::clone(&self.panel);
        self.operation = true;
        self.task = Some(self.runtime.spawn(async move {
            let mut panel = panel.lock().await;
            match kind {
                TaskKind::Test(form) => match panel.test_connection(&form).await {
                    Ok(duration) => {
                        panel.status = Some(format!(
                            "Connection successful in {} ms",
                            duration.as_millis()
                        ));
                        panel.error = None;
                    }
                    Err(error) => panel.error = Some(error.to_string()),
                },
                TaskKind::Save(form) => match panel.add_connection(&form).await {
                    Ok(_) => load_workspace(&mut panel).await,
                    Err(error) => panel.error = Some(error.to_string()),
                },
                TaskKind::Catalog => load_workspace(&mut panel).await,
                TaskKind::Run(cancellation, kind) => {
                    if let Err(error) = panel.execute(cancellation, kind).await {
                        panel.error = Some(error.to_string());
                    }
                }
                TaskKind::Edit(statement) => {
                    if let Err(error) = panel.run_edit(statement).await {
                        panel.error = Some(error.to_string());
                    }
                }
            }
        }));
    }
}

/// After connecting: the tables and views on the left and the diagram on the right.
async fn load_workspace(panel: &mut DatabasePanel) {
    match panel.load_catalog().await {
        Ok(()) => panel.tab = DatabaseTab::Diagram,
        Err(error) => panel.error = Some(format!("Could not read the tables: {error}")),
    }
}

fn contains(area: Rect, point: (u16, u16)) -> bool {
    point.0 >= area.x
        && point.0 < area.x.saturating_add(area.width)
        && point.1 >= area.y
        && point.1 < area.y.saturating_add(area.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_form_and_query_are_keyboard_driven() {
        let directory = tempfile::tempdir().unwrap();
        let mut controller =
            DatabaseController::with_config_path(directory.path().join("connections.toml"))
                .unwrap();
        controller.action(DatabaseAction::AddConnection);
        for character in "Local".chars() {
            controller.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(controller.form.as_ref().unwrap().name, "Local");
        controller.action(DatabaseAction::CancelForm);
        controller.action(DatabaseAction::Tab(DatabaseTab::Sql));
        controller.paste("SELECT 1");
        assert_eq!(controller.panel.blocking_lock().query_text, "SELECT 1");
        assert!(controller.panel.blocking_lock().sql_edited);
    }

    #[test]
    fn form_close_and_tab_changes_use_the_animation_clock() {
        let directory = tempfile::tempdir().unwrap();
        let mut controller =
            DatabaseController::with_config_path(directory.path().join("connections.toml"))
                .unwrap();
        controller.action(DatabaseAction::AddConnection);
        controller.action(DatabaseAction::CancelForm);
        let closed = Instant::now();
        controller.tick_at(closed);
        assert!(controller.form.is_some());
        controller.tick_at(closed + crate::animation::DIALOG.close);
        assert!(controller.form.is_none());

        controller.panel.blocking_lock().tab = DatabaseTab::Sql;
        let start = Instant::now();
        controller.tick_at(start);
        assert_eq!(controller.motion.content(), 0.0);
        assert_eq!(controller.motion.edge(), crate::animation::Edge::Right);
        controller.tick_at(start + crate::animation::TAB.open / 2);
        assert!(controller.motion.content() > 0.0 && controller.motion.content() < 1.0);
        controller.tick_at(start + crate::animation::TAB.open);
        assert_eq!(controller.motion.content(), 1.0);
    }

    #[test]
    fn runs_sqlite_query_asynchronously() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("test.db");
        let mut controller =
            DatabaseController::with_config_path(directory.path().join("connections.toml"))
                .unwrap();
        let mut form = ConnectionForm::default();
        form.name = "Test".into();
        form.database = database.display().to_string();
        controller.spawn(TaskKind::Save(form));
        wait(&mut controller);
        controller
            .panel
            .blocking_lock()
            .insert_query("CREATE TABLE items (id INTEGER); INSERT INTO items VALUES (1);");
        controller.action(DatabaseAction::RunQuery);
        wait(&mut controller);
        let panel = controller.panel.blocking_lock();
        assert!(panel.error.is_none(), "{:?}", panel.error);
        assert!(panel.results.is_some());
    }

    #[test]
    fn connecting_opens_the_diagram_and_the_builder_makes_runs_and_copies_sql() {
        let directory = tempfile::tempdir().unwrap();
        let mut controller =
            DatabaseController::with_config_path(directory.path().join("connections.toml"))
                .unwrap();
        let mut form = ConnectionForm::default();
        form.name = "Shop".into();
        form.database = directory.path().join("shop.db").display().to_string();
        controller.spawn(TaskKind::Save(form));
        wait(&mut controller);
        controller.panel.blocking_lock()
            .insert_query(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);\
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id), total REAL);\
             INSERT INTO customers VALUES (1, 'Ana'), (2, 'Rui');\
             INSERT INTO orders VALUES (1, 1, 5), (2, 1, 20), (3, 2, 7);",
        );
        controller.action(DatabaseAction::RunQuery);
        wait(&mut controller);
        // Connecting again reads the new tables and opens the diagram.
        controller.action(DatabaseAction::Tab(DatabaseTab::Connections));
        controller.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        wait(&mut controller);
        assert_eq!(controller.panel.blocking_lock().tab, DatabaseTab::Diagram);
        let key = |controller: &mut DatabaseController, code| {
            controller.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        };
        controller.focus = DatabaseFocus::Objects;
        key(&mut controller, KeyCode::Enter);
        key(&mut controller, KeyCode::Down);
        key(&mut controller, KeyCode::Char(' '));
        assert_eq!(
            controller.panel.blocking_lock().design.objects,
            ["customers", "orders"]
        );
        // In the diagram: customers.name and orders.total.
        key(&mut controller, KeyCode::Right);
        assert_eq!(controller.focus, DatabaseFocus::Content);
        key(&mut controller, KeyCode::Down);
        key(&mut controller, KeyCode::Enter);
        controller.action(DatabaseAction::Column(1, 2));
        assert_eq!(controller.diagram_cursor, 4);
        // In the builder: Sum of the total, the biggest first, only above 6.
        controller.action(DatabaseAction::Tab(DatabaseTab::Builder));
        controller.focus = DatabaseFocus::Content;
        for _ in 0..3 {
            controller.action(DatabaseAction::Cell(1, GridColumn::Total));
        }
        controller.action(DatabaseAction::Cell(1, GridColumn::Sort));
        controller.action(DatabaseAction::Cell(1, GridColumn::Sort));
        for _ in 0..5 {
            controller.action(DatabaseAction::Cell(1, GridColumn::Filter));
        }
        key(&mut controller, KeyCode::Right);
        assert_eq!(controller.builder_column, GridColumn::Value);
        key(&mut controller, KeyCode::Char('6'));
        controller.paste("0");
        key(&mut controller, KeyCode::Backspace);
        let sql = controller.panel.blocking_lock().query_text.clone();
        assert_eq!(
            sql,
            "SELECT customers.name,\n       SUM(orders.total) AS sum_total\n\
             FROM customers\n  JOIN orders ON orders.customer_id = customers.id\n\
             GROUP BY customers.name\nHAVING SUM(orders.total) > 6\n\
             ORDER BY SUM(orders.total) DESC"
        );
        // A builder that cannot make its SQL does not run the old one.
        controller.action(DatabaseAction::Cell(0, GridColumn::Total));
        controller.action(DatabaseAction::Cell(0, GridColumn::Total));
        controller.action(DatabaseAction::Cell(0, GridColumn::Total));
        key(&mut controller, KeyCode::F(5));
        assert!(!controller.operation);
        let error = controller.panel.blocking_lock().error.clone().unwrap();
        assert!(
            error.starts_with("Sum needs a column of numbers"),
            "{error}"
        );
        for _ in 0..4 {
            controller.action(DatabaseAction::Cell(0, GridColumn::Total));
        }
        assert_eq!(controller.panel.blocking_lock().query_text, sql);
        key(&mut controller, KeyCode::F(5));
        assert_eq!(controller.focus, DatabaseFocus::Results);
        wait(&mut controller);
        let rows = controller
            .panel
            .blocking_lock()
            .results
            .clone()
            .unwrap()
            .rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1], crate::database::DatabaseValue::Float(25.0));
        key(&mut controller, KeyCode::Right);
        key(&mut controller, KeyCode::Down);
        key(&mut controller, KeyCode::Down);
        key(&mut controller, KeyCode::Right);
        assert_eq!(
            controller.cell,
            (1, 1),
            "the cell stops at the last row and column"
        );
        controller.action(DatabaseAction::Copy);
        assert_eq!(controller.copy_request.as_deref(), Some(sql.as_str()));
        key(&mut controller, KeyCode::F(6));
        assert_eq!(controller.focus, DatabaseFocus::Objects);
        // Delete takes the row out of the builder.
        controller.action(DatabaseAction::Tab(DatabaseTab::Builder));
        controller.focus = DatabaseFocus::Content;
        key(&mut controller, KeyCode::Delete);
        assert_eq!(controller.panel.blocking_lock().design.fields.len(), 1);
    }

    #[test]
    fn a_cell_is_edited_only_after_its_statement_is_shown_and_results_are_exported() {
        let directory = tempfile::tempdir().unwrap();
        let mut controller =
            DatabaseController::with_config_path(directory.path().join("connections.toml"))
                .unwrap();
        let mut form = ConnectionForm::default();
        form.name = "Edit".into();
        form.database = directory.path().join("edit.db").display().to_string();
        controller.spawn(TaskKind::Save(form));
        wait(&mut controller);
        controller.panel.blocking_lock().insert_query(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t (name) VALUES ('a'), ('b')",
        );
        controller.action(DatabaseAction::RunQuery);
        wait(&mut controller);
        controller.spawn(TaskKind::Catalog);
        wait(&mut controller);
        {
            let mut panel = controller.panel.blocking_lock();
            panel.set_query("SELECT id, name FROM t ORDER BY id");
            panel.sql_edited = true;
        }
        controller.action(DatabaseAction::RunQuery);
        wait(&mut controller);
        let key = |controller: &mut DatabaseController, code: KeyCode| {
            controller.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        };
        let name = |controller: &DatabaseController| {
            controller
                .panel
                .blocking_lock()
                .results
                .as_ref()
                .unwrap()
                .rows[0][1]
                .clone()
        };
        // Enter opens the value, Enter again shows the UPDATE; Esc there changes nothing.
        {
            let panel = controller.panel.blocking_lock();
            assert!(panel.error.is_none(), "{:?}", panel.error);
            assert_eq!(
                (panel.tab, controller.focus),
                (DatabaseTab::Diagram, DatabaseFocus::Results)
            );
        }
        key(&mut controller, KeyCode::Right);
        key(&mut controller, KeyCode::Enter);
        assert_eq!(controller.cell_edit.as_deref(), Some("a"));
        key(&mut controller, KeyCode::Backspace);
        for character in "it's".chars() {
            key(&mut controller, KeyCode::Char(character));
        }
        key(&mut controller, KeyCode::Enter);
        let statement = controller.confirm.clone().unwrap();
        assert!(statement.sql.starts_with("UPDATE t SET name = ?"));
        assert!(
            key(&mut controller, KeyCode::Esc),
            "Esc closes the box, not the panel"
        );
        assert!(controller.confirm.is_none());
        assert!(!controller.operation);
        assert_eq!(
            name(&controller),
            crate::database::DatabaseValue::Text("a".into())
        );
        // The same again, with Enter: it runs and the results come back.
        controller.confirm = Some(statement);
        key(&mut controller, KeyCode::Enter);
        wait(&mut controller);
        assert_eq!(
            name(&controller),
            crate::database::DatabaseValue::Text("it's".into())
        );
        // Delete asks too.
        key(&mut controller, KeyCode::Delete);
        assert!(
            controller
                .confirm
                .as_ref()
                .unwrap()
                .sql
                .starts_with("DELETE FROM t")
        );
        key(&mut controller, KeyCode::Esc);
        // Space shows the cell whole, Esc closes it.
        key(&mut controller, KeyCode::Char(' '));
        assert!(controller.cell_popup);
        assert!(key(&mut controller, KeyCode::Esc));
        assert!(!controller.cell_popup);
        // C copies CSV, Shift+M asks for a Markdown file.
        key(&mut controller, KeyCode::Char('c'));
        let (text, file) = controller.export_request.take().unwrap();
        assert_eq!(
            (text.as_str(), file),
            ("id,name\r\n1,it's\r\n2,b\r\n", None)
        );
        key(&mut controller, KeyCode::Char('M'));
        assert_eq!(controller.export_request.take().unwrap().1, Some("md"));
        // Alt+T opens a new SQL tab, Alt+1 goes back to the first.
        let alt = |controller: &mut DatabaseController, character: char| {
            controller.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::ALT))
        };
        alt(&mut controller, 't');
        assert_eq!(controller.panel.blocking_lock().sql_tab, 1);
        alt(&mut controller, '1');
        assert_eq!(
            controller.panel.blocking_lock().query_text,
            "SELECT id, name FROM t ORDER BY id"
        );
        // Ctrl+Space shows the names, Esc hides them without closing the panel.
        controller.focus = DatabaseFocus::Content;
        controller.panel.blocking_lock().set_query("SELECT na");
        controller.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL));
        assert_eq!(controller.panel.blocking_lock().completions, ["name"]);
        controller.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(controller.panel.blocking_lock().query_text, "SELECT name");
        assert_eq!(
            controller.panel.blocking_lock().tab,
            DatabaseTab::Sql,
            "Tab took the name, not the next tab"
        );
    }

    fn wait(controller: &mut DatabaseController) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while controller.operation && std::time::Instant::now() < deadline {
            controller.tick();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!controller.operation);
    }
}
