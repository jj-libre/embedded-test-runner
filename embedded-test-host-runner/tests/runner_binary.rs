//! The shipped binary, run as a binary.

use test_util::run;

const BINARY: &str = env!("CARGO_BIN_EXE_embedded-test-host-runner");

#[test]
fn test_binary_that_cannot_be_spawned_exits_as_runner_error() {
    let output = run(BINARY, ["no-such-binary"]);
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("no-such-binary"), "{output}");
}

#[test]
fn test_missing_binary_argument_is_refused_as_a_usage_error() {
    let output = run(BINARY, [] as [&str; 0]);
    assert_eq!(output.status.code(), Some(2), "{output}");
    assert!(output.stderr.contains("<ELF>"), "{output}");
}
