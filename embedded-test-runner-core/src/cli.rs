use std::path::PathBuf;

use clap::Args;

use crate::ExecutionMode;

/// CLI surface shared by every venue runner.
#[derive(Args, Debug)]
pub struct CommonArgs {
    /// Per-test timeout fallback in seconds; `#[timeout(N)]` overrides it.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
    pub timeout: u32,

    /// Print discovered tests and venue invocations before running.
    #[arg(long)]
    pub verbose: bool,

    /// Hold one test for a debugger, dropping the timeout.
    #[arg(long, env = "EMBEDDED_TEST_DEBUG")]
    pub debug: bool,

    /// Binary under test, the first trailing argument cargo passes.
    #[arg(value_name = "ELF")]
    pub elf: PathBuf,

    /// libtest args (filters, `--list`, `--include-ignored`, ...).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "LIBTEST"
    )]
    pub libtest: Vec<String>,
}

impl CommonArgs {
    /// What this invocation is doing, which every venue and the harness read.
    pub fn mode(&self) -> ExecutionMode {
        if self.debug {
            ExecutionMode::Debug
        } else {
            ExecutionMode::Suite
        }
    }

    /// Argv for the libtest-mimic parser, without the ELF path.
    pub(crate) fn libtest_argv(&self, program: &str) -> Vec<String> {
        let mut argv = vec![program.to_string()];
        argv.extend(self.libtest.iter().cloned());
        argv
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    /// `CommonArgs` is flattened into every venue CLI; this stands in for one.
    #[derive(Parser, Debug)]
    struct Runner {
        #[command(flatten)]
        common: CommonArgs,
    }

    fn parse(argv: &[&str]) -> Result<CommonArgs, clap::Error> {
        Runner::try_parse_from(argv).map(|runner| runner.common)
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    fn args(libtest: &[&str]) -> CommonArgs {
        CommonArgs {
            timeout: 10,
            verbose: false,
            debug: false,
            elf: PathBuf::from("target/debug/smoke"),
            libtest: strings(libtest),
        }
    }

    #[test]
    fn test_argv_keeps_libtest_args() {
        assert_eq!(
            args(&["--list", "--format", "terse"]).libtest_argv("runner"),
            strings(&["runner", "--list", "--format", "terse"])
        );
    }

    #[test]
    fn test_argv_from_nothing_is_just_the_program() {
        assert_eq!(args(&[]).libtest_argv("runner"), strings(&["runner"]));
    }

    #[test]
    fn test_elf_is_required() {
        assert_eq!(
            parse(&["runner"]).unwrap_err().kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn test_timeout_below_the_range_is_refused() {
        assert_eq!(
            parse(&["runner", "--timeout", "0", "smoke"])
                .unwrap_err()
                .kind(),
            clap::error::ErrorKind::ValueValidation
        );
    }

    #[test]
    fn test_timeout_is_read_from_the_command_line() {
        assert_eq!(
            parse(&["runner", "--timeout", "45", "smoke"])
                .unwrap()
                .timeout,
            45
        );
    }

    #[test]
    fn test_elf_is_taken_before_the_libtest_args() {
        let common = parse(&["runner", "target/debug/smoke", "--list"]).unwrap();
        assert_eq!(common.elf, PathBuf::from("target/debug/smoke"));
        assert_eq!(common.libtest, strings(&["--list"]));
    }
}
