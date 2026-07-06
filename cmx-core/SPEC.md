# cmx-core — Behavioral Specification

**Status: REVIEWED (2026-07-05).** Distilled from the stable Rust
reference implementation (`cmx-core` 0.2.0). This is the contract every port
(Python, TypeScript, …) must satisfy, byte-for-byte where noted. It is the input
to the conformance fixtures (EMBEDDING.md "What remains" #1) — ports are ports,
not divergent cousins, only because they pass the same fixtures derived from this
spec.

Companion to [EMBEDDING.md](../EMBEDDING.md) (the *why* and the roadmap) and
[cmx-core/README.md](README.md) (the Rust API surface). This document is
language-neutral: it describes observable behavior, not Rust types.

> **Contract-vs-detail decisions are settled** (Stacey, 2026-07-05). The five
> judgment calls EMBEDDING.md flagged are resolved inline below (marked
> **✅ DECIDED**) and collected in §11. Everything else is contract: ports must
> match it and fixtures pin it.

---

## 1. Scope

cmx-core answers one request: *"Here is my skill (name, version, files). Install
it at this scope. Tell me what you did."* A port must implement four operations:

| Operation | Purpose |
| --------- | ------- |
| `plan`   | Compute a dry-run install plan; write nothing. |
| `apply`  | Execute a plan: write files, update lock entries, optionally register a cmx source. |
| `status` | Report install/tracked/drift state per platform. |
| `remove` | Delete installed files and clear lock entries. |

The integration contract is **the lockfile format** (§3), not any binary. A tool
writes lock entries even on machines that have never seen cmx, so a later cmx
arrival finds everything tracked. No port may depend on the `cmx` binary being
present.

**Out of scope for cmx-core** (stays in the `cmx` CLI): marketplace/manifest
machinery (`plugin_types`), git-source cloning, agent installation from sources.
cmx-core installs **bundled skills** only — an agent-kind lock entry may be read
(§3) but cmx-core never installs agents.

---

## 2. Config root and path layout

The **config root** is `$HOME/.config/context-mixer/`, always — independent of
platform and of scope. It holds the global lockfiles, `config.json`,
`sources.json`, `sets.json`, and the default artifact home (`home/`).

Path resolution depends on **scope**:

| Artifact | Global scope | Local scope |
| -------- | ------------ | ----------- |
| Skill install dir | `$HOME/<platform-subpath>` | `<project-root>/<platform-subpath>` |
| Lockfile | `$HOME/.config/context-mixer/<lockname>` | `<project-root>/.context-mixer/<lockname>` |
| `config.json`, `sources.json` | `$HOME/.config/context-mixer/` | (same — config is global only) |

Where `<platform-subpath>` comes from the platform matrix (§4) and `<lockname>`
from the lock naming rule (§3.2). Local install subpaths are relative to the
project root (CWD); global subpaths are anchored at `$HOME`.

⚖️ **`$HOME` / CWD resolution.** The Rust impl uses `dirs::home_dir()` and
process CWD. Ports must resolve the OS home the same way (`os.homedir()` in
Node, `Path.home()` in Python). Proposed contract: home = OS home dir; project
root = process CWD at call time. Fixtures inject both, so this only bites real
runs.

---

## 3. Lockfile format

### 3.1 Schema

A lockfile is JSON:

```json
{
  "version": 1,
  "packages": {
    "<artifact-name>": {
      "type": "skill",
      "version": "1.2.0",
      "installed_at": "2026-07-05T12:00:00+00:00",
      "source": { "repo": "bundled:mytool", "path": "skills/mytool" },
      "source_checksum": "sha256:<hex>",
      "installed_checksum": "sha256:<hex>"
    }
  }
}
```

Field rules (contract):

- **`version`** (top level): integer, currently `1`.
- **`packages`**: object keyed by artifact name. **Serialized in sorted key
  order** (Rust uses `BTreeMap`; ports must sort keys on write — see §3.3).
- **`type`**: `"skill"` or `"agent"`, lowercase. cmx-core writes only `"skill"`;
  it must *read* `"agent"` entries other tools wrote without error.
- **`version`** (per entry): optional string. **Omitted entirely when absent**
  (not `null`). For a bundled-skill install it is always present (the tool's
  version).
- **`installed_at`**: RFC 3339 timestamp string. **✅ DECIDED (§11.1)** — any
  valid RFC 3339 string is *accepted* on read; the exact emitted precision is
  **implementation detail** (it is a timestamp, not a checksum input). Fixtures
  pin it exactly by injecting a **fixed clock** so runs are fully deterministic
  (§10), rather than masking the field.
- **`source`**: object with `repo` and `path` strings. For a bundled skill:
  `repo = "bundled:<name>"`, `path = "skills/<name>"`.
- **`source_checksum`** / **`installed_checksum`**: `sha256:<lowercase-hex>`
  (§5). For a fresh bundled install the two are **equal** (both the just-computed
  source checksum).

### 3.2 Lockfile naming (per platform)

Each platform writes its own lockfile, named by the platform **slug**:

- Claude (slug `""`): **`cmx-lock.json`** (no slug — backward compatibility).
- All others: **`cmx-lock-<slug>.json`**, e.g. `cmx-lock-codex.json`.

Slugs are the lowercase platform names (§4). A single install that targets N
platforms writes N lockfiles (one per platform), even when several platforms
**share** an install directory (e.g. codex + pi both use `.agents/skills`): the
files are written once per shared dir, but each platform's lock entry is written
to its own lockfile.

### 3.3 Serialization

- **Pretty-printed JSON** (`serde_json::to_string_pretty` — 2-space indent).
  **✅ DECIDED (§11.2)** — **contract** = valid JSON with sorted package keys;
  **whitespace/indentation is implementation detail**, ports may differ. Fixtures
  compare *parsed values*, not bytes, for lockfiles (unlike SKILL.md, §6, which
  *is* byte-compared).
- **Atomic write**: serialize to a sibling `<name>.tmp` in the same directory,
  then rename onto the target. A failed write never corrupts an existing
  lockfile. ⚖️ Proposed **contract** (matters for crash-safety on real runs);
  fixtures can't easily observe it. Ports should implement it regardless.
- **Absent file** loads as the default empty lockfile (`{version:1, packages:{}}`),
  never an error. Malformed JSON *is* an error.

---

## 4. Platform matrix

14 platforms. Skill install subpaths below (agents omitted — cmx-core installs
skills only, but the matrix is the same table the reference uses):

| Platform | Slug | Skill subpath (local) | Skill subpath (global, under `$HOME`) |
| -------- | ---- | --------------------- | ------------------------------------- |
| claude    | *(empty)* | `.claude/skills` | `.claude/skills` |
| copilot   | copilot | `.github/skills` | `.copilot/skills` |
| cursor    | cursor | `.cursor/skills` | `.cursor/skills` |
| windsurf  | windsurf | `.windsurf/skills` | `.codeium/windsurf/skills` |
| gemini    | gemini | `.gemini/skills` | `.gemini/skills` |
| opencode  | opencode | `.agents/skills` | `.agents/skills` |
| codex     | codex | `.agents/skills` | `.agents/skills` |
| pi        | pi | `.agents/skills` | `.agents/skills` |
| crush     | crush | `.agents/skills` | `.agents/skills` |
| amp       | amp | `.agents/skills` | `.config/agents/skills` |
| zed       | zed | `.agents/skills` | `.agents/skills` |
| openhands | openhands | `.agents/skills` | `.agents/skills` |
| hermes    | hermes | `.agents/skills` | `.hermes/skills` |
| devin     | devin | `.agents/skills` | `.agents/skills` |

Notes that are **contract**:

- Copilot diverges by scope (`.github` local vs `.copilot` global).
- Windsurf global nests under `.codeium/windsurf`.
- Amp and Hermes diverge **only at global scope**; local is the shared
  `.agents/skills`.
- The nine `.agents`-standard tools share `.agents/skills` — a single physical
  directory. Installing to two of them writes the dir once, both lockfiles.
- A skill installs to a **directory named after the artifact** under the subpath:
  `<subpath>/<name>/` containing `SKILL.md` and any bundled files.

Platform name serializes lowercase and round-trips with the `--platform` token
and the `config.json` `platforms` list.

---

## 5. Checksum algorithm

**This is the highest-stakes parity surface.** A checksum computed by the TS port
must equal the one the Rust reference wrote, or every cross-tool install shows
spurious drift.

Algorithm (`sha256:` + lowercase hex of a SHA-256 over a byte stream):

1. Take the skill's files as `(rel_path, bytes)` pairs.
2. **Filter to canonical files** (§5.1).
3. **Sort by `rel_path`** (byte/lexicographic order of the path string).
4. For each file, in order, feed the hasher: **the rel_path as UTF-8 bytes**,
   then **the file content bytes**. No separators, no length prefixes.
5. Output `"sha256:" + lowercase_hex(digest)`.

The in-memory checksum of a bundle and the on-disk checksum after writing it
**must be identical** — this is what lets `plan` detect drift.

### 5.1 Canonical-file filter

Exclude a file if **any component** of its `rel_path`:

- starts with `.` (dotfiles/dotdirs at any depth — `.env`, `scripts/.hidden`), or
- is a **transient** name: `node_modules`, `__pycache__`, `.git`, `.DS_Store`, or
- has a `.pyc` extension (case-insensitive).

Excluded files are **still written to disk** on install (a normal copy), but are
**never** included in a checksum. Ports must apply the identical filter on both
the write path and the checksum path.

**✅ DECIDED (§11.3) — Path-separator in the hash.** `rel_path` is **normalized
to `/`-separated before hashing** (and before sorting). Rust hashes
`rel_path.to_string_lossy()`, which is already `/`-separated on the macOS/Linux
platforms cmx-core runs; mandating normalization means a Windows port (or a Node
path built with `\`) cannot silently diverge. Ports must replace the OS separator
with `/` on every `rel_path` before it enters the sort or the hash.

**✅ DECIDED (§11.4) — Sort collation.** Sort by the `/`-joined path **string**,
plain byte/lexicographic comparison (not Rust's component-wise `PathBuf` order).
A fixture with files like `a`, `a/b`, `a.b` pins the ordering, since string sort
and component-wise sort differ at the `/`-vs-`.` boundary. Ports sort the
normalized path strings directly.

> **Note for the reference implementation.** Rust currently sorts `PathBuf`
> component-wise (`checksum_dir` sorts `Vec<PathBuf>`; `canonical_files` sorts by
> `rel_path.cmp`). For single-segment and simple nested paths this already
> matches string sort, but the two can diverge (`a.b` vs `a/b`). To make the Rust
> impl the faithful oracle for §11.4, its sort should key on the `/`-joined
> string. Flagged as a follow-up fix in the reference before fixtures are
> generated (§10) — otherwise a generated fixture could encode the wrong order.

---

## 6. Frontmatter version reconciliation

Before checksumming or writing, cmx-core rewrites the bundled `SKILL.md`'s
`metadata.version` to the tool's declared version (the one string the embedder
passes). This keeps the lockfile and the readable frontmatter in lockstep, so
`cmx doctor` / `cmx list` (which parse `metadata.version` back out) report the
right version. The reconciled bytes are what gets checksummed **and** written, so
the algorithm must match across ports byte-for-byte.

Rules (contract — applies only to the file whose rel_path is exactly `SKILL.md`
at the bundle root):

1. If the content does not start with a `---\n` or `---\r\n` frontmatter fence,
   return it **unchanged**.
2. Find the closing fence — the first subsequent line equal to `---` (ignoring a
   trailing `\r`). If none, return unchanged.
3. Within the frontmatter block:
   - **Remove any top-level `version:` line** (a top-level key shadows
     `metadata.version` for community readers).
   - If a top-level `metadata:` block exists, set or insert its `version:` child.
     Insert at the indentation of the block's first child; default to **2 spaces**
     if the block is empty.
   - If no `metadata:` block exists, append (after ensuring a trailing newline):

     ```yaml
     metadata:
       version: "<version>"
     ```

4. The written value is **quoted**: `version: "1.2.0"`.
5. Reconciliation is **idempotent** — running it on already-reconciled content
   yields byte-identical output (so a re-install is a clean `Skip`, not drift).

⚖️ This is intricate string surgery (preserving key order, folded blocks,
comments). It is **contract** and needs a rich fixture set: no-fence, empty
metadata block, existing `metadata.version`, shadowing top-level `version`,
CRLF line endings, 4-space-indented metadata, metadata with a folded
`description:` above `version:`. I propose we generate these from the Rust impl
as the oracle (§9).

---

## 7. Version-guard decision table

Given the bundled version `B`, the lock entry's installed version `I` (may be
absent), whether the skill dir exists on disk, the on-disk checksum vs the source
checksum, and the `force` flag — `plan` assigns each target one action:

**Version comparison** `cmp(I, B)`:

- `I` absent → `Less`.
- Both parse as semver → semver comparison.
- Otherwise → string equality: equal → `Equal`, else `Less`.

**Action** (no lock entry ⇒ always `Install`, whether or not a dir is on disk):

| `cmp` | on disk? | disk == source? | force | Action |
| ----- | -------- | --------------- | ----- | ------ |
| Less    | — | — | — | **Update** (from `I`) |
| Equal   | no | — | — | **Install** |
| Equal   | yes | yes | — | **Skip** |
| Equal   | yes | no | false | **DriftedSkip** (local edits preserved) |
| Equal   | yes | no | true  | **Update** |
| Greater | — | — | false | **RefuseNewer** (blocks the plan) |
| Greater | — | — | true  | **Downgrade** |

- A plan is **blocked** if any target is `RefuseNewer`. `apply` on a blocked plan
  is an error (`force` is required to override).
- `will_write` = `Install | Update | Downgrade`. `Skip | DriftedSkip |
  RefuseNewer` write nothing.
- **`DriftedSkip`** is rendered distinctly ("local edits preserved"), not as a
  silent skip — a UX contract, though the exact wording is ⚖️ detail.

---

## 8. Target resolution and cmx-detection

### 8.1 Which platforms an install targets

Given no explicit `--platform` selector (the companion-skill case):

1. If `config.json` has a **non-empty** `platforms` list → target exactly those
   (filtered to those supporting skills = all of them).
2. Else, target every platform whose lockfile for this scope is **non-empty**
   (i.e. already in use).
3. Else (fresh machine) → target **Claude only**.

**✅ DECIDED (§11.5)** — the asymmetry is **deliberate contract**: `install`
infers "platforms already in use" when unmanaged, whereas `remove` (§8.3) falls
back to **all** platforms. Rationale: install should not spray a skill onto
platforms the user never adopted, but remove should leave nothing behind. Ports
must preserve both behaviors.

### 8.2 cmx-detection and source registration

- **`cmx_managed`** = `config.json` has a non-empty `platforms` list.
- **`cmx_present`** = `cmx_managed` OR any platform lockfile (this scope) is
  non-empty.
- On `apply`, a **cmx source is registered** (an entry `bundled:<name>` added to
  `sources.json`, and the skill materialized under the artifact home
  `<config>/home/skills/<name>/`) **only when `cmx_managed`** is true. On an
  unmanaged machine, files + lock entries are written but no source is
  registered (`source_registered = false`).

This is what makes the skill a first-class tracked artifact on a cmx machine
while still "just working" without cmx.

### 8.3 Remove

`remove` considers the **managed-or-all** platform set (managed set if
configured, else all 14), filtered to skill-supporting. For each: delete the
skill directory if present, and clear the tool's entry from that platform's
lockfile. It **leaves `cmx-lock.json` (the Claude/shared lock file) on disk** even
when emptied — that file is shared with other tools and cmx itself. It also
unregisters the `bundled:<name>` source and removes the materialized home copy if
present.

---

## 9. Plan → apply parity guard

`apply` takes both the bundle and the plan. It **re-reconciles and re-checksums**
the bundle and compares against the plan's `source_checksum`; a mismatch is an
error ("the bundle changed since plan()"). This guarantees `apply` writes exactly
what `plan` previewed. Ports must implement this guard.

`apply` also, under `force`, computes the set of **discarded paths** (files that
differ between the on-disk skill and the bundle) before replacing an
`Update`/`Downgrade` target directory, and reports them so a user sees what
`--force` overwrote.

---

## 10. Conformance fixtures (the deliverable this spec feeds)

Proposed structure — language-neutral, generated from the Rust reference as
oracle (`test-support` feature), re-run by each port:

```text
cmx-core/conformance/
  checksum/        # (files[]) → expected "sha256:…"  — pins §5 incl. filter+sort
  frontmatter/     # (skill_md_in, version) → skill_md_out  — byte-compared, §6
  version-guard/   # (I, B, on_disk?, disk==source?, force) → action  — §7
  paths/           # (platform, kind, scope) → subpath + lockname  — §4, §3.2
  target-resolve/  # (config, existing locks) → [platforms]  — §8.1
  install-e2e/     # (bundle, pre-state tree, pre-lock) → (post tree, post locks, report)
```

Fixture rules:

- **Checksums and SKILL.md**: byte/string exact.
- **Lockfiles**: compared as **parsed values** (whitespace is detail, §11.2) with
  an **injected fixed clock** (§11.1) so `installed_at` is pinned exactly and
  every fixture is fully deterministic. The Rust reference already injects a clock
  in tests; ports must expose the same seam. Package keys sorted.
- **Directory trees**: exact file set + contents.

---

## 11. Decisions (reviewed with Stacey, 2026-07-05)

1. **`installed_at` (§3.1)** — accept any valid RFC 3339 on read; emitted
   precision is implementation detail. Fixtures pin it via an **injected fixed
   clock**, not by masking the field.
2. **Lockfile serialization (§3.3)** — contract is parsed-value equality with
   **sorted package keys**; **whitespace/indentation is implementation detail**.
   Lockfile fixtures compare parsed values, not bytes.
3. **Checksum path separator (§5.1)** — `rel_path` is **normalized to `/`** before
   sorting and hashing. Closes a latent cross-platform (Windows-port) divergence.
4. **Checksum sort collation (§5.1)** — **string sort** of the `/`-joined path,
   plain byte comparison — *not* component-wise. **Follow-up:** the Rust reference
   currently sorts component-wise (`checksum_dir` / `canonical_files`); its sort
   key must move to the joined string before fixtures are generated, so the oracle
   is faithful. Tracked as a code fix, not just a doc note.
5. **install-vs-remove platform asymmetry (§8.1)** — **kept deliberate**: install
   infers platforms-in-use, remove falls back to all.

With these settled, the two remaining build steps are: **(a)** the §11.4 Rust sort
fix, then **(b)** the fixture generator against the Rust oracle, after which the
TS port queues with the friction-report gate (EMBEDDING.md #3).
