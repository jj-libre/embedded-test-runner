use std::num::NonZeroU16;

use clap::Parser;
use embedded_test_runner_core::CommonArgs;

#[derive(Parser, Debug)]
#[command(
    name = "embedded-test-qemu-runner",
    about = "Run embedded-test binaries inside QEMU"
)]
pub(crate) struct Cli {
    /// QEMU command: the emulator plus board arguments, as one shell-quoted
    /// string. Must not include `-kernel`, `-semihosting-config`, `-gdb`,
    /// `-s` or `-S`.
    #[arg(long, value_name = "COMMAND")]
    pub(crate) qemu: String,

    /// Serve a GDB stub on this port and hold the guest until a debugger
    /// connects. Takes exactly one test, and drops the timeout.
    #[arg(long, env = "EMBEDDED_TEST_GDB", value_name = "PORT")]
    pub(crate) gdb: Option<NonZeroU16>,

    #[command(flatten)]
    pub(crate) common: CommonArgs,
}
