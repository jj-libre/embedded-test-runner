//! The shipped binary, run as a binary.

use test_util::{on_disk, run, valid_elf};

const BINARY: &str = env!("CARGO_BIN_EXE_embedded-test-qemu-runner");

#[test]
fn test_unreadable_elf_exits_as_runner_error() {
    let output = run(BINARY, ["--qemu", "qemu-system-arm", "no-such-elf"]);
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("no-such-elf"), "{output}");
}

#[test]
fn test_missing_elf_argument_is_refused_as_a_usage_error() {
    let output = run(BINARY, ["--qemu", "qemu-system-arm"]);
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
            "--qemu",
            "no-such-emulator",
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

/// A venue that never started says nothing about the tests, so it is a runner
/// error and not a suite that failed.
#[test]
fn test_emulator_that_never_starts_is_a_runner_error() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(BINARY, ["--qemu", "no-such-emulator", path.as_str()]);
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("not a verdict"), "{output}");
}

#[test]
fn test_unparseable_qemu_command_exits_as_runner_error() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(BINARY, ["--qemu", "qemu-system-arm \"boot", path.as_str()]);
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("--qemu"), "{output}");
}

/// A venue that never started has nothing to announce, and this is the only
/// place the ordering is observable.
#[test]
fn test_a_stub_that_never_started_is_not_announced() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(
        BINARY,
        [
            "--qemu",
            "no-such-emulator",
            "--debug",
            "--gdb",
            "3333",
            path.as_str(),
        ],
    );

    // The exit code says the run got as far as the venue, so the absence of
    // the line below is the ordering and not an earlier refusal.
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(
        !output.stderr.contains("waiting for a debugger"),
        "{output}"
    );
}

/// A stub is served for a debug run, so a port on its own is a mistake and not
/// a suite to run anyway.
#[test]
fn test_a_port_without_a_debug_run_is_refused() {
    let elf = on_disk(&valid_elf());
    let path = elf.path().display().to_string();

    let output = run(
        BINARY,
        ["--qemu", "no-such-emulator", "--gdb", "3333", path.as_str()],
    );
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("--debug"), "{output}");
}
