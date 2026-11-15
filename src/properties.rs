//! Property tests of the parsers (TODO 6.1.3, F44): whatever comes from outside — a Markdown
//! file, a program's output, a server's frames, a settings or sessions file, a compiler's lines,
//! the command line — is read without a panic and with its own limits.

use proptest::prelude::*;

/// Text with what hostile input has: control characters, wide characters, quotes, `..`.
fn hostile() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            any::<char>(),
            Just('\u{1b}'),
            Just('\n'),
            Just('|'),
            Just('['),
            Just(']'),
            Just('`'),
            Just('漢'),
            Just('\u{200d}'),
            Just(':'),
        ],
        0..300,
    )
    .prop_map(|characters| characters.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]

    #[test]
    fn markdown_of_any_text_draws_without_a_panic(text in hostile()) {
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let document = crate::editor::MarkdownDocument::parse(&lines);
        let rows = document.rows(lines.len());
        prop_assert!(rows.len() >= lines.len());
        for row in rows.iter().take(50) {
            let line = lines.get(row.source).map_or("", String::as_str);
            let _ = document.render(*row, &[], 80, line, &[]);
        }
        let _ = crate::editor::headings(&lines);
    }

    #[test]
    fn any_output_of_a_program_is_read_without_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
        let mut parser = vt100::Parser::new_with_callbacks(10, 40, 50, crate::terminal::Replies::default());
        parser.process(&bytes);
        let root = std::env::temp_dir();
        let line: String = parser.screen().rows(0, 40).next().unwrap_or_default();
        for column in [0, 5, 39] {
            let _ = crate::terminal::link_in(&line, column, &[], &root);
        }
    }

    #[test]
    fn any_frames_of_a_language_server_are_read_without_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
        let (sender, receiver) = std::sync::mpsc::channel();
        crate::lsp::client::read_messages(std::io::Cursor::new(bytes), &sender);
        drop(sender);
        prop_assert!(receiver.iter().count() <= 2000);
    }

    #[test]
    fn any_settings_or_sessions_file_reads_as_defaults_at_worst(text in hostile()) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        std::fs::write(&path, &text).unwrap();
        let (settings, _) = crate::settings::load(&path, |_| false);
        prop_assert!(settings.tab_width >= 1 && settings.tab_width <= 16);
        prop_assert!(settings.terminal_scrollback <= 100_000);
    }

    #[test]
    fn compiler_lines_and_command_lines_of_any_text_are_read_without_a_panic(text in hostile()) {
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let root = std::env::temp_dir();
        for problem in crate::run::problems_in(&lines, &root) {
            prop_assert!(problem.path.starts_with(root.canonicalize().unwrap()));
        }
        let _ = crate::cli::parse(std::slice::from_ref(&text), &root);
        for regex in [false, true] {
            let options = crate::editor::SearchOptions { regex, ..Default::default() };
            let _ = crate::editor::matcher(&text, options);
        }
    }
}
