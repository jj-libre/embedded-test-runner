//! Which of the discovered examples the flags asked for.

use anyhow::{Result, bail};
use clap::Args;

use super::catalog::{Example, Venue};

#[derive(Args, Debug)]
pub(crate) struct Selection {
    /// Restrict the run to these venues.
    #[arg(long = "venue", value_enum)]
    venues: Vec<Venue>,

    /// Restrict the run to these example directories.
    #[arg(long = "example")]
    examples: Vec<String>,
}

impl Selection {
    /// Refuses anything that would leave the run with nothing to do: a
    /// misspelled `--example` would otherwise check no example at all and
    /// report success for it.
    pub(crate) fn resolve<'a>(&self, discovered: &'a [Example]) -> Result<Vec<&'a Example>> {
        for name in &self.examples {
            if !discovered.iter().any(|example| example.dir == *name) {
                bail!("no such example: {name}");
            }
        }

        let selected: Vec<&Example> = discovered
            .iter()
            .filter(|example| self.selects(example))
            .collect();

        if selected.is_empty() {
            bail!("the selection matches no example");
        }
        Ok(selected)
    }

    fn selects(&self, example: &Example) -> bool {
        let in_venue = self.venues.is_empty() || self.venues.contains(&example.venue);
        let in_directory = self.examples.is_empty() || self.examples.contains(&example.dir);
        in_venue && in_directory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discovered() -> Vec<Example> {
        vec![
            Example {
                dir: "embedded-test-host-runner/examples/std".to_string(),
                venue: Venue::Host,
            },
            Example {
                dir: "embedded-test-qemu-runner/examples/cortex-m".to_string(),
                venue: Venue::Qemu,
            },
            Example {
                dir: "embedded-test-qemu-runner/examples/rv32".to_string(),
                venue: Venue::Qemu,
            },
        ]
    }

    fn selection(venues: &[Venue], examples: &[&str]) -> Selection {
        Selection {
            venues: venues.to_vec(),
            examples: examples.iter().map(|name| (*name).to_string()).collect(),
        }
    }

    fn dirs(selected: &[&Example]) -> Vec<String> {
        selected.iter().map(|example| example.dir.clone()).collect()
    }

    #[test]
    fn test_no_flags_selects_every_example() {
        let discovered = discovered();
        let selected = selection(&[], &[]).resolve(&discovered).unwrap();
        assert_eq!(selected.len(), discovered.len());
    }

    #[test]
    fn test_venue_narrows_the_selection() {
        let discovered = discovered();
        let selected = selection(&[Venue::Qemu], &[]).resolve(&discovered).unwrap();
        assert_eq!(
            dirs(&selected),
            [
                "embedded-test-qemu-runner/examples/cortex-m",
                "embedded-test-qemu-runner/examples/rv32"
            ]
        );
    }

    #[test]
    fn test_example_narrows_the_selection() {
        let discovered = discovered();
        let selection = selection(&[], &["embedded-test-qemu-runner/examples/rv32"]);
        let selected = selection.resolve(&discovered).unwrap();
        assert_eq!(dirs(&selected), ["embedded-test-qemu-runner/examples/rv32"]);
    }

    #[test]
    fn test_misspelled_example_is_refused_by_name() {
        let discovered = discovered();
        let selection = selection(
            &[],
            &[
                "embedded-test-host-runner/examples/std",
                "embedded-test-qemu-runner/examples/rv322",
            ],
        );
        let error = selection.resolve(&discovered).unwrap_err().to_string();
        assert!(error.contains("rv322"), "{error}");
    }

    #[test]
    fn test_crossed_flags_select_nothing_and_are_refused() {
        let discovered = discovered();
        let selection = selection(
            &[Venue::Renode],
            &["embedded-test-host-runner/examples/std"],
        );
        assert!(selection.resolve(&discovered).is_err());
    }
}
