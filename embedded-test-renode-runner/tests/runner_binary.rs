//! The shipped binary, run as a binary.

use test_util::{on_disk, run, valid_elf};

const BINARY: &str = env!("CARGO_BIN_EXE_embedded-test-renode-runner");

#[test]
fn test_unreadable_elf_exits_as_runner_error() {
    let output = run(BINARY, ["--platform", "board.repl", "no-such-elf"]);
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("no-such-elf"), "{output}");
}

#[test]
fn test_missing_elf_argument_is_refused_as_a_usage_error() {
    let output = run(BINARY, ["--platform", "board.repl"]);
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("<ELF>"), "{output}");
}

#[test]
fn test_debugging_a_filter_that_matches_nothing_exits_as_runner_error() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(
        BINARY,
        [
            "--platform",
            "board.repl",
            "--renode",
            "no-such-renode",
            "--debug",
            "--gdb",
            "3333",
            path.as_str(),
            "--exact",
            "no_such_test",
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("no test to debug"), "{output}");
}

#[test]
fn test_renode_that_never_starts_is_a_runner_error() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(
        BINARY,
        [
            "--platform",
            "board.repl",
            "--renode",
            "no-such-renode",
            path.as_str(),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("not a verdict"), "{output}");
}

#[test]
fn test_a_stub_that_never_started_is_not_announced() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(
        BINARY,
        [
            "--platform",
            "board.repl",
            "--renode",
            "no-such-renode",
            "--debug",
            "--gdb",
            "3333",
            path.as_str(),
        ],
    );

    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(
        !output.stderr.contains("waiting for a debugger"),
        "{output}"
    );
}

#[test]
fn test_a_port_without_a_debug_run_is_refused() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(
        BINARY,
        [
            "--platform",
            "board.repl",
            "--renode",
            "no-such-renode",
            "--gdb",
            "3333",
            path.as_str(),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("--debug"), "{output}");
}
