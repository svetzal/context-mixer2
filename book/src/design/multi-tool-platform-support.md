# Multi-Tool Platform Support

> **Status:** Living design document. This is published in the open on purpose —
> it records *why* cmx supports the platforms it does, the way it does, so users
> and contributors can see our design goals and trade-offs rather than having to
> reverse-engineer them. Last substantive update: 2026-05.

## Why this document exists

cmx installs **agents** (markdown system-prompt files) and **skills**
(directories containing `SKILL.md` + supporting files) into the per-tool
locations that AI coding assistants read. As we expanded beyond Claude Code, we
needed to know — concretely, per tool — *where* artifacts go, *what format* each
tool expects, and *which of cmx's artifact kinds each tool can even consume*.

This note records that research (so we don't re-derive it), the structural
patterns it revealed, the abstraction options we weighed, and the scope
decisions that produced the shipped design.

## TL;DR — the three findings that matter

1. **`.agents/skills/` is a real cross-tool standard.** The
   [agentskills.io](https://agentskills.io) `SKILL.md`-in-a-directory format,
   read from project `.agents/skills/` and user `~/.agents/skills/`, is consumed
   natively by opencode, Codex, Pi, Crush, Amp, Zed, and OpenHands. It is byte
   compatible with cmx's existing skill model. **One skill install serves the
   whole cohort at once** — this is cmx's highest-leverage surface.

2. **"Agent" is the un-portable dimension, and often isn't a file at all.** A
   portable markdown agent only works for Claude and opencode. Codex needs TOML
   (we transform it). Cline/Crush/Amp/Goose/Zed/Hermes have *no file-droppable
   agent* — they use rules, recipes, TS plugins, settings-embedded tool-gating
   profiles, or runtime delegation. So a cmx "agent" maps to different things per
   tool, or to nothing.

3. **A second install *mechanism* exists that cmx deliberately does not model:
   structured config-merge.** Skills/commands are *file-drop* (copy → checksum →
   lockfile — cmx's model). MCP servers, Zed profiles, Goose extensions, and
   Crush/Continue config are *merge a key into a shared JSON/YAML/TOML file* — no
   per-artifact file to checksum, requires deep-merge without clobbering
   hand-edits. **Decision: cmx stays file-drop only** (see
   [Scope decisions](#scope-decisions)).

## Capability matrix

Legend: ✓✓ native/first-class · ✓ supported · ✗ none/unsupported.
"Reads `.agents/skills`?" = consumes the shared cross-tool skill location.

| Tool | Skill = `SKILL.md` dir? | Reads `.agents/skills`? | Agent-as-file? | Commands | MCP config | Plugin/registry | cmx status |
|---|---|---|---|---|---|---|---|
| Claude Code | ✓✓ | (uses `.claude/skills`) | ✓ md | ✓ `.claude/commands` | ✓ | `.claude-plugin/` marketplace | **implemented** |
| GitHub Copilot | ✓ | ✗ | ✓ md | — | — | (cmf target) | implemented |
| Cursor | ✓ | ✗ (`.cursor/skills`) | ✓ md | — | ✓ | (cmf target) | implemented |
| Windsurf | ✓ | ✗ | ✓ md | — | ✓ | (cmf target) | implemented |
| Gemini CLI | ✓ | ✗ | ✓ md | — | ✓ | (cmf target) | implemented |
| opencode | ✓✓ | ✓ | ✓ md (`.opencode/agent`) | ✓ | ✓ | ✗ (npm/JS) | **implemented** |
| Codex CLI | ✓✓ | ✓ | ✓ **TOML** (transform) | ✓ prompts (deprecated) | ✓ | ✗ | **implemented** |
| Pi | ✓✓ | ✓ | ✗ | ✗ | ✓ | tap (GitHub repos) | **implemented** |
| Crush | ✓✓ | ✓ | ✗ (internal only) | skills w/ flag | ✓ `crush.json` | ✗ | **implemented** |
| Amp | ✓✓ | ✓ (user: `~/.config/agents/skills`) | ✗ (TS plugins) | ✓ `.agents/commands` | ✓ settings.json | ✗ (TS plugins) | **implemented** |
| Zed | ✓✓ | ✓ (flat only) | ✗ (settings.json profiles) | skills via `/name` | ✓ `context_servers` | `extension.toml` (not agents/skills) | **implemented** |
| OpenHands | ✓✓ | ✓ | agents *are* triggered skills | ✗ distinct | ✓ | ✓ extensions registry (npm) | **implemented** |
| Hermes | ✓✓ | ✓ (opt-in `external_dirs`; user: `~/.hermes/skills`) | ✗ (SOUL.md + runtime delegate) | built-in slash only | ✓ `config.yaml` | tap + consumes `claude-marketplace` | **implemented** |
| Aider | ✗ | ✗ | ✗ (fixed modes; `--read` a md file) | ✗ (built-in only) | ✗ native | ✗ | researched, not impl |
| Cline | ✓ (`.claude/skills`, `~/.cline/skills`) | ✗ | ✗ (Plan/Act + rules) | ✓ workflows `.md` | ✓✓ + marketplace | ✗ (MCP marketplace only) | researched, not impl |
| Continue | ✗ (typed "blocks") | ✗ | YAML assistants / CLI md+frontmatter | ✓ prompts | ✓ | ✓✓ **Continue Hub** (`uses: owner/name@ver`) | researched, not impl |
| Goose | ✗ (use MCP ext) | ✗ | recipes (YAML) | recipe-backed | ✓✓ (extensions *are* MCP) | `goose://` deeplink registry | researched, not impl |

## Install path reference (implemented tools)

Project paths are relative to the repo root; user paths to `$HOME`.

| Tool | Project agents | User agents | Project skills | User skills | Lock slug |
|---|---|---|---|---|---|
| claude | `.claude/agents/` | `~/.claude/agents/` | `.claude/skills/` | `~/.claude/skills/` | *(none)* |
| copilot | `.github/agents/` | `~/.copilot/agents/` | `.github/skills/` | `~/.copilot/skills/` | `copilot` |
| cursor | `.cursor/agents/` | `~/.cursor/agents/` | `.cursor/skills/` | `~/.cursor/skills/` | `cursor` |
| windsurf | `.windsurf/agents/` | `~/.codeium/windsurf/agents/` | `.windsurf/skills/` | `~/.codeium/windsurf/skills/` | `windsurf` |
| gemini | `.gemini/agents/` | `~/.gemini/agents/` | `.gemini/skills/` | `~/.gemini/skills/` | `gemini` |
| opencode | `.opencode/agent/` | `~/.config/opencode/agent/` | `.agents/skills/` | `~/.agents/skills/` | `opencode` |
| codex | `.codex/agents/` (TOML) | `~/.codex/agents/` (TOML) | `.agents/skills/` | `~/.agents/skills/` | `codex` |
| pi | — | — | `.agents/skills/` | `~/.agents/skills/` | `pi` |
| crush | — | — | `.agents/skills/` | `~/.agents/skills/` | `crush` |
| amp | — | — | `.agents/skills/` | `~/.config/agents/skills/` | `amp` |
| zed | — | — | `.agents/skills/` | `~/.agents/skills/` | `zed` |
| openhands | — | — | `.agents/skills/` | `~/.agents/skills/` | `openhands` |
| hermes | — | — | `.agents/skills/` | `~/.hermes/skills/` | `hermes` |

Notes:

- **Codex agents are transformed**, not copied: cmx parses the source markdown
  frontmatter + body and emits a Codex subagent TOML
  (`name`, `description`, `developer_instructions`, optional `model`). See
  `cmx/src/codex_agent.rs`.
- **Amp** resolves *user-scoped* skills under XDG (`~/.config/agents/skills/`),
  not `~/.agents/skills/`. Project skills use the shared path.
- **Hermes** is global-centric: its auto-read source of truth is
  `~/.hermes/skills/`. It reads `.agents/skills/` only if you add it to
  `skills.external_dirs` in `~/.hermes/config.yaml`.
- **Shared-directory caveat:** because `.agents/skills/` is shared, uninstalling
  a skill under one of these platforms removes the directory for *all* tools that
  read it (each platform still keeps its own lockfile).

## The abstraction we considered (and why the lean version won)

The richest model on the table was an **`Artifact × Target` capability
descriptor** with pluggable install strategies:

```rust
enum InstallStrategy {
    FileDrop    { dir, transform: Option<Transform> }, // copy/transform a file or dir
    ConfigMerge { file, pointer, entry_schema },        // merge a key into shared JSON/YAML/TOML
    Reference   { registry, slug_fmt },                 // tool's own package manager (e.g. Continue Hub)
    Unsupported { reason },
}
// Platform::target(kind, scope) -> Target { strategy, .. }
```

…plus growing `ArtifactKind` from `{Agent, Skill}` toward
`{Agent, Skill, Command, McpServer}`.

**We did not build this.** Once the [scope decision](#scope-decisions) ruled out
config-merge, the only live strategies were `FileDrop` and `Unsupported` — which
the existing design already models cleanly via:

- `Platform::install_subpath(kind, scope) -> PathBuf` — per-(platform, kind,
  scope) file-drop location,
- `Platform::supports(kind) -> bool` — the `Unsupported` gate,
- `Platform::agent_extension()` + `Platform::transforms_agent_to_toml()` — the
  one `Transform` we need (Codex).

Introducing the full enum would have been speculative abstraction (YAGNI) for a
single strategy. The capability descriptor is the right shape to revisit *if and
when* config-merge is in scope.

## Scope decisions

| Decision | Choice | Rationale |
|---|---|---|
| Unsupported artifact kinds | **Skip with a clear error** | Each platform declares supported kinds; e.g. `cmx agent install --platform pi` fails loudly rather than writing where the tool never reads. |
| cmf manifest generation for new tools | **Don't generate** | None of opencode/codex/pi/crush/amp/zed/openhands/hermes has a Claude-style plugin/marketplace manifest; generating `.X-plugin/` dirs would be dead files. |
| Codex agents | **Transform md → TOML now** | Codex subagents are TOML; a verbatim copy wouldn't work. Hand-rolled emitter, no new dependency. |
| Codex / cohort skill location | **Shared `.agents/skills/`** | Matches official docs and the cross-tool standard; serves multiple tools at once. |
| **Config-merge / MCP servers** | **Out of scope (file-drop only)** | MCP/Zed-profile/Goose/Crush config is a *merge into a shared file*, not a file install. It's a different engine and brushes the charter non-goal "managing LLM API keys" (MCP entries can carry secrets). Revisit deliberately if pursued. |
| Continue Hub (`Reference`) | **Out of scope** | Continue has its own package manager; cmx would be redundant/parallel to it. |

## Phasing

- **Phase 1 (done):** opencode, codex, pi (commit `308523f`); then crush, amp,
  zed, openhands, hermes (commit `71021dd`). All file-drop.
- **Phase 2 (not started):** a `Command` artifact kind — portable markdown
  slash-commands (Amp `.agents/commands/`, Cline workflows, codex/Continue
  prompts). File-droppable and fairly portable.
- **Phase 3 (deferred, needs charter call):** a `ConfigMerge` strategy + an
  `McpServer` artifact kind. Highest value for "manage the complexity," but a
  different engine and a scope expansion.

## Known future snags

- **Cline breaks the shared-path pattern.** It reads skills from `.claude/skills`
  / `~/.cline/skills`, **not** `.agents/skills`. Adding Cline means a tool-specific
  skill target (and the `.agents`-cohort assumption no longer being universal).
- **Editor-embedded config is brittle to locate.** Cline's MCP settings live
  under VS Code `globalStorage/<publisher>/…`, which varies per editor fork
  (Code, Insiders, VSCodium, Cursor, Windsurf). Any future config-merge work must
  detect the host editor.
- **Version-dialect drift.** OpenHands has three coexisting skill layouts
  (`.agents/skills`, deprecated `.openhands/skills`, deprecated
  `.openhands/microagents`) and two frontmatter dialects. We target only the
  current standard.

## Per-tool research summaries & sources

Each tool was researched against an identical capability template (identity,
config dirs, agents, skills, commands, instruction files, MCP, plugin manifest,
distinctive strengths, abstraction friction).

### Implemented

- **opencode** — `~/.config/opencode/`, `.opencode/`. Markdown agents in
  `agent/` (loader globs `{agent,agents}`); skills `.opencode/skills` +
  `.agents/skills` + `.claude/skills`. AGENTS.md (falls back to CLAUDE.md). No
  plugin manifest (npm/JS modules).
  Sources: <https://opencode.ai/docs/agents/>, <https://opencode.ai/docs/skills/>,
  <https://opencode.ai/docs/rules/>, <https://opencode.ai/docs/plugins/>.
- **Codex CLI** — `~/.codex/`, `.codex/`. **TOML** subagents in `agents/`
  (`name`/`description`/`developer_instructions`); skills officially at
  `.agents/skills/`. AGENTS.md (+`AGENT.md`/`CLAUDE.md` fallback). No plugin
  manifest.
  Sources: <https://developers.openai.com/codex/subagents>,
  <https://developers.openai.com/codex/skills>,
  <https://developers.openai.com/codex/config-basic>.
- **Pi** (pi.dev) — `~/.pi/agent/`, `.pi/`. Skills = `SKILL.md` dirs in
  `~/.pi/agent/skills/`, `~/.agents/skills/`, `.pi/skills/`, `.agents/skills/`.
  No agents. AGENTS.md. npm packages.
  Sources: <https://pi.dev>,
  <https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md>,
  <https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md>.
- **Crush** (charmbracelet/crush) — `~/.config/crush/crush.json`,
  `crush.json`/`.crush.json`. Skills (agentskills.io) at `.agents/skills`,
  `.crush/skills`, `.claude/skills`, `~/.agents/skills`. No user-definable agents
  (internal `coder`/`task` only). MCP in `crush.json`. Reads
  AGENTS.md/CLAUDE.md/CRUSH.md. Native LSP.
  Sources: <https://github.com/charmbracelet/crush>,
  <https://agentskills.io>, <https://deepwiki.com/charmbracelet/crush/2.2-configuration>.
- **Amp** (Sourcegraph) — `~/.config/amp/`, `.amp/`. Skills at `.agents/skills`,
  `.claude/skills`, `~/.config/agents/skills`, `~/.config/amp/skills`,
  `~/.claude/skills`. No file agents (subagents via TS plugins;
  `amp.experimental.createAgent`). Commands `.agents/commands/*.md`. AGENTS.md
  (+`AGENT.md`/`CLAUDE.md`). MCP `amp.mcpServers` in settings.json. **Toolboxes
  deprecated → TS plugins.**
  Sources: <https://ampcode.com/manual>,
  <https://ampcode.com/news/slashing-custom-commands>, <https://ampcode.com/news/AGENTS.md>.
- **OpenHands** — `~/.openhands/` + `~/.agents/skills/`; project `.agents/skills/`
  (→ legacy `.openhands/microagents/`). Skills = AgentSkills standard; "agents"
  are keyword-triggered knowledge skills (`triggers:` frontmatter). AGENTS.md
  (+CLAUDE.md/GEMINI.md). MCP via `config.toml`/`~/.openhands/mcp.json`. Official
  extensions registry (npm `@openhands/extensions`).
  Sources: <https://docs.openhands.dev/overview/skills>,
  <https://github.com/OpenHands/extensions>, <https://agentskills.io/specification>.
- **Zed** — `~/.config/zed/` (settings.json, AGENTS.md); project `.zed/`,
  `.rules`, `<repo>/.agents/skills/`. Skills native at `~/.agents/skills/`
  (**flat only**). "Agent profiles" are tool-gating JSON in `settings.json`
  (no system prompt). MCP = `context_servers` in settings.json. Reads
  `.rules`/`AGENTS.md`/`CLAUDE.md` (+6, precedence-ordered). `extension.toml`
  registry covers MCP/themes/langs, **not** agents/skills.
  Sources: <https://zed.dev/docs/ai/skills>, <https://zed.dev/docs/ai/rules>,
  <https://zed.dev/docs/ai/mcp>.
- **Hermes** (NousResearch/hermes-agent) — `~/.hermes/` (config.yaml, SOUL.md,
  skills/). Skills = agentskills.io at `~/.hermes/skills/`; reads `~/.agents/skills`
  only via opt-in `skills.external_dirs`. No file agents (single global SOUL.md +
  runtime `delegate_task`). MCP `mcp_servers` in config.yaml. Context files
  `.hermes.md`/`HERMES.md` > AGENTS.md > CLAUDE.md > `.cursorrules`. "Tap" registry
  (GitHub repos of SKILL.md) **and consumes Claude-compatible marketplace
  manifests**.
  Sources: <https://hermes-agent.nousresearch.com/docs/user-guide/features/skills>,
  <https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp>,
  <https://github.com/NousResearch/hermes-agent>.

### Researched, not implemented

- **Aider** — `.aider.conf.yml` dotfile (home + repo + cwd). No agents (4 fixed
  modes), no skills, no native MCP, no plugin system. Instruction files only via
  explicit `--read`/`read:` (no auto-load). Best cmx fit: install a markdown
  convention file + register it in `.aider.conf.yml`; skills have no home.
  Sources: <https://aider.chat/docs/config/aider_conf.html>,
  <https://aider.chat/docs/usage/conventions.html>.
- **Cline** — VS Code/JetBrains/CLI. Skills = Anthropic-compatible at
  `.claude/skills`, `~/.cline/skills` (**not** `.agents/skills`). Agents = built-in
  Plan/Act modes + `.clinerules` (file or dir). Workflows = slash commands.
  MCP-first with a marketplace; MCP config under VS Code `globalStorage`.
  Sources: <https://docs.cline.bot/customization/skills>,
  <https://docs.cline.bot/customization/cline-rules>,
  <https://docs.cline.bot/mcp/adding-and-configuring-servers>.
- **Continue** — `~/.continue/`, `.continue/`. No `SKILL.md`; compositional
  "blocks" (rules/prompts/models/context/docs/mcp) + "assistants" (YAML or CLI
  md+frontmatter). **Continue Hub** registry; install by reference
  `uses: owner/name@version` — its own package manager. Best cmx fit: parallel
  local installer at best; Continue manages itself.
  Sources: <https://docs.continue.dev/reference>,
  <https://docs.continue.dev/customize/deep-dives/rules>,
  <https://docs.continue.dev/guides/understanding-configs>.
- **Goose** (Block / AAIF) — `~/.config/goose/`, `.goose/`. No skills; capabilities
  = extensions (which *are* MCP servers) configured in `config.yaml`. "Recipes"
  (YAML) are the shareable persona/workflow unit. AGENTS.md + `.goosehints`.
  Install via `goose://` deeplinks / config merge. Best cmx fit: map cmx agents →
  recipes; skills unsupported.
  Sources: <https://goose-docs.ai/docs/guides/recipes/recipe-reference/>,
  <https://goose-docs.ai/docs/guides/config-files/>,
  <https://goose-docs.ai/docs/getting-started/using-extensions/>.

## Source code map

- `cmx/src/platform.rs` — the `Platform` enum and all per-tool knowledge
  (`install_subpath`, `supports`, `agent_extension`, `transforms_agent_to_toml`,
  `slug`, `manifest_dir`, `targets`).
- `cmx/src/codex_agent.rs` — markdown → Codex TOML transform (pure functional
  core).
- `cmx/src/paths.rs` — `ConfigPaths::install_dir`, `installed_artifact_path`
  (platform-aware filename), `ensure_supports`.
- `book/src/reference/platforms.md` — user-facing platform path & lockfile tables.
