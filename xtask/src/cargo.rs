use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;

use crate::examples::Venue;

/// Runner binaries the examples invoke.
pub(crate) fn build_runners(root: &Path) -> Result<()> {
    let status = build_command()
        .current_dir(root)
        .status()
        .context("spawning cargo build")?;

    if !status.success() {
        bail!("building the runners exited with {status}");
    }
    Ok(())
}

/// One release build covering every venue's runner.
fn build_command() -> Command {
    let mut cargo = Command::new("cargo");
    cargo.args(["build", "--release"]);
    for venue in Venue::value_variants() {
        cargo.args(["-p", venue.runner()]);
    }
    cargo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Vec<String> {
        build_command()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn test_one_release_build_covers_every_venue_runner() {
        let args = args();
        assert_eq!(args[..2], ["build", "--release"], "{args:?}");
        for venue in Venue::value_variants() {
            assert!(args.contains(&venue.runner().to_string()), "{args:?}");
        }
    }
}
