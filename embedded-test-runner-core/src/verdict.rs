use std::fmt;
use std::time::Duration;

use crate::process::ProcessError;

/// Outcome of a test run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Observation {
    /// Ran to completion.
    Finished {
        output: String,
    },
    /// Ran and aborted.
    Aborted {
        output: String,
    },
    TimedOut {
        after: Duration,
        output: String,
    },
    /// Venue failure; nothing learned about the test.
    HarnessError {
        reason: String,
        output: String,
    },
}

impl Observation {
    pub fn harness_error(reason: &str, output: &str) -> Self {
        Observation::HarnessError {
            reason: reason.to_string(),
            output: output.to_string(),
        }
    }
}

/// A process that never ran taught nothing about the test, whichever venue was
/// trying to run it.
impl From<ProcessError> for Observation {
    fn from(e: ProcessError) -> Self {
        Observation::HarnessError {
            // The reason and everything under it, as one sentence. A venue
            // reports a String and has nowhere else to put a source chain.
            reason: format!("{:#}", anyhow::Error::new(e)),
            output: String::new(),
        }
    }
}

/// Why a test did not pass, and whatever the venue captured while finding out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) reason: String,
    pub(crate) output: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let output = self.output.trim();
        if output.is_empty() {
            write!(f, "{}", self.reason)
        } else {
            write!(f, "{}\n{output}", self.reason)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Pass,
    Fail(Failure),
}

impl Verdict {
    #[expect(clippy::match_same_arms, reason = "the arms spell out a truth table")]
    pub(crate) fn from_observation(observation: Observation, should_panic: bool) -> Self {
        match (observation, should_panic) {
            (Observation::Finished { .. }, false) => Verdict::Pass,
            (Observation::Finished { .. }, true) => Verdict::fail(
                "test was expected to panic (#[should_panic]) but exited successfully",
                String::new(),
            ),
            (Observation::Aborted { .. }, true) => Verdict::Pass,
            (Observation::Aborted { output }, false) => Verdict::fail("the test aborted", output),
            (Observation::TimedOut { after, output }, _) => {
                Verdict::fail(&format!("timed out after {}s", after.as_secs()), output)
            }
            (Observation::HarnessError { reason, output }, _) => {
                Verdict::fail(&format!("runner error: {reason}"), output)
            }
        }
    }

    fn fail(reason: &str, output: String) -> Self {
        Verdict::Fail(Failure {
            reason: reason.to_string(),
            output,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished() -> Observation {
        Observation::Finished {
            output: String::new(),
        }
    }

    fn aborted() -> Observation {
        Observation::Aborted {
            output: String::new(),
        }
    }

    fn timed_out() -> Observation {
        Observation::TimedOut {
            after: Duration::from_secs(7),
            output: String::new(),
        }
    }

    fn failure(observation: Observation, should_panic: bool) -> Failure {
        match Verdict::from_observation(observation, should_panic) {
            Verdict::Fail(failure) => failure,
            Verdict::Pass => panic!("expected a failure"),
        }
    }

    #[test]
    fn test_clean_exit_passes() {
        assert_eq!(Verdict::from_observation(finished(), false), Verdict::Pass);
    }

    #[test]
    fn test_clean_exit_fails_should_panic_test() {
        let failure = failure(finished(), true);
        assert!(failure.reason.contains("expected to panic"), "{failure:?}");
    }

    #[test]
    fn test_abort_fails() {
        let observation = Observation::Aborted {
            output: "  panicked at src/lib.rs  ".to_string(),
        };
        let failure = failure(observation, false);

        assert_eq!(failure.reason, "the test aborted");
        assert_eq!(
            failure.to_string(),
            "the test aborted\npanicked at src/lib.rs"
        );
    }

    #[test]
    fn test_abort_satisfies_should_panic() {
        assert_eq!(Verdict::from_observation(aborted(), true), Verdict::Pass);
    }

    #[test]
    fn test_timeout_reports_bound_it_exceeded() {
        assert_eq!(failure(timed_out(), false).reason, "timed out after 7s");
    }

    #[test]
    fn test_timeout_never_satisfies_should_panic() {
        assert_eq!(failure(timed_out(), true).reason, "timed out after 7s");
    }

    #[test]
    fn test_harness_error_carries_the_output() {
        let observation = Observation::harness_error("QEMU was killed", "  boom  ");
        let failure = failure(observation, false);

        assert_eq!(failure.reason, "runner error: QEMU was killed");
        assert_eq!(failure.to_string(), "runner error: QEMU was killed\nboom");
    }

    #[test]
    fn test_harness_error_without_output_is_the_reason_alone() {
        let observation = Observation::harness_error("QEMU was killed", "  ");
        assert_eq!(
            failure(observation, false).to_string(),
            "runner error: QEMU was killed"
        );
    }

    #[test]
    fn test_a_process_that_never_ran_is_a_harness_error_carrying_its_cause() {
        let error = ProcessError::Spawn {
            program: "qemu-system-arm".to_string(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        };

        let Observation::HarnessError { reason, .. } = Observation::from(error) else {
            panic!("a process that never ran says nothing about the test");
        };

        assert!(reason.contains("qemu-system-arm"), "{reason}");
        assert!(
            reason.contains("not found"),
            "the cause has to survive the conversion: {reason}"
        );
    }

    #[test]
    fn test_harness_error_never_satisfies_should_panic() {
        let observation = Observation::harness_error("failed to spawn `qemu-system-arm`", "");
        let reported = failure(observation.clone(), false);
        let expected_to_panic = failure(observation, true);

        assert!(
            reported.reason.starts_with("runner error: "),
            "{reported:?}"
        );
        assert_eq!(expected_to_panic, reported);
    }
}
