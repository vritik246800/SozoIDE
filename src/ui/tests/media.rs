//! Pictures, GIFs, videos and models in the editor.

use super::*;

#[test]
fn a_video_shows_its_bar_and_buttons_and_fills_the_screen_with_f() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("clip.mp4")).unwrap();
    app.open_files[0].media = Some(crate::media::sample_video());
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        " Pause  ",
        " Stop  ",
        " -5s  ",
        " Vol+  ",
        " Mute  ",
        " Full screen  ",
        "Vol 100%  1x",
        " 00:00 ─",
        "─ 01:15 ",
        "Space  S",
    ] {
        assert!(text.contains(part), "{part}");
    }
    let header = app.regions.header_help;
    assert!(header.width > 0, "the normal layout has its header");

    app.fullscreen = true;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(
        app.regions.pane_bodies[0], buffer.area,
        "the whole terminal"
    );
    assert_eq!(app.regions.header_help.width, 0, "no header");
    assert!(buffer[(0, 0)].symbol() != " " && row(buffer, 0).contains("clip.mp4"));
    // The buttons on the last row but one, over the bottom border.
    assert!(row(buffer, 28).contains(" Full screen "));
}

#[test]
fn a_video_is_half_blocks_also_in_a_terminal_with_pictures() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    let mut app = App::with_picker(directory.path().to_path_buf(), picker, false).unwrap();
    app.open_file(directory.path().join("clip.mp4")).unwrap();
    app.open_files[0].media = Some(crate::media::sample_video_with_a_frame());
    settle(&mut app);
    assert!(app.open_files[0].image.is_none(), "no picture protocol");
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let green = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .filter(|cell| cell.symbol() == "▀" && cell.fg == ratatui::style::Color::Rgb(0, 255, 0))
        .count();
    assert!(green > 100, "{green} green half blocks");
}

#[test]
fn a_kitty_picture_leaves_the_rest_of_the_screen_drawn() {
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    let picture = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 255, 0, 255]));
    let mut image = picker.new_resize_protocol(image::DynamicImage::ImageRgba8(picture));
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    // The first frame sends the picture, the next ones only its rows (E35).
    for _ in 0..2 {
        terminal
            .draw(|frame| {
                crate::ui::editor::render_picture(
                    frame,
                    &mut image,
                    Rect::new(2, 1, 8, 4),
                    ratatui_image::Resize::Fit(None),
                );
                frame.render_widget(Paragraph::new("right side"), Rect::new(12, 2, 20, 1));
                frame.render_widget(Paragraph::new("under it"), Rect::new(0, 6, 20, 1));
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(row(buffer, 2).contains("right side"));
        assert!(row(buffer, 6).contains("under it"));
        for y in 1..5 {
            assert!(buffer[(2, y)].symbol().contains('\u{10EEEE}'), "row {y}");
        }
    }
}

#[test]
fn a_model_is_drawn_in_half_blocks_with_its_keys_in_the_title() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ship.glb");
    std::fs::write(&path, crate::media::sample_glb()).unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert_eq!(app.active_file().unwrap().language, "3D model");
    assert!(
        text.contains("←/→ ↑/↓ turn  +/- zoom  W wireframe  0 reset  P animate"),
        "{text}"
    );
    let red = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .filter(|cell| {
            matches!(cell.symbol(), "▀" | "▄")
                && matches!(cell.fg, ratatui::style::Color::Rgb(red, 0, 0) if red > 60)
        })
        .count();
    assert!(red > 20, "{red}");
}
