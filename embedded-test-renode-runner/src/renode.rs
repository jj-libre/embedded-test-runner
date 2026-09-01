use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use embedded_test_runner_core::process::{self, ProcessError, ProcessOutcome, ProcessStatus};
use embedded_test_runner_core::{
    ExecutionMode, Invocation, Observation, TestMeta, Venue, announce_debug_port, debug_port,
};
use tempfile::TempDir;

use crate::cli::Cli;
use crate::report::Report;

/// Flags the transcript parsing depends on, not for the user to set.
const FLAGS: [&str; 4] = ["--disable-gui", "--console", "--plain", "--hide-log"];

/// Config file inside the private directory, passed to `--config`.
const CONFIG_NAME: &str = "renode-config";

/// Defaults shared with the CLI, so the two cannot drift.
pub(crate) const DEFAULT_EMULATOR: &str = "renode";
pub(crate) const DEFAULT_CPU: &str = "cpu";
pub(crate) const DEFAULT_WALL_TIMEOUT_SECS: u64 = 120;

/// Everything a run needs that the command line decides.
pub(crate) struct RenodeConfig {
    emulator: PathBuf,
    platform: PathBuf,
    script: Option<PathBuf>,
    cpu: String,
    elf: PathBuf,
    wall_timeout: Duration,
    verbose: bool,
    mode: ExecutionMode,
    gdb: Option<NonZeroU16>,
}

impl RenodeConfig {
    /// Reads the command line, resolving what the monitor will be given.
    ///
    /// The monitor resolves a relative path against Renode's working
    /// directory, not the runner's.
    pub(crate) fn from_cli(cli: &Cli) -> Result<Self> {
        Ok(Self {
            emulator: cli.renode.clone(),
            platform: absolute(&cli.platform)?,
            script: cli.script.as_deref().map(absolute).transpose()?,
            cpu: cli.cpu.clone(),
            elf: absolute(&cli.common.elf)?,
            wall_timeout: Duration::from_secs(cli.wall_timeout),
            verbose: cli.common.verbose,
            mode: cli.common.mode(),
            gdb: debug_port(cli.common.mode(), cli.gdb)?,
        })
    }
}

impl Venue for Renode {
    fn run(&self, test: TestMeta, timeout: Duration) -> Observation {
        let monitor_commands = self.monitor_commands(&test.invocation, timeout);

        if self.config.verbose {
            eprintln!("Renode:\n  {}", monitor_commands.join("\n  "));
        }

        let directory = match private_config(&std::env::temp_dir()) {
            Ok(directory) => directory,
            Err(e) => return Observation::harness_error(&format!("{e:#}"), ""),
        };

        let mut command = Command::new(&self.config.emulator);
        command
            .args(FLAGS)
            .arg("--config")
            .arg(directory.path().join(CONFIG_NAME));

        for monitor_command in monitor_commands {
            command.arg("-e").arg(monitor_command);
        }

        match self.run_command(command) {
            Ok(outcome) => observe_transcript(&outcome, timeout, self.config.wall_timeout),
            Err(e) => e.into(),
        }
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("resolving {}", path.display()))
}

pub(crate) struct Renode {
    config: RenodeConfig,
}

impl Renode {
    pub(crate) fn new(config: RenodeConfig) -> Self {
        Self { config }
    }

    fn run_command(&self, command: Command) -> Result<ProcessOutcome, ProcessError> {
        let running = process::start(command)?;
        announce_debug_port(self.config.gdb);
        running.finish(self.config.mode.bound(self.config.wall_timeout))
    }

    fn monitor_commands(&self, invocation: &Invocation, budget: Duration) -> Vec<String> {
        let mut commands = self.machine_setup();
        commands.extend(self.semihosting(invocation));
        commands.extend(self.debugger());
        commands.extend(self.run_and_report(budget));
        commands
    }

    fn machine_setup(&self) -> Vec<String> {
        let mut commands = vec![
            r#"mach create "embedded-test""#.to_string(),
            format!(
                "machine LoadPlatformDescription @{}",
                monitor_path(&self.config.platform)
            ),
        ];

        if let Some(script) = &self.config.script {
            commands.push(format!("include @{}", monitor_path(script)));
        }

        commands.push(format!(
            "sysbus LoadELF @{}",
            monitor_path(&self.config.elf)
        ));
        commands
    }

    fn semihosting(&self, invocation: &Invocation) -> Vec<String> {
        let cpu = &self.config.cpu;
        vec![
            format!(
                r#"{cpu}.semihosting ProgramName "{}""#,
                invocation.command()
            ),
            // The trailing space matters: Renode reports the command line
            // length including its terminator, so without it the operand would
            // arrive with a NUL attached and fail to parse.
            format!(
                r#"{cpu}.semihosting ProgramArguments "{} ""#,
                invocation.operand()
            ),
        ]
    }

    fn debugger(&self) -> Vec<String> {
        let cpu = &self.config.cpu;
        let Some(port) = self.config.gdb else {
            return Vec::new();
        };

        vec![
            format!("machine StartGdbServer {port} false"),
            // `SingleStep` is blocking, so the guest waits at reset until the
            // debugger continues it; without it `RunFor` races the debugger
            // to the end of the test.
            format!("{cpu} ExecutionMode SingleStep"),
        ]
    }

    fn run_and_report(&self, budget: Duration) -> Vec<String> {
        let cpu = &self.config.cpu;
        vec![
            format!(r#"emulation RunFor "{}""#, timespan(budget)),
            format!("{cpu}.semihosting Exited"),
            format!("{cpu}.semihosting ExitCode"),
            "quit".to_string(),
        ]
    }
}

fn observe_transcript(
    outcome: &ProcessOutcome,
    budget: Duration,
    wall_timeout: Duration,
) -> Observation {
    let output = outcome.captured.to_string();

    match outcome.status {
        ProcessStatus::Exited(_) => match Report::parse(&outcome.captured.stdout) {
            Ok(Report::Exited(code)) => Observation::from_exit_code(code, output),
            Ok(Report::Running) => Observation::TimedOut {
                after: budget,
                output,
            },
            Err(e) => Observation::harness_error(&e.to_string(), &output),
        },
        ProcessStatus::TimedOut => Observation::harness_error(
            &format!(
                "Renode did not finish within {}s (--wall-timeout)",
                wall_timeout.as_secs()
            ),
            &output,
        ),
        ProcessStatus::Terminated => {
            Observation::harness_error("Renode ended without an exit code", &output)
        }
    }
}

/// Renode locks its config file and writes a history beside it, both shared by
/// every instance unless told otherwise, so each run gets its own.
fn private_config(parent: &Path) -> Result<TempDir> {
    let directory = tempfile::Builder::new()
        .prefix("embedded-test-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "creating a directory for the Renode config in {}",
                parent.display()
            )
        })?;

    let history = monitor_path(&directory.path().join("history"));
    std::fs::write(
        directory.path().join(CONFIG_NAME),
        format!("[general]\nhistory-path = {history}\n"),
    )
    .context("writing the Renode config")?;

    Ok(directory)
}

/// Monitor paths use forward slashes.
fn monitor_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn timespan(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use embedded_test_runner_core::process::Captured;

    use embedded_test_runner_core::TestMeta;

    use super::*;

    fn config() -> RenodeConfig {
        RenodeConfig {
            emulator: PathBuf::from(DEFAULT_EMULATOR),
            platform: PathBuf::from("/boards/board.repl"),
            script: None,
            cpu: DEFAULT_CPU.to_string(),
            elf: PathBuf::from("/build/smoke"),
            wall_timeout: WALL_TIMEOUT,
            verbose: false,
            mode: ExecutionMode::Suite,
            gdb: None,
        }
    }

    fn renode() -> Renode {
        Renode::new(config())
    }

    fn commands(renode: &Renode) -> Vec<String> {
        renode.monitor_commands(&Invocation::RunAddr(1181), Duration::from_secs(1))
    }

    fn debug_commands(port: u16) -> Vec<String> {
        commands(&Renode::new(RenodeConfig {
            mode: ExecutionMode::Debug,
            gdb: NonZeroU16::new(port),
            ..config()
        }))
    }

    fn position(commands: &[String], needle: &str) -> usize {
        commands
            .iter()
            .position(|command| command.contains(needle))
            .unwrap()
    }

    fn outcome(stdout: &str) -> ProcessOutcome {
        ProcessOutcome {
            status: ProcessStatus::Exited(0),
            captured: Captured {
                stdout: stdout.to_string(),
                stderr: String::new(),
            },
        }
    }

    const WALL_TIMEOUT: Duration = Duration::from_secs(DEFAULT_WALL_TIMEOUT_SECS);

    fn dispatch() -> TestMeta {
        TestMeta::new("tests::it_passes", Invocation::RunAddr(1181))
    }

    #[test]
    fn test_renode_that_cannot_be_spawned_is_a_harness_error() {
        let renode = Renode::new(RenodeConfig {
            emulator: PathBuf::from("no-such-renode"),
            wall_timeout: Duration::from_secs(5),
            verbose: true,
            ..config()
        });
        let reported = format!("{:?}", renode.run(dispatch(), Duration::from_secs(1)));

        assert!(reported.starts_with("HarnessError"), "{reported}");
        assert!(reported.contains("no-such-renode"), "{reported}");
    }

    #[test]
    fn test_private_config_redirects_shell_history() {
        let directory = private_config(&std::env::temp_dir()).unwrap();
        let config = std::fs::read_to_string(directory.path().join(CONFIG_NAME)).unwrap();
        assert!(config.contains("history-path"), "{config}");
    }

    #[test]
    fn test_command_line_is_run_addr_then_address() {
        let commands = commands(&renode());
        assert!(commands.contains(&r#"cpu.semihosting ProgramName "run_addr""#.to_string()));
        assert!(commands.contains(&r#"cpu.semihosting ProgramArguments "1181 ""#.to_string()));
    }

    #[test]
    fn test_budget_is_rendered_as_timespan() {
        assert_eq!(timespan(Duration::from_secs(1)), "00:00:01");
        assert_eq!(timespan(Duration::from_secs(90)), "00:01:30");
        assert_eq!(timespan(Duration::from_secs(3725)), "01:02:05");
    }

    #[test]
    fn test_script_is_included_between_platform_and_elf() {
        let renode = Renode::new(RenodeConfig {
            script: Some(PathBuf::from("/boards/setup.resc")),
            ..config()
        });
        let commands = commands(&renode);
        assert!(position(&commands, "LoadPlatformDescription") < position(&commands, "include @"));
        assert!(position(&commands, "include @") < position(&commands, "LoadELF"));
    }

    #[test]
    fn test_no_script_means_no_include() {
        assert!(!commands(&renode()).iter().any(|c| c.starts_with("include")));
    }

    #[test]
    fn test_renamed_cpu_is_used_throughout() {
        let renode = Renode::new(RenodeConfig {
            cpu: "cpu0".to_string(),
            ..config()
        });
        let commands = commands(&renode);

        // Naming no cpu at all would satisfy the absence on its own.
        let addressed: Vec<&String> = commands
            .iter()
            .filter(|command| command.contains("semihosting") || command.contains("ExecutionMode"))
            .collect();
        assert!(!addressed.is_empty(), "{commands:?}");
        assert!(
            addressed.iter().all(|command| command.starts_with("cpu0")),
            "{addressed:?}"
        );
    }

    #[test]
    fn test_commands_serve_a_stub_on_the_requested_port() {
        assert!(
            debug_commands(3333).contains(&"machine StartGdbServer 3333 false".to_string()),
            "{:?}",
            debug_commands(3333)
        );
    }

    #[test]
    fn test_commands_hold_the_guest_for_the_debugger() {
        assert!(debug_commands(3333).contains(&"cpu ExecutionMode SingleStep".to_string()));
    }

    #[test]
    fn test_stub_is_started_before_the_guest_runs() {
        let commands = debug_commands(3333);
        assert!(position(&commands, "StartGdbServer") < position(&commands, "ExecutionMode"));
        assert!(position(&commands, "ExecutionMode") < position(&commands, "RunFor"));
    }

    #[test]
    fn test_renamed_cpu_holds_the_guest_too() {
        let renode = Renode::new(RenodeConfig {
            cpu: "cpu0".to_string(),
            mode: ExecutionMode::Debug,
            gdb: NonZeroU16::new(3333),
            ..config()
        });
        assert!(commands(&renode).contains(&"cpu0 ExecutionMode SingleStep".to_string()));
    }

    #[test]
    fn test_commands_without_a_port_serve_no_stub() {
        let commands = commands(&renode());
        assert!(!commands.iter().any(|c| c.contains("StartGdbServer")));
        assert!(!commands.iter().any(|c| c.contains("ExecutionMode")));
    }

    #[test]
    fn test_config_directory_that_cannot_be_created_names_the_parent() {
        let error = private_config(Path::new("no-such-parent")).err().unwrap();
        let message = format!("{error:#}");
        assert!(message.contains("no-such-parent"), "{message}");
    }

    #[test]
    fn test_paths_are_slash_separated() {
        assert!(!commands(&renode()).iter().any(|c| c.contains('\\')));
        assert_eq!(
            monitor_path(Path::new(r"C:\boards\board.repl")),
            "C:/boards/board.repl"
        );
    }

    #[test]
    fn test_a_relative_path_is_made_absolute() {
        let resolved = absolute(Path::new("board.repl")).unwrap();
        assert!(resolved.is_absolute(), "{}", resolved.display());
        assert!(resolved.ends_with("board.repl"), "{}", resolved.display());
    }

    #[test]
    fn test_a_path_that_cannot_be_made_absolute_names_it() {
        let error = absolute(Path::new("")).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.starts_with("resolving"), "{message}");
    }

    #[test]
    fn test_verdict_comes_from_monitor_not_process() {
        let observation = observe_transcript(
            &outcome("True\n0x00000086"),
            Duration::from_secs(1),
            WALL_TIMEOUT,
        );
        assert_eq!(
            observation,
            Observation::Aborted {
                output: "True\n0x00000086".to_string()
            }
        );
    }

    #[test]
    fn test_program_still_running_at_budget_is_timeout() {
        let observation =
            observe_transcript(&outcome("False"), Duration::from_secs(1), WALL_TIMEOUT);
        assert_eq!(
            observation,
            Observation::TimedOut {
                after: Duration::from_secs(1),
                output: "False".to_string()
            }
        );
    }

    #[test]
    fn test_unreadable_transcript_is_harness_error() {
        let observation = observe_transcript(
            &outcome("Error E04: Could not resolve type"),
            Duration::from_secs(1),
            WALL_TIMEOUT,
        );
        assert_eq!(
            observation,
            Observation::harness_error(
                "Renode did not report whether the program exited",
                "Error E04: Could not resolve type"
            )
        );
    }

    #[test]
    fn test_renode_ending_without_an_exit_code_is_harness_error() {
        let outcome = ProcessOutcome {
            status: ProcessStatus::Terminated,
            captured: Captured {
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert_eq!(
            observe_transcript(&outcome, Duration::from_secs(1), WALL_TIMEOUT),
            Observation::harness_error("Renode ended without an exit code", "")
        );
    }

    #[test]
    fn test_renode_hanging_is_harness_error() {
        let outcome = ProcessOutcome {
            status: ProcessStatus::TimedOut,
            captured: Captured {
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert_eq!(
            observe_transcript(&outcome, Duration::from_secs(1), WALL_TIMEOUT),
            Observation::harness_error(
                &format!(
                    "Renode did not finish within {}s (--wall-timeout)",
                    WALL_TIMEOUT.as_secs()
                ),
                ""
            )
        );
    }
}
