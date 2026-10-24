use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::{
    database::{
        ConnectionField, DatabasePanel, DatabaseTab,
        controller::{DatabaseAction, DatabaseController},
    },
    ui::{ACTIVE, MUTED, TEXT, centered, panel_block, reveal, scale_centered},
};

pub fn render(frame: &mut Frame, controller: &mut DatabaseController, area: Rect) {
    controller.regions.area = area;
    controller.regions.content = Rect::default();
    controller.regions.actions.clear();
    controller.regions.form_fields.clear();
    let block = panel_block(" Database [Ctrl+D] ".into(), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(inner);
    let mut x = sections[0].x;
    for tab in [
        DatabaseTab::Connections,
        DatabaseTab::Query,
        DatabaseTab::Results,
    ] {
        let selected = controller
            .panel
            .try_lock()
            .map(|panel| panel.tab == tab)
            .unwrap_or(false);
        button(
            frame,
            controller,
            &mut x,
            sections[0].y,
            tab.label(),
            DatabaseAction::Tab(tab),
            selected,
        );
    }
    let content_area = reveal(sections[2], controller.motion.content());
    controller.regions.content = content_area;
    let panel_handle = std::sync::Arc::clone(&controller.panel);
    let Ok(panel) = panel_handle.try_lock() else {
        frame.render_widget(
            Paragraph::new(format!(
                "{} Database operation in progress...",
                controller.motion.spinner()
            ))
            .alignment(Alignment::Center),
            content_area,
        );
        render_status(frame, controller, sections[3], None);
        if controller.form.is_some() {
            render_form(frame, controller, area);
        }
        return;
    };
    let mut content_sections = sections.to_vec();
    content_sections[2] = content_area;
    match panel.tab {
        DatabaseTab::Connections => {
            render_connections(frame, controller, &panel, &content_sections)
        }
        DatabaseTab::Query => render_query(frame, controller, &panel, &content_sections),
        DatabaseTab::Results => render_results(frame, controller, &panel, &content_sections),
    }
    render_status(frame, controller, sections[3], Some(&panel));
    drop(panel);
    if controller.form.is_some() {
        render_form(frame, controller, area);
    }
}

fn render_connections(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    sections: &[Rect],
) {
    let mut x = sections[1].x;
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "Add Connection",
        DatabaseAction::AddConnection,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "Use Selected",
        DatabaseAction::UseConnection,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "New Query",
        DatabaseAction::NewQuery,
        false,
    );
    if panel.connections.is_empty() {
        frame.render_widget(
            Paragraph::new("No connections. Select Add Connection or press N to create one.")
                .style(Style::default().fg(MUTED)),
            sections[2],
        );
        return;
    }
    let lines = panel
        .connections
        .iter()
        .enumerate()
        .map(|(index, connection)| {
            let row = sections[2].y + index as u16;
            if row < sections[2].bottom() {
                controller.regions.actions.push((
                    DatabaseAction::SelectConnection(index),
                    Rect::new(sections[2].x, row, sections[2].width, 1),
                ));
            }
            let active = panel.active_connection.as_deref() == Some(&*connection.id);
            let style = if panel.selection == index {
                Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(TEXT)
            };
            Line::from(Span::styled(
                format!(
                    "{} {}  {}  {}",
                    if active { "*" } else { " " },
                    connection.name,
                    connection.driver.label(),
                    connection
                        .database
                        .as_deref()
                        .or(connection.host.as_deref())
                        .unwrap_or("URL")
                ),
                style,
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), sections[2]);
}

fn render_query(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    sections: &[Rect],
) {
    let mut x = sections[1].x;
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "Run [Ctrl+Enter]",
        DatabaseAction::RunQuery,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "Cancel",
        DatabaseAction::CancelQuery,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "New Query",
        DatabaseAction::NewQuery,
        false,
    );
    let active = panel
        .active_config()
        .map(|connection| connection.name.as_str())
        .unwrap_or("No active connection");
    let content = Layout::vertical([Constraint::Min(3), Constraint::Length(7)]).split(sections[2]);
    let query = format!("-- Connection: {active}\n{}", panel.query_text);
    frame.render_widget(
        Paragraph::new(query).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" SQL editor ")
                .style(Style::default().fg(TEXT)),
        ),
        content[0],
    );
    let history = panel
        .history
        .iter()
        .rev()
        .take(6)
        .map(|entry| {
            let result = match &entry.result {
                Ok(rows) => format!("{rows} row(s)"),
                Err(error) => format!("Error: {error}"),
            };
            Line::from(format!(
                "{} [{}] @{} | {} ms | {} | {}",
                entry.connection_name,
                entry.connection_id,
                entry
                    .executed_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0),
                entry.duration.as_millis(),
                result,
                entry.query.replace('\n', " ")
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(history).block(Block::default().title(" Session history ")),
        content[1],
    );
}

fn render_results(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    sections: &[Rect],
) {
    let mut x = sections[1].x;
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "New Query",
        DatabaseAction::NewQuery,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "Run Again",
        DatabaseAction::RunQuery,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        sections[1].y,
        "Clear",
        DatabaseAction::ClearResults,
        false,
    );
    let Some(result) = &panel.results else {
        frame.render_widget(
            Paragraph::new("No results. Run a query in the Query tab.")
                .style(Style::default().fg(MUTED)),
            sections[2],
        );
        return;
    };
    if result.columns.is_empty() || result.is_empty() {
        frame.render_widget(
            Paragraph::new(format!(
                "{} row(s) returned, {} affected row(s)",
                result.rows.len(),
                result.affected_rows
            )),
            sections[2],
        );
        return;
    }
    let widths = result
        .columns
        .iter()
        .enumerate()
        .map(|(column, name)| {
            result
                .rows
                .iter()
                .map(|row| {
                    row.get(column)
                        .map(|value| value.to_string().len())
                        .unwrap_or(0)
                })
                .fold(name.len(), usize::max)
                .clamp(4, 28)
        })
        .collect::<Vec<_>>();
    let start = controller
        .result_horizontal_scroll
        .min(result.columns.len().saturating_sub(1));
    let visible_columns = (start..result.columns.len())
        .take_while(|column| widths[*column] as u16 <= sections[2].width)
        .take(12)
        .collect::<Vec<_>>();
    let header = visible_columns
        .iter()
        .map(|column| {
            format!(
                "{:<width$}",
                result.columns[*column],
                width = widths[*column]
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let max_scroll = result
        .rows
        .len()
        .saturating_sub(sections[2].height as usize);
    controller.result_scroll = controller.result_scroll.min(max_scroll);
    let mut lines = vec![Line::from(Span::styled(
        header,
        Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
    ))];
    lines.extend(
        result
            .rows
            .iter()
            .enumerate()
            .skip(controller.result_scroll)
            .take(sections[2].height.saturating_sub(1) as usize)
            .map(|(row_index, _)| {
                Line::from(
                    visible_columns
                        .iter()
                        .map(|column| {
                            let value = result
                                .cell(row_index, *column)
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            format!(
                                "{:<width$}",
                                truncate(&value, widths[*column]),
                                width = widths[*column]
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" | "),
                )
            }),
    );
    frame.render_widget(Paragraph::new(lines), sections[2]);
}

fn render_status(
    frame: &mut Frame,
    controller: &DatabaseController,
    area: Rect,
    panel: Option<&DatabasePanel>,
) {
    let (message, error) = if controller.operation {
        (format!("{} Working...", controller.motion.spinner()), false)
    } else if let Some(panel) = panel {
        if let Some(error) = &panel.error {
            (error.clone(), true)
        } else {
            (panel.status.clone().unwrap_or_default(), false)
        }
    } else {
        (String::new(), false)
    };
    frame.render_widget(
        Paragraph::new(message).style(Style::default().fg(if error { Color::Red } else { MUTED })),
        area,
    );
}

fn render_form(frame: &mut Frame, controller: &mut DatabaseController, area: Rect) {
    let area = scale_centered(centered(area, 66, 82), controller.motion.dialog());
    if area.width < 2 || area.height < 4 {
        return;
    }
    frame.render_widget(Clear, area);
    let Some(form) = &controller.form else {
        return;
    };
    let title = if form.editing_id().is_some() {
        " Edit Connection "
    } else {
        " Add Connection "
    };
    let block = panel_block(title.into(), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical(
        std::iter::repeat_n(Constraint::Length(1), ConnectionField::ALL.len())
            .chain([
                Constraint::Min(1),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .collect::<Vec<_>>(),
    )
    .split(inner);
    for (index, field) in ConnectionField::ALL.into_iter().enumerate() {
        let value = match field {
            ConnectionField::Name => form.name.clone(),
            ConnectionField::Driver => form.driver.label().to_owned(),
            ConnectionField::Url => form.url.clone(),
            ConnectionField::Database => form.database.clone(),
            ConnectionField::Host => form.host.clone(),
            ConnectionField::Port => form.port.clone(),
            ConnectionField::Username => form.username.clone(),
            ConnectionField::Password => form.password.clone(),
        };
        let shown = if field.is_secret() {
            "*".repeat(value.chars().count())
        } else if field == ConnectionField::Driver {
            format!("< {value} >")
        } else {
            value
        };
        let style = if form.field == field {
            Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(TEXT)
        };
        frame.render_widget(
            Paragraph::new(format!("{:<10} {}", field.label(), shown)).style(style),
            rows[index],
        );
        controller.regions.form_fields.push((field, rows[index]));
    }
    let mut x = rows[ConnectionField::ALL.len() + 1].x;
    let y = rows[ConnectionField::ALL.len() + 1].y;
    button(
        frame,
        controller,
        &mut x,
        y,
        "Save",
        DatabaseAction::SaveConnection,
        true,
    );
    button(
        frame,
        controller,
        &mut x,
        y,
        "Test [F5]",
        DatabaseAction::TestConnection,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        y,
        "Cancel",
        DatabaseAction::CancelForm,
        false,
    );
    frame.render_widget(
        Paragraph::new("Tab: next field  Left/Right: change driver  Enter: save")
            .style(Style::default().fg(MUTED)),
        rows[ConnectionField::ALL.len() + 2],
    );
}

#[allow(clippy::too_many_arguments)]
fn button(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    x: &mut u16,
    y: u16,
    label: &str,
    action: DatabaseAction,
    selected: bool,
) {
    let width = label.len() as u16 + 4;
    let area = Rect::new(*x, y, width, 1);
    let style = if selected {
        Style::default()
            .fg(ACTIVE)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(TEXT)
    };
    frame.render_widget(Paragraph::new(format!(" {label} ")).style(style), area);
    controller.regions.actions.push((action, area));
    *x = x.saturating_add(width + 1);
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        value.to_owned()
    } else {
        value
            .chars()
            .take(width.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}
