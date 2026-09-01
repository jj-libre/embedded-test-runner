//! Verdict from the Renode console transcript.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Report {
    Exited(i32),
    Running,
}

impl Report {
    /// Results of the trailing `semihosting Exited` and `ExitCode` commands;
    /// under `--hide-log` those are the only bare values Renode prints.
    pub(crate) fn parse(console: &str) -> Result<Self, ReportError> {
        let mut lines = console.lines().map(str::trim);

        let exited = lines
            .find(|line| *line == "True" || *line == "False")
            .ok_or(ReportError::NoExitedLine)?;

        if exited == "False" {
            return Ok(Report::Running);
        }

        let code = lines
            .find(|line| !line.is_empty())
            .ok_or(ReportError::NoExitCode)?;

        parse_code(code)
            .map(Report::Exited)
            .ok_or_else(|| ReportError::UnparseableExitCode(code.to_string()))
    }
}

/// Where the transcript stopped answering the two questions asked of it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum ReportError {
    #[error("Renode did not report whether the program exited")]
    NoExitedLine,
    #[error("Renode did not report an exit code")]
    NoExitCode,
    #[error("unexpected exit code from Renode: {0:?}")]
    UnparseableExitCode(String),
}

fn parse_code(text: &str) -> Option<i32> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "a high code reads back negative, as an exit code should"
        )]
        Some(hex) => u32::from_str_radix(hex, 16).ok().map(|code| code as i32),
        None => text.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim from a Renode run of the cortex-m example.
    const FINISHED: &str = "\
Renode, version 1.16.1 (1.16.1+20260821gitd9c416e99)

(board) sysbus LoadELF @smoke; cpu.semihosting Exited; cpu.semihosting ExitCode; quit
True
0x00000000
Renode is quitting";

    const RUNNING: &str = "\
(board) sysbus LoadELF @smoke; cpu.semihosting Exited; cpu.semihosting ExitCode; quit
False
Renode is quitting";

    #[test]
    fn test_clean_exit_is_read_back() {
        assert_eq!(Report::parse(FINISHED), Ok(Report::Exited(0)));
    }

    #[test]
    fn test_abort_code_is_read_back() {
        assert_eq!(
            Report::parse(&FINISHED.replace("0x00000000", "0x00000086")),
            Ok(Report::Exited(134))
        );
    }

    #[test]
    fn test_program_still_running_is_reported_without_exit_code() {
        assert_eq!(Report::parse(RUNNING), Ok(Report::Running));
    }

    #[test]
    fn test_carriage_returns_are_tolerated() {
        assert_eq!(
            Report::parse("True\r\n0x00000086\r\n"),
            Ok(Report::Exited(134))
        );
    }

    #[test]
    fn test_decimal_exit_code_is_accepted() {
        assert_eq!(Report::parse("True\n134"), Ok(Report::Exited(134)));
    }

    #[test]
    fn test_transcript_without_results_is_error() {
        assert_eq!(
            Report::parse("Error E04: Could not resolve type: 'CPU.SemihostingHandler'."),
            Err(ReportError::NoExitedLine)
        );
    }

    #[test]
    fn test_missing_exit_code_is_error() {
        assert_eq!(Report::parse("True\n"), Err(ReportError::NoExitCode));
    }

    #[test]
    fn test_unparseable_exit_code_is_error() {
        assert_eq!(
            Report::parse("True\nnot-a-number"),
            Err(ReportError::UnparseableExitCode("not-a-number".to_string()))
        );
    }
}
