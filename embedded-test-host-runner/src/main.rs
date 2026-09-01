mod cli;
mod host;
mod listing;

use anyhow::Result;
use clap::Parser;

use embedded_test_runner_core::{
    ExecutionMode, Outcome, announce_test_to_debug, select_test_to_debug,
};

use crate::cli::Cli;
use crate::host::{Host, HostConfig};

fn main() -> Outcome {
    embedded_test_runner_core::main(|| execute(&Cli::parse()))
}

fn execute(cli: &Cli) -> Result<Outcome> {
    let config = HostConfig::from_cli(cli);
    let tests = listing::discover_tests(&cli.common.elf)?;

    // The debugger launches the test itself, so the runner only says which one.
    if cli.common.mode() == ExecutionMode::Debug {
        announce_test_to_debug(&select_test_to_debug(&cli.common, &tests)?);
        return Ok(Outcome::Passed);
    }

    embedded_test_runner_core::run(&cli.common, tests, Host::new(config))
}
