//! The big test only with `--features budgets`, run alone (other tests at the same time would be
//! measured too): the CI step after the tests and `./git.sh verify` do it. `within` scales the
//! budgets of the other tests that time something by how busy the machine is (E85).
//!
//! The times a person feels (TODO 6.1.4, F44): the first frame, a key until its frame, opening a
//! file of 20 000 lines and searching 10 000 files. Each budget is the time measured here on
//! 2026-11-14 plus 20 %, scaled by how fast this machine runs a fixed piece of work against
//! the machine that measured them: a slower runner gets more time, never a faster one less.

use std::time::{Duration, Instant};

#[cfg(feature = "budgets")]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[cfg(feature = "budgets")]
use crate::app::App;

/// The fixed work (sorting 300 000 numbers) and the times measured with it, in a debug build,
/// alone on the machine.
const CALIBRATION: Duration = Duration::from_millis(CALIBRATION_MS);
/// Calibration, first frame, key to frame, 20 000 lines open, 10 000 files searched (ms).
#[cfg(feature = "budgets")]
const BUDGETS: (u64, u64, u64, u64, u64) = (CALIBRATION_MS, 35, 4, 46, 70);
const CALIBRATION_MS: u64 = 190;

/// How much slower this machine is right now (other tests run at the same time) than the one
/// that measured the budgets, at least 1: asked again before each measure.
pub(crate) fn slower() -> f64 {
    let mut numbers: Vec<u64> = (0..300_000u64)
        .map(|n| n.wrapping_mul(6_364_136_223_846_793_005).rotate_left(17))
        .collect();
    let started = Instant::now();
    numbers.sort_unstable();
    std::hint::black_box(&numbers);
    (started.elapsed().as_secs_f64() / CALIBRATION.as_secs_f64()).max(1.0)
}

#[cfg(feature = "budgets")]
fn within(name: &str, took: Duration, budget_ms: u64, factor: f64) {
    let _ = BUDGETS.0;
    // 20 % more, and 25 ms for the noise of every test running at once.
    let measured = Duration::from_millis(budget_ms).mul_f64(factor);
    let budget = measured.mul_f64(1.2) + Duration::from_millis(25);
    eprintln!(
        "{name}: {} ms (budget {} ms)",
        took.as_millis(),
        budget.as_millis()
    );
    assert!(
        took <= budget,
        "{name} took {took:?}, over its budget of {budget:?}"
    );
}

/// `budget`, made longer by how much slower this machine is right now: a test that times
/// something among all the others asks it just before (E85).
pub(crate) fn scaled(budget: Duration) -> Duration {
    budget.mul_f64(slower())
}

#[cfg(feature = "budgets")]
#[test]
fn what_a_person_feels_stays_within_its_budget() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let long: String = (0..20_000)
        .map(|n| format!("let value_{n} = compute({n}); // line {n}\n"))
        .collect();
    std::fs::write(root.join("long.rs"), &long).unwrap();
    for folder in 0..100 {
        let path = root.join(format!("folder{folder}"));
        std::fs::create_dir(&path).unwrap();
        for file in 0..100 {
            std::fs::write(
                path.join(format!("file{file}.txt")),
                format!("text {folder} {file}\nmore\n"),
            )
            .unwrap();
        }
    }
    // The first frame: the app made and drawn once.
    let factor = slower();
    let started = Instant::now();
    let mut app = App::new(root.to_path_buf()).unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    within("first frame", started.elapsed(), BUDGETS.1, factor);
    // Opening 20 000 lines.
    let factor = slower();
    let started = Instant::now();
    app.open_file(root.join("long.rs")).unwrap();
    within("open 20 000 lines", started.elapsed(), BUDGETS.3, factor);
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    // A key until its frame, the median of 30.
    app.focus = crate::app::Focus::Editor;
    let mut times: Vec<Duration> = (0..30)
        .map(|_| {
            let started = Instant::now();
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .unwrap();
            started.elapsed()
        })
        .collect();
    times.sort();
    let factor = slower();
    within("key to frame", times[times.len() / 2], BUDGETS.2, factor);
    // Searching 10 000 files to the end.
    let factor = slower();
    let started = Instant::now();
    let mut search = crate::editor::SearchState::new(crate::editor::SearchScope::Workspace);
    search.query = "more".into();
    search.refresh(root, None);
    while search.searching {
        search.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(search.matches.len() >= crate::editor::MAX_MATCHES.min(10_000));
    within("search 10 000 files", started.elapsed(), BUDGETS.4, factor);
}
