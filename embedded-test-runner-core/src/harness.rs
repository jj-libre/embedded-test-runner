use std::num::NonZeroU16;
use std::process::{ExitCode, Termination};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};
use libtest_mimic::{Arguments, Failed, Trial};
use thiserror::Error;

use crate::cli::CommonArgs;
use crate::verdict::Verdict;
use crate::{Observation, TestMeta};

/// What a runner finished as; `Termination` turns it into a message and a code.
#[derive(Debug)]
pub enum Outcome {
    Passed,
    TestsFailed,
    /// The runner, not a test. Whatever the tests reported cannot be trusted.
    RunnerError(anyhow::Error),
}

impl Termination for Outcome {
    fn report(self) -> ExitCode {
        match self {
            Outcome::Passed => ExitCode::SUCCESS,
            // Distinct from the 101 a panicking runner exits with.
            Outcome::TestsFailed => ExitCode::from(1),
            Outcome::RunnerError(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(2)
            }
        }
    }
}

/// One emulator, configured and ready to run the tests it is handed.
pub trait Venue: Send + Sync + 'static {
    /// Runs one test, bounded by `timeout`, and says what was observed.
    fn run(&self, test: TestMeta, timeout: Duration) -> Observation;
}

/// What the runner was asked to do, decided once from the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Every selected test, each bounded by its timeout.
    Suite,
    /// One test, held for a debugger.
    Debug,
}

impl ExecutionMode {
    /// Wall-clock bound on the venue's own process. A debug session lasts as
    /// long as the person debugging takes, so it has none.
    pub fn bound(self, timeout: Duration) -> Option<Duration> {
        match self {
            ExecutionMode::Suite => Some(timeout),
            ExecutionMode::Debug => None,
        }
    }
}

/// Wrapper for a runner `main`.
pub fn main(body: impl FnOnce() -> Result<Outcome>) -> Outcome {
    body().unwrap_or_else(Outcome::RunnerError)
}

/// Announces the stub, once the venue is running and has one to serve.
///
/// An editor connects on seeing this line, and a gdb-remote to a port nobody
/// serves blocks for about two minutes.
pub fn announce_debug_port(port: Option<NonZeroU16>) {
    if let Some(port) = port {
        eprintln!("{}", debug_port_announcement(port));
    }
}

/// Editors watch for this line, so every runner spells it the same way.
fn debug_port_announcement(port: NonZeroU16) -> String {
    format!("waiting for a debugger on port {port}")
}

/// Names the test a debugger is to launch, for a venue that does not run it.
pub fn announce_test_to_debug(test: &str) {
    eprintln!("{}", test_to_debug_announcement(test));
}

/// Editors watch for this line, so every runner spells it the same way.
fn test_to_debug_announcement(test: &str) -> String {
    format!("waiting for a debugger to run {test}")
}

/// Body of a runner `main`: the tests the mode allows, each bounded by its
/// timeout.
pub fn run(args: &CommonArgs, tests: Vec<TestMeta>, venue: impl Venue) -> Result<Outcome> {
    if args.verbose {
        eprintln!("{}", discovery_report(&tests));
    }

    if args.mode() == ExecutionMode::Debug {
        select_test_to_debug(args, &tests)?;
    }

    // Set by any test whose venue reported a failure of its own instead of a
    // verdict about the test.
    let venue_failed = Arc::new(AtomicBool::new(false));

    let venue = Arc::new(venue);
    let trials: Vec<Trial> = tests
        .into_iter()
        .map(|test| {
            trial(
                test,
                Arc::clone(&venue),
                args.timeout,
                Arc::clone(&venue_failed),
            )
        })
        .collect();

    let program = std::env::args().next().unwrap_or_default();
    let libtest_args = Arguments::from_iter(args.libtest_argv(&program));

    let conclusion = libtest_mimic::run(&libtest_args, trials);

    // A venue that broke mid-suite is not a suite that failed: reporting it as
    // one sends whoever reads it to debug a test that never ran.
    if venue_failed.load(Ordering::Relaxed) {
        bail!(
            "the venue failed while running the tests, so the results above are not a verdict on them"
        );
    }

    Ok(if conclusion.has_failed() {
        Outcome::TestsFailed
    } else {
        Outcome::Passed
    })
}

/// Why a debug run and the port it was given do not agree.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DebugPortError {
    #[error("--debug needs --gdb <PORT>, the port a debugger connects to")]
    Missing,
    #[error("--gdb serves a stub for a debug run; add --debug or drop the port")]
    Unused,
}

/// The port a stub is to be served on, for a venue that serves one.
pub fn debug_port(
    mode: ExecutionMode,
    port: Option<NonZeroU16>,
) -> Result<Option<NonZeroU16>, DebugPortError> {
    match (mode, port) {
        (ExecutionMode::Debug, None) => Err(DebugPortError::Missing),
        (ExecutionMode::Suite, Some(_)) => Err(DebugPortError::Unused),
        _ => Ok(port),
    }
}

/// Why a debug run has no one test to serve.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DebugSelectionError {
    #[error("no test to debug: nothing matches the filter")]
    NoMatch,
    #[error(
        "{} tests match, and a debugger takes one:\n  {}\n\
         pick one with `-- --exact <name>`",
        .names.len(),
        .names.join("\n  ")
    )]
    MultipleMatches { names: Vec<String> },
}

/// The one test a debug run selects, or why the selection is not one.
///
/// Each test is a separate run of the binary, so refuse before the first one
/// starts rather than part way through the selection. The trials exist to ask
/// libtest's own filter, not to run anything.
pub fn select_test_to_debug(
    args: &CommonArgs,
    tests: &[TestMeta],
) -> Result<String, DebugSelectionError> {
    let program = std::env::args().next().unwrap_or_default();
    let libtest_args = Arguments::from_iter(args.libtest_argv(&program));
    let trials: Vec<Trial> = tests
        .iter()
        .map(|test| Trial::test(test.name.clone(), || Ok(())).with_ignored_flag(test.ignored))
        .collect();

    let mut selected: Vec<String> = trials
        .iter()
        .filter(|trial| !libtest_args.is_filtered_out(trial) && !libtest_args.is_ignored(trial))
        .map(|trial| trial.name().to_string())
        .collect();

    match selected.len() {
        1 => Ok(selected.remove(0)),
        0 => Err(DebugSelectionError::NoMatch),
        _ => Err(DebugSelectionError::MultipleMatches { names: selected }),
    }
}

fn trial(
    test: TestMeta,
    venue: Arc<impl Venue>,
    default_timeout: u32,
    venue_failed: Arc<AtomicBool>,
) -> Trial {
    let timeout = Duration::from_secs(test.timeout.unwrap_or(default_timeout).into());
    let ignored = test.ignored;
    let should_panic = test.should_panic;
    let name = test.name.clone();

    Trial::test(name, move || {
        let observation = venue.run(test, timeout);
        if matches!(observation, Observation::HarnessError { .. }) {
            venue_failed.store(true, Ordering::Relaxed);
        }

        match Verdict::from_observation(observation, should_panic) {
            Verdict::Pass => Ok(()),
            Verdict::Fail(failure) => Err(Failed::from(failure.to_string())),
        }
    })
    .with_ignored_flag(ignored)
}

fn discovery_report(tests: &[TestMeta]) -> String {
    let mut lines = vec![format!("discovered {} test(s):", tests.len())];

    for test in tests {
        let mut tags = Vec::new();
        if test.ignored {
            tags.push("ignored".to_string());
        }
        if test.should_panic {
            tags.push("should_panic".to_string());
        }
        if let Some(seconds) = test.timeout {
            tags.push(format!("timeout={seconds}s"));
        }
        lines.push(if tags.is_empty() {
            format!("  {}", test.name)
        } else {
            format!("  {}  [{}]", test.name, tags.join(", "))
        });
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use anyhow::anyhow;

    use std::path::PathBuf;

    use super::*;
    use crate::Invocation;

    fn args(timeout: u32) -> CommonArgs {
        CommonArgs {
            timeout,
            verbose: false,
            debug: false,
            elf: PathBuf::from("target/debug/smoke"),
            libtest: Vec::new(),
        }
    }

    fn verbose_args() -> CommonArgs {
        CommonArgs {
            verbose: true,
            ..args(10)
        }
    }

    fn filtered_args(filter: &[&str]) -> CommonArgs {
        CommonArgs {
            libtest: filter.iter().map(ToString::to_string).collect(),
            ..args(10)
        }
    }

    fn debug_args(filter: &[&str]) -> CommonArgs {
        CommonArgs {
            debug: true,
            ..filtered_args(filter)
        }
    }

    fn test(name: &str) -> TestMeta {
        TestMeta::new(name, Invocation::Run(name.to_string()))
    }

    /// Venue that answers every test the same way and records what reached it.
    struct Fake {
        answer: Observation,
        reached: Arc<Mutex<Vec<(String, Duration)>>>,
    }

    impl Fake {
        fn new(answer: Observation) -> Self {
            Self {
                answer,
                reached: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// Taken before the venue is handed over, since `run` consumes it.
        fn reached(&self) -> Arc<Mutex<Vec<(String, Duration)>>> {
            Arc::clone(&self.reached)
        }
    }

    impl Venue for Fake {
        fn run(&self, test: TestMeta, timeout: Duration) -> Observation {
            self.reached.lock().unwrap().push((test.name, timeout));
            self.answer.clone()
        }
    }

    fn names(reached: &Mutex<Vec<(String, Duration)>>) -> Vec<String> {
        reached
            .lock()
            .unwrap()
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

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

    #[track_caller]
    fn assert_passed(outcome: &Outcome) {
        assert!(matches!(outcome, Outcome::Passed), "{outcome:?}");
    }

    #[track_caller]
    fn assert_tests_failed(outcome: &Outcome) {
        assert!(matches!(outcome, Outcome::TestsFailed), "{outcome:?}");
    }

    #[track_caller]
    fn assert_runner_error(outcome: &Outcome) {
        assert!(matches!(outcome, Outcome::RunnerError(_)), "{outcome:?}");
    }

    /// `run` reports through anyhow, which is what a runner `main` wants; the
    /// refusal underneath it is still the typed one.
    fn selection(error: &anyhow::Error) -> &DebugSelectionError {
        error
            .downcast_ref()
            .unwrap_or_else(|| panic!("expected a debug selection error, got {error:?}"))
    }

    #[test]
    fn test_suite_that_passes_exits_zero() {
        let outcome = run(
            &args(10),
            vec![test("tests::it_passes"), test("tests::it_also_passes")],
            Fake::new(finished()),
        )
        .unwrap();
        assert_passed(&outcome);
    }

    #[test]
    fn test_failing_test_exits_as_tests_failed() {
        let outcome = run(
            &args(10),
            vec![test("tests::it_fails")],
            Fake::new(aborted()),
        )
        .unwrap();
        assert_tests_failed(&outcome);
    }

    #[test]
    fn test_abort_satisfies_a_should_panic_test() {
        let mut expects_panic = test("tests::it_panics_as_expected");
        expects_panic.should_panic = true;

        let outcome = run(&args(10), vec![expects_panic], Fake::new(aborted())).unwrap();
        assert_passed(&outcome);
    }

    #[test]
    fn test_an_error_from_the_body_is_a_runner_error() {
        let outcome = main(|| Err(anyhow!("no such emulator")));
        assert_runner_error(&outcome);
    }

    #[test]
    fn test_main_passes_the_outcome_of_its_body_through() {
        let outcome = main(|| Ok(Outcome::TestsFailed));
        assert_tests_failed(&outcome);
    }

    #[test]
    fn test_a_venue_that_fails_mid_suite_is_a_runner_error_not_a_failed_suite() {
        let error = run(
            &args(10),
            vec![test("tests::it_runs")],
            Fake::new(Observation::harness_error(
                "failed to spawn `qemu-system-arm`",
                "",
            )),
        )
        .unwrap_err();

        assert!(error.to_string().contains("not a verdict"), "{error}");
    }

    #[test]
    fn test_ignored_test_never_reaches_the_venue() {
        let mut skipped = test("tests::it_is_ignored");
        skipped.ignored = true;

        let venue = Fake::new(finished());
        let reached = venue.reached();
        let outcome = run(&args(10), vec![skipped, test("tests::it_passes")], venue).unwrap();

        assert_passed(&outcome);
        assert_eq!(names(&reached), ["tests::it_passes"]);
    }

    #[test]
    fn test_declared_timeout_overrides_the_default() {
        let mut declared = test("tests::it_declares_a_timeout");
        declared.timeout = Some(3);

        let venue = Fake::new(finished());
        let seen = venue.reached();
        run(
            &args(10),
            vec![declared, test("tests::it_uses_the_default")],
            venue,
        )
        .unwrap();

        let seen = seen.lock().unwrap();
        assert!(
            seen.contains(&(
                "tests::it_declares_a_timeout".to_string(),
                Duration::from_secs(3)
            )),
            "{seen:?}"
        );
        assert!(
            seen.contains(&(
                "tests::it_uses_the_default".to_string(),
                Duration::from_secs(10)
            )),
            "{seen:?}"
        );
    }

    #[test]
    fn test_verbose_suite_still_reports_its_exit_code() {
        let outcome = run(
            &verbose_args(),
            vec![test("tests::it_passes")],
            Fake::new(finished()),
        )
        .unwrap();
        assert_passed(&outcome);
    }

    #[test]
    fn test_debug_runs_the_one_test_that_matches() {
        let venue = Fake::new(finished());
        let reached = venue.reached();
        let outcome = run(
            &debug_args(&["--exact", "tests::it_passes"]),
            vec![test("tests::it_passes"), test("tests::it_also_passes")],
            venue,
        )
        .unwrap();

        assert_passed(&outcome);
        // Which test ran is the whole point; an exit code alone is satisfied
        // just as well by the wrong one.
        assert_eq!(names(&reached), ["tests::it_passes"]);
    }

    #[test]
    fn test_a_suite_run_is_bounded_by_the_timeout() {
        let timeout = Duration::from_secs(10);
        assert_eq!(ExecutionMode::Suite.bound(timeout), Some(timeout));
    }

    #[test]
    fn test_a_debug_run_is_not_bounded_by_the_timeout() {
        assert_eq!(ExecutionMode::Debug.bound(Duration::from_secs(10)), None);
    }

    #[test]
    fn test_debug_refuses_a_filter_matching_several_tests() {
        let venue = Fake::new(finished());
        let reached = venue.reached();
        let error = run(
            &debug_args(&["tests::"]),
            vec![test("tests::it_passes"), test("tests::it_also_passes")],
            venue,
        )
        .unwrap_err();
        assert!(
            matches!(
                selection(&error),
                DebugSelectionError::MultipleMatches { names } if names.len() == 2
            ),
            "{error:?}"
        );
        assert!(reached.lock().unwrap().is_empty());
    }

    #[test]
    fn test_debug_names_the_candidates_it_will_not_choose_between() {
        let venue = Fake::new(finished());
        let reached = venue.reached();
        let error = run(
            &debug_args(&["tests::"]),
            vec![test("tests::it_passes"), test("tests::it_also_passes")],
            venue,
        )
        .unwrap_err();
        let DebugSelectionError::MultipleMatches { names } = selection(&error) else {
            panic!("expected several matches, got {error:?}");
        };
        assert_eq!(names, &["tests::it_passes", "tests::it_also_passes"]);
        assert!(reached.lock().unwrap().is_empty());
    }

    #[test]
    fn test_debug_refuses_a_filter_matching_nothing() {
        let venue = Fake::new(finished());
        let reached = venue.reached();
        let error = run(
            &debug_args(&["--exact", "tests::no_such_test"]),
            vec![test("tests::it_passes")],
            venue,
        )
        .unwrap_err();
        assert_eq!(selection(&error), &DebugSelectionError::NoMatch);
        assert!(reached.lock().unwrap().is_empty());
    }

    #[test]
    fn test_debug_does_not_count_an_ignored_test_as_the_one() {
        let mut skipped = test("tests::it_is_ignored");
        skipped.ignored = true;

        let outcome = run(
            &debug_args(&["tests::"]),
            vec![skipped, test("tests::it_passes")],
            Fake::new(finished()),
        )
        .unwrap();
        assert_passed(&outcome);
    }

    #[test]
    fn test_test_to_debug_announcement_wording_is_a_contract() {
        assert_eq!(
            test_to_debug_announcement("tests::it_passes"),
            "waiting for a debugger to run tests::it_passes"
        );
    }

    #[test]
    fn test_debug_port_announcement_wording_is_a_contract() {
        assert_eq!(
            debug_port_announcement(NonZeroU16::new(3333).unwrap()),
            "waiting for a debugger on port 3333"
        );
    }

    #[test]
    fn test_a_debug_run_serves_the_port_it_was_given() {
        let port = NonZeroU16::new(3333);
        assert_eq!(debug_port(ExecutionMode::Debug, port), Ok(port));
    }

    #[test]
    fn test_a_debug_run_without_a_port_is_refused() {
        assert_eq!(
            debug_port(ExecutionMode::Debug, None),
            Err(DebugPortError::Missing)
        );
    }

    #[test]
    fn test_a_port_without_a_debug_run_is_refused() {
        assert_eq!(
            debug_port(ExecutionMode::Suite, NonZeroU16::new(3333)),
            Err(DebugPortError::Unused)
        );
    }

    #[test]
    fn test_a_suite_serves_no_stub() {
        assert_eq!(debug_port(ExecutionMode::Suite, None), Ok(None));
    }

    #[test]
    fn test_discovery_report_lists_every_test() {
        let report = discovery_report(&[test("tests::it_passes"), test("tests::it_also_passes")]);
        assert!(report.contains("discovered 2 test(s):"), "{report}");
        assert!(report.contains("tests::it_passes"), "{report}");
        assert!(report.contains("tests::it_also_passes"), "{report}");
    }

    #[test]
    fn test_discovery_report_tags_the_attributes() {
        let mut tagged = test("tests::it_is_ignored");
        tagged.ignored = true;
        tagged.should_panic = true;
        tagged.timeout = Some(5);

        let report = discovery_report(&[tagged]);
        for tag in ["ignored", "should_panic", "timeout=5s"] {
            assert!(report.contains(tag), "{tag} missing from {report}");
        }
    }

    #[test]
    fn test_discovery_report_leaves_a_plain_test_untagged() {
        let report = discovery_report(&[test("tests::it_passes")]);
        assert!(!report.contains('['), "{report}");
    }
}
