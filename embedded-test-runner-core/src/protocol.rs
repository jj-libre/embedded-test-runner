//! Wire contract between a runner and an embedded-test binary.

use crate::Observation;

/// Exit codes the embedded-test protocol defines.
const EXIT_FINISHED: i32 = 0;
const EXIT_ABORTED: i32 = 134;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestMeta {
    pub name: String,
    pub ignored: bool,
    pub should_panic: bool,
    /// Seconds from the descriptor, before a default resolves it.
    pub timeout: Option<u32>,
    pub invocation: Invocation,
}

impl TestMeta {
    /// A test with none of the attributes set, which is what a discovered test
    /// looks like unless the binary said otherwise.
    pub fn new(name: &str, invocation: Invocation) -> Self {
        Self {
            name: name.to_string(),
            ignored: false,
            should_panic: false,
            timeout: None,
            invocation,
        }
    }
}

/// Command line that runs one test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Entry point address, read from the ELF.
    RunAddr(u64),
    /// Test name, resolved by the binary itself.
    Run(String),
}

impl Invocation {
    pub fn command(&self) -> &'static str {
        match self {
            Invocation::RunAddr(_) => "run_addr",
            Invocation::Run(_) => "run",
        }
    }

    pub fn operand(&self) -> String {
        match self {
            Invocation::RunAddr(addr) => addr.to_string(),
            Invocation::Run(name) => name.clone(),
        }
    }
}

impl Observation {
    /// Observation from an exit code the test binary wrote.
    pub fn from_exit_code(code: i32, output: String) -> Self {
        match code {
            EXIT_FINISHED => Observation::Finished { output },
            EXIT_ABORTED => Observation::Aborted { output },
            _ => Observation::harness_error(&unexpected_code_reason(code), &output),
        }
    }
}

fn unexpected_code_reason(code: i32) -> String {
    format!(
        "exit code {code} is not one the embedded-test protocol produces \
         ({EXIT_FINISHED} or {EXIT_ABORTED}), so the emulator most likely failed \
         before the test ran"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_dispatch_is_decimal() {
        let invocation = Invocation::RunAddr(0x1000);
        assert_eq!(invocation.command(), "run_addr");
        assert_eq!(invocation.operand(), "4096");
    }

    #[test]
    fn test_name_dispatch_keeps_the_module_path() {
        let invocation = Invocation::Run("tests::it_passes".to_string());
        assert_eq!(invocation.command(), "run");
        assert_eq!(invocation.operand(), "tests::it_passes");
    }

    #[test]
    fn test_clean_exit_is_finished() {
        assert_eq!(
            Observation::from_exit_code(EXIT_FINISHED, "out".to_string()),
            Observation::Finished {
                output: "out".to_string()
            }
        );
    }

    #[test]
    fn test_abort_code_is_aborted() {
        assert_eq!(
            Observation::from_exit_code(EXIT_ABORTED, "out".to_string()),
            Observation::Aborted {
                output: "out".to_string()
            }
        );
    }

    #[test]
    fn test_other_code_is_harness_error() {
        assert_eq!(
            Observation::from_exit_code(1, String::new()),
            Observation::harness_error(&unexpected_code_reason(1), "")
        );
    }

    #[test]
    fn test_harness_error_names_the_code_and_the_expected_ones() {
        let observation = Observation::from_exit_code(37, String::new());
        assert!(
            matches!(&observation, Observation::HarnessError { reason, .. }
                if reason.contains("37") && reason.contains("134")),
            "{observation:?}"
        );
    }
}
