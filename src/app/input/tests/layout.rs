//! Panels, tabs, dividers and the mouse that moves them.

use super::*;

#[test]
fn shortcuts_toggle_all_panels() {
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(!app.show_explorer);
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert!(app.show_bottom_terminal);
    let right_terminal_key = if cfg!(any(target_os = "macos", target_os = "linux")) {
        KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        )
    } else {
        KeyEvent::new(
            KeyCode::Char('B'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )
    };
    app.handle_key(right_terminal_key);
    assert!(!app.show_explorer);
    assert!(app.show_right_terminal);
    // Each one took the focus; out of the terminal the Ctrl keys are the commands (Q81).
    app.handle_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    assert!(app.show_git);
    assert!(!app.show_database);
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(app.show_database);
    assert!(!app.show_git);
}

#[test]
fn mouse_opens_files_and_controls_tabs() {
    let mut app = test_app();
    app.regions.explorer = Rect::new(0, 0, 30, 20);
    let file_index = app
        .entries
        .iter()
        .position(|entry| entry.path == app.root.join("Cargo.toml"))
        .unwrap();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 2,
        row: file_index as u16 + 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.open_files.len(), 1);
    app.open_file(app.root.join("src/main.rs")).unwrap();
    app.regions.explorer = Rect::default();
    app.regions.tab_items = vec![
        (0, Rect::new(0, 0, 12, 1), Rect::new(12, 0, 3, 1)),
        (1, Rect::new(15, 0, 12, 1), Rect::new(27, 0, 3, 1)),
    ];
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 3,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.active_tab, Some(0));
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 13,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.open_files.len(), 1);
}

#[test]
fn mouse_changes_focus_and_scrolls_panels() {
    let mut app = test_app();
    app.open_file(app.root.join("src/main.rs")).unwrap();
    app.show_bottom_terminal = true;
    app.show_right_terminal = true;
    app.regions.editor = Rect::new(30, 2, 60, 20);
    app.regions.editor_body = app.regions.editor;
    app.regions.pane_bodies[0] = app.regions.editor;
    app.regions.bottom_terminal = Rect::new(30, 22, 60, 10);
    app.regions.right_terminal = Rect::new(90, 0, 40, 32);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 40,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.open_files[0].scroll, 3);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 40,
        row: 25,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 95,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    for (column, row, expected) in [
        (40, 25, Focus::BottomTerminal),
        (95, 5, Focus::RightTerminal),
        (40, 5, Focus::Editor),
    ] {
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.focus, expected);
    }
}

#[test]
fn dragging_resizers_changes_panel_dimensions() {
    let mut app = test_app();
    app.regions.body = Rect::new(0, 1, 120, 38);
    app.regions.tiling = app.tiles.place(app.regions.body, &|panel, direction| {
        app.panel_length(panel, direction)
    });
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 30,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 39,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(crossterm::event::MouseButton::Left),
        column: 39,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.explorer_width, 39);
}

#[test]
fn a_panel_held_by_its_title_lands_where_it_is_dropped() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    let right = app.regions.right_terminal;
    let bottom = app.regions.bottom_terminal;
    // A click on a title only gives the focus.
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        right.x + 2,
        right.y,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        right.x + 2,
        right.y,
    );
    assert_eq!(app.focus, Focus::RightTerminal);
    assert_eq!(app.tiles, Tile::default());

    // The Explorer on top of the right terminal: the rest follows on the next frame.
    let title = app.regions.explorer.y;
    drag_panel(
        &mut app,
        (2, title),
        (right.x + right.width / 2, right.y + 2),
    );
    draw(&mut app, &mut terminal);
    assert_eq!(app.regions.explorer.x, right.x);
    assert!(app.regions.right_terminal.y > app.regions.explorer.y);
    assert_eq!(app.regions.editor.x, 0, "the editor takes the room it left");

    // The bottom terminal over the whole top of the window.
    let (title, top) = (app.regions.bottom_terminal.y, app.regions.body.y);
    drag_panel(&mut app, (bottom.x + 2, title), (40, top));
    draw(&mut app, &mut terminal);
    assert_eq!(app.regions.bottom_terminal.y, app.regions.body.y);
    assert_eq!(app.regions.bottom_terminal.width, 120);
    assert!(app.tiles.is_complete());
}

#[test]
fn the_editor_moves_from_the_free_part_of_its_tab_bar_and_tabs_still_click() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("note.txt");
    std::fs::write(&path, "text\n").unwrap();
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    app.open_file(path).unwrap();
    draw(&mut app, &mut terminal);
    let editor = app.regions.tiling.area(Panel::Editor);
    let right = app.regions.right_terminal;
    // On the tab it is the tab that moves, not the panel.
    let (_, tab, _) = app.regions.tab_items[0];
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        tab.x + 1,
        tab.y,
    );
    assert!(app.panel_drag.is_none() && app.tab_drag.is_some());
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        tab.x + 1,
        tab.y,
    );

    drag_panel(
        &mut app,
        (editor.right() - 2, editor.y),
        (right.x + right.width / 2, right.y + right.height / 2),
    );
    draw(&mut app, &mut terminal);
    let moved = app.regions.tiling.area(Panel::Editor);
    assert!(moved.x > app.regions.right_terminal.x, "swapped");
    assert_eq!(app.regions.right_terminal.x, editor.x);
}

#[test]
fn the_preview_shows_where_a_panel_really_ends_up() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    let right = app.regions.right_terminal;
    let title = app.regions.explorer;
    for (to, swap) in [
        // A side of a panel, its centre and the edge of the window.
        ((right.x + right.width / 2, right.y + 2), false),
        (
            (right.x + right.width / 2, right.y + right.height / 2),
            true,
        ),
        ((60, app.regions.body.bottom() - 1), false),
    ] {
        let tiles = app.tiles.clone();
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            2,
            title.y,
        );
        assert_eq!(app.panel_landing(), None, "held, not moved yet");
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            to.0,
            to.1,
        );
        let landing = app.panel_landing().unwrap();
        draw(&mut app, &mut terminal);
        let screen = terminal.backend().to_string();
        let name = if swap {
            "Swap with Right terminal"
        } else {
            "Explorer here"
        };
        assert!(screen.contains(name), "{name}: {screen}");
        // Nothing moved yet, and letting go puts it exactly there.
        assert_eq!(app.tiles, tiles);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), to.0, to.1);
        draw(&mut app, &mut terminal);
        assert_eq!(app.regions.explorer, landing);
        // Back to the start for the next one.
        app.tiles = Tile::default();
        draw(&mut app, &mut terminal);
    }
}

#[test]
fn a_panel_moves_by_any_corner_of_its_border() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    let bottom = app.regions.bottom_terminal;
    for corner in [
        (bottom.x, bottom.y),
        (bottom.right() - 1, bottom.bottom() - 1),
        (bottom.x, bottom.bottom() - 1),
    ] {
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            corner.0,
            corner.1,
        );
        assert_eq!(
            app.panel_drag.map(|drag| drag.panel),
            Some(Panel::BottomTerminal),
            "{corner:?}"
        );
        assert!(app.terminal_selection.is_none());
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            corner.0,
            corner.1,
        );
    }
    // Inside the border the mouse is the terminal's again.
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        bottom.x + 3,
        bottom.y + 3,
    );
    assert!(app.panel_drag.is_none() && app.terminal_selection.is_some());
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        bottom.x + 3,
        bottom.y + 3,
    );
}

#[test]
fn borders_resize_a_panel_in_both_directions_wherever_it_is() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    // The right terminal under the Explorer, then the two as one column on the left.
    app.tiles = app.tiles.clone().moved(
        Panel::RightTerminal,
        Some(Panel::Explorer),
        DropZone::Bottom,
    );
    draw(&mut app, &mut terminal);
    let explorer = app.regions.explorer;
    let right = app.regions.right_terminal;
    assert_eq!(explorer.x, right.x);
    // Its right border makes the column wider, for the two.
    let (x, y) = (right.right() - 1, right.y + 5);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x + 8, y);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x + 8, y);
    draw(&mut app, &mut terminal);
    // The divider comes under the pointer, a column from the border.
    assert_eq!(app.regions.right_terminal.width, right.width + 7);
    assert_eq!(app.regions.explorer.width, right.width + 7);
    // The bottom border of the Explorer makes it taller and the terminal shorter.
    let explorer = app.regions.explorer;
    let right = app.regions.right_terminal;
    let (x, y) = (explorer.x + 4, explorer.bottom() - 1);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x, y + 4);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y + 4);
    draw(&mut app, &mut terminal);
    assert_eq!(app.regions.explorer.height, explorer.height + 3);
    assert_eq!(app.regions.right_terminal.y, right.y + 3);
    assert!(app.panel_drag.is_none());
}

#[test]
fn a_border_with_nothing_beside_it_does_not_take_the_click() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    // The left border of the Explorer: the window ends there.
    let explorer = app.regions.explorer;
    assert!(app.divider_beside((explorer.x, explorer.y + 5)).is_none());
    // The tabs of a terminal keep their clicks on its top border.
    let (_, _, tab, _) = app.regions.terminal_tabs[0];
    assert!(app.panel_handle_at((tab.x, tab.y)).is_none());
    assert!(app.divider_beside((tab.x, tab.y)).is_none());
}

#[test]
fn dividers_resize_panels_on_any_side() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    // The right terminal, by the divider on its left.
    let divider = app.regions.tiling.dividers[1].handle;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        divider.x,
        5,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        divider.x - 5,
        5,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        divider.x - 5,
        5,
    );
    assert_eq!(app.right_terminal_width, 47);
    // The Explorer on top of the window: its divider sets its height.
    app.tiles = app
        .tiles
        .clone()
        .moved(Panel::Explorer, None, DropZone::Top);
    draw(&mut app, &mut terminal);
    let divider = app.regions.tiling.dividers[0].handle;
    assert_eq!(divider.height, 1);
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        5,
        divider.y,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        5,
        divider.y + 3,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        5,
        divider.y + 3,
    );
    assert_eq!(app.explorer_height, 15);
    draw(&mut app, &mut terminal);
    assert_eq!(app.regions.explorer.height, 15);
}
