//! Tests the binary lists when run with `list`.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use embedded_test_runner_core::process::{self, ProcessOutcome, ProcessStatus};
use embedded_test_runner_core::{Invocation, TestMeta};
use serde::Deserialize;

const LISTING_TIMEOUT: Duration = Duration::from_secs(30);

/// Version embedded-test stamps on the listing.
const SUPPORTED_VERSION: u32 = 0;

/// Listing the binary prints, as one JSON object on stdout.
#[derive(Deserialize)]
struct Listing {
    version: u32,
    tests: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    ignored: bool,
    should_panic: bool,
    timeout: Option<u32>,
}

impl Entry {
    /// The host venue runs a test by name, which is also how it is reported.
    fn into_test_meta(self) -> TestMeta {
        let Entry {
            name,
            ignored,
            should_panic,
            timeout,
        } = self;
        let invocation = Invocation::Run(name.clone());

        TestMeta {
            name,
            invocation,
            ignored,
            should_panic,
            timeout,
        }
    }
}

pub(crate) fn discover_tests(binary: &Path) -> Result<Vec<TestMeta>> {
    let mut command = Command::new(binary);
    command.arg("list");

    tests_listed(&process::run(command, Some(LISTING_TIMEOUT))?, binary)
}

fn tests_listed(outcome: &ProcessOutcome, binary: &Path) -> Result<Vec<TestMeta>> {
    match outcome.status {
        ProcessStatus::Exited(0) => parse(&outcome.captured.stdout),
        _ => bail!(
            "`{} list` did not print a test listing — is it an embedded-test \
             binary built with the `std` feature?\n{}",
            binary.display(),
            outcome.captured
        ),
    }
}

fn parse(stdout: &str) -> Result<Vec<TestMeta>> {
    let listing = stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with('{'))
        .ok_or_else(|| anyhow!("`list` printed no JSON object"))?;

    let listing: Listing = serde_json::from_str(listing).with_context(|| {
        format!(
            "listing did not parse — the embedded-test metadata format has \
             probably changed:\n  {listing}"
        )
    })?;

    if listing.version != SUPPORTED_VERSION {
        bail!(
            "test listing version {} is not supported by this runner \
             (supports v{SUPPORTED_VERSION})",
            listing.version
        );
    }

    Ok(listing
        .tests
        .into_iter()
        .map(Entry::into_test_meta)
        .collect())
}

#[cfg(test)]
mod tests {
    use embedded_test_runner_core::process::Captured;

    use super::*;

    /// Verbatim from a std example built against embedded-test 0.7.
    const LISTING: &str = r#"{"version":0,"tests":[{"name":"tests::it_passes","should_panic":false,"ignored":false,"timeout":null},{"name":"tests::slow","should_panic":true,"ignored":true,"timeout":42}]}"#;

    fn outcome(status: ProcessStatus, stdout: &str) -> ProcessOutcome {
        ProcessOutcome {
            status,
            captured: Captured {
                stdout: stdout.to_string(),
                stderr: String::new(),
            },
        }
    }

    fn error(status: ProcessStatus, stdout: &str) -> String {
        let error = tests_listed(&outcome(status, stdout), Path::new("smoke")).unwrap_err();
        format!("{error:#}")
    }

    #[test]
    fn test_listing_becomes_name_dispatched_tests() {
        let tests = parse(LISTING).unwrap();
        assert_eq!(tests[0].name, "tests::it_passes");
        assert_eq!(
            tests[0].invocation,
            Invocation::Run("tests::it_passes".to_string())
        );
    }

    #[test]
    fn test_attributes_survive_the_listing() {
        let tests = parse(LISTING).unwrap();
        assert!(tests[1].ignored);
        assert!(tests[1].should_panic);
        assert_eq!(tests[1].timeout, Some(42));
    }

    #[test]
    fn test_object_is_found_among_other_output() {
        let stdout = format!("some setup chatter\n{LISTING}\n");
        assert_eq!(parse(&stdout).unwrap().len(), 2);
    }

    #[test]
    fn test_output_without_json_is_error() {
        assert!(parse("no tests here").is_err());
    }

    #[test]
    fn test_unsupported_listing_version_is_error() {
        let message = format!(
            "{:#}",
            parse(&LISTING.replace(r#""version":0"#, r#""version":1"#)).unwrap_err()
        );
        assert!(message.contains("listing version 1"), "{message}");
    }

    #[test]
    fn test_unknown_listing_format_is_error() {
        let message = format!("{:#}", parse(r#"{"tests":[{"name":1}]}"#).unwrap_err());
        assert!(message.contains("format has probably changed"), "{message}");
    }

    #[test]
    fn test_clean_exit_yields_the_listed_tests() {
        let tests = tests_listed(
            &outcome(ProcessStatus::Exited(0), LISTING),
            Path::new("smoke"),
        )
        .unwrap();
        assert_eq!(tests.len(), 2);
    }

    #[test]
    fn test_non_zero_exit_names_the_binary_and_the_std_feature() {
        let message = error(ProcessStatus::Exited(1), "");
        assert!(message.contains("smoke"), "{message}");
        assert!(message.contains("`std` feature"), "{message}");
    }

    #[test]
    fn test_timeout_is_not_read_as_a_listing() {
        let message = error(ProcessStatus::TimedOut, LISTING);
        assert!(
            message.contains("did not print a test listing"),
            "{message}"
        );
    }
}
