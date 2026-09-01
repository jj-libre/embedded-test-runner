//! Contract each example must satisfy, and parsing of the libtest report.

use std::collections::{BTreeMap, HashSet};

/// Outcomes a runner must report for the example attribute matrix;
/// `it_panics_as_expected` is `Ok` because the runner inverts a
/// `#[should_panic]` test that panics.
pub(crate) const CONTRACT: &[(&str, Outcome)] = &[
    ("tests::it_passes", Outcome::Ok),
    ("tests::arithmetic_holds", Outcome::Ok),
    ("tests::it_fails", Outcome::Failed),
    ("tests::it_panics_as_expected", Outcome::Ok),
    ("tests::should_panic_but_doesnt", Outcome::Failed),
    ("tests::it_is_ignored", Outcome::Ignored),
    ("tests::it_times_out", Outcome::TimedOut),
];

/// How the runner opens the failure it reports for a test it stopped at the
/// deadline. Spelled out here rather than shared with the runner, so that a
/// change to the wording is caught instead of agreed with on both sides.
const TIMED_OUT: &str = "timed out after ";

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Outcome {
    Ok,
    Failed,
    /// A failure the runner attributed to the deadline. Libtest reports it as
    /// `FAILED` like any other, so it is told apart by its reason.
    TimedOut,
    Ignored,
}

impl Outcome {
    fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "ok" => Some(Outcome::Ok),
            "FAILED" => Some(Outcome::Failed),
            "ignored" => Some(Outcome::Ignored),
            _ => None,
        }
    }
}

pub(crate) fn check(transcript: &str) -> Result<(), Vec<String>> {
    let actual = outcomes(transcript);
    let mut mismatches = Vec::new();

    for (name, expected) in CONTRACT {
        match actual.get(*name) {
            Some(got) if got == expected => {}
            Some(got) => mismatches.push(format!("  {name}: expected {expected:?}, got {got:?}")),
            None => mismatches.push(format!("  {name}: missing (did the test run?)")),
        }
    }

    // A new test must force a contract update rather than pass unnoticed.
    let expected: HashSet<&str> = CONTRACT.iter().map(|(name, _)| *name).collect();
    for name in actual.keys() {
        if !expected.contains(name.as_str()) {
            mismatches.push(format!("  {name}: ran but is not in the contract"));
        }
    }

    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(mismatches)
    }
}

/// Harvests `test <name> ... <outcome>` lines. Summary lines start with
/// `test ` too, so a name containing a space is rejected.
fn outcomes(transcript: &str) -> BTreeMap<String, Outcome> {
    let reasons = reasons(transcript);
    let mut found = BTreeMap::new();
    for line in transcript.lines() {
        let Some(rest) = line.trim_start().strip_prefix("test ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once("...") else {
            continue;
        };
        let name = name.trim();
        if name.contains(' ') {
            continue;
        }
        if let Some(outcome) = Outcome::parse(tail) {
            found.insert(name.to_string(), refine(outcome, reasons.get(name)));
        }
    }
    found
}

/// A failure the runner blamed on the deadline is a different outcome from one
/// the test reached on its own, and only the reason says which it was.
fn refine(outcome: Outcome, reason: Option<&String>) -> Outcome {
    match (outcome, reason) {
        (Outcome::Failed, Some(reason)) if reason.starts_with(TIMED_OUT) => Outcome::TimedOut,
        (outcome, _) => outcome,
    }
}

/// Opening line of each `---- <name> ----` block in libtest's failure detail.
fn reasons(transcript: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let mut lines = transcript.lines();
    while let Some(line) = lines.next() {
        let Some(name) = line
            .trim()
            .strip_prefix("---- ")
            .and_then(|rest| rest.strip_suffix(" ----"))
        else {
            continue;
        };
        if let Some(reason) = lines.next() {
            found.insert(name.to_string(), reason.trim().to_string());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSING: &str = "\
running 7 tests
test tests::it_is_ignored           ... ignored
test tests::arithmetic_holds        ... ok
test tests::it_fails                ... FAILED
test tests::it_panics_as_expected   ... ok
test tests::should_panic_but_doesnt ... FAILED
test tests::it_passes               ... ok
test tests::it_times_out            ... FAILED

failures:

---- tests::should_panic_but_doesnt ----
test was expected to panic (#[should_panic]) but exited successfully

---- tests::it_fails ----
the test aborted

---- tests::it_times_out ----
timed out after 2s


failures:
    tests::should_panic_but_doesnt
    tests::it_fails
    tests::it_times_out

test result: FAILED. 3 passed; 3 failed; 1 ignored; 0 measured; 0 filtered out";

    #[test]
    fn test_expected_matrix_satisfies_contract() {
        assert_eq!(check(PASSING), Ok(()));
    }

    #[test]
    fn test_summary_line_is_not_read_as_test() {
        assert!(!outcomes(PASSING).contains_key("result:"));
        assert_eq!(outcomes(PASSING).len(), CONTRACT.len());
    }

    #[test]
    fn test_wrong_outcome_is_reported() {
        let transcript = PASSING.replace(
            "test tests::it_passes               ... ok",
            "test tests::it_passes               ... FAILED",
        );
        let mismatches = check(&transcript).unwrap_err();
        assert!(mismatches[0].contains("it_passes"), "{mismatches:?}");
    }

    #[test]
    fn test_missing_test_is_reported() {
        let transcript = PASSING.replace("test tests::it_passes               ... ok\n", "");
        assert!(check(&transcript).unwrap_err()[0].contains("missing"));
    }

    #[test]
    fn test_empty_transcript_fails_contract() {
        assert_eq!(check("").unwrap_err().len(), CONTRACT.len());
    }

    #[test]
    fn test_timeout_is_not_satisfied_by_an_ordinary_failure() {
        let transcript = PASSING.replace("timed out after 2s", "the test aborted");
        let mismatches = check(&transcript).unwrap_err();
        assert!(
            mismatches.iter().any(|m| m.contains("it_times_out")),
            "{mismatches:?}"
        );
    }

    #[test]
    fn test_failure_list_is_not_read_as_a_reason() {
        assert_eq!(reasons(PASSING).len(), 3);
    }

    #[test]
    fn test_unexpected_test_is_reported() {
        let transcript = format!("{PASSING}\ntest tests::brand_new ... ok");
        let mismatches = check(&transcript).unwrap_err();
        assert!(
            mismatches.iter().any(|m| m.contains("brand_new")),
            "{mismatches:?}"
        );
    }
}
