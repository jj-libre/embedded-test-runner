use clap::Parser;
use embedded_test_runner_core::CommonArgs;

#[derive(Parser, Debug)]
#[command(
    name = "embedded-test-host-runner",
    about = "Run embedded-test binaries as native host processes"
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}
