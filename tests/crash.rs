//! A panic never leaves the terminal unusable (R12), in a real PTY.

mod common;

#[cfg(unix)]
#[test]
#[cfg_attr(not(debug_assertions), ignore = "only debug builds panic on request")]
fn a_panic_in_any_thread_gives_the_terminal_back_and_says_where() {
    for thread in ["main", "thread"] {
        let home = common::Home::new();
        let mut sozo = home.start_with(&[("SOZO_TEST_PANIC", thread.to_string())]);
        assert_eq!(sozo.exit_code(), 101, "{thread}");
        assert!(sozo.terminal_given_back(), "{thread}:\n{}", sozo.screen());
        let screen = sozo.screen();
        for expected in [
            "stopped unexpectedly",
            "asked for by SOZO_TEST_PANIC",
            "src/main.rs",
            "Please report it",
        ] {
            assert!(screen.contains(expected), "{thread}: {expected}\n{screen}");
        }
        let log = std::fs::read_to_string(home.cache().join("crash.log")).unwrap();
        assert!(
            log.starts_with(&format!("SozoIDE {} stopped", env!("CARGO_PKG_VERSION"))),
            "{log}"
        );
        assert!(log.contains("at src/main.rs:"), "{log}");
        assert!(log.contains("asked for by SOZO_TEST_PANIC"), "{log}");
        // Nothing of the project went into it.
        assert!(!log.contains(&home.path().display().to_string()), "{log}");
    }
}
