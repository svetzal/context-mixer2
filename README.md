# Context Mixer

Three command-line tools that manage the lifecycle of curated agentic context
— agents, skills, and the engineering intents behind them — across AI coding
assistants.

| Tool | Role |
| --- | --- |
| **cmx** | Package manager: installs, versions, updates, and reconciles agents and skills across Claude Code, Copilot, Cursor, Codex, and a dozen other platforms |
| **cmf** | Context Mixer Forge: compiles a profile-specific slice of an intent atlas into an installed agent or skill, and records what it compiled |
| **cmv** | Context Mixer Verify: checks, deterministically, that a repository holds the intents cmf compiled for it |

The **intent atlas** is the externally maintained knowledge base cmf and cmv
work from: the ecosystems it supports and how to sense them, structured intent
records in and around those ecosystems, and the validators beside them. The
reference atlas is [svetzal/guidelines](https://github.com/svetzal/guidelines);
fork it or replace it, and the tooling works the same way.

## Documentation

The book at **<https://vetzal.ca/context-mixer2/>** covers installation,
the user guide, writing agents, skills, and intents, the command references for
all three tools, and the design documents.

- [Quick Start](https://vetzal.ca/context-mixer2/getting-started/quick-start.html)
- [Compiling and Verifying Intents](https://vetzal.ca/context-mixer2/guide/compiling-and-verifying.html)
- [Deterministic Verification of Intents](https://vetzal.ca/context-mixer2/design/deterministic-verification.html)

## Install

```bash
brew install svetzal/tap/cmx
```

installs all three binaries (cmv from release 3.2.0). From a checkout,
`./install.sh` builds and installs them with `cargo install`.

## Repository

- `cmx/`, `cmf/`, `cmv/` — the three binaries
- `cmx-core/` — the embeddable core library, published to crates.io and twinned
  with the TypeScript port in `cmx-core-ts/`
- `intent-atlas/` — the shared reader of an intent atlas that cmf and cmv use
- `book/` — the mdBook source for the documentation site
- `benchmark/` — assembly benchmarks and behavioural exercises for cmf and cmv

`CHARTER.md` states the project's purpose and non-goals; `AGENTS.md` holds the
quality gates, release process, and architecture map for contributors and
coding agents.

## License

MIT.
