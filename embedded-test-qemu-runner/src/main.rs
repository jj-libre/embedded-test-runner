mod cli;
mod qemu;

use anyhow::Result;
use clap::Parser;
use embedded_test_runner_core::{Outcome, elf};

use crate::cli::Cli;
use crate::qemu::{Qemu, QemuConfig};

fn main() -> Outcome {
    embedded_test_runner_core::main(|| execute(&Cli::parse()))
}

fn execute(cli: &Cli) -> Result<Outcome> {
    let config = QemuConfig::from_cli(cli)?;
    let tests = elf::discover_tests(&cli.common.elf)?;

    embedded_test_runner_core::run(&cli.common, tests, Qemu::new(config))
}
