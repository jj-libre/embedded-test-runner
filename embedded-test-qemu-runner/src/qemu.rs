use std::num::NonZeroU16;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use embedded_test_runner_core::process::{self, ProcessError, ProcessOutcome, ProcessStatus};
use embedded_test_runner_core::{
    ExecutionMode, Invocation, Observation, TestMeta, Venue, announce_debug_port, debug_port,
};

use crate::cli::Cli;

/// What `--qemu` resolved to: the emulator and its board arguments.
pub(crate) struct QemuCommand {
    emulator: String,
    args: Vec<String>,
}

impl QemuCommand {
    pub(crate) fn parse(command: &str) -> Result<Self> {
        let mut tokens =
            shell_words::split(command).with_context(|| format!("parsing --qemu: {command:?}"))?;
        if tokens.is_empty() {
            bail!("--qemu must contain at least the QEMU binary name");
        }
        let emulator = tokens.remove(0);

        if let Some(reserved) = tokens
            .iter()
            .find(|token| is_reserved_for_the_runner(token))
        {
            bail!("--qemu must not contain {reserved}: it is reserved for the runner");
        }

        Ok(Self {
            emulator,
            args: tokens,
        })
    }
}

/// Everything a run needs that the command line decides.
pub(crate) struct QemuConfig {
    command: QemuCommand,
    elf: PathBuf,
    verbose: bool,
    mode: ExecutionMode,
    gdb: Option<NonZeroU16>,
}

impl QemuConfig {
    /// Reads the command line, refusing a `--qemu` the runner cannot append to.
    pub(crate) fn from_cli(cli: &Cli) -> Result<Self> {
        Ok(Self {
            command: QemuCommand::parse(&cli.qemu)?,
            elf: cli.common.elf.clone(),
            verbose: cli.common.verbose,
            mode: cli.common.mode(),
            gdb: debug_port(cli.common.mode(), cli.gdb)?,
        })
    }
}

pub(crate) struct Qemu {
    config: QemuConfig,
}

impl Qemu {
    pub(crate) fn new(config: QemuConfig) -> Self {
        Self { config }
    }

    fn run_command(
        &self,
        command: Command,
        timeout: Duration,
    ) -> Result<ProcessOutcome, ProcessError> {
        let running = process::start(command)?;
        announce_debug_port(self.config.gdb);
        running.finish(self.config.mode.bound(timeout))
    }

    /// Board arguments, the semihosting payload and the binary to boot.
    fn argv(&self, invocation: &Invocation) -> Vec<String> {
        let mut argv = self.config.command.args.clone();

        argv.push("-semihosting-config".to_string());
        argv.push(format!(
            "enable=on,target=native,arg={},arg={}",
            invocation.command(),
            invocation.operand()
        ));

        if let Some(port) = self.config.gdb {
            argv.push("-gdb".to_string());
            argv.push(format!("tcp:127.0.0.1:{port}"));
            argv.push("-S".to_string());
        }

        argv.push("-kernel".to_string());
        argv.push(self.config.elf.display().to_string());

        argv
    }
}

impl Venue for Qemu {
    fn run(&self, test: TestMeta, timeout: Duration) -> Observation {
        let argv = self.argv(&test.invocation);

        if self.config.verbose {
            let line = std::iter::once(&self.config.command.emulator).chain(&argv);
            eprintln!("QEMU: {}", shell_words::join(line));
        }

        let mut command = Command::new(&self.config.command.emulator);
        command.args(&argv);

        match self.run_command(command, timeout) {
            Ok(outcome) => observe_process(&outcome, timeout),
            Err(e) => e.into(),
        }
    }
}

/// A second copy of what `argv` appends either overrides the runner's silently
/// or makes QEMU refuse to start. QEMU accepts both `-kernel` and `--kernel`,
/// and `-s` is its shorthand for `-gdb`.
fn is_reserved_for_the_runner(token: &str) -> bool {
    let Some(name) = token.strip_prefix('-') else {
        return false;
    };

    matches!(
        name.trim_start_matches('-'),
        "kernel" | "semihosting-config" | "gdb" | "s" | "S"
    )
}

fn observe_process(outcome: &ProcessOutcome, timeout: Duration) -> Observation {
    let output = outcome.captured.to_string();
    match outcome.status {
        ProcessStatus::Exited(code) => Observation::from_exit_code(code, output),
        ProcessStatus::TimedOut => Observation::TimedOut {
            after: timeout,
            output,
        },
        ProcessStatus::Terminated => {
            Observation::harness_error("QEMU ended without an exit code", &output)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use embedded_test_runner_core::process::Captured;

    use embedded_test_runner_core::TestMeta;

    use super::*;

    fn qemu(board: &str, elf: &str, gdb: Option<NonZeroU16>) -> Qemu {
        Qemu::new(QemuConfig {
            command: QemuCommand::parse(board).unwrap(),
            elf: PathBuf::from(elf),
            verbose: false,
            mode: match gdb {
                Some(_) => ExecutionMode::Debug,
                None => ExecutionMode::Suite,
            },
            gdb,
        })
    }

    fn argv(board: &str) -> Vec<String> {
        qemu(board, "smoke.elf", None).argv(&Invocation::RunAddr(0x1000))
    }

    fn debug_argv(port: u16) -> Vec<String> {
        qemu("qemu-system-arm", "smoke.elf", NonZeroU16::new(port))
            .argv(&Invocation::RunAddr(0x1000))
    }

    fn rejection(command: &str) -> String {
        format!("{:#}", QemuCommand::parse(command).err().unwrap())
    }

    fn dispatch() -> TestMeta {
        TestMeta::new("tests::it_passes", Invocation::RunAddr(0x1000))
    }

    fn outcome(status: ProcessStatus) -> ProcessOutcome {
        ProcessOutcome {
            status,
            captured: Captured {
                stdout: "boom".to_string(),
                stderr: String::new(),
            },
        }
    }

    const TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn test_exit_code_is_read_as_the_protocol_verdict() {
        assert_eq!(
            observe_process(&outcome(ProcessStatus::Exited(134)), TIMEOUT),
            Observation::Aborted {
                output: "boom".to_string()
            }
        );
    }

    #[test]
    fn test_timeout_carries_bound_it_exceeded() {
        assert_eq!(
            observe_process(&outcome(ProcessStatus::TimedOut), TIMEOUT),
            Observation::TimedOut {
                after: TIMEOUT,
                output: "boom".to_string()
            }
        );
    }

    #[test]
    fn test_emulator_ending_without_an_exit_code_is_harness_error() {
        assert_eq!(
            observe_process(&outcome(ProcessStatus::Terminated), TIMEOUT),
            Observation::harness_error("QEMU ended without an exit code", "boom")
        );
    }

    #[test]
    fn test_empty_command_is_rejected() {
        assert!(QemuCommand::parse("   ").is_err());
    }

    #[test]
    fn test_unbalanced_quote_in_command_names_the_flag() {
        let message = rejection("qemu-system-arm -append \"boot");
        assert!(message.contains("--qemu"), "{message}");
    }

    #[test]
    fn test_semihosting_config_from_the_caller_is_rejected() {
        let message = rejection("qemu-system-arm -semihosting-config enable=on");
        assert!(message.contains("-semihosting-config"), "{message}");
    }

    #[test]
    fn test_kernel_from_the_caller_is_rejected() {
        let message = rejection("qemu-system-arm -kernel other.elf");
        assert!(message.contains("-kernel"), "{message}");
    }

    #[test]
    fn test_reserved_argument_is_rejected_in_its_double_dashed_spelling() {
        let message = rejection("qemu-system-arm --kernel other.elf");
        assert!(message.contains("--kernel"), "{message}");
    }

    #[test]
    fn test_gdb_stub_from_the_caller_is_rejected() {
        let message = rejection("qemu-system-arm -gdb tcp:127.0.0.1:3333");
        assert!(message.contains("-gdb"), "{message}");
    }

    #[test]
    fn test_gdb_shorthand_from_the_caller_is_rejected() {
        let message = rejection("qemu-system-arm -s");
        assert!(message.contains("-s"), "{message}");
    }

    #[test]
    fn test_halted_start_from_the_caller_is_rejected() {
        let message = rejection("qemu-system-arm -S");
        assert!(message.contains("-S"), "{message}");
    }

    #[test]
    fn test_argument_value_spelled_like_a_reserved_name_is_kept() {
        let arguments = argv("qemu-system-arm -append s");
        assert!(
            arguments.windows(2).any(|pair| pair == ["-append", "s"]),
            "{arguments:?}"
        );
        assert!(!arguments.iter().any(|argument| argument == "-S"));
    }

    #[test]
    fn test_argv_encodes_the_invocation_for_semihosting() {
        let arguments = argv("qemu-system-arm");
        assert!(
            arguments.contains(&"enable=on,target=native,arg=run_addr,arg=4096".to_string()),
            "{arguments:?}"
        );
    }

    #[test]
    fn test_argv_keeps_the_board_arguments_first() {
        let arguments = argv("qemu-system-arm -machine virt -nographic");
        assert_eq!(arguments[..3], ["-machine", "virt", "-nographic"]);
    }

    #[test]
    fn test_argv_boots_the_elf_as_the_kernel() {
        let arguments = argv("qemu-system-arm");
        let kernel = arguments
            .iter()
            .position(|argument| argument == "-kernel")
            .unwrap();
        assert_eq!(arguments[kernel + 1], "smoke.elf");
    }

    #[test]
    fn test_emulator_that_cannot_be_spawned_is_a_harness_error() {
        let qemu = Qemu::new(QemuConfig {
            command: QemuCommand::parse("no-such-emulator").unwrap(),
            elf: PathBuf::from("smoke.elf"),
            verbose: true,
            mode: ExecutionMode::Suite,
            gdb: None,
        });
        let reported = format!("{:?}", qemu.run(dispatch(), TIMEOUT));

        assert!(reported.starts_with("HarnessError"), "{reported}");
        assert!(reported.contains("no-such-emulator"), "{reported}");
    }

    #[test]
    fn test_argv_serves_a_stub_on_the_requested_port() {
        let arguments = debug_argv(3333);
        let gdb = arguments
            .iter()
            .position(|argument| argument == "-gdb")
            .unwrap();
        assert_eq!(arguments[gdb + 1], "tcp:127.0.0.1:3333");
    }

    #[test]
    fn test_argv_holds_the_guest_for_the_debugger() {
        assert!(debug_argv(3333).contains(&"-S".to_string()));
    }

    #[test]
    fn test_argv_without_a_port_serves_no_stub() {
        let arguments = argv("qemu-system-arm");
        assert!(!arguments.iter().any(|argument| argument == "-gdb"));
        assert!(!arguments.iter().any(|argument| argument == "-S"));
    }

    #[test]
    fn test_argv_leaves_out_the_emulator_itself() {
        let arguments = argv("qemu-system-arm");
        assert!(
            !arguments.iter().any(|argument| argument.contains("qemu")),
            "{arguments:?}"
        );
    }
}
