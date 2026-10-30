use std::path::Path;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
};

use crate::{
    database::{
        Catalog, ConnectionField, DatabasePanel, DatabaseTab, DatabaseValue, Design,
        controller::{DatabaseAction, DatabaseController, DatabaseFocus, GridColumn},
    },
    editor::highlight_lines,
    ui::{
        ACTIVE, BORDER, MUTED, TEXT, busy, centered, key, panel_block, render_dialog, skip_columns,
    },
};

/// The colours of the tables in the diagram, from the terminal's palette.
const TABLE_COLOURS: [Color; 6] = [
    Color::Cyan,
    Color::Green,
    Color::Magenta,
    Color::Yellow,
    Color::Blue,
    Color::Red,
];

/// `key` is the key the user gave the Database (F29).
pub fn render(frame: &mut Frame, controller: &mut DatabaseController, area: Rect, key: &str) {
    controller.regions.area = area;
    controller.regions.content = Rect::default();
    controller.regions.objects = Rect::default();
    controller.regions.results = Rect::default();
    controller.regions.actions.clear();
    controller.regions.form_fields.clear();
    let block = panel_block(format!(" Database [{key}] "), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [tabs, toolbar, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(inner);
    let current = controller.panel.try_lock().map(|panel| panel.tab).ok();
    let mut x = tabs.x;
    for tab in DatabaseTab::ALL {
        button(
            frame,
            controller,
            &mut x,
            tabs.y,
            tab.label(),
            DatabaseAction::Tab(tab),
            current == Some(tab),
        );
    }
    let panel_handle = std::sync::Arc::clone(&controller.panel);
    let Ok(panel) = panel_handle.try_lock() else {
        controller.regions.content = body;
        frame.render_widget(
            Paragraph::new(busy(
                controller.motion.spinner(),
                "Database operation in progress...",
            ))
            .alignment(Alignment::Center),
            body,
        );
        render_status(frame, controller, status, None);
        return;
    };
    if panel.tab == DatabaseTab::Connections {
        controller.regions.content = body;
        render_connections(frame, controller, &panel, toolbar, body);
    } else {
        render_workspace(frame, controller, &panel, toolbar, body);
    }
    render_status(frame, controller, status, Some(&panel));
}

fn render_connections(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    toolbar: Rect,
    body: Rect,
) {
    let mut x = toolbar.x;
    button(
        frame,
        controller,
        &mut x,
        toolbar.y,
        "Add Connection [N]",
        DatabaseAction::AddConnection,
        false,
    );
    button(
        frame,
        controller,
        &mut x,
        toolbar.y,
        "Connect [Enter]",
        DatabaseAction::UseConnection,
        false,
    );
    if panel.connections.is_empty() {
        frame.render_widget(
            Paragraph::new("No connections. Select Add Connection or press N to create one.")
                .style(Style::default().fg(MUTED)),
            body,
        );
        return;
    }
    let lines = panel
        .connections
        .iter()
        .enumerate()
        .map(|(index, connection)| {
            let row = body.y + 1 + index as u16;
            if row < body.bottom() {
                controller.regions.actions.push((
                    DatabaseAction::SelectConnection(index),
                    Rect::new(body.x, row, body.width, 1),
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
    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            y: body.y + 1,
            ..body
        }
        .intersection(body),
    );
}

/// After connecting (Q37): tables and views on the left, the tab on the right, the rows of the
/// last query across the bottom.
fn render_workspace(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    toolbar: Rect,
    body: Rect,
) {
    let mut x = toolbar.x;
    for (label, action) in [
        (format!("Run [{}]", key("F5")), DatabaseAction::RunQuery),
        ("Cancel".to_owned(), DatabaseAction::CancelQuery),
        ("Copy SQL".to_owned(), DatabaseAction::Copy),
        ("Rebuild".to_owned(), DatabaseAction::Rebuild),
        ("Clear results".to_owned(), DatabaseAction::ClearResults),
    ] {
        button(frame, controller, &mut x, toolbar.y, &label, action, false);
    }
    let Some(catalog) = &panel.catalog else {
        controller.regions.content = body;
        frame.render_widget(
            Paragraph::new(
                "Connect to a database first: choose one in Connections and press Enter.",
            )
            .style(Style::default().fg(MUTED)),
            body,
        );
        return;
    };
    let results_height = if panel.results.is_some() {
        (body.height * 2 / 5).max(6)
    } else {
        3
    };
    let [top, results] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(results_height)]).areas(body);
    let [objects, content] = Layout::horizontal([
        Constraint::Length((top.width / 4).clamp(18, 32)),
        Constraint::Min(10),
    ])
    .areas(top);
    controller.regions.objects = objects;
    controller.regions.results = results;
    render_objects(frame, controller, panel, catalog, objects);
    let focused = controller.focus == DatabaseFocus::Content;
    let block = focus_block(format!(" {} ", panel.tab.label()), focused);
    let inner = block.inner(content);
    frame.render_widget(block, content);
    controller.regions.content = inner;
    match panel.tab {
        DatabaseTab::Diagram => render_diagram(frame, controller, panel, catalog, inner),
        DatabaseTab::Builder => render_builder(
            frame,
            controller,
            &panel.design,
            panel.builder_error.as_deref(),
            inner,
        ),
        DatabaseTab::Sql => render_sql(frame, controller, panel, inner),
        DatabaseTab::Connections => {}
    }
    render_results(frame, controller, panel, results);
}

fn focus_block(title: String, focused: bool) -> Block<'static> {
    let colour = if focused { ACTIVE } else { BORDER };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(colour))
        .title(Span::styled(
            title,
            Style::default().fg(if focused { ACTIVE } else { TEXT }),
        ))
}

/// The cursor row: reversed where the keys go, bold elsewhere.
fn cursor_style(cursor: bool, focused: bool) -> Style {
    match (cursor, focused) {
        (true, true) => Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED),
        (true, false) => Style::default().add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

fn render_objects(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    catalog: &Catalog,
    area: Rect,
) {
    let focused = controller.focus == DatabaseFocus::Objects;
    let block = focus_block(" Tables & views ".into(), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    controller.object_cursor = controller
        .object_cursor
        .min(catalog.objects.len().saturating_sub(1));
    // Tables then views, each under its heading.
    let mut rows = Vec::<(Option<usize>, Line<'static>)>::new();
    for view in [false, true] {
        let members = catalog
            .objects
            .iter()
            .enumerate()
            .filter(|(_, object)| object.view == view)
            .collect::<Vec<_>>();
        rows.push((
            None,
            Line::from(Span::styled(
                format!(
                    "{} ({})",
                    if view { "Views" } else { "Tables" },
                    members.len()
                ),
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
        ));
        for (index, object) in members {
            let chosen = panel.design.objects.contains(&object.name);
            rows.push((
                Some(index),
                Line::from(vec![
                    Span::styled(
                        if chosen { "[x] " } else { "[ ] " },
                        Style::default().fg(if chosen { ACTIVE } else { MUTED }),
                    ),
                    Span::raw(object.name.clone()),
                ])
                .style(cursor_style(index == controller.object_cursor, focused)),
            ));
        }
    }
    let cursor_row = rows
        .iter()
        .position(|(index, _)| *index == Some(controller.object_cursor))
        .unwrap_or(0);
    let first = (cursor_row + 1).saturating_sub(usize::from(inner.height).max(1));
    let mut lines = Vec::new();
    for (row, (index, line)) in rows
        .into_iter()
        .enumerate()
        .skip(first)
        .take(inner.height.into())
    {
        if let Some(index) = index {
            controller.regions.actions.push((
                DatabaseAction::Object(index),
                Rect::new(inner.x, inner.y + (row - first) as u16, inner.width, 1),
            ));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The tables of the design as boxes left to right with their columns: `[x]` for the ones in
/// the builder, `*` a primary key, `→` a foreign key; boxes next to each other that are
/// related are joined by a line, and every relation is listed under them.
fn render_diagram(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    catalog: &Catalog,
    area: Rect,
) {
    let design = &panel.design;
    if design.objects.is_empty() {
        frame.render_widget(
            Paragraph::new("Choose tables and views on the left to draw them here.")
                .style(Style::default().fg(MUTED)),
            area,
        );
        return;
    }
    let focused = controller.focus == DatabaseFocus::Content;
    let places = DatabaseController::diagram_columns(panel);
    controller.diagram_cursor = controller
        .diagram_cursor
        .min(places.len().saturating_sub(1));
    let relations = design
        .objects
        .iter()
        .enumerate()
        .flat_map(|(index, a)| {
            design.objects[index + 1..]
                .iter()
                .flat_map(move |b| catalog.keys_between(a, b))
        })
        .collect::<Vec<_>>();
    let diagram = Rect {
        height: area
            .height
            .saturating_sub(relations.len().min(4) as u16 + 1),
        ..area
    };
    let gap = 6;
    let (mut x, mut y, mut row_height) = (diagram.x, diagram.y, 0);
    let mut boxes = Vec::<(String, Rect)>::new();
    for (number, name) in design.objects.iter().enumerate() {
        let Some(index) = catalog
            .objects
            .iter()
            .position(|object| &object.name == name)
        else {
            continue;
        };
        let object = &catalog.objects[index];
        let keys = |column: &str| {
            catalog
                .keys
                .iter()
                .any(|key| key.from == object.name && key.from_column == column)
        };
        let lines = object
            .columns
            .iter()
            .map(|column| {
                let mark = if column.key {
                    "*"
                } else if keys(&column.name) {
                    "→"
                } else {
                    " "
                };
                format!("{mark} {}  {}", column.name, column.kind.to_lowercase())
            })
            .collect::<Vec<_>>();
        let width = lines
            .iter()
            .map(|line| line.chars().count() + 4)
            .chain([object.name.chars().count() + 4])
            .max()
            .unwrap_or(10) as u16
            + 2;
        let height = object.columns.len() as u16 + 2;
        if x > diagram.x && x + width > diagram.right() {
            (x, y) = (diagram.x, y + row_height + 1);
            row_height = 0;
        }
        let place = Rect::new(x, y, width, height).intersection(diagram);
        let colour = TABLE_COLOURS[number % TABLE_COLOURS.len()];
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(colour))
            .title(Span::styled(
                format!(" {} ", object.name),
                Style::default().fg(colour).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(place);
        frame.render_widget(block, place);
        for (column, text) in lines.into_iter().enumerate() {
            let line_area = Rect::new(inner.x, inner.y + column as u16, inner.width, 1);
            if line_area.y >= inner.bottom() {
                break;
            }
            let chosen = design
                .field(&object.name, &object.columns[column].name)
                .is_some();
            let cursor = places.get(controller.diagram_cursor) == Some(&(index, column));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        if chosen { "[x] " } else { "[ ] " },
                        Style::default().fg(if chosen { colour } else { MUTED }),
                    ),
                    Span::raw(text),
                ]))
                .style(cursor_style(cursor, focused)),
                line_area,
            );
            controller
                .regions
                .actions
                .push((DatabaseAction::Column(index, column), line_area));
        }
        boxes.push((object.name.clone(), place));
        x += width + gap;
        row_height = row_height.max(height);
    }
    // Lines between neighbours on the same row, from one key column to the other.
    for pair in boxes.windows(2) {
        let ((left, from), (right, to)) = (&pair[0], &pair[1]);
        if from.y != to.y {
            continue;
        }
        for key in catalog.keys_between(left, right) {
            let row_of = |table: &str, place: &Rect| {
                let column = if key.from == table {
                    &key.from_column
                } else {
                    &key.to_column
                };
                catalog
                    .object(table)
                    .and_then(|object| {
                        object
                            .columns
                            .iter()
                            .position(|other| &other.name == column)
                    })
                    .map(|index| place.y + 1 + index as u16)
            };
            let (Some(a), Some(b)) = (row_of(left, from), row_of(right, to)) else {
                continue;
            };
            draw_link(frame, from.right(), to.x, a, b, diagram);
        }
    }
    let list = relations
        .iter()
        .take(4)
        .map(|key| {
            Line::from(vec![
                Span::styled(
                    format!("{}.{}", key.from, key.from_column),
                    Style::default().fg(TEXT),
                ),
                Span::styled(" → ", Style::default().fg(MUTED)),
                Span::styled(
                    format!("{}.{}", key.to, key.to_column),
                    Style::default().fg(TEXT),
                ),
            ])
        })
        .collect::<Vec<_>>();
    if !list.is_empty() {
        let height = list.len() as u16;
        frame.render_widget(
            Paragraph::new(list),
            Rect::new(
                area.x,
                area.bottom().saturating_sub(height),
                area.width,
                height,
            ),
        );
    }
}

/// A line in the gap from `(from, a)` to `(to - 1, b)`: across, down or up in the middle,
/// across again.
fn draw_link(frame: &mut Frame, from: u16, to: u16, a: u16, b: u16, clip: Rect) {
    if to <= from + 1 {
        return;
    }
    let middle = from + (to - from) / 2;
    let buffer = frame.buffer_mut();
    let style = Style::default().fg(MUTED);
    let mut put = |x: u16, y: u16, symbol: &str| {
        if clip.contains((x, y).into()) {
            buffer[(x, y)].set_symbol(symbol).set_style(style);
        }
    };
    for x in from..middle {
        put(x, a, "─");
    }
    for x in middle + 1..to {
        put(x, b, "─");
    }
    let (top, bottom) = (a.min(b), a.max(b));
    for y in top + 1..bottom {
        put(middle, y, "│");
    }
    if a == b {
        put(middle, a, "─");
    } else if a < b {
        put(middle, a, "╮");
        put(middle, b, "╰");
    } else {
        put(middle, a, "╯");
        put(middle, b, "╭");
    }
}

/// The builder grid: one row per column chosen, what it shows and how it adds up, sorts and
/// filters; Enter or a click changes a cell, typing fills the value.
fn render_builder(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    design: &Design,
    error: Option<&str>,
    area: Rect,
) {
    if design.fields.is_empty() {
        frame.render_widget(
            Paragraph::new("Tick columns in the Diagram to add them here.")
                .style(Style::default().fg(MUTED)),
            area,
        );
        return;
    }
    let focused = controller.focus == DatabaseFocus::Content;
    controller.builder_row = controller.builder_row.min(design.fields.len() - 1);
    let width = |values: &mut dyn Iterator<Item = usize>, title: &str| {
        values.chain([title.chars().count()]).max().unwrap_or(4) as u16 + 2
    };
    let table = width(
        &mut design
            .fields
            .iter()
            .map(|field| field.object.chars().count()),
        "Table",
    );
    let column = width(
        &mut design
            .fields
            .iter()
            .map(|field| field.column.chars().count()),
        "Column",
    );
    let widths = [table, column, 6, 10, 12, 14, 16];
    let titles = [
        "Table", "Column", "Show", "Total", "Sort", "Filter", "Value",
    ];
    let header = titles
        .iter()
        .zip(widths)
        .map(|(title, width)| {
            Span::styled(
                format!("{title:<width$}", width = usize::from(width)),
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Line::from(header)),
        Rect { height: 1, ..area },
    );
    // Between the header and the hint.
    let room = usize::from(area.height.saturating_sub(2));
    let first = (controller.builder_row + 1).saturating_sub(room.max(1));
    for (row, field) in design.fields.iter().enumerate().skip(first).take(room) {
        let y = area.y + 1 + (row - first) as u16;
        let mut x = area.x;
        let cells = [
            field.object.clone(),
            field.column.clone(),
            if field.show {
                "[x]".into()
            } else {
                "[ ]".into()
            },
            field.total.label().into(),
            field.sort.label().into(),
            field.operator.label().into(),
            field.value.clone(),
        ];
        for (index, (text, width)) in cells.into_iter().zip(widths).enumerate() {
            let cell = Rect::new(x, y, width.min(area.right().saturating_sub(x)), 1);
            let editable = index.checked_sub(2).map(|index| GridColumn::ALL[index]);
            let current =
                editable == Some(controller.builder_column) && row == controller.builder_row;
            let mut style = if index < 2 {
                Style::default().fg(MUTED)
            } else {
                Style::default().fg(TEXT)
            };
            if current {
                style = cursor_style(true, focused);
            }
            let shown = if current && editable == Some(GridColumn::Value) && focused {
                format!("{text}█")
            } else {
                text
            };
            frame.render_widget(Paragraph::new(format!("{shown} ")).style(style), cell);
            if let Some(editable) = editable {
                controller
                    .regions
                    .actions
                    .push((DatabaseAction::Cell(row, editable), cell));
            }
            x += width;
        }
    }
    let hint = match error {
        Some(error) => Span::styled(error.to_owned(), Style::default().fg(Color::Yellow)),
        None => Span::styled(
            "Enter or click: change  Type: value  Delete: remove the row",
            Style::default().fg(MUTED),
        ),
    };
    frame.render_widget(
        Paragraph::new(hint),
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
    );
}

/// The SQL the builder made, or typed over: highlighted, with the cursor when focused.
fn render_sql(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    area: Rect,
) {
    let note = match (&panel.builder_error, panel.sql_edited) {
        (_, true) => Line::from(Span::styled(
            "Edited by hand: Rebuild makes it again from the builder",
            Style::default().fg(MUTED),
        )),
        (Some(error), false) => Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Yellow),
        )),
        (None, false) => Line::from(Span::styled(
            "Made by the builder",
            Style::default().fg(MUTED),
        )),
    };
    frame.render_widget(Paragraph::new(note), Rect { height: 1, ..area });
    // The last queries of the session under the text, newest first.
    let history = panel
        .history
        .iter()
        .rev()
        .take(usize::from(area.height.saturating_sub(4) / 3).min(3))
        .map(|entry| {
            let time =
                chrono::DateTime::<chrono::Local>::from(entry.executed_at).format("%H:%M:%S");
            let (outcome, colour) = match &entry.result {
                Ok(rows) => (format!("{rows} row(s)"), MUTED),
                Err(error) => (format!("Error: {error}"), Color::Red),
            };
            Line::from(vec![
                Span::styled(
                    format!("{time}  {}  ", entry.connection_name),
                    Style::default().fg(MUTED),
                ),
                Span::styled(
                    format!("{outcome}  {} ms  ", entry.duration.as_millis()),
                    Style::default().fg(colour),
                ),
                Span::styled(entry.query.replace('\n', " "), Style::default().fg(MUTED)),
            ])
        })
        .collect::<Vec<_>>();
    let history_height = history.len() as u16;
    if history_height > 0 {
        frame.render_widget(
            Paragraph::new(history),
            Rect::new(
                area.x,
                area.bottom() - history_height,
                area.width,
                history_height,
            ),
        );
    }
    let text = Rect {
        y: area.y + 1,
        height: area
            .height
            .saturating_sub(1 + history_height + u16::from(history_height > 0)),
        ..area
    };
    let lines = panel
        .query_text
        .split('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let before = &panel.query_text[..panel.query_cursor.min(panel.query_text.len())];
    let row = before.matches('\n').count();
    let column = before.rsplit('\n').next().unwrap_or("").chars().count();
    let first = (row + 1).saturating_sub(usize::from(text.height).max(1));
    let highlighted = highlight_lines(Path::new("query.sql"), &lines);
    frame.render_widget(
        Paragraph::new(highlighted.into_iter().map(Line::from).collect::<Vec<_>>())
            .scroll((first as u16, 0)),
        text,
    );
    if controller.focus == DatabaseFocus::Content {
        let (x, y) = (text.x + column as u16, text.y + (row - first) as u16);
        if x < text.right() && y < text.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
}

/// The rows of the last query as a grid (from `SQL_Idea`): row numbers, `│` between columns,
/// `─┼─` under the header, numbers on the right, NULL dimmed; it scrolls down and sideways.
fn render_results(
    frame: &mut Frame,
    controller: &mut DatabaseController,
    panel: &DatabasePanel,
    area: Rect,
) {
    let focused = controller.focus == DatabaseFocus::Results;
    let Some(result) = &panel.results else {
        let block = focus_block(" Results ".into(), focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(format!(
                "Run the query [{}] to see its rows here.",
                key("F5")
            ))
            .style(Style::default().fg(MUTED)),
            inner,
        );
        return;
    };
    let title = format!(
        " Results: {} row{} in {} ms ",
        result.rows.len(),
        if result.rows.len() == 1 { "" } else { "s" },
        result.duration.as_millis()
    );
    let block = focus_block(title, focused)
        .title_bottom(Line::from(" Arrows, wheel, Shift+wheel: scroll ").right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if result.columns.is_empty() {
        frame.render_widget(
            Paragraph::new(format!("{} row(s) affected", result.affected_rows)),
            inner,
        );
        return;
    }
    let text = |value: &DatabaseValue| truncate(&value.to_string(), 40);
    let widths = result
        .columns
        .iter()
        .enumerate()
        .map(|(column, name)| {
            result
                .rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|value| text(value).chars().count())
                .fold(name.chars().count(), usize::max)
        })
        .collect::<Vec<_>>();
    let number_width = result.rows.len().max(1).to_string().len();
    let total = widths.iter().map(|width| width + 3).sum::<usize>();
    let room = usize::from(inner.width).saturating_sub(number_width + 1);
    controller.result_horizontal_scroll = controller
        .result_horizontal_scroll
        .min(total.saturating_sub(room));
    let visible = usize::from(inner.height.saturating_sub(2));
    controller.result_scroll = controller
        .result_scroll
        .min(result.rows.len().saturating_sub(visible));
    let scroll = controller.result_horizontal_scroll;
    let line = |number: Span<'static>, cells: Vec<Span<'static>>| {
        let mut spans = vec![number];
        spans.extend(skip_columns(cells, scroll));
        Line::from(spans)
    };
    let mut lines = vec![line(
        Span::styled(
            format!("{:>number_width$} ", "#"),
            Style::default().fg(MUTED),
        ),
        result
            .columns
            .iter()
            .zip(&widths)
            .flat_map(|(name, width)| {
                [
                    Span::styled("│ ", Style::default().fg(BORDER)),
                    Span::styled(
                        format!("{name:<width$} "),
                        Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
                    ),
                ]
            })
            .collect(),
    )];
    lines.push(line(
        Span::styled("─".repeat(number_width + 1), Style::default().fg(BORDER)),
        widths
            .iter()
            .map(|width| {
                Span::styled(
                    format!("┼{}", "─".repeat(width + 2)),
                    Style::default().fg(BORDER),
                )
            })
            .collect(),
    ));
    for (index, row) in result
        .rows
        .iter()
        .enumerate()
        .skip(controller.result_scroll)
        .take(visible)
    {
        let cells = row
            .iter()
            .zip(&widths)
            .flat_map(|(value, width)| {
                let shown = text(value);
                let (cell, style) = match value {
                    DatabaseValue::Null => (
                        format!("{shown:<width$} "),
                        Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
                    ),
                    DatabaseValue::Integer(_) | DatabaseValue::Float(_) => {
                        (format!("{shown:>width$} "), Style::default().fg(TEXT))
                    }
                    _ => (format!("{shown:<width$} "), Style::default().fg(TEXT)),
                };
                [
                    Span::styled("│ ", Style::default().fg(BORDER)),
                    Span::styled(cell, style),
                ]
            })
            .collect();
        lines.push(line(
            Span::styled(
                format!("{:>number_width$} ", index + 1),
                Style::default().fg(MUTED),
            ),
            cells,
        ));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_status(
    frame: &mut Frame,
    controller: &DatabaseController,
    area: Rect,
    panel: Option<&DatabasePanel>,
) {
    let (message, error) = if controller.operation {
        (busy(controller.motion.spinner(), "Working..."), false)
    } else if let Some(panel) = panel {
        if let Some(error) = &panel.error {
            (Line::from(error.clone()), true)
        } else {
            (Line::from(panel.status.clone().unwrap_or_default()), false)
        }
    } else {
        (Line::default(), false)
    };
    frame.render_widget(
        Paragraph::new(message).style(Style::default().fg(if error { Color::Red } else { MUTED })),
        area,
    );
}

/// The Connection dialog, drawn by `ui::draw` over everything else.
pub fn render_form(frame: &mut Frame, controller: &mut DatabaseController, area: Rect) {
    let Some(form) = &controller.form else {
        return;
    };
    let title = if form.editing_id().is_some() {
        " Edit Connection "
    } else {
        " Add Connection "
    };
    let progress = controller.motion.dialog();
    render_dialog(
        frame,
        centered(area, 66, 82),
        progress,
        title.into(),
        |frame, inner| render_form_fields(frame, controller, inner),
    );
}

fn render_form_fields(frame: &mut Frame, controller: &mut DatabaseController, inner: Rect) {
    let Some(form) = &controller.form else {
        return;
    };
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
        &format!("Test [{}]", crate::ui::key("F5")),
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
