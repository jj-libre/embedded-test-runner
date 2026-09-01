//! Apparatus the runner tests stand on: synthesised ELFs and process capture.
#![expect(
    clippy::return_self_not_must_use,
    clippy::missing_panics_doc,
    reason = "scaffolding rather than API: a builder is chained in one go, and panicking is how a fixture reports failure"
)]

pub mod elf;
mod process;

pub use elf::{TempElf, on_disk, valid_elf};
pub use process::{Run, run};
