# cmx — Context Mixer

A package manager for curated agentic context (agents and skills), written in Rust.

## Reference repositories

- **guidelines**: `~/Work/Projects/Personal/guidelines` — the reference source repository used for local testing of cmx features (artifact scanning, install, versioning, upgrades).

## Architecture

- `src/cli.rs` — clap CLI definition
- `src/config.rs` — config dir paths, sources.json read/write
- `src/source.rs` — `cmx source` subcommands
- `src/install.rs` — `cmx agent install` / `cmx skill install`
- `src/list.rs` — `cmx agent list` / `cmx skill list` / `cmx list`
- `src/scan.rs` — artifact detection (walks source repos, matches agents/skills by frontmatter)
- `src/types.rs` — shared types (SourceEntry, Artifact, ArtifactKind)

## Spec

See `SPEC.md` for the full design spec.
