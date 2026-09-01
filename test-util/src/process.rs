//! Running a built binary and capturing what it reported.

use std::ffi::OsStr;
use std::fmt;
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
    let output = Command::new(binary).args(args).output().unwrap();

    Run {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}
