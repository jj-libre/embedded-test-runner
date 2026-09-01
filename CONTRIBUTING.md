# Contributing

## Conventions

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
Releases are cut from them, so a message that does not follow the convention
leaves its change out of the changelog and out of the version.

Changelogs follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Each published crate keeps its own; both are written by the release
automation, so a pull request does not edit them.

## Commands

```sh
cargo fmt --all --check     # formatting
cargo clippy --all-targets  # lints, denied in CI
cargo test --workspace      # unit and integration tests
cargo xtask examples test   # every example, through its runner
```
