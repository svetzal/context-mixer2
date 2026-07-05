# Changelog

All notable changes to cmx and cmf will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `--json` now covers every read-only data-reporting `cmx` command, not just `doctor` and `init`: `list`, kind-scoped `agent|skill list`, `outdated`, `search`, `info`, `source list`, `source browse`, `set list`, `set show`, `config show`, and `home path` all emit machine-readable JSON to stdout while preserving their existing human output as the default.
- `cmx completions <shell>` now generates shell completion scripts for `bash`, `zsh`, `fish`, `elvish`, and `powershell`, writing the script to stdout so it can be redirected into the target shell's completion directory.

### Changed

- The global `--platform` help is now compact on subcommands and keeps the full annotated platform roster only in top-level `cmx --help`, so command-specific help is no longer drowned by the repeated platform enum block.
- `cmx {skill,agent} promote`, `cmx skill sync`, `cmx set activate`, and `cmx set deactivate` now show a concrete reconciliation plan by default and only mutate when re-run with `--apply`. The plan names the source and target paths and ends with `Re-run with --apply to make these changes.` `cmx set delete --purge` now follows the same preview/apply flow for its deactivation step.
- `cmx {skill,agent} install --force` and `cmx {skill,agent} update --force` still execute immediately, but now print the exact local file paths whose edits are being discarded before the normal success output.
- `cmx {skill,agent} promote <name>` now accepts `--from <platform>` to choose which installed copy wins, matching `sync`. The global `--platform` selector still works as a fallback, but `promote` now documents and prefers `--from` for winner selection.
- `cmx set create <name>` now uses `--from-plugin <source>:<plugin>` for marketplace-plugin seeding, replacing the overloaded `--from`.
- `cmx {skill,agent} adopt --all` now uses `--from-dir <dir>` for install-directory filtering, replacing the overloaded `--from`.
- `cmx list`, `cmx {skill,agent} list`, and `cmx outdated` now use explicit human table labels: the `Tools` header is now `Platforms`, version gaps read as `unversioned` or `source missing` instead of `-`, list rows include a named `Status` column, and `cmx outdated` ends with an update hint while still printing `Everything is up to date.` on an empty result.
- JSON for `cmx list`, `cmx {skill,agent} list`, `cmx outdated`, and `cmx search` now emits semantic machine values instead of human placeholders: absent versions are `null` instead of `"-"`, list rows use `platforms` instead of `tools`, and status fields are enum strings such as `outdated`, `unversioned`, or `source_missing`.
- `install.sh` now refreshes zsh completions at `~/.zfunc/_cmx` when `~/.zfunc/` exists, and otherwise prints a manual `cmx completions zsh` hint without failing the install.

### Deprecated

- `cmx skill sync --dry-run`, `cmx set activate --dry-run`, and `cmx set deactivate --dry-run` still work for one release as hidden aliases, but now print a one-line stderr warning steering to the default preview plus `--apply`.
- `cmx set create --from <source>:<plugin>` and `cmx {skill,agent} adopt --all --from <dir>` still work for one release as hidden aliases, but now print a one-line stderr warning steering to `--from-plugin` and `--from-dir` respectively.

### Fixed

- LLM-backed degradation paths (`cmx info` summaries and compact `cmx diff`) now collapse provider failures to one-line, action-oriented notes instead of leaking raw upstream JSON payloads.
- Argument-validation failures now include a `try:` line with the next command to run, and unknown-artifact failures now suggest a close match when one exists or point at the relevant discovery command.

## [3.0.0] - 2026-07-05

### Added

- **Sets — consumer-side activation groups.** A new top-level noun, `cmx set`, for grouping installed skills and agents into named units that can be **activated** and **deactivated** together — a lever for managing the standing context cost of an installation (every installed artifact's trigger description is loaded into an assistant's context whether or not it fires; a set lets you switch the cost of unrelated bodies of work on and off as a unit). Sets are local, user-composed state, distinct from publisher-side marketplace plugins. See `SETS.md` for the full design.
  - **Definitions and curation:** `cmx set create <name> [--desc <text>] [--from <source>:<plugin>] [--local]`, plus `list`, `show`, `add`, `remove`, `delete`, and `rename`. Membership is stored in a `sets.json` state file — global at `~/.config/context-mixer/sets.json`, project-local at `.context-mixer/sets.json`. `set add` snapshots each member's source from the lockfile so reactivation is deterministic.
  - **Activation lifecycle:** `cmx set activate <name>` installs every member from its pinned source (idempotent — doubles as a repair); `cmx set deactivate <name>` uninstalls them while **retaining the set definition**. Deactivation is reference-count-aware (a member also held by another active set is retained, not removed) and drift-guarded (a member with local edits blocks its own uninstall unless `--force`). Both verbs support `--dry-run`. `cmx set delete --purge` deactivates before deleting.
  - **Context-footprint reporting:** `set list` and `set show` report the character footprint of each set's trigger descriptions, annotating inactive sets as not-currently-loaded — so the cost of a set is visible before you activate it.
  - **`--from <source>:<plugin>` seeding:** `set create` can pre-populate a set from an existing marketplace plugin's declared agents and skills, pinning their source. The set is created inactive; the artifacts need not be installed yet.
  - **doctor integration:** `cmx doctor` now surveys set consistency — flagging active sets missing a member, and inactive sets whose members linger installed (reference-count-aware) — under the existing issue model and exit-code-2 contract.

## [2.12.1] - 2026-07-04

### Fixed

- The **embedded companion agent skill** (baked into the binary and installed by `cmx init`) is refreshed to match the current surface. It no longer points at the deprecated `cmx doctor --adopt-all` — orphan adoption now steers to the canonical `cmx skill adopt --all` / `cmx agent adopt --all` — and it documents `cmx doctor --json`. The `2.12.0` binary shipped the pre-reconciliation skill; installing `2.12.1` (then re-running `cmx init`) picks up the corrected guidance.

### Internal

- CI workflows bump `actions/checkout`, `actions/upload-artifact`, and `actions/download-artifact` from their deprecated Node 20 majors (`@v4`) to Node 24 (`@v5`), clearing the GitHub deprecation warnings on CI and release runs. No user-visible change.

## [2.12.0] - 2026-07-04

### Added

- **`cmx doctor --json`** emits the full survey as machine-readable JSON to stdout (human table and prose suppressed; warnings still go to stderr). The schema mirrors the human output: `scope`, `platforms_surveyed`, `showing` (`"needs_attention"` or `"all"`, matching `--all`), a `summary` object with the same seven counts as the human "Summary:" line, and an `artifacts` array with structured `locations` (`path`/`version`/`state`) for diverged artifacts in place of the free-text "diverges: ..." prose. Exit codes are unchanged: `2` when there are actionable issues, `0` when clean.

### Changed

- **`cmx doctor`'s diverged-artifact guidance** is now scannable and case-directed — short branches by situation (source-/home-backed edited in place → `promote`; source-backed restore → `update --force`; external/source-less → `sync`; not sure → `diff`) instead of one dense paragraph. The underlying `promote`/`update`/`sync` commands are unchanged; this is wording only.

### Deprecated

- **`cmx doctor --adopt-all` / `--from`** are soft-deprecated: still functional this release, but now print a stderr warning steering to the canonical `cmx skill adopt --all` / `cmx agent adopt --all` (with `--from <dir>`), and will be removed in the next major version. `doctor`'s own orphaned-artifact hint no longer advertises `--adopt-all`.

## [2.11.0] - 2026-07-04

### Added

- **`cmx init`** — cmx now installs its own companion agent skill through the shared `cmx-core` library, the same embeddable installer other fleet tools (parite, foundry) use. Global scope by default (a companion skill describes the tool, not the project); `--local` installs into the current project instead; `--force` overrides the newer-installed refusal; `--remove` uninstalls; `--json` emits a machine-readable report (the first `cmx` command to do so — every other command is human-text only); `--global` is a no-op alias kept for one release. cmx becomes a dogfooding consumer of its own embeddable installer.
- **Devin** (Cognition's cloud software engineer) joins the supported platforms (`--platform devin`), bringing the roster to fourteen. Devin is skills-only — it has no file-droppable agent concept (its knowledge base and playbooks live platform-side) — and follows the open Agent Skills standard, so it slots into the shared `.agents/skills` cohort alongside Pi, Crush, Zed, and OpenHands. Its skill discovery is repository-scoped (it scans repos connected to it, recommending `.agents/skills/`), so project-scoped installs are the ones Devin actually reads; a global install lands in the shared `~/.agents/skills/` for cohort consistency but reaches Devin only once committed to a repo it can see.

### Documentation

- `CHARTER.md` is brought current with where the project actually went, correcting charter drift. It is now the **Context Mixer** charter covering both binaries — cmf (facets, recipes, plugin/marketplace tooling) previously wasn't chartered at all — with an explicit consumer (cmx) / publisher (cmf) split replacing the "single CLI" promise. Cross-platform curation and reconciliation (canonical home, `doctor`, `adopt`, `promote`, `sync`, drift detection) is elevated to a pillar of equal weight with marketplace distribution, and the hone non-goal now draws the line explicitly: recipe assembly is deterministic composition of hand-curated facets, not derivation from repository structure.

### Changed

- **`cmx skill promote <name>` now selects the copy to canonicalize by drift**, and honors `--platform <name>` to break ties. It previously promoted whichever copy path resolution landed on — always the default platform (Claude), blind to which copy was actually edited. Because agents edit their own installed copy without knowing cmx's canonical home, an in-place edit on a non-default platform (e.g. the shared `.agents/skills` copy Codex reads) could be silently discarded when a stale Claude copy got promoted instead. Promotion now chooses the copy that was **edited in place**: exactly one drifted copy is promoted regardless of platform; an explicit `--platform <name>` always wins (matching the `--platform` target `cmx skill diff` already suggests); no drifted copy is a no-op (or a refusal when the home diverged elsewhere); and two or more copies that disagree are refused with a pointer to `cmx skill diff <name>` and `--platform <name>` to pick the winner. Agents remain single-copy on the active platform — they're reformatted per platform, so a cross-platform byte comparison is meaningless.

## [2.10.2] - 2026-06-23

### Fixed

- **`cmx {skill,agent} adopt <name>`** now homes a skill that exists in several install locations at once (a copy diverged across platforms) under the **highest-priority configured platform**, instead of whichever install directory sorted first alphabetically. Because the shared `.agents/skills` directory sorts before `~/.claude/skills`, adopting a skill present in both could silently track it for Codex even when Claude was listed first in `config platforms` — and then report `now tracked for: codex`. Adoption now follows the configured `platforms` order (falling back to the default platform order when no managed set is declared), preferring an adoptable orphaned copy.
- **`cmx doctor` divergence detection is now content-based.** It previously flagged an artifact as diverged when its copies differed in *version or tracking state*, never comparing actual content. That was wrong in both directions: two byte-identical copies that merely differed in tracking state (e.g. tracked for one tool, untracked for another) were reported as "diverged" and pointed at `cmx skill sync` — which would do nothing, since the bytes already matched; and two genuinely different **unversioned** copies that shared a tracking state were reported as **not diverged at all** (a silent miss). Doctor now hashes each copy and flags divergence only when the content actually differs — catching edited-but-unversioned skills that drift apart, and no longer crying wolf over pure tracking asymmetry (which surfaces through the per-copy state and the `untracked`/`install` hint instead).

## [2.10.1] - 2026-06-23

### Fixed

- `cmf validate` (and `cmf facet validate`, `cmf plugin validate`, `cmf marketplace validate`) now **exit non-zero** when validation surfaces an error-level issue, instead of always exiting `0`. The report was printed either way, but a publisher running validation in CI couldn't gate on the exit status — a failing validation looked like success. Error-level issues now map to exit code `2` (matching `cmx doctor`); warnings-only and clean runs still exit `0`.

### Documentation

- The mdBook documentation site is brought current with the 2.10.0 feature set, which had drifted behind the code. New guide pages for **Promoting Local Edits** (`promote`) and **Reconciling Across Platforms** (`skill sync`); the command reference gains `promote`, `skill sync`, and `config platforms`, documents multi-platform `install`/`uninstall` and `--platform` scoping, and lists all 13 platform values. Corrected stale pages: directional `diff` (with `--full`), multi-platform-by-default install, optional marketplace `metadata`, and the `cmf validate` exit-code note above.

### Internal

- Consolidated duplicated platform-set handling (a shared `platforms_label` helper and a `managed_or_all_platforms` config helper) and fixed two spots that silently swallowed config-load I/O errors. Made `ArtifactKind::installed_path` platform-aware via an explicit agent extension, removing a latent path-construction mismatch for Codex agents. No user-visible behaviour change.

## [2.10.0] - 2026-06-22

### Added

- **`cmx {skill,agent} promote <name>`** — the mirror of `update`: push in-place edits of an installed artifact back into the canonical **home**. Where `update` pulls the home copy over the installed one (discarding local edits), `promote` copies the *installed* copy into the home and refreshes the `home`-provenance lock baselines so the artifact reads as tracked again. This supports the common authoring loop — an assistant edits its own skill where it's installed, then you promote those edits to the canonical home. Promotes the copy `cmx diff` shows (global scope preferred, then project). Home target only for now: a git-sourced artifact is rejected with guidance (edit the source clone, or `update --force` to discard the edits), as is an untracked one (steered to `adopt`/`install`); agents on a platform that reformats them to TOML are rejected too (the installed copy is no longer canonical markdown). Other home-tracked platforms whose copy still differs from the promoted content are reported as drifted afterwards and pointed at `sync`.
- **`cmx skill sync <name>`** — reconcile a skill that has diverged across platforms by copying one copy over the others, so every platform carries the same content. Unlike `update` (which pulls from a registered *source*), `sync` works **between install locations**, so it also reconciles `external` skills and any skill with no source. By default the **newest version wins**; pass `--from <platform>` to force the direction, or `--dry-run` to preview. When copies are unversioned (or tie) and differ, it asks for `--from` rather than guessing. `cmx doctor`'s divergence hint now points here (and to `cmx skill diff`). Skills only for now — agents are reformatted per platform (e.g. Codex TOML), so cross-platform agent reconciliation needs format-aware handling.
- **`cmx config platforms add|remove|list`** — declare the platforms cmx manages instead of letting it infer. When the list is non-empty it becomes authoritative: a default (no `--platform`) `install`/`uninstall` acts on exactly those platforms, and `cmx doctor` surveys only those — so cmx ignores tools you don't use rather than scanning all thirteen. When the list is empty (the default), behaviour is unchanged: install infers from platforms in use, while uninstall and doctor consider every supported platform. The set is stored as lowercase names in `config.json` (`"platforms": ["claude", "codex"]`) and shown by `cmx config show` (as `(inferred)` when unset). Onboard a tool before any install with `cmx config platforms add codex`.
- `cmx skill info <name>` / `cmx agent info <name>` — kind-scoped detail for an installed artifact, alongside the existing (both-kinds) `cmx info <name>`. On top of the usual metadata, `info` now shows **Activates when** (for a skill, its `description` frontmatter — precisely the activation trigger the assistant reads to decide whether to load it; for an agent, its role description) and **What it does**, a short LLM-generated paragraph produced via the configured gateway (`cmx config gateway`/`model`). The summary requires a build with the **`llm` feature**; a lean build prints a one-line hint instead, and generation is best-effort so an unreachable provider never fails the command.

### Changed

- **`cmx {skill,agent} diff` is now directional and offers both reconciliation paths.** Previously the output couldn't tell you which copy held which change, dumped both files in full, and only ever suggested `update` (source → installed) — which silently overwrites in-place edits when the installed copy is the newer, authoritative one. Now it names both sides with their paths (flagging the installed copy as locally edited), prints a per-file change summary (M/A/D with +added/−removed counts) under a stated convention (− source, + installed), and shows a real directional unified diff (with context and collapsed unchanged runs) instead of the full-content dump. The LLM analysis now also recommends a direction. Crucially, the footer offers **both** reconciliation directions and picks neither: `promote` (keep the installed edits → home) when the source is the home, and `update [--force]` (discard the installed edits) with its overwrite caveat. The unified diff is produced by a small self-contained line diff — no new dependency. By default the output stays **compact** — header, file summary, analysis, and reconcile directions — and the line-by-line diff prints only with `--full` (a one-line hint points there), so a large change reads in ~20 lines instead of hundreds. `diff` is now also **multi-platform aware**, so it agrees with `cmx doctor`: it surveys every installed copy of a skill across the managed platforms instead of only the active platform's, shows a per-platform matrix (which copies match the source, which differ) when more than one copy exists, and focuses the detailed diff on a copy that actually differs — so it never reports "matches home" while another platform's copy has drifted. The reconcile commands are qualified with `--platform <p>` for the focused copy (preferring a managed platform, e.g. `--platform codex` over `opencode`), and a fully-consistent skill reports "matches … on all N installed copies". Agents stay single-copy (they're reformatted per platform, so a cross-platform byte comparison is meaningless). The output now also uses **one vocabulary** end to end: the two copies are named concretely — the source name (`home`, or the repo) and the platform name (`codex`) — and the LLM summary is instructed to use those same names, so it no longer says "source"/"installed copy" while the UI says "home"/"codex". The diff convention is stated once ("− lines are home, + lines are codex") and the file table, `--full` diff, and reconcile directions all speak it ("keep codex's edits — copy codex into home", "only in codex").
- **`cmx skill sync`'s ambiguity failure is now actionable, and divergence guidance steers to the right tool.** When `sync` can't auto-pick a winner (the differing copies are unversioned or share a version) it no longer bails with a bare "use `--from`": it lists each diverging copy (platforms, location, size), prints the exact per-copy `--from` command — scoped to a managed platform, so a shared `.agents/skills` copy reads as `--from codex` rather than `--from opencode` — and, when the skill is tracked from the home, points at `cmx skill promote` as the make-one-copy-canonical-then-re-project alternative. `cmx doctor`'s divergence hint now matches that model: `promote`/`update` for source- or home-tracked skills, `sync` for source-less or external ones.
- **`install` and `uninstall` are now multi-platform by default.** With no `--platform`, `cmx {skill,agent} install` lands the artifact on every platform already **in use** — those with tracked artifacts at the target scope — so a new install joins the tools you actually use (e.g. Claude + Codex + Hermes) and stays in sync across them; it falls back to Claude when nothing is tracked yet. Pass `--platform <tool>` to constrain the install to one platform (which also onboards a new tool). Previously a bare install only ever targeted Claude. Each landing is reported on its own line, naming the platform.
- **`cmx {skill,agent} uninstall` now honours `--platform`.** Without it, uninstall still sweeps every platform (removing the artifact wherever it's tracked); with `--platform <tool>` it removes only from that platform, leaving the others intact. Previously `--platform` was ignored on uninstall, so there was no way to remove an artifact from just one tool. Together these make it possible to reconcile a divergent set of skills between, say, Claude and Codex.
- Artifact frontmatter is now parsed with `serde_yaml_ng` — a real YAML library — instead of hand-rolled line-prefix matching. This correctly handles single-quoted strings, inline comments, numeric/bool scalars, and flow-style mapping blocks that the previous implementation silently mishandled.

### Fixed

- `cmx doctor`'s header now reports the number of platforms it **actually** surveyed (the managed set when one is configured, e.g. "2 managed platform(s) surveyed") instead of always claiming all 13. The count was hardcoded and became misleading once `doctor` could be scoped to a managed-platform set.
- `marketplace.json` `metadata` fields (`version`, `description`) are now optional. Both are optional in the Claude Code marketplace spec, but cmx required them, so any source whose `metadata` block omitted either field failed to parse with `missing field \`version\``. Because the parse happens during the survey that backs `cmx list`, `cmx doctor`, and related commands, a single such source aborted the whole command. Partial or absent `metadata` blocks are now tolerated.
- `scan::extract_field` now reads YAML **block scalars** (`description: >` folded and `description: |` literal). Previously a multi-line `description` collapsed to just the `>`/`|` indicator, so e.g. `cmx info` showed an activation trigger of `>`. Folded scalars join with spaces, literal scalars keep newlines; an inline value that merely starts with `>` (like `>= 2.0`) is still taken verbatim.
- The source scanner no longer **silently drops** a skill or agent whose frontmatter is accepted by Claude Code but is not strictly valid YAML — most commonly an unquoted, multi-paragraph `description:` broken by a blank line (a plain scalar can't resume after a blank line at column 0). Such artifacts vanished from `cmx search`, `cmx {skill,agent} diff`, `cmx outdated`, and the `Available` column of `cmx list`, while `cmx doctor` — which reads files against lock entries rather than scanning — still listed them and pointed at `cmx skill diff <name>`, which then dead-ended with "No skill named … found in any registered source." Frontmatter that fails a strict YAML parse now falls back to a lenient line scan that recovers the top-level fields (whitespace-joining multi-line/multi-paragraph values); well-formed frontmatter is untouched and keeps its exact YAML semantics.

## [2.9.0] - 2026-05-30

### Changed

- `cmx doctor` now shows **only artifacts that need attention** by default (drifted, untracked, orphaned, missing, diverged) — it's a doctor, for fixing broken things. Healthy `tracked` and `external` artifacts are tallied in the summary but not listed. Pass `--all` for the full inventory. When nothing's wrong it reports "everything cmx manages is healthy."
- `cmx list` / `cmx {skill,agent} list` exclude external artifacts by default (the cmx-managed inventory); pass `--all` to include external ones too.
- `cmx {skill,agent} install <name>...` now accepts **multiple names** in one command (e.g. `cmx skill install frontend-design pptx xlsx`). Best-effort: each is installed independently; failures (not found, ambiguous source, locally modified without `--force`) are collected with their reason rather than aborting the batch. Exits non-zero if any failed. `--all` is unchanged.
- `cmx list` and `cmx {skill,agent} list` are now **cross-platform** and built from the same grouped survey as `cmx doctor`: one row per logical artifact across every platform, instead of only the active `--platform`'s view. Previously, after projecting skills to (say) codex, a bare `cmx list` (defaulting to Claude) silently omitted skills that lived only in codex's `.agents/skills`. The listing now also shows a **Tools** column (the tools each artifact is tracked for, e.g. `claude, codex`) and a **Source** column with just the source repo name (no install path). `cmx list` **excludes external artifacts** (those declared managed by another tool) — they were appearing as empty-everything rows; they remain visible in `cmx doctor`'s full audit.
- `cmx {skill,agent} uninstall <name>...` now accepts **multiple names** in one command (e.g. `cmx skill uninstall webapp-testing web-artifacts-builder`). Best-effort: each name is removed everywhere it's tracked; names that aren't installed anywhere are listed as "not found" rather than aborting the batch. Exits non-zero only when nothing at all was removed.
- `cmx {skill,agent} uninstall <name>` is now **cross-platform**: it removes the artifact everywhere cmx tracks it (every platform's lock entry) and deletes every physical copy, rather than only acting on the active `--platform`. Previously a skill projected to (say) codex and living in the shared `.agents/skills` directory couldn't be removed with a bare `cmx skill uninstall <name>` — it failed with "not on disk, no lock entry" because the command defaulted to Claude, even though `cmx doctor` clearly listed it. The shared `.agents/skills` copy is deleted once (it's one physical directory read by the whole cohort), and each platform that tracked it has its lock entry cleared. The result reports which platforms it was removed from.
- `cmx doctor` now presents **one logical artifact per skill**, with a `Tools` column listing every tool it's installed for, instead of one row per install location. A skill projected to several tools is no longer reported as N "duplicates" — that's the intended "curate once, project to many" outcome. The old `duplicated` flag is replaced by `diverged`, which fires only when a skill's copies actually **disagree** across locations (different version or state); `cmx <kind> update <name> --force` re-syncs them. Counts in the summary are now per logical artifact.

### Added

- `cmx {skill,agent} unadopt <name>...` — the inverse of `adopt`. Removes the artifact's canonical copy from the home and clears every `home`-provenance lock entry for it (un-tracking it across platforms), while **leaving the on-disk originals in place** (they revert to orphaned). Useful when a skill was adopted by mistake — e.g. one a tool creates for itself (`gilt`, `hone`, `mailctl`) that belongs to that tool, not your curated home. Accepts multiple names; a `--external` flag also marks each as external in one step, so `doctor` reports them as managed-by-another-tool rather than orphaned.
- **External artifacts.** Declare artifacts that another tool manages — e.g. a tool's bundled/stock skills in its own directory — so `cmx doctor` reports them as `external` (a steady state, not flagged) instead of flagging them as orphaned, and so `adopt`/`--adopt-all` never sweep them into your home. Manage the list with `cmx config external add|remove|list`; `cmx config show` displays it. Each rule is either a **directory** (an install location, `~` expands to home — covers everything under it) or a bare **artifact name**. A directory rule like `~/.hermes/skills` lets `doctor` reach a clean (zero-exit) resting point while a tool's stock bundle stays acknowledged but unflagged.
- `cmx doctor` **names a divergence** instead of leaving it opaque. When a diverged artifact's copies are at different versions, the `Version` column shows the skew (e.g. `3.2.0 / 3.3.0`) rather than `-`, and a detail line under the summary names which copy is where: `• hopper-coordinator diverges: ~/.agents/skills @ 3.2.0, ~/.claude/skills @ 3.3.0` (with each location's state appended when copies disagree on state, not just version). So a skew reads at a glance as "this copy is stale."

### Fixed

- `cmx doctor` no longer reports "everything healthy · 0 diverged" while an artifact is visibly diverged. A divergence is a real anomaly worth surfacing *whoever* owns the artifact, so it's now flagged (shown in the default view, counted in the tally, exit code `2`) even for `external` artifacts — cmx just can't be the one to re-sync an external one, so the hint points at the owning tool instead of `cmx update --force`. A *consistent* `external` or `tracked` artifact is still healthy and unflagged; only a genuine divergence surfaces.

## [2.8.0] - 2026-05-30

### Added

- `cmx doctor` now distinguishes two kinds of no-lock-entry artifact: **`untracked`** (a registered source provides it — installed out-of-band, fix by `install`) versus **`orphaned`** (no source provides it — hand-authored, the `adopt` candidate). Previously both were lumped as "orphaned".
- `cmx {skill,agent} adopt <name>...` now accepts **multiple names** in one call (all-or-nothing: an invalid name aborts the batch before anything is adopted).
- `cmx {skill,agent} adopt --all [--from <dir>]` and `cmx doctor --adopt-all [--from <dir>]` — bulk-adopt orphans, optionally restricted to a single install location. `--from ~/.claude/skills`, for example, adopts your own skills while leaving another tool's bundled-skill directory untouched.

### Changed

- `cmx {skill,agent} adopt` and `cmx doctor --adopt-all` now act **only on orphaned** artifacts. An untracked (source-available) artifact is no longer adopted as if it were private — `adopt <name>` steers it to `cmx <kind> install <name>` instead, and `--adopt-all` skips it. This prevents adopting a tool's bundled/stock skills, or any source-backed artifact, into the personal canonical home.
- Skill checksums and copies now ignore transient/generated content: `node_modules/`, `__pycache__/`, `*.pyc`, `.git/`, and `.DS_Store`. Previously a skill carrying runnable scripts would show as `drifted` the moment its dependencies or bytecode appeared (e.g. after `npm install` or running a Python script), because the directory checksum hashed every file. Ignoring these regenerable paths keeps the drift signal honest and keeps the canonical home and projected installs lean (no vendored `node_modules` dragged along on adopt/install). Authored content — including `package.json`/`package-lock.json` — is still tracked and copied.

### Fixed

- `cmx {agent,skill} uninstall <name>` now reconciles a tracked-but-absent artifact instead of bailing. Previously it errored `No <kind> named '<name>' found` whenever the file was already gone — which is exactly the "missing" state `cmx doctor` reports and tells you to fix, so the stale lock entry could not be cleared through the CLI. It now removes the stale lock entry and reports that the file was already absent. The `doctor` footer hint for missing entries is corrected accordingly (uninstall clears the entry; reinstall only works if the source still has it).

## [2.7.0] - 2026-05-30

### Added

- `cmx doctor` — a read-only survey of the whole system installation across every supported platform. It cross-references each platform's install directories and per-platform lock files and classifies every artifact as `tracked`, `drifted` (locally edited after install), `orphaned` (on disk but untracked — e.g. hand-authored skills), or `missing` (in a lock file but gone from disk), and flags artifacts duplicated across distinct install locations. Skills in the shared `.agents/skills` directory are reported once for the whole cohort rather than once per tool. `cmx doctor --local` also includes project scope. Exits non-zero (`2`) when drift, orphans, or missing entries are found, so it can gate a hook or CI check.
- `cmx::platform::Platform::ALL` — the exhaustive slice of platform variants, so cross-platform operations (like the survey) automatically cover every platform.
- `ConfigPaths::with_platform` — derive a path view bound to a different platform from a single base, reusing all platform-aware path resolution.
- **Canonical home** for hand-authored private artifacts — a tool-neutral source of truth that survives switching coding assistants. Defaults to `~/.config/context-mixer/home` (inside cmx's existing config root, alongside `sources.json` and the lockfiles), overridable via the `home` field in `config.json`. `cmx home init` creates it and registers it as a visible local source named `home`; `cmx home path` prints the resolved location.
- `cmx skill adopt <name>` / `cmx agent adopt <name>` and `cmx doctor --adopt-all` — bring orphaned, hand-authored artifacts under management. Adoption copies the artifact **verbatim** into the canonical home, auto-registers the `home` source, and records `home` provenance (with the artifact's checksum) in the lock file of every platform that reads the orphan's location, so it reclassifies from `orphaned` to `tracked`. The original on-disk copy is never moved or rewritten. Once adopted, projecting the set to another tool is just `cmx skill install --all --platform <tool>` — the home is a normal registered source.
- `home` field on `CmxConfig` for overriding the canonical home location.

## [2.6.0] - 2026-05-29

### Added

- `--platform` global flag (and `CMX_PLATFORM` env var) for selecting the target AI coding assistant: `claude` (default), `copilot`, `cursor`, `windsurf`, `gemini`. All install, uninstall, update, list, outdated, info, and search commands respect the platform setting.
- Platform-aware install paths: agents and skills now install to the correct directory for each platform (e.g. `.cursor/agents/` for Cursor, `~/.codeium/windsurf/skills/` for Windsurf globally).
- Per-platform lock files: non-Claude platforms use `cmx-lock-<platform>.json` so installations for different tools remain independent. Claude keeps `cmx-lock.json` for backward compatibility.
- `cmx::platform::Platform` is now a public type in the `cmx` crate; `cmf` imports it from there rather than defining its own copy.
- `cmf manifest generate` now emits `.windsurf-plugin/` manifests, so marketplaces built with cmf no longer silently exclude Windsurf users.
- Three additional `--platform` targets: `opencode`, `codex`, and `pi`. Skills for all three install to the shared cross-tool `.agents/skills/` (project) and `~/.agents/skills/` (user) convention that opencode, Codex, and Pi all read.
- opencode agents install as markdown to `.opencode/agent/` (project) and `~/.config/opencode/agent/` (user).
- Codex agents are transformed from cmx markdown into Codex subagent TOML (`<name>.toml`) on install, mapping `name`, `description`, the markdown body (`developer_instructions`), and an optional `model` field. Installed to `.codex/agents/` / `~/.codex/agents/`.
- Per-platform support gating: platforms declare which artifact kinds they support. Pi supports skills only, so `cmx agent install --platform pi` (and uninstall/update) fails with a clear, actionable error rather than installing into a directory Pi never reads.
- Five additional skills-only `--platform` targets: `crush`, `amp`, `zed`, `openhands`, and `hermes`. All consume the cross-tool `.agents/skills/` standard, so a single skill install serves the whole cohort (plus opencode/codex/pi) at once. None has a file-droppable agent concept, so `cmx agent install` for these fails with a clear error. Two have user-scope path nuances: Amp resolves user skills under `~/.config/agents/skills/` (XDG), and Hermes under `~/.hermes/skills/` (its global source of truth).

### Notes

- opencode, Codex, and Pi have no Claude-style plugin/marketplace manifest format, so `cmf manifest generate` intentionally does not emit manifest directories for them.
- Because opencode/Codex/Pi share the `.agents/skills/` directory, uninstalling a skill under one of these platforms removes it for all tools that read `.agents/`.

### Changed

- Renamed the `Codex` platform variant to `Copilot` and its generated manifest directory from `.codex-plugin/` to `.copilot-plugin/`, matching the documented platform name (GitHub Copilot). Re-run `cmf manifest generate` to refresh manifest directories.

### Fixed

- `cmx agent install` and `cmx skill install` now roll back a freshly copied artifact when the lockfile write fails, eliminating the ghost-install state where an artifact exists on disk with no lockfile entry
- `json_file::save_json` now writes atomically via a sibling `.tmp` file followed by a rename, preventing partial writes from corrupting an existing JSON file

## [2.5.3] - 2026-04-11

### Changed

- Extracted `find_entry_with` helper in lockfile module for reusable lock entry lookup across scopes
- Extracted `split_frontmatter_str` helper in scan module to DRY up frontmatter parsing
- Refactored `update_with` in install module to use the new `find_entry_with` helper

## [2.5.2] - 2026-04-10

### Fixed

- `cmx list` now only shows the installed version on the row matching the source from which the artifact was actually installed, leaving the column blank for other sources offering the same artifact
- Disambiguated "not installed from this source" (blank) from "installed but unversioned" (`-`) in the Installed column

## [2.5.1] - 2026-04-09

### Fixed

- Agent scanner no longer recurses into skill directories — `.md` reference files inside skills were being falsely detected as agents
- Agent scanner now requires `.md` files to live in an `agents/` directory to be recognized as agents, preventing false positives from documentation or other markdown files with similar frontmatter

## [2.5.0] - 2026-04-05

### Added

- **cmf (context mixer forge)** — new publisher/authoring tool for managing agentic context artifacts, shipped alongside cmx in the same distribution
  - `cmf status` — repo overview dashboard showing plugins, agents, skills, facets, validation summary
  - `cmf validate` — aggregate validation across plugins, marketplace, and facets
  - `cmf plugin list` — list all plugins with agent/skill counts per plugin
  - `cmf plugin init <name>` — scaffold new plugin directory with plugin.json, agents/, skills/
  - `cmf plugin validate` — check plugin structure and frontmatter integrity
  - `cmf marketplace validate` — check marketplace.json consistency against plugin directories
  - `cmf marketplace generate` — regenerate marketplace.json from plugin directory structure, preserving owner metadata and categories
  - `cmf facet list` — list facets grouped by category and recipes
  - `cmf facet validate` — validate facet frontmatter, scope fields, and recipe references
  - `cmf recipe list` — list available recipes with target paths
  - `cmf recipe assemble <name>` / `--all` — assemble agents from facets via naive concatenation
  - `cmf recipe diff <name>` — compare assembled output against current agent file
  - `cmf manifest generate` — generate multi-platform manifests for Codex, Cursor, and Gemini from Claude plugin sources

### Changed

- Converted project to Cargo workspace with `cmx` and `cmf` as separate binaries sharing the cmx library crate
- Unified versioning via `[workspace.package]` — both binaries share the same version
- Promoted `json_file` module from `pub(crate)` to `pub` for cross-crate use
- Release archives now include both `cmx` and `cmf` binaries
- Homebrew formula (`brew install svetzal/tap/cmx`) now installs both `cmx` and `cmf`
- mdbook documentation expanded with pages for plugins, facets, recipes, and cmf command reference

## [2.4.2] - 2026-03-28

### Fixed

- Show all sources when the same artifact exists in multiple registered repos

## [2.4.1] - 2026-03-27

### Fixed

- Show installed version from disk for untracked artifacts in `cmx list`

## [2.4.0] - 2026-03-27

### Added

- Support metadata-nested version extraction (`metadata.version` in frontmatter)

## [2.3.0] - 2026-03-25

### Added

- Display source repository version for skills in `cmx source browse`
- Gate `tokio` and `mojentic` behind optional `llm` feature for lean default builds

### Changed

- Refactored tests to eliminate knowledge duplication

### Security

- Updated `sha2` and transitive cryptographic dependencies
- Updated `uuid` to 1.23.0

## [2.2.0] - 2026-03-24

### Fixed

- Marketplace scanning now discovers agents and skills from plugins that use `source` paths without explicit `agents`/`skills` arrays (e.g. `anthropics/claude-code` bundled plugin format)
- Remote source objects (`url`, `github`, `git-subdir`, `npm`) now emit a clear warning instead of being silently ignored

## [2.1.1] - 2026-03-23

### Security

- Updated transitive dependency `iri-string` to 0.7.11 to address security vulnerabilities

## [2.1.0] - 2026-03-20

### Added

- `cmx search <keyword>` command — searches all registered sources for agents and skills by name and description
- mdbook documentation site deployed to GitHub Pages
- Artifact descriptions extracted from frontmatter for search matching

## [2.0.0] - 2026-03-20

### Added

- `cmx source add/list/browse/update/remove` for managing plugin marketplace sources
- `cmx agent install/update/list/diff` for managing agents
- `cmx skill install/update/list/diff` for managing skills
- `cmx list` aggregate view of all installed artifacts with status indicators (✅ ⚠️ ⛔)
- `cmx outdated` to show artifacts needing attention (untracked, changed, deprecated)
- `cmx config show/gateway/model` for LLM configuration
- `--all` flag for `install` and `update` commands
- `--local` flag for project-scoped installation
- Lock file tracking with SHA-256 checksums and version metadata
- LLM-powered diff analysis via mojentic (OpenAI and Ollama gateways)
- Plugin marketplace format support (`.claude-plugin/marketplace.json`)
- Fallback tree-walking scanner for repos without marketplace.json
- Auto-update for stale git-backed sources (>60 min)
- Deprecation support in frontmatter (`deprecated`, `deprecated_reason`, `deprecated_replacement`)
- Versioning support in frontmatter with semver
- Source cleanup on remove (deletes cloned git repos)
- GitHub Actions CI (fmt, clippy, tests) and release pipeline
- Homebrew tap distribution via `brew tap svetzal/tap && brew install cmx`
- Cross-platform builds (macOS ARM64, macOS x64, Linux x64)
