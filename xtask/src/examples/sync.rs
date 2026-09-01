//! Keeps the files every example of a venue shares identical to the venue's
//! template.
//!
//! Only files that carry nothing per-target live under `templates/`; anything
//! per-target is written by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args;
use walkdir::WalkDir;

use super::catalog::{Venue, discover_examples};

#[derive(Args, Debug)]
pub(crate) struct SyncExamples {
    /// Report the files that have drifted instead of writing them.
    #[arg(long)]
    check: bool,
}

impl SyncExamples {
    pub(crate) fn run(&self, root: &Path) -> ExitCode {
        match self.sync(root) {
            Ok(drifted) if drifted.is_empty() => ExitCode::SUCCESS,
            Ok(drifted) => {
                for path in &drifted {
                    eprintln!("  {path}");
                }
                eprintln!("\n{} file(s) differ from their template", drifted.len());
                eprintln!("run `cargo xtask examples sync` to write them");
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(2)
            }
        }
    }

    /// The files that differed. Under `--check` they are left alone, otherwise
    /// they are written and reported as nothing.
    fn sync(&self, root: &Path) -> Result<Vec<String>> {
        let mut drifted = Vec::new();

        for example in discover_examples(root)? {
            for file in templates(root, example.venue)? {
                let target = root.join(&example.dir).join(&file.relative);
                if reads_the_same(&target, &file.contents)? {
                    continue;
                }
                if self.check {
                    drifted.push(format!("{}/{}", example.dir, file.relative.display()));
                } else {
                    write(&target, &file.contents)?;
                    println!("{}/{}", example.dir, file.relative.display());
                }
            }
        }

        Ok(drifted)
    }
}

struct TemplateFile {
    /// Path inside an example directory.
    relative: PathBuf,
    contents: Vec<u8>,
}

fn templates(root: &Path, venue: Venue) -> Result<Vec<TemplateFile>> {
    let dir = template_dir(root, venue);
    let mut files = Vec::new();

    for entry in WalkDir::new(&dir).sort_by_file_name() {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&dir)
            .expect("walked below the template directory")
            .to_path_buf();
        let contents = fs::read(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        files.push(TemplateFile { relative, contents });
    }

    Ok(files)
}

fn template_dir(root: &Path, venue: Venue) -> PathBuf {
    root.join("templates").join(venue.name())
}

fn reads_the_same(path: &Path, contents: &[u8]) -> Result<bool> {
    match fs::read(path) {
        Ok(found) => Ok(found == contents),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every venue has a template directory, and every example of a venue is
    /// already in step with it. A new example that skipped `sync` fails here.
    #[test]
    fn test_every_example_matches_its_template() {
        let root = crate::examples::repo_root();
        let drifted = SyncExamples { check: true }.sync(&root).unwrap();
        assert!(drifted.is_empty(), "{drifted:?}");
    }

    #[test]
    fn test_each_venue_has_a_template() {
        let root = crate::examples::repo_root();
        for venue in [Venue::Host, Venue::Qemu, Venue::Renode] {
            let dir = template_dir(&root, venue);
            assert!(dir.is_dir(), "{}", dir.display());
            assert!(!templates(&root, venue).unwrap().is_empty(), "{venue:?}");
        }
    }

    #[test]
    fn test_a_template_is_read_relative_to_the_example() {
        let root = crate::examples::repo_root();
        let relatives: Vec<PathBuf> = templates(&root, Venue::Qemu)
            .unwrap()
            .into_iter()
            .map(|file| file.relative)
            .collect();
        assert!(
            relatives.contains(&PathBuf::from(".gitignore")),
            "{relatives:?}"
        );
        assert!(
            relatives.contains(&Path::new(".vscode").join("settings.json")),
            "{relatives:?}"
        );
    }

    #[test]
    fn test_a_missing_file_counts_as_drift() {
        let root = crate::examples::repo_root();
        let absent = root.join("templates").join("no-such-file");
        assert!(!reads_the_same(&absent, b"anything").unwrap());
    }

    #[test]
    fn test_the_venue_decides_the_template_directory() {
        let root = Path::new("root");
        assert_eq!(
            template_dir(root, Venue::Renode),
            Path::new("root").join("templates").join("renode")
        );
    }
}
