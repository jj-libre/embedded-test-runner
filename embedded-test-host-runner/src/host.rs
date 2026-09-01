use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use embedded_test_runner_core::process::{self, ProcessOutcome, ProcessStatus};
use embedded_test_runner_core::{Observation, TestMeta, Venue};

use crate::cli::Cli;

/// Everything a run needs that the command line decides.
pub(crate) struct HostConfig {
    binary: PathBuf,
    verbose: bool,
}

impl HostConfig {
    pub(crate) fn from_cli(cli: &Cli) -> Self {
        Self {
            binary: cli.common.elf.clone(),
            verbose: cli.common.verbose,
        }
    }
}

pub(crate) struct Host {
    config: HostConfig,
}

impl Host {
    pub(crate) fn new(config: HostConfig) -> Self {
        Self { config }
    }
}

impl Venue for Host {
    fn run(&self, test: TestMeta, timeout: Duration) -> Observation {
        let mut command = Command::new(&self.config.binary);
        command
            .arg(test.invocation.command())
            .arg(test.invocation.operand());

        if self.config.verbose {
            eprintln!(
                "host: {} {} {}",
                self.config.binary.display(),
                test.invocation.command(),
                test.invocation.operand()
            );
        }

        match process::run(command, Some(timeout)) {
            Ok(outcome) => observe_process(&outcome, timeout),
            Err(e) => e.into(),
        }
    }
}

/// No emulator stands between runner and test, so a process that fails is a
/// test that failed, however the platform reports the abnormal end.
fn observe_process(outcome: &ProcessOutcome, timeout: Duration) -> Observation {
    let output = outcome.captured.to_string();
    match outcome.status {
        ProcessStatus::Exited(0) => Observation::Finished { output },
        ProcessStatus::Exited(_) | ProcessStatus::Terminated => Observation::Aborted { output },
        ProcessStatus::TimedOut => Observation::TimedOut {
            after: timeout,
            output,
        },
    }
}

#[cfg(test)]
mod tests {
    use embedded_test_runner_core::process::Captured;

    use embedded_test_runner_core::{Invocation, TestMeta};

    use super::*;

    fn outcome(status: ProcessStatus) -> ProcessOutcome {
        ProcessOutcome {
            status,
            captured: Captured {
                stdout: "boom".to_string(),
                stderr: String::new(),
            },
        }
    }

    const TIMEOUT: Duration = Duration::from_secs(5);

    fn dispatch() -> TestMeta {
        TestMeta::new(
            "tests::it_passes",
            Invocation::Run("tests::it_passes".to_string()),
        )
    }

    #[test]
    fn test_binary_that_cannot_be_spawned_is_a_harness_error() {
        let host = Host::new(HostConfig {
            binary: PathBuf::from("no-such-binary"),
            verbose: true,
        });
        let reported = format!("{:?}", host.run(dispatch(), TIMEOUT));

        assert!(reported.starts_with("HarnessError"), "{reported}");
        assert!(reported.contains("no-such-binary"), "{reported}");
    }

    #[test]
    fn test_clean_exit_is_finished() {
        assert_eq!(
            observe_process(&outcome(ProcessStatus::Exited(0)), TIMEOUT),
            Observation::Finished {
                output: "boom".to_string()
            }
        );
    }

    #[test]
    fn test_non_zero_exit_is_abort() {
        assert_eq!(
            observe_process(&outcome(ProcessStatus::Exited(1)), TIMEOUT),
            Observation::Aborted {
                output: "boom".to_string()
            }
        );
    }

    #[test]
    fn test_termination_is_abort_rather_than_harness_error() {
        assert_eq!(
            observe_process(&outcome(ProcessStatus::Terminated), TIMEOUT),
            Observation::Aborted {
                output: "boom".to_string()
            }
        );
    }

    #[test]
    fn test_timeout_carries_bound_it_exceeded() {
        assert_eq!(
            observe_process(&outcome(ProcessStatus::TimedOut), TIMEOUT),
            Observation::TimedOut {
                after: TIMEOUT,
                output: "boom".to_string()
            }
        );
    }
}
