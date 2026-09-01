//! Runs each example through its runner and checks the per-test outcomes.
//!
//! Examples contain tests meant to fail, so `cargo test` in one exits non-zero
//! by design.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, anyhow};
use clap::Args;

use super::catalog::{Example, discover_examples};
use super::outcome;
use super::selection::Selection;
use crate::cargo;

#[derive(Args, Debug)]
pub(crate) struct TestExamples {
    #[command(flatten)]
    selection: Selection,

    /// Renode executable. Semihosting needs a nightly from builds.renode.io.
    #[arg(long, default_value = "renode", env = "RENODE")]
    renode: PathBuf,
}

impl TestExamples {
    pub(crate) fn run(&self, root: &Path) -> ExitCode {
        let discovered = match discover_examples(root) {
            Ok(discovered) => discovered,
            Err(e) => {
                eprintln!("error: {e:#}");
                return ExitCode::from(2);
            }
        };

        let selected = match self.selection.resolve(&discovered) {
            Ok(selected) => selected,
            Err(e) => {
                eprintln!("error: {e:#}");
                return ExitCode::from(2);
            }
        };

        if let Err(e) = cargo::build_runners(root) {
            eprintln!("error: {e:#}");
            return ExitCode::from(2);
        }

        let mut failures = 0;
        for example in selected {
            match check(example, root, &self.renode) {
                Ok(()) => println!("[{}] OK", example.dir),
                Err(e) => {
                    eprintln!("[{}] {e:#}", example.dir);
                    failures += 1;
                }
            }
        }

        if failures == 0 {
            ExitCode::SUCCESS
        } else {
            eprintln!("\n{failures} example(s) failed");
            ExitCode::FAILURE
        }
    }
}

/// Runs the example the way a user does: `cargo test` takes the runner and its
/// arguments from the example's own `.cargo/config.toml`.
fn check(example: &Example, root: &Path, renode: &Path) -> Result<()> {
    let output = Command::new("cargo")
        .args(["test", "--release"])
        .current_dir(root.join(&example.dir))
        .env("PATH", runner_path(root)?)
        .env("RENODE", renode)
        .output()
        .context("spawning cargo test")?;

    let transcript = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let failure = |head: String| anyhow!("{head}\n  transcript:\n{}", indent(&transcript));

    if let Err(mismatches) = outcome::check(&transcript) {
        return Err(failure(format!(
            "outcome mismatch:\n{}",
            mismatches.join("\n")
        )));
    }

    // The contract requires failing tests, so cargo has to have been told they
    // failed: a runner that exits zero leaves every suite green.
    if output.status.success() {
        return Err(failure(
            "cargo test exited zero although the contract requires failing tests".to_string(),
        ));
    }

    Ok(())
}

/// `PATH` with the freshly built runners in front, so the bare runner name in
/// an example's `.cargo/config.toml` resolves to them.
fn runner_path(root: &Path) -> Result<OsString> {
    let inherited = env::var_os("PATH").unwrap_or_default();
    let dirs =
        std::iter::once(target_dir(root).join("release")).chain(env::split_paths(&inherited));
    env::join_paths(dirs).context("building PATH")
}

/// Where cargo put the runners. `CARGO_TARGET_DIR` moves them, and joining
/// leaves an absolute setting alone while resolving a relative one against the
/// directory the build ran in.
fn target_dir(root: &Path) -> PathBuf {
    root.join(env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| OsString::from("target")))
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
