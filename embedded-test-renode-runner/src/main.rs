mod cli;
mod renode;
mod report;

use anyhow::Result;
use clap::Parser;
use embedded_test_runner_core::{Outcome, elf};

use crate::cli::Cli;
use crate::renode::{Renode, RenodeConfig};

fn main() -> Outcome {
    embedded_test_runner_core::main(|| execute(&Cli::parse()))
}

fn execute(cli: &Cli) -> Result<Outcome> {
    let config = RenodeConfig::from_cli(cli)?;
    let tests = elf::discover_tests(&cli.common.elf)?;

    embedded_test_runner_core::run(&cli.common, tests, Renode::new(config))
}
