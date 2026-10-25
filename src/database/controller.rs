use std::{sync::Arc, time::Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use tokio::{runtime::Runtime, sync::Mutex, task::JoinHandle};

use crate::{
    animation::InterfaceMotion,
    database::{ConnectionField, ConnectionForm, DatabasePanel, DatabaseTab, QueryCancellation},
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
    NewQuery,
    ClearResults,
}

#[derive(Default)]
pub struct DatabaseRegions {
    pub area: Rect,
    pub content: Rect,
    pub actions: Vec<(DatabaseAction, Rect)>,
    pub form_fields: Vec<(ConnectionField, Rect)>,
}

enum TaskKind {
    Test(ConnectionForm),
    Save(ConnectionForm),
    Run(QueryCancellation),
}

pub struct DatabaseController {
    pub panel: Arc<Mutex<DatabasePanel>>,
    pub form: Option<ConnectionForm>,
    pub regions: DatabaseRegions,
    pub motion: InterfaceMotion<DatabaseTab>,
    pub operation: bool,
    pub result_scroll: usize,
    pub result_horizontal_scroll: usize,
    runtime: Runtime,
    task: Option<JoinHandle<()>>,
    cancellation: Option<QueryCancellation>,
}

impl DatabaseController {
    pub fn new() -> Result<Self> {
        Ok(Self {
            panel: Arc::new(Mutex::new(DatabasePanel::new()?)),
            form: None,
            regions: DatabaseRegions::default(),
            motion: InterfaceMotion::new(Instant::now(), DatabaseTab::Connections),
            operation: false,
            result_scroll: 0,
            result_horizontal_scroll: 0,
            runtime: Runtime::new()?,
            task: None,
            cancellation: None,
        })
    }

    #[cfg(test)]
    pub fn with_config_path(path: std::path::PathBuf) -> Result<Self> {
        Ok(Self {
            panel: Arc::new(Mutex::new(DatabasePanel::with_config_path(path)?)),
            form: None,
            regions: DatabaseRegions::default(),
            motion: InterfaceMotion::new(Instant::now(), DatabaseTab::Connections),
            operation: false,
            result_scroll: 0,
            result_horizontal_scroll: 0,
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
        if key.code == KeyCode::Esc {
            return false;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            if let Ok(mut panel) = self.panel.try_lock() {
                panel.tab = if key.code == KeyCode::Tab {
                    panel.tab.next()
                } else {
                    panel.tab.previous()
                };
            }
            return true;
        }
        let tab = self.panel.try_lock().map(|panel| panel.tab).ok();
        match tab {
            Some(DatabaseTab::Connections) => match key.code {
                KeyCode::Up => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.select_previous();
                    }
                }
                KeyCode::Down => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.select_next();
                    }
                }
                KeyCode::Enter => self.action(DatabaseAction::UseConnection),
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.action(DatabaseAction::AddConnection)
                }
                _ => {}
            },
            Some(DatabaseTab::Query) => match key.code {
                KeyCode::Enter
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        || key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.action(DatabaseAction::RunQuery)
                }
                KeyCode::F(5) => self.action(DatabaseAction::RunQuery),
                KeyCode::Esc if self.operation => self.action(DatabaseAction::CancelQuery),
                KeyCode::Enter => self.insert_query("\n"),
                KeyCode::Backspace => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.query_backspace();
                    }
                }
                KeyCode::Delete => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.query_delete();
                    }
                }
                KeyCode::Left => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.query_left();
                    }
                }
                KeyCode::Right => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.query_right();
                    }
                }
                KeyCode::Home => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.query_home();
                    }
                }
                KeyCode::End => {
                    if let Ok(mut panel) = self.panel.try_lock() {
                        panel.query_end();
                    }
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.insert_query(&character.to_string());
                }
                _ => {}
            },
            Some(DatabaseTab::Results) => match key.code {
                KeyCode::Up => self.result_scroll = self.result_scroll.saturating_sub(1),
                KeyCode::Down => self.result_scroll = self.result_scroll.saturating_add(1),
                KeyCode::Left => {
                    self.result_horizontal_scroll = self.result_horizontal_scroll.saturating_sub(1)
                }
                KeyCode::Right => self.result_horizontal_scroll += 1,
                _ => {}
            },
            None => {}
        }
        true
    }

    fn handle_form_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_form(),
            KeyCode::Tab => {
                if let Some(form) = &mut self.form {
                    form.next_field();
                }
            }
            KeyCode::BackTab => {
                if let Some(form) = &mut self.form {
                    form.previous_field();
                }
            }
            KeyCode::Enter => self.action(DatabaseAction::SaveConnection),
            KeyCode::F(5) => self.action(DatabaseAction::TestConnection),
            KeyCode::Left => {
                if let Some(form) = &mut self.form {
                    if form.field == ConnectionField::Driver {
                        form.cycle_driver(false);
                    } else {
                        form.move_left();
                    }
                }
            }
            KeyCode::Right => {
                if let Some(form) = &mut self.form {
                    if form.field == ConnectionField::Driver {
                        form.cycle_driver(true);
                    } else {
                        form.move_right();
                    }
                }
            }
            KeyCode::Home => {
                if let Some(form) = &mut self.form {
                    form.move_home();
                }
            }
            KeyCode::End => {
                if let Some(form) = &mut self.form {
                    form.move_end();
                }
            }
            KeyCode::Backspace => {
                if let Some(form) = &mut self.form {
                    form.backspace();
                }
            }
            KeyCode::Delete => {
                if let Some(form) = &mut self.form {
                    form.delete();
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(form) = &mut self.form {
                    if form.field == ConnectionField::Driver {
                        form.cycle_driver(true);
                    } else {
                        form.insert(&character.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.form.is_some() && !self.motion.dialog_closing() {
            if let Some(form) = &mut self.form {
                form.insert(text);
            }
        } else {
            self.insert_query(text);
        }
    }

    fn insert_query(&mut self, text: &str) {
        if let Ok(mut panel) = self.panel.try_lock() {
            panel.insert_query(text);
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

    pub fn scroll(&mut self, up: bool) {
        let tab = self.panel.try_lock().map(|panel| panel.tab).ok();
        match tab {
            Some(DatabaseTab::Connections) => {
                if let Ok(mut panel) = self.panel.try_lock() {
                    if up {
                        panel.select_previous();
                    } else {
                        panel.select_next();
                    }
                }
            }
            Some(DatabaseTab::Results) => {
                self.result_scroll = if up {
                    self.result_scroll.saturating_sub(3)
                } else {
                    self.result_scroll.saturating_add(3)
                };
            }
            _ => {}
        }
    }

    pub fn action(&mut self, action: DatabaseAction) {
        match action {
            DatabaseAction::Tab(tab) => {
                if let Ok(mut panel) = self.panel.try_lock() {
                    panel.tab = tab;
                }
            }
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
            DatabaseAction::SelectConnection(index) => {
                if let Ok(mut panel) = self.panel.try_lock()
                    && index < panel.connections.len()
                {
                    panel.selection = index;
                }
            }
            DatabaseAction::UseConnection => {
                if let Ok(mut panel) = self.panel.try_lock() {
                    let selected = panel
                        .selected_config()
                        .map(|connection| connection.id.clone());
                    let result = selected.map_or_else(
                        || Err(anyhow::anyhow!("No connection is selected")),
                        |id| panel.set_active_connection(&id),
                    );
                    if let Err(error) = result {
                        panel.error = Some(error.to_string());
                    }
                }
            }
            DatabaseAction::RunQuery => {
                if !self.operation {
                    let cancellation = QueryCancellation::new();
                    self.cancellation = Some(cancellation.clone());
                    self.spawn(TaskKind::Run(cancellation));
                }
            }
            DatabaseAction::CancelQuery => {
                if let Some(cancellation) = &self.cancellation {
                    cancellation.cancel();
                } else if let Ok(panel) = self.panel.try_lock() {
                    panel.cancel_query();
                }
            }
            DatabaseAction::NewQuery => {
                if let Ok(mut panel) = self.panel.try_lock() {
                    panel.set_query("");
                    panel.tab = DatabaseTab::Query;
                }
            }
            DatabaseAction::ClearResults => {
                if let Ok(mut panel) = self.panel.try_lock() {
                    panel.results = None;
                    self.result_scroll = 0;
                    self.result_horizontal_scroll = 0;
                }
            }
        }
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
                TaskKind::Save(form) => {
                    if let Err(error) = panel.add_connection(&form).await {
                        panel.error = Some(error.to_string());
                    }
                }
                TaskKind::Run(cancellation) => {
                    if let Err(error) = panel.execute_query_cancellable(cancellation).await {
                        panel.error = Some(error.to_string());
                    }
                }
            }
        }));
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
        controller.action(DatabaseAction::Tab(DatabaseTab::Query));
        controller.paste("SELECT 1");
        assert_eq!(controller.panel.blocking_lock().query_text, "SELECT 1");
        controller.action(DatabaseAction::NewQuery);
        assert!(controller.panel.blocking_lock().query_text.is_empty());
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
        controller.tick_at(closed + crate::animation::DIALOG_DURATION);
        assert!(controller.form.is_none());

        controller.panel.blocking_lock().tab = DatabaseTab::Results;
        let start = Instant::now();
        controller.tick_at(start);
        assert_eq!(controller.motion.content(), 0.0);
        controller.tick_at(start + crate::animation::CONTENT_DURATION / 2);
        assert!(controller.motion.content() > 0.0 && controller.motion.content() < 1.0);
        controller.tick_at(start + crate::animation::CONTENT_DURATION);
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
            .set_query("CREATE TABLE items (id INTEGER); INSERT INTO items VALUES (1);");
        controller.action(DatabaseAction::RunQuery);
        wait(&mut controller);
        let panel = controller.panel.blocking_lock();
        assert!(panel.error.is_none(), "{:?}", panel.error);
        assert!(panel.results.is_some());
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
