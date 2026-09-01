//! The venues, the examples under each, and the files that have to name them.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use clap::ValueEnum;

/// Venue selector for `--venue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Venue {
    Host,
    Qemu,
    Renode,
}

impl Venue {
    /// Directory under `templates/` holding what every example of the venue
    /// shares.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Venue::Host => "host",
            Venue::Qemu => "qemu",
            Venue::Renode => "renode",
        }
    }

    /// Crate the venue belongs to, and the directory its examples live under.
    pub(crate) fn runner(self) -> &'static str {
        match self {
            Venue::Host => "embedded-test-host-runner",
            Venue::Qemu => "embedded-test-qemu-runner",
            Venue::Renode => "embedded-test-renode-runner",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Example {
    /// Directory, relative to the workspace root.
    pub(crate) dir: String,
    pub(crate) venue: Venue,
}

/// Every `<runner>/examples/<name>` directory holding a manifest.
pub(crate) fn discover_examples(root: &Path) -> Result<Vec<Example>> {
    let mut examples = Vec::new();

    for venue in Venue::value_variants() {
        let parent = root.join(venue.runner()).join("examples");
        let entries =
            fs::read_dir(&parent).with_context(|| format!("reading {}", parent.display()))?;

        for entry in entries {
            let path = entry
                .with_context(|| format!("reading {}", parent.display()))?
                .path();
            if !path.join("Cargo.toml").is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            examples.push(Example {
                dir: format!("{}/examples/{name}", venue.runner()),
                venue: *venue,
            });
        }
    }

    examples.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(examples)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Files that have to name an example for it to run in CI or open in the
    /// editor. Nothing can generate them: a workflow matrix is static YAML.
    const REGISTRIES: [&str; 2] = [
        ".github/workflows/ci.yml",
        "embedded-test-runner.code-workspace",
    ];

    fn unregistered(root: &Path) -> Vec<String> {
        let examples = discover_examples(root).unwrap();

        REGISTRIES
            .iter()
            .flat_map(|registry| {
                let contents = fs::read_to_string(root.join(registry)).unwrap();
                missing_from(registry, &contents, &examples)
            })
            .collect()
    }

    fn missing_from(registry: &str, contents: &str, examples: &[Example]) -> Vec<String> {
        examples
            .iter()
            .filter(|example| !names(contents, &example.dir))
            .map(|example| format!("{registry} names no {}", example.dir))
            .collect()
    }

    /// `path` as a whole entry rather than the prefix of a longer one, so a
    /// misspelled `rv64-x` does not answer for `rv64`.
    fn names(contents: &str, path: &str) -> bool {
        contents.match_indices(path).any(|(at, _)| {
            contents[at + path.len()..]
                .chars()
                .next()
                .is_none_or(|next| !matches!(next, 'a'..='z' | '0'..='9' | '-' | '_' | '/'))
        })
    }

    /// An example that runs locally but sits in no CI matrix fails here.
    #[test]
    fn test_every_example_is_registered() {
        let missing = unregistered(&crate::examples::repo_root());
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn test_an_example_no_registry_names_is_reported() {
        let examples = discover_examples(&crate::examples::repo_root()).unwrap();
        let missing = missing_from("ci.yml", "names nothing", &examples);
        assert_eq!(missing.len(), examples.len());
    }

    #[test]
    fn test_a_longer_path_does_not_count_as_naming_the_example() {
        let dir = "embedded-test-qemu-runner/examples/rv64";
        assert!(names(&format!("- example: {dir} "), dir));
        assert!(!names(&format!("- example: {dir}-x "), dir));
    }

    /// Every example is `<venue>-<directory>-example`, so a package name says
    /// which venue runs it and matches the directory it sits in.
    #[test]
    fn test_every_example_package_is_named_for_its_venue_and_directory() {
        let root = crate::examples::repo_root();

        for example in discover_examples(&root).unwrap() {
            let directory = example.dir.rsplit('/').next().unwrap();
            let expected = format!("{}-{directory}-example", example.venue.name());
            let manifest = fs::read_to_string(root.join(&example.dir).join("Cargo.toml")).unwrap();

            assert!(
                manifest.contains(&format!("name = \"{expected}\"")),
                "{} is not {expected}",
                example.dir
            );
        }
    }

    #[test]
    fn test_each_venue_names_its_runner_crate() {
        for venue in Venue::value_variants() {
            assert!(crate::examples::repo_root().join(venue.runner()).is_dir());
        }
    }
}
