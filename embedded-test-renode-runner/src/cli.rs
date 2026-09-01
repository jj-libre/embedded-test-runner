use std::num::NonZeroU16;
use std::path::PathBuf;

use clap::Parser;
use embedded_test_runner_core::CommonArgs;

use crate::renode::{DEFAULT_CPU, DEFAULT_EMULATOR, DEFAULT_WALL_TIMEOUT_SECS};

#[derive(Parser, Debug)]
#[command(
    name = "embedded-test-renode-runner",
    about = "Run embedded-test binaries inside the Renode emulator"
)]
pub(crate) struct Cli {
    /// Platform description for the board under test.
    #[arg(long, value_name = "FILE")]
    pub(crate) platform: PathBuf,

    /// Extra Renode script, run after the platform and before the ELF.
    #[arg(long, value_name = "FILE")]
    pub(crate) script: Option<PathBuf>,

    /// Renode executable. Semihosting needs a nightly from builds.renode.io.
    #[arg(long, default_value = DEFAULT_EMULATOR, env = "RENODE", value_name = "PATH")]
    pub(crate) renode: PathBuf,

    /// Name of the CPU in the platform description.
    #[arg(long, default_value = DEFAULT_CPU, value_name = "NAME")]
    pub(crate) cpu: String,

    /// Wall-clock bound on Renode itself, in seconds. `--timeout` bounds the
    /// test in virtual time; raise this for suites with a large virtual budget.
    #[arg(long, default_value_t = DEFAULT_WALL_TIMEOUT_SECS, value_name = "SECS")]
    pub(crate) wall_timeout: u64,

    /// Serve a GDB stub on this port and hold the guest at reset until a
    /// debugger connects. Takes exactly one test, and drops `--wall-timeout`.
    #[arg(long, env = "EMBEDDED_TEST_GDB", value_name = "PORT")]
    pub(crate) gdb: Option<NonZeroU16>,

    #[command(flatten)]
    pub(crate) common: CommonArgs,
}
