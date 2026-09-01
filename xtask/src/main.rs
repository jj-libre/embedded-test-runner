mod cargo;
mod examples;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::examples::Examples;

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Development tasks for this workspace")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand, Debug)]
enum Task {
    /// Work on the runner examples.
    Examples {
        #[command(subcommand)]
        command: Examples,
    },
}

fn main() -> ExitCode {
    match Cli::parse().task {
        Task::Examples { command } => command.run(),
    }
}
