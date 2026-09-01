//! The composed argv, seen from the emulator's side.
//!
//! This binary is both the driver and the stand-in emulator, chosen by the
//! environment variable below.

use std::process::Command;

use test_util::{on_disk, valid_elf};

const RUNNER: &str = env!("CARGO_BIN_EXE_embedded-test-qemu-runner");

/// Where the stand-in emulator writes down the argv it was started with.
const ARGV_FILE: &str = "EMBEDDED_TEST_STAND_IN_ARGV";

fn main() {
    match std::env::var(ARGV_FILE) {
        Ok(path) => record(&path),
        Err(_) => drive(),
    }
}

/// The stand-in emulator: records its argv and exits the way a passing test
/// does, so the run reaches its end rather than an error path.
fn record(path: &str) {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    std::fs::write(path, arguments.join("\n")).expect("recording the argv");
}

fn drive() {
    let stand_in = std::env::current_exe().expect("test binary path");
    let elf = on_disk(&valid_elf());
    let argv_file = elf.path().with_file_name("argv");

    let output = Command::new(RUNNER)
        .env(ARGV_FILE, &argv_file)
        .arg("--qemu")
        .arg(shell_words::quote(&stand_in.display().to_string()).as_ref())
        .arg(elf.path())
        .output()
        .expect("spawning the runner");

    assert_eq!(
        output.status.code(),
        Some(0),
        "the stand-in exits cleanly, so the suite passes\n{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let recorded =
        std::fs::read_to_string(&argv_file).expect("the stand-in ran and wrote its argv");
    let arguments: Vec<&str> = recorded.lines().collect();

    let kernel = arguments
        .iter()
        .position(|argument| *argument == "-kernel")
        .unwrap_or_else(|| panic!("no -kernel in {arguments:?}"));
    assert_eq!(arguments[kernel + 1], elf.path().display().to_string());

    assert!(arguments.contains(&"-semihosting-config"), "{arguments:?}");
    assert!(
        arguments
            .iter()
            .any(|argument| argument.starts_with("enable=on,target=native,arg=run_addr,arg=")),
        "{arguments:?}"
    );
}
