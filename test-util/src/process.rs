//! Running a built binary and capturing what it reported.

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::process::{Command, ExitStatus};

/// How a binary exited and what it wrote.
#[derive(Debug)]
pub struct Run {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Both streams, so a failing assertion shows the report as well as the error.
impl fmt::Display for Run {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}\nstdout:\n{}stderr:\n{}",
            self.status, self.stdout, self.stderr
        )
    }
}

/// Runs `binary`, which the caller names through `CARGO_BIN_EXE_<name>`.
pub fn run<I, S>(binary: &str, args: I) -> Run
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    capture(Command::new(binary).args(args))
}

/// Runs `binary` like [`run`], with `std::env::temp_dir` answering `temp_dir`.
pub fn run_with_temp_dir<I, S>(binary: &str, temp_dir: &Path, args: I) -> Run
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    capture(
        Command::new(binary)
            .args(args)
            .env("TMPDIR", temp_dir)
            .env("TMP", temp_dir)
            .env("TEMP", temp_dir),
    )
}

fn capture(command: &mut Command) -> Run {
    let output = command.output().unwrap();

    Run {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}
