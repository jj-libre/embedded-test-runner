//! Tasks that work on the runner examples.

mod catalog;
mod outcome;
mod selection;
mod sync;
mod test;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;

pub(crate) use self::catalog::Venue;
use self::sync::SyncExamples;
use self::test::TestExamples;

#[derive(Subcommand, Debug)]
pub(crate) enum Examples {
    /// Run each example through its runner and check the per-test outcomes.
    Test(TestExamples),

    /// Write the files every example of a venue shares from its template.
    Sync(SyncExamples),
}

impl Examples {
    pub(crate) fn run(&self) -> ExitCode {
        match self {
            Examples::Test(task) => task.run(&repo_root()),
            Examples::Sync(task) => task.run(&repo_root()),
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}
