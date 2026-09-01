//! The host venue end to end, with this binary as the device under test.

use std::process::Command;

const RUNNER: &str = env!("CARGO_BIN_EXE_embedded-test-host-runner");

/// Shape embedded-test prints for `list`, with one test per outcome the
/// runner has to tell apart.
const LISTING: &str = concat!(
    r#"{"version":0,"tests":["#,
    r#"{"name":"tests::it_passes","should_panic":false,"ignored":false,"timeout":null},"#,
    r#"{"name":"tests::it_fails","should_panic":false,"ignored":false,"timeout":null},"#,
    r#"{"name":"tests::it_panics_as_expected","should_panic":true,"ignored":false,"timeout":null},"#,
    r#"{"name":"tests::it_is_ignored","should_panic":false,"ignored":true,"timeout":null}"#,
    r#"]}"#
);

fn main() {
    let arguments: Vec<String> = std::env::args().collect();

    match arguments.get(1).map(String::as_str) {
        Some("list") => println!("{LISTING}"),
        Some("run") => report(&arguments[2]),
        _ => drive(),
    }
}

/// A test that exits cleanly finished; any other code is an abort.
fn report(name: &str) {
    match name {
        "tests::it_passes" => std::process::exit(0),
        "tests::it_fails" | "tests::it_panics_as_expected" => std::process::exit(134),
        other => panic!("the runner asked for {other}, which the listing does not name"),
    }
}

fn drive() {
    let device = std::env::current_exe().expect("device path");
    let output = Command::new(RUNNER)
        .arg(&device)
        .output()
        .expect("spawning the runner");
    let transcript = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a failing test must exit as tests-failed\n{transcript}"
    );
    assert!(
        reported(&transcript, "tests::it_passes", "ok"),
        "{transcript}"
    );
    assert!(
        reported(&transcript, "tests::it_panics_as_expected", "ok"),
        "an abort satisfies #[should_panic]\n{transcript}"
    );
    assert!(
        reported(&transcript, "tests::it_fails", "FAILED"),
        "{transcript}"
    );
    assert!(
        reported(&transcript, "tests::it_is_ignored", "ignored"),
        "{transcript}"
    );

    debug(&device);
}

/// A debug run names the one test and leaves the running to the debugger.
fn debug(device: &std::path::Path) {
    let output = Command::new(RUNNER)
        .arg("--debug")
        .arg(device)
        .args(["--exact", "tests::it_passes"])
        .output()
        .expect("spawning the runner");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("waiting for a debugger to run tests::it_passes"),
        "{stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("test tests::it_passes"),
        "the runner must not run the test it hands over"
    );

    let several = Command::new(RUNNER)
        .arg("--debug")
        .arg(device)
        .output()
        .expect("spawning the runner");
    let stderr = String::from_utf8_lossy(&several.stderr);

    assert_eq!(several.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("tests::it_passes"), "{stderr}");
}

/// Names are padded out to a column, so only the ends of the line are fixed.
fn reported(transcript: &str, name: &str, outcome: &str) -> bool {
    let head = format!("test {name} ");
    transcript
        .lines()
        .map(str::trim)
        .any(|line| line.starts_with(&head) && line.ends_with(outcome))
}
