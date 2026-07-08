# EMBEDDING.md — cmx-core, the embeddable skill-installation library

**Status: cmx-core published on crates.io (Rust, 0.1.0) and npm (TypeScript, 0.3.0). Four production consumers migrated — parite, foundry (Rust); hopper, mailctl (TS).** Remaining roadmap: the Python port (gilt, researcher, evt) and the last TS migration (hone). See "What remains" below.

## Why this exists

Our CLI tools ship companion agent skills, and before this work every one of them had invented its own installation machinery (surveyed 2026-07-03): parite, gilt, hopper, hone, researcher, and foundry each hand-rolled the same idea — parse a frontmatter version, compare semver, copy files into a hard-wired `.claude/skills/<name>/` — while diverging on everything cosmetic (`skill-init` vs `init`; four different frontmatter version keys). None could uninstall, none knew any platform besides Claude, and none integrated with cmx: on a cmx-managed system, `cmx doctor` saw every one of these skills as an untracked orphan. evt had no skill at all.

cmx-core replaces all of that with one library call:

```text
"Here is my skill (name, version, files). Install it at this scope.
 Tell me what you did."
```

The library:

1. **Detects cmx management.** If the machine or project is cmx-managed (config/lockfiles present), it registers the bundled skill as a source and records a proper lock entry — the skill becomes a first-class tracked artifact that `cmx doctor`, `cmx update`, and `cmx list` all understand.
2. **Falls back gracefully.** With no cmx present, it performs the platform-aware, version-guarded copy consistently — and it *still writes the lock entry*. The lockfile format, not the cmx binary, is the integration contract: a later `cmx` arrival finds everything already tracked instead of orphaned.
3. **Plans before applying.** Consistent with our CLI UX conventions (guidelines repo, `conventions/cli-ux.md`): the install renders as a dry-run plan naming each file and destination, and the report states what changed in countable terms. Plan, report, and remove-report all carry `Display` impls so every embedding tool prints identical-shaped output.
4. **Uninstalls.** Tools get `<tool> init --remove` for free, honoring "leave the machine as you found it."

This preserves the tools-stay-independent rule (Operations `AGENTS.md`): tools depend on a *library* and share state through a *schema'd lockfile* — a neutral artifact. No tool shells out to the `cmx` CLI; cmx could be deleted and every tool still installs its skill correctly.

## What shipped (2026-07-03)

### The crate

`cmx-core` is a workspace member of this repo and the reference implementation, published to crates.io. **It versions independently of the cmx CLI** (the CLI stays on the workspace version; the crate started at 0.1.0) because embedders pin the library's API, which stabilizes on its own schedule.

- **Extraction** (`3e1924d`): the clean lower layer moved out of the cmx binary — `types`, `platform` (14 platforms), `paths`, `gateway`/`context`, `lockfile`, `checksum`, `config`. One deliberate deviation from the original sketch: `install`/`copy`/`adopt` **stayed in the cmx crate**, because they transitively depend on `scan → scan_marketplace → plugin_types`, which decision 5 excludes from core. cmx-core instead carries its own self-contained `skill_fs`/`skill_install` path.
- **Embeddable API** (`1004cdb`): `SkillInstaller` (`plan`/`apply`/`status`/`remove`), `ToolIdentity`, `BundledSkill`, `ProductionContext` factory, public managed-platform target resolution.
- **API stabilization** (`e30003e`, breaking, pre-0.1.0): driven by the first consumer's friction report — one-call `ProductionContext::claude()`, `BundledSkill::single_md`, `ToolIdentity::new`, `Display` on plan/report/remove, unified `Vec<TargetOutcome>` report shape (destinations on skips; drift-detected skips rendered distinctly as "local edits preserved"), `#[non_exhaustive]` `TargetAction`.
- **Final ergonomics** (`3bfd35b`): `SkillFile::text(rel_path, content)` for multi-file `include_str!` bundles; README sections on multi-file bundles and the `test-support` dev-dependency pattern.

The canonical API documentation is [cmx-core/README.md](cmx-core/README.md) (rendered on crates.io). The API sketch that used to live here is retired — the README's example is compiled and tested.

### Publishing

- Tag scheme: `cmx-core-v<version>` (distinct from the CLI's `v*` release tags).
- Workflow: `.github/workflows/publish-cmx-core.yml` — full quality gate, tag-vs-crate-version consistency check, then `cargo publish` using the `CRATES_IO_TOKEN` repo secret (same pattern as mojentic-ru).
- `cmx` and `cmf` are explicitly `publish = false`; only the library can reach crates.io.

**TypeScript port (`cmx-core` on npm, source in `cmx-core-ts/`):**

- Tag scheme: `cmx-core-ts-v<version>` (distinct from the crate's `cmx-core-v*` and the CLI's `v*`).
- Workflow: `.github/workflows/publish-cmx-core-ts.yml` — quality gate (bun install / tsc / biome / test / build), tag-vs-`package.json`-version check, then `npm publish` via **OIDC trusted publishing (no stored token)**. Node 24 + `npm@latest` (trusted publishing needs npm ≥ 11.5.1); provenance is automatic for the public repo+package. Bun runs the `prepack` build.
- Two setup steps happen **once, out of band** and can't be scripted here: (1) the **first publish is manual** — `npm login` (session-based, 2FA/OTP) then `npm publish` from `cmx-core-ts/` — because npm has no pending-publisher flow, so the package must exist before a Trusted Publisher can be attached; (2) after it exists, configure the **Trusted Publisher** on npmjs.com → package Settings, pointing at workflow file `publish-cmx-core-ts.yml`. Every release after that is keyless via the workflow.
- npm auth context (2026): classic tokens are revoked; local `npm login` issues a 2-hour session with 2FA enforced on publish; CI uses OIDC. See `cmx-core-ts/README.md` if that surface shifts again.

### Proving consumers

Both migrations ran as autonomous hopper engineering items, each required to file a candid "cmx-core API friction" report — those reports drove the stabilization pass, and **this consumer-files-friction-report pattern should be repeated for each port's first consumer**.

| Consumer | Exercised | Notes |
| -------- | --------- | ----- |
| parite (`parite init`) | single-file bundle, first contact with the raw API | friction report produced the `e30003e` stabilization; now on published 0.1.0 |
| foundry (`foundry init`) | multi-file bundle (SKILL.md + 2 references), deletion of in-content version stamping | registry contract preserved: `{binary} init --global --force` (the invocation foundry's registry derives for skill-installing tools) still exits 0; `--json` schema unchanged; now on published 0.1.0 |
| hopper (`hopper init`) — **TS port consumer #1** | single-file bundle on the published npm `cmx-core` 0.2.0; dropped hand-rolled version stamping + `hopper-version:` field; `bun build --compile` single binary bundles cmx-core cleanly | migrated `c1d64c5` (2026-07-06); `--global --force` exits 0, existing file-level `--json` schema preserved; verified clean-checkout (1430 tests, lint, compile, temp-HOME install/idempotent/remove smoke) |
| mailctl (`mailctl init`) — **TS port consumer #2** | single-file bundle on published npm `cmx-core` 0.3.0; `src/init.js` rewritten to `SkillInstaller`/`ToolIdentity`, pure plan/outcome→result mapping, targets + version-guard owned by cmx-core lockfiles | migrated v1.3.0 (`e4bedef`, 2026-07-07); confirms the port on a second consumer |

Both Rust tools adopted the settled `init` conventions: **global scope by default**, `--local` for project scope, `--global` as a temporary no-op alias, `--force` to override the newer-installed refusal, `--remove` to uninstall. Both deleted their hand-rolled per-target rendering in favor of the crate's `Display` impls; hopper did the same on the TS port.

**TS-port friction findings (hopper migration, 2026-07-06)** — carry these into the Python port and a possible cmx-core 0.2.x/0.3.0:

- **Report shape is per-target, consumer contracts are often per-file.** cmx-core's `apply`/`remove` reports are platform/target-level; hopper's `--json` contract is file-level, so preserving it meant stitching `status()` + `plan.targets[].files[].dest_path` + action mapping in consumer code. A file-level view (or a helper) on the report would remove that boilerplate for every consumer.
- **`refuse-newer` is all-or-nothing at `apply()`.** For a future multi-platform managed install, one blocked target blocks the whole apply through the library surface — no clean partial apply.
- **Deprecated-dir cleanup has no hook.** hopper's `.claude/skills/hopper-worker` removal must live outside the installer as a consumer-only step.
- **"Global by default" is consumer policy, not library-encoded.** Each consumer re-implements the default-scope convention; cmx-core could offer it as a documented default.

### Fleet status

| Tool | Skill install today | Status |
| ---- | ------------------- | ------ |
| parite (Rust) | cmx-core 0.1.0 (crates.io) | **migrated** |
| foundry (Rust) | cmx-core 0.1.0 (crates.io) | **migrated** |
| gilt (Python) | hand-rolled (`gilt skill-init`) | awaiting Python port |
| researcher (Python) | hand-rolled (`researcher init`) | awaiting Python port |
| hopper (Bun/TS) | cmx-core 0.3.0 (npm) | **migrated** (`c1d64c5`, 2026-07-06) |
| hone (Bun/TS) | hand-rolled (`hone init`) | TS port shipped — awaiting migration to `cmx-core` (npm) |
| mailctl (Bun/TS) | cmx-core 0.3.0 (npm) | **migrated** (v1.3.0, `e4bedef`, 2026-07-07) |
| evt (Python) | no skill yet | gains its first skill with the Python port |

## What remains

Targets align with the mojentic framework's ports: **Rust, Python, TypeScript, Elixir, Swift, Kotlin** — native ports over FFI bindings, because the domain (file copying, JSON lockfiles, semver comparison) is small enough that a port is cheaper than dragging a Rust toolchain into gilt's pure-Python build or complicating hopper's `bun build --compile` single-binary story. The conformance suite carries the correctness burden.

1. **Behavioral spec + conformance fixtures** — distilled from the now-stable Rust behavior: lockfile schema, path-resolution rules, version-guard semantics, cmx-detection rules, plus golden fixtures (lockfiles, before/after directory trees). This is what makes ports *ports* rather than divergent cousins — the same discipline as mojentic's PARITY.md. Judgment-heavy (what is contract vs. implementation detail); review the spec before queueing ports against it. **Spec: [cmx-core/SPEC.md](cmx-core/SPEC.md) — reviewed 2026-07-05.** The five contract-vs-detail decisions are settled (§11). The one code follow-up from the review — moving the Rust checksum sort from component-wise to `/`-joined-string collation (§11.3–§11.4) — **landed 2026-07-05** with regression tests, so the reference is now a faithful oracle. Only the fixture generator remains before the ports.
2. **Python port** (`cmx-core` on PyPI) — migrate gilt (folding `skill-init` into `init` per decision 3), researcher, and give evt its first skill. First consumer files a friction report before the PyPI publish, mirroring the Rust sequence.
3. **TypeScript port** (npm) — **DONE / published `cmx-core` 0.2.0** (2026-07-06). Native Bun/TS port in `cmx-core-ts/`, passes the full conformance suite; published via OIDC trusted publishing (`.github/workflows/publish-cmx-core-ts.yml`). The friction-report gate ran and surfaced one real wart — a `//home` double-slash in the fixture report serialization — since fixed at the source. **hopper migrated 2026-07-06** (`c1d64c5`) as TS-port consumer #1, filing a friction report (see "Proving consumers"). **mailctl migrated 2026-07-07** (v1.3.0, `e4bedef`) as consumer #2 on the published `cmx-core` 0.3.0. **Remaining: migrate hone** off its hand-rolled `init` onto the published package.
4. **Elixir/Swift/Kotlin** — demand-driven: follow when a tool in that ecosystem ships a skill.
5. **Retire the `--global` no-op aliases** in parite and foundry after one release cycle.
6. **Optional, unscheduled**: unify cmx's own internal `install` path onto `skill_install` if it earns its keep.
7. **Declarative skill-set reconciliation** (design direction, raised 2026-07-06; propose before the Python port so both ports share the API). Today the embedder API is single-skill and imperative: `SkillInstaller(ToolIdentity)` installs/updates/removes one named skill, and any *deprecated-skill* cleanup (hopper's `hopper-worker`, folded into a command — see friction finding above) is hand-rolled by each consumer outside the library. As a tool's companion skills evolve into a **curated set** (skills added, renamed, and removed across versions), that model pushes recurring per-consumer cleanup — the "undue work" to avoid. Direction: let an embedder **declare its current full set of skills** for this version; cmx-core reconciles installed state to the declaration — install new, update changed, skip unchanged, and **prune** skills this tool previously installed but no longer declares. The declaration is the source of truth; removals fall out of reconciliation instead of imperative per-consumer cleanup. cmx-core already has the state to do this safely: the lockfile records which skills a tool installed (`source = bundled:<tool>`), so the prune set is exactly "installed-by-this-tool AND no-longer-declared" — it never touches another tool's or a user's artifacts. Distinct from cmx's user-facing `sets`/`SETS.md` (named groups with activation state); this is the *embedder* declaring the set it owns. Retiring this generalizes and removes friction finding #3.

## Decisions (reviewed with Stacey, 2026-07-03)

1. **Naming: `cmx-core`** — everywhere (crates.io, PyPI, npm). The lockfile format is cmx's; honesty about that beats false neutrality.
2. **Lock entries without cmx present: yes** — the lockfile format is the integration contract. Tools write entries into `~/.config/context-mixer/` even on machines that have never seen cmx, so a later cmx arrival finds everything tracked.
3. **Command convention: `<tool> init`** — the standard companion-skill command across the fleet. gilt's `skill-init` folds into `init` during migration; codified in `conventions/cli-ux.md` §12.
4. **Scope default: global** — skills install to the user's global platform directory (`~/.claude/skills/`, etc.) by default; `--local` opts into project scope. A tool's companion skill describes the tool, not the project.
5. **cmf stays external for now** — the marketplace/manifest machinery (`plugin_types`) remains CLI-side; cmx-core carries only what embedding tools need.
6. **Port version parity (2026-07-06)** — the cmx-core ports (Rust, TypeScript `cmx-core` on npm, eventually Python) track each other on **`major.minor`; patch is independent** — the mojentic model. A port's `major.minor` asserts *which SPEC/contract version it conforms to*, and the shared conformance fixtures (§"What remains" #1) prove it: a port on 0.2.x is one that passes the 0.2.x fixtures. The **cmx CLI is explicitly outside this parity** — it is the consuming application, versions on its own `v*` tag track (3.x today), and cmx-core already versions independently of it (`cmx-core-v*` tags; see `cmx-core/Cargo.toml`). So `cmx-core-ts` starts at **0.2.0** to match the Rust crate, not 0.1.0.

## Chronology

| Commit | What |
| ------ | ---- |
| `477f7bc` | Design drafted |
| `030ec9d` | Decisions reviewed and recorded |
| `3e1924d` | cmx-core crate extracted (hopper `add773a7`) |
| `1004cdb` | Embeddable API added (hopper `d4fcb146`) |
| `524aa1c` | crates.io publishing established |
| — | parite migrated, git-pinned (hopper `f9c7df3f`, parite `ea63f45` final) |
| `e30003e` | API stabilized from parite friction report (hopper `77879e95`) |
| — | foundry migrated, git-pinned (hopper `193942bc`, foundry `2616bd9` final) |
| `3bfd35b` | `SkillFile::text` + docs; tagged `cmx-core-v0.1.0`, published |
| — | Both consumers flipped to published 0.1.0 (hopper `c4827bc2`, `996da5fa`) |
