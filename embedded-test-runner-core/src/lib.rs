//! Shared core for `embedded-test` runners.

mod cli;
pub mod elf;
mod harness;
pub mod process;
mod protocol;
mod verdict;

pub use cli::CommonArgs;
pub use harness::{
    DebugPortError, DebugSelectionError, ExecutionMode, Outcome, Venue, announce_debug_port,
    announce_test_to_debug, debug_port, main, run, select_test_to_debug,
};
pub use protocol::{Invocation, TestMeta};
pub use verdict::Observation;
