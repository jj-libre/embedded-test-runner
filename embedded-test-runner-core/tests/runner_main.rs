//! A runner `main`, run as one: what each outcome exits with, and the
//! announcement only another process can see.

use std::env;
use std::num::NonZeroU16;
use std::process::Command;

use embedded_test_runner_core::{Outcome, announce_debug_port, announce_test_to_debug};

/// Selects what the re-executed runner does; absent in the parent.
const ROLE: &str = "EMBEDDED_TEST_RUNNER_MAIN_ROLE";
const PORT: u16 = 3333;
const TEST: &str = "tests::it_passes";

fn main() -> Outcome {
    match env::var(ROLE) {
        Ok(role) => act(&role),
        Err(_) => drive(),
    }
}

fn act(role: &str) -> Outcome {
    match role {
        "passed" => Outcome::Passed,
        "tests-failed" => Outcome::TestsFailed,
        "runner-error" => Outcome::RunnerError(anyhow::anyhow!("the venue never started")),
        "stub" => {
            announce_debug_port(NonZeroU16::new(PORT));
            Outcome::Passed
        }
        "no-stub" => {
            announce_debug_port(None);
            Outcome::Passed
        }
        "handover" => {
            announce_test_to_debug(TEST);
            Outcome::Passed
        }
        other => panic!("no role named {other}"),
    }
}

/// This binary again, in one role, with its exit code and stderr.
fn as_role(role: &str) -> (Option<i32>, String) {
    let output = Command::new(env::current_exe().expect("this binary"))
        .env(ROLE, role)
        .output()
        .expect("re-running this binary");

    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn drive() -> Outcome {
    let (code, stderr) = as_role("passed");
    assert_eq!(code, Some(0), "{stderr}");

    let (code, stderr) = as_role("tests-failed");
    assert_eq!(code, Some(1), "{stderr}");

    let (code, stderr) = as_role("runner-error");
    assert_eq!(code, Some(2), "{stderr}");
    assert!(
        stderr.contains("error: the venue never started"),
        "{stderr}"
    );

    let (_, stderr) = as_role("stub");
    assert!(
        stderr.contains(&format!("waiting for a debugger on port {PORT}")),
        "{stderr}"
    );

    let (_, stderr) = as_role("no-stub");
    assert!(!stderr.contains("waiting for a debugger"), "{stderr}");

    let (_, stderr) = as_role("handover");
    assert!(
        stderr.contains(&format!("waiting for a debugger to run {TEST}")),
        "{stderr}"
    );

    Outcome::Passed
}
