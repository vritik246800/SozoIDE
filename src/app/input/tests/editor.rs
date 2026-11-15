//! The editor: text, search, links, PDF, models and videos.

use super::*;

#[test]
fn a_click_on_a_wiki_link_opens_its_document_or_goes_to_its_tab() {
    let directory = tempfile::tempdir().unwrap();
    let notes = directory.path().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(directory.path().join("guide.md"), "# Guide\n").unwrap();
    std::fs::write(notes.join("deep.md"), "# Deep\n").unwrap();
    std::fs::write(
        directory.path().join("index.md"),
        "# Top\n\nSee [[guide|the guide]] and [[deep]] and [[gone]]\n",
    )
    .unwrap();
    let (mut app, mut terminal) = drawn(120, 30);
    app.root = directory.path().to_path_buf();
    app.show_explorer = false;
    app.open_file(directory.path().join("index.md")).unwrap();
    // The cursor elsewhere: the row shows only the text.
    app.open_files[0].move_cursor_to(0, 0);
    draw(&mut app, &mut terminal);
    draw(&mut app, &mut terminal);
    let screen = terminal.backend().to_string();
    assert!(
        screen.contains("See the guide and deep and gone"),
        "{screen}"
    );
    assert!(!screen.contains("[["));

    let click = |app: &mut App, terminal: &mut _, word: &str| {
        draw(app, terminal);
        let (row, line) = terminal_lines(terminal)
            .into_iter()
            .enumerate()
            .find(|(_, line)| line.contains(word))
            .unwrap();
        let column = line.chars().take(line.find(word).unwrap()).count() as u16;
        mouse(app, MouseEventKind::Moved, column, row as u16);
        draw(app, terminal);
        mouse(
            app,
            MouseEventKind::Down(MouseButton::Left),
            column,
            row as u16,
        );
        mouse(
            app,
            MouseEventKind::Up(MouseButton::Left),
            column,
            row as u16,
        );
    };
    click(&mut app, &mut terminal, "the guide");
    assert_eq!(app.open_files.len(), 2);
    assert_eq!(app.open_files[1].name, "guide.md");
    assert_eq!(app.active_tab, Some(1));

    // Open already: the same tab comes on screen.
    app.select_tab(0);
    click(&mut app, &mut terminal, "the guide");
    assert_eq!(app.open_files.len(), 2);
    assert_eq!(app.active_tab, Some(1));

    app.select_tab(0);
    click(&mut app, &mut terminal, "deep");
    assert_eq!(app.open_files[2].name, "deep.md");

    app.select_tab(0);
    click(&mut app, &mut terminal, "gone");
    assert_eq!(app.open_files.len(), 3);
    assert_eq!(app.status, "No document for [[gone]]");
}

#[test]
fn the_search_follows_its_match_in_the_editor_and_enter_selects_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("long.txt");
    let text = (0..60)
        .map(|line| {
            if line % 20 == 5 {
                format!("line {line} Needle")
            } else {
                format!("line {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, text).unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    press(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    app.search_paste("needle");
    let active = app.active_tab.unwrap();
    assert_eq!(app.search.as_ref().unwrap().matches.len(), 3);
    assert_eq!(app.open_files[active].scroll, 2);
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.open_files[active].scroll, 22);
    app.step_match(true);
    assert_eq!(app.open_files[active].scroll, 42);
    app.step_match(true);
    assert_eq!(app.open_files[active].scroll, 2);
    app.step_match(false);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let file = &app.open_files[active];
    assert_eq!(file.anchor, Some((45, 8)));
    assert_eq!((file.cursor_row, file.cursor_column), (45, 14));
    assert_eq!(file.selected_text().as_deref(), Some("Needle"));
}

#[test]
fn the_editor_menu_and_ctrl_v_cut_copy_and_paste_text() {
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "alpha beta\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    let right_click = |app: &mut App| {
        mouse(app, MouseEventKind::Down(MouseButton::Right), 40, 4);
        let menu = app.entry_menu.as_ref().unwrap();
        assert!(menu.editor);
        menu.actions.clone()
    };
    // Without a selection there is only Paste, and nothing to paste yet; then the
    // questions to the language server (F31).
    use crate::lsp::Goal;
    let code = [
        Goal::Definition,
        Goal::Peek,
        Goal::Declaration,
        Goal::TypeDefinition,
        Goal::Implementation,
        Goal::References,
    ]
    .map(EntryAction::Code);
    assert_eq!(
        right_click(&mut app),
        [&[EntryAction::Paste][..], &code].concat()
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.status, "The clipboard is empty");
    // Without a selection the click put the cursor where it was.
    let file = app.active_file().unwrap();
    assert_eq!((file.cursor_row, file.cursor_column), (0, 5));
    press(&mut app, KeyCode::Home, KeyModifiers::NONE);
    for _ in 0..5 {
        press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    }
    assert_eq!(
        right_click(&mut app),
        [
            &[EntryAction::Cut, EntryAction::Copy, EntryAction::Paste][..],
            &code
        ]
        .concat()
    );
    app.choose_entry_action(EntryAction::Copy);
    assert_eq!(app.clipboard.take().as_deref(), Some("alpha"));
    press(&mut app, KeyCode::End, KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(app.active_file().unwrap().lines, ["alpha betaalpha"]);
    press(&mut app, KeyCode::Home, KeyModifiers::SHIFT);
    right_click(&mut app);
    app.choose_entry_action(EntryAction::Cut);
    assert_eq!(app.active_file().unwrap().lines, [""]);
    right_click(&mut app);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.active_file().unwrap().lines, ["alpha betaalpha"]);
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(
        app.active_file().unwrap().lines,
        [""],
        "a paste undoes at once"
    );
}

#[test]
fn models_turn_and_zoom_with_keys_and_the_mouse() {
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("ship.glb"),
        crate::media::sample_glb(),
    )
    .unwrap();
    std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("ship.glb")).unwrap();
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    let drawn = |app: &mut App| {
        let media = app.open_files[0].media.as_mut().unwrap();
        media.lines(40, 15).to_vec()
    };
    let mut last = drawn(&mut app);
    let mut changed = |app: &mut App, what: &str| {
        let now = drawn(app);
        assert_ne!(now, last, "{what}");
        last = now;
    };
    press(&mut app, KeyCode::Right, KeyModifiers::NONE);
    changed(&mut app, "→ turns");
    press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    changed(&mut app, "↑ tilts");
    press(&mut app, KeyCode::Char('+'), KeyModifiers::NONE);
    changed(&mut app, "+ comes closer");
    press(&mut app, KeyCode::Char('-'), KeyModifiers::NONE);
    changed(&mut app, "- goes back");
    mouse(&mut app, MouseEventKind::ScrollUp, 50, 10);
    changed(&mut app, "the wheel zooms");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 50, 10);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 56, 12);
    changed(&mut app, "a drag turns");
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 56, 12);
    assert!(app.turning.is_none() && !app.selecting);
    assert!(
        app.open_files[0].lines.is_empty(),
        "nothing typed into a model"
    );
}

#[test]
fn videos_answer_to_their_keys_buttons_bar_and_wheel() {
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("clip.mp4")).unwrap();
    // A video of 75.5 s with sound, playing; seeking starts `ffmpeg` on the file above,
    // which ends at once: the state is what is checked.
    app.open_files[0].media = Some(crate::media::sample_video());
    app.regions.editor = Rect::new(30, 2, 60, 20);
    app.regions.editor_body = app.regions.editor;
    app.regions.pane_bodies[0] = app.regions.editor;
    let video = |app: &App| {
        let media = app.open_files[0].media.as_ref().unwrap();
        let video = media.video().unwrap();
        (
            video.paused,
            video.volume,
            video.muted,
            video.speed(),
            video.position().as_secs(),
        )
    };
    let key = |app: &mut App, code| press(app, code, KeyModifiers::NONE);

    key(&mut app, KeyCode::Char(' '));
    assert!(video(&app).0, "Space pauses");
    key(&mut app, KeyCode::Right);
    assert_eq!(video(&app).4, 5, "→ 5 s on");
    press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    assert_eq!(video(&app).4, 35, "Shift+→ 30 s on");
    key(&mut app, KeyCode::Left);
    assert_eq!(video(&app).4, 30);
    assert!(video(&app).0, "seeking keeps it paused");
    key(&mut app, KeyCode::Char('-'));
    key(&mut app, KeyCode::Char('m'));
    assert_eq!(video(&app).1, 90);
    assert!(video(&app).2, "M mutes");
    key(&mut app, KeyCode::Char(']'));
    assert_eq!(video(&app).3, 1.25);
    key(&mut app, KeyCode::Char('['));
    key(&mut app, KeyCode::Char('['));
    assert_eq!(video(&app).3, 0.75);
    key(&mut app, KeyCode::Char('s'));
    assert_eq!(
        (video(&app).0, video(&app).4),
        (true, 0),
        "S stops at the start"
    );
    app.editor_paste("text");
    key(&mut app, KeyCode::Char('x'));
    assert!(
        app.open_files[0].lines.is_empty(),
        "nothing typed into a video"
    );

    // The border is at 30, 2: the picture from row 3, the bar on row 19, buttons on 20.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 50, 10);
    assert!(!video(&app).0, "a click on the picture plays");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 32, 20);
    assert!(video(&app).0, "the first button pauses");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 46, 20);
    assert_eq!(video(&app).4, 0, "-5s stops at the start");
    mouse(&mut app, MouseEventKind::ScrollUp, 50, 10);
    assert_eq!(
        (video(&app).1, video(&app).2),
        (100, false),
        "the wheel turns it up"
    );
    mouse(&mut app, MouseEventKind::ScrollDown, 50, 10);
    assert_eq!(video(&app).1, 90);
    // The bar is the 44 cells from column 38: a drag only moves the bar, letting go
    // in its middle goes to the middle of the video.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 40, 19);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 60, 19);
    let scrub = |app: &App| {
        let media = app.open_files[0].media.as_ref().unwrap();
        media.video().unwrap().scrub
    };
    assert!(scrub(&app).is_some() && video(&app).4 == 0);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 59, 19);
    assert!(scrub(&app).is_none());
    assert!((36..=40).contains(&video(&app).4), "{}", video(&app).4);

    // F fills the screen; Esc leaves it, then closes the tab.
    key(&mut app, KeyCode::Char('f'));
    assert_eq!(app.fullscreen_video(), Some(0));
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(
        app.fullscreen_video(),
        None,
        "Help shows over the normal layout"
    );
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.tick_at(Instant::now() + Duration::from_millis(200));
    assert_eq!(app.fullscreen_video(), Some(0), "and back once it closed");
    key(&mut app, KeyCode::Esc);
    assert!(app.fullscreen_video().is_none() && app.open_files.len() == 1);
    key(&mut app, KeyCode::Esc);
    assert!(app.open_files.is_empty(), "Esc closes the video");
}

#[test]
fn a_gif_plays_in_its_tab() {
    use image::{Delay, Frame, Rgba, RgbaImage, codecs::gif::GifEncoder};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("blink.gif");
    let mut encoder = GifEncoder::new(std::fs::File::create(&path).unwrap());
    for colour in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let frame = RgbaImage::from_pixel(4, 4, Rgba(colour));
        let delay = Delay::from_numer_denom_ms(100, 1);
        encoder
            .encode_frame(Frame::from_parts(frame, 0, 0, delay))
            .unwrap();
    }
    drop(encoder);
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    let hint = |app: &App| app.active_file().unwrap().media.as_ref().unwrap().hint();
    assert_eq!(hint(&app).as_deref(), Some("Frame 1/2"));
    app.tick_at(Instant::now() + Duration::from_millis(150));
    assert_eq!(hint(&app).as_deref(), Some("Frame 2/2"));
}

#[test]
fn the_keyboard_and_the_mouse_select_copy_and_cut_text() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "alpha beta\ngamma\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    let shift = KeyModifiers::SHIFT;
    press(&mut app, KeyCode::Right, shift);
    press(&mut app, KeyCode::Right, shift);
    press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(app.clipboard.take().as_deref(), Some("al"));
    press(&mut app, KeyCode::Down, shift);
    press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert_eq!(app.clipboard.take().as_deref(), Some("alpha beta\nga"));
    assert_eq!(app.active_file().unwrap().lines, ["mma"]);
    press(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('Z'), shift);
    assert_eq!(app.active_file().unwrap().lines, ["Z"]);

    // A drag from the first column to the third selects two characters.
    app.editor_paste("\nsecond line");
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    let text_x = 30 + 1 + 4;
    mouse(
        &mut app,
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        text_x,
        4,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        text_x + 3,
        4,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(crossterm::event::MouseButton::Left),
        text_x + 3,
        4,
    );
    assert_eq!(
        app.active_file().unwrap().selected_text().as_deref(),
        Some("sec")
    );
    press(&mut app, KeyCode::Left, KeyModifiers::NONE);
    assert!(app.active_file().unwrap().selection().is_none());
}

#[test]
fn pdf_tabs_turn_their_pages_with_keys_and_the_wheel() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("manual.pdf");
    std::fs::write(&path, crate::editor::sample_pdf(3)).unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    let page = |app: &App| app.active_file().unwrap().page().unwrap().0;
    press(&mut app, KeyCode::PageDown, KeyModifiers::NONE);
    press(&mut app, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(page(&app), 2);
    press(&mut app, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(page(&app), 2, "the last page");
    press(&mut app, KeyCode::Home, KeyModifiers::NONE);
    assert_eq!(page(&app), 0);
    app.regions.pane_bodies[0] = Rect::new(30, 2, 60, 20);
    mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
    mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
    assert_eq!(page(&app), 1, "one page per turn of the wheel");
    press(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(app.open_files.is_empty());
}

#[test]
fn a_text_page_of_a_pdf_is_read_selected_copied_and_scrolled_into_the_next() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.pdf");
    let long = (1..=30)
        .map(|line| format!("({line}. text) Tj 0 -3 Td"))
        .collect::<Vec<_>>()
        .join(" ");
    let first = format!("BT /F1 2 Tf 5 96 Td {long} ET");
    let second = "BT /F1 10 Tf 5 80 Td (Second page) Tj ET";
    std::fs::write(&path, crate::editor::sample_pdf_pages(&[&first, second])).unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    let page = |app: &App| app.active_file().unwrap().page().unwrap().0;
    fn file(app: &App) -> &crate::editor::OpenFile {
        app.active_file().unwrap()
    }
    assert_eq!(file(&app).lines.len(), 30);
    // The arrows move the cursor and select; Ctrl+C copies; nothing is typed.
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(page(&app), 0, "arrows stay on a page of text");
    press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(app.copied.as_deref(), Some("2. text\n"));
    for code in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Backspace] {
        press(&mut app, code, KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(file(&app).lines[1], "2. text");
    assert!(!file(&app).dirty);
    // The wheel scrolls the 30 lines in 18 rows, then turns the page at the end.
    app.regions.pane_bodies[0] = Rect::new(30, 2, 60, 20);
    for _ in 0..4 {
        mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
    }
    assert_eq!((page(&app), file(&app).scroll), (0, 12));
    mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
    assert_eq!(page(&app), 1);
    assert_eq!(file(&app).lines, ["Second page"]);
    app.page_turned = None;
    mouse(&mut app, MouseEventKind::ScrollUp, 40, 5);
    assert_eq!(page(&app), 0, "and back at the top");
    press(&mut app, KeyCode::End, KeyModifiers::NONE);
    assert_eq!(page(&app), 1);
    press(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
    assert_eq!(page(&app), 0);
}

#[test]
fn a_document_ticks_its_tasks_follows_links_to_headings_and_notes_and_lists_its_headings() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("guide.md"),
        "# Guide\n\ntext\n\n## Setup steps\n\nmore\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("index.md"),
        "# Top\n\n- [ ] write tests\n\nRead [the setup](guide.md#setup-steps) or [the web](https://example.com/x) or [bad](javascript:x).\n\nA fact[^src].\n\n[^src]: The source.\n\n[[guide#setup steps|wiki setup]]\n",
    )
    .unwrap();
    let (mut app, mut terminal) = drawn(140, 40);
    app.root = directory.path().to_path_buf();
    app.show_explorer = false;
    let index = directory.path().join("index.md");
    app.open_file(index.clone()).unwrap();
    app.open_files[0].move_cursor_to(0, 0);
    let click = |app: &mut App, terminal: &mut _, word: &str| {
        draw(app, terminal);
        draw(app, terminal);
        let (row, line) = terminal_lines(terminal)
            .into_iter()
            .enumerate()
            .find(|(_, line)| line.contains(word))
            .unwrap_or_else(|| panic!("{word} is not on screen"));
        // In characters, not bytes: the border `│` is three bytes.
        let column = line[..line.find(word).unwrap()].chars().count() as u16;
        mouse(
            app,
            MouseEventKind::Down(MouseButton::Left),
            column,
            row as u16,
        );
        mouse(
            app,
            MouseEventKind::Up(MouseButton::Left),
            column,
            row as u16,
        );
    };
    // A click on the box ticks the task, in the file's text.
    click(&mut app, &mut terminal, "☐");
    let file = &app.open_files[0];
    assert_eq!(
        file.lines[2],
        "- [x] write tests",
        "cursor {:?} status {}\n{}",
        (file.cursor_row, file.cursor_column),
        app.status,
        terminal_lines(&terminal).join("\n")
    );
    // A link opens the document at its heading.
    click(&mut app, &mut terminal, "the setup");
    let file = app.active_file().unwrap();
    assert_eq!((file.name.as_str(), file.cursor_row), ("guide.md", 4));
    // A web address goes to the browser; another scheme opens nothing.
    app.select_tab(0);
    app.open_files[0].move_cursor_to(0, 0);
    click(&mut app, &mut terminal, "the web");
    assert_eq!(app.status, "Opened https://example.com/x");
    click(&mut app, &mut terminal, "bad");
    assert_eq!(
        app.status,
        "Only web addresses and files of the project open"
    );
    // A note goes to its definition; a wiki link to its heading.
    click(&mut app, &mut terminal, "[src]");
    assert_eq!(app.open_files[0].cursor_row, 8);
    app.open_files[0].move_cursor_to(0, 0);
    click(&mut app, &mut terminal, "wiki setup");
    assert_eq!(app.active_file().unwrap().cursor_row, 4);
    // Ctrl+O with @ lists the headings of the document.
    app.select_tab(0);
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('@'), KeyModifiers::NONE);
    let rows = app.finder_rows();
    assert_eq!(rows.len(), 1, "the one heading of index.md");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
}
