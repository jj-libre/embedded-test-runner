//! Child-process handling, exercised by re-running this binary as the child.

use std::env;
use std::process::Command;
use std::time::{Duration, Instant};

use embedded_test_runner_core::process::{self, ProcessError, ProcessStatus};

/// Selects what the re-executed child does; absent in the parent.
const ROLE: &str = "EMBEDDED_TEST_RUNNER_CHILD_ROLE";
const FLOOD_BYTES: usize = 1 << 20;
const TIMEOUT: Duration = Duration::from_secs(30);

fn child(role: &str) -> Command {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["test_child", "--exact", "--ignored", "--nocapture"])
        .env(ROLE, role);
    command
}

#[test]
#[ignore = "re-executed as the child of the tests below"]
fn test_child() {
    let Ok(role) = env::var(ROLE) else { return };
    match role.as_str() {
        "abort" => std::process::abort(),
        // Outlasts any test that waits on it, but ends on its own: a kill under
        // test can fail, and on Windows a child that never ends holds this
        // binary open against the next build.
        "hang" => std::thread::sleep(TIMEOUT),
        "flood" => print!("{}", "x".repeat(FLOOD_BYTES)),
        "streams" => {
            print!("to stdout");
            eprint!("to stderr");
        }
        "invalid-utf8" => {
            use std::io::Write;
            let mut stdout = std::io::stdout();
            stdout.write_all(b"before \xff after").unwrap();
            stdout.flush().unwrap();
        }
        code => std::process::exit(code.parse().unwrap()),
    }
}

#[test]
fn test_exit_code_is_reported() {
    assert_eq!(
        process::run(child("134"), Some(TIMEOUT)).unwrap().status,
        ProcessStatus::Exited(134)
    );
}

#[test]
fn test_output_beyond_a_pipe_buffer_does_not_deadlock() {
    let outcome = process::run(child("flood"), Some(TIMEOUT)).unwrap();
    assert_eq!(outcome.status, ProcessStatus::Exited(0));
    assert!(outcome.captured.stdout.len() >= FLOOD_BYTES);
}

#[test]
fn test_streams_stay_apart() {
    let outcome = process::run(child("streams"), Some(TIMEOUT)).unwrap();
    assert!(outcome.captured.stdout.contains("to stdout"));
    assert!(outcome.captured.stderr.contains("to stderr"));
    assert!(!outcome.captured.stdout.contains("to stderr"));
}

#[test]
fn test_unbounded_wait_lets_the_child_finish() {
    assert_eq!(
        process::run(child("0"), None).unwrap().status,
        ProcessStatus::Exited(0)
    );
}

#[test]
fn test_child_running_at_the_timeout_is_killed() {
    let started = Instant::now();
    let outcome = process::run(child("hang"), Some(Duration::from_millis(200))).unwrap();

    assert_eq!(outcome.status, ProcessStatus::TimedOut);
    // A child left alive holds its pipes open, and each drain then waits out
    // its whole grace.
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn test_output_around_a_byte_that_is_not_utf8_survives() {
    let outcome = process::run(child("invalid-utf8"), Some(TIMEOUT)).unwrap();
    assert!(
        outcome.captured.stdout.contains("before"),
        "{:?}",
        outcome.captured.stdout
    );
    assert!(
        outcome.captured.stdout.contains("after"),
        "{:?}",
        outcome.captured.stdout
    );
}

#[test]
fn test_missing_binary_never_starts() {
    let error = process::run(Command::new("no-such-binary-xyzzy"), Some(TIMEOUT)).unwrap_err();

    assert!(
        matches!(&error, ProcessError::Spawn { program, .. } if program == "no-such-binary-xyzzy"),
        "{error:?}"
    );
}

#[test]
fn test_a_child_that_starts_can_be_finished_separately() {
    let running = process::start(child("7")).unwrap();
    let outcome = running.finish(Some(TIMEOUT)).unwrap();

    assert_eq!(outcome.status, ProcessStatus::Exited(7));
}

#[cfg(unix)]
#[test]
fn test_terminated_child_has_no_exit_code() {
    assert_eq!(
        process::run(child("abort"), Some(TIMEOUT)).unwrap().status,
        ProcessStatus::Terminated
    );
}
