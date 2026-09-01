//! The monitor script, seen from Renode's side.
//!
//! This binary is both the driver and the stand-in for Renode, chosen by the
//! environment variable below.

use std::process::Command;

use test_util::{on_disk, valid_elf};

const RUNNER: &str = env!("CARGO_BIN_EXE_embedded-test-renode-runner");

/// Where the stand-in writes down the argv it was started with.
const ARGV_FILE: &str = "EMBEDDED_TEST_STAND_IN_ARGV";

fn main() {
    match std::env::var(ARGV_FILE) {
        Ok(path) => record(&path),
        Err(_) => drive(),
    }
}

/// The stand-in for Renode: records its argv, then prints the two answers the
/// trailing monitor commands ask for.
fn record(path: &str) {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    std::fs::write(path, arguments.join("\n")).expect("recording the argv");

    println!("True");
    println!("0x00000000");
}

fn drive() {
    let stand_in = std::env::current_exe().expect("test binary path");
    let elf = on_disk(&valid_elf());
    let argv_file = elf.path().with_file_name("argv");

    let output = Command::new(RUNNER)
        .env(ARGV_FILE, &argv_file)
        .arg("--platform")
        .arg("board.repl")
        .arg("--renode")
        .arg(&stand_in)
        .arg(elf.path())
        .output()
        .expect("spawning the runner");

    assert_eq!(
        output.status.code(),
        Some(0),
        "the stand-in reports a clean exit, so the suite passes\n{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let recorded =
        std::fs::read_to_string(&argv_file).expect("the stand-in ran and wrote its argv");
    let arguments: Vec<&str> = recorded.lines().collect();

    // Everything after a -e is one monitor command.
    let script: Vec<&str> = arguments
        .windows(2)
        .filter(|pair| pair[0] == "-e")
        .map(|pair| pair[1])
        .collect();

    assert!(
        script.iter().any(|command| command.contains("LoadELF")),
        "{script:?}"
    );
    assert!(
        script
            .iter()
            .any(|command| command.contains("semihosting ProgramName")),
        "{script:?}"
    );
    assert!(
        script.iter().any(|command| command.contains("RunFor")),
        "{script:?}"
    );
    assert_eq!(script.last(), Some(&"quit"), "{script:?}");
}
