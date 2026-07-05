# Sets — Consumer-Side Activation Groups

> Design draft. Status: approved for Phase 1 implementation (2026-07-04).
> Companion to [SPEC.md](SPEC.md) and [CHARTER.md](CHARTER.md).

## Motivation

An installed skill or agent is never free. On Claude Code (and every assistant
that injects skill/agent descriptions), each *installed* artifact's trigger
description is standing context — loaded on every turn whether or not it fires.
A machine curated to fifty skills carries fifty descriptions of fixed cost, most
of them irrelevant to whatever the user is doing right now.

cmx today has one lever for this cost: **install / uninstall** — presence on
disk. That lever is too coarse. Uninstalling to reclaim context means losing the
record that you ever wanted the artifact, and reinstalling means finding it in a
source again. Users end up hoarding installed artifacts they rarely use because
the cost of removing-and-rediscovering exceeds the standing context cost.

**Sets** add a second, cheaper lever: a named group of artifacts the user can
**activate** and **deactivate** together, so the context impact of unrelated
bodies of work can be switched on and off as a unit. "Turn off my client-ORT
skills while I'm doing blog work" becomes one command, fully reversible, with the
grouping remembered.

## Relationship to the existing "plugin" concept

cmx already has a grouping primitive — the **plugin** (`MarketplaceEntry` in
[cmx/src/plugin_types.rs](cmx/src/plugin_types.rs)), a named bundle of `agents`
and `skills` declared in a marketplace's `marketplace.json`. But a plugin is a
**publisher-side / distribution** construct: it groups artifacts *at the source*,
and cmx flattens it on install — the lockfile tracks each artifact individually
and the fact that six skills arrived as one plugin is forgotten.

A **set** is the missing **consumer-side** half:

| | Plugin (`MarketplaceEntry`) | Set (`SetDef`) |
| --- | --- | --- |
| Defined by | Publisher, in `marketplace.json` | User, locally |
| Purpose | Distribution — ship artifacts together | Curation — toggle context together |
| Lifecycle | install / uninstall | **activate / deactivate** |
| Lives in | Source repo | cmx local state (`sets.json`) |
| Composable across sources | No | Yes |

A plugin can **seed** a set (`cmx set create --from <source>:<plugin>`), but sets
are user-composed and span whatever installed artifacts the user chooses,
regardless of which plugin or source they came from.

## Design decisions (locked)

1. **New consumer-side layer.** `set` is its own top-level noun spanning both
   artifact kinds. Not an extension of `plugin`, not folded under `skill`/`agent`.
2. **Deactivate = uninstall, remember membership.** `activate` installs members;
   `deactivate` uninstalls them and retains the set definition. There is no
   on-disk "parking" area.
3. **Local state only.** Sets are personal machine/project state. No publishing
   surface, no `cmf` authoring. The schema is kept clean enough that a future
   publish path *could* be added, but that is explicitly out of scope.

### Why uninstall (not park) dissolves the update-cadence concern

A concern raised during design: if Claude Code's native plugin system updates a
skill on a different cadence than cmx, would a deactivated set go stale?

With the uninstall mechanism, **no**. A parked (on-disk-but-inactive) copy could
drift against an upstream update while it sat idle. An *uninstalled* member has
nothing to drift — `activate` re-installs fresh from the source at whatever
version the source is at *now*. There is no stale intermediate copy to
reconcile. Existing `outdated` / `update` remains the only reconciliation story,
and sets introduce no new conflict into it.

## Data model

Sets live in a dedicated state file, mirroring how `sources.json` and the
lockfile are handled (rather than in the global-only `config.json`), so project
scope is symmetric with global scope:

- **Global:** `~/.config/context-mixer/sets.json`
- **Local:** `.context-mixer/sets.json` (committable, like the local lockfile)

```rust
struct SetsFile {
    version: u32,
    sets: BTreeMap<String, SetDef>,
}

struct SetDef {
    description: Option<String>,
    state: SetState,               // Active | Inactive
    members: Vec<SetMember>,
}

enum SetState { Active, Inactive }

struct SetMember {
    kind: ArtifactKind,            // reuse cmx-core/src/types.rs ArtifactKind
    name: String,
    source: Option<String>,        // source repo pin, snapshotted at add-time
}
```

### The source pin

When a member is added, cmx reads its current lock entry
(`lockfile::find_entry`, [cmx-core/src/lockfile.rs](cmx-core/src/lockfile.rs))
and **snapshots the source repo name** into `SetMember.source`. This is the key
robustness detail: after `deactivate` clears the lock entry, `activate` still
knows exactly where each member came from and re-installs deterministically.
Platform targeting is *not* pinned — a set inherits normal install-target
resolution (`resolve_targets`) at activation time.

### Example `sets.json`

```json
{
  "version": 1,
  "sets": {
    "rust-work": {
      "description": "Rust craftsmanship + foundry",
      "state": "active",
      "members": [
        { "type": "agent", "name": "rust-craftsperson", "source": "guidelines" },
        { "type": "skill", "name": "foundry", "source": "home" }
      ]
    },
    "client-ort": {
      "state": "inactive",
      "members": [
        { "type": "skill", "name": "ubiquity-router", "source": "home" }
      ]
    }
  }
}
```

## Command surface

A new top-level noun, matching cmx's existing noun-verb grammar:

```text
cmx set create <name> [--desc <text>] [--from <source>:<plugin>] [--local]
cmx set list                        # sets, state, member count, context footprint
cmx set show <name>                 # members + per-member source and install status
cmx set add <name> <artifact>...    # snapshot installed artifacts into the set
cmx set remove <name> <artifact>... # drop from set (does NOT uninstall)
cmx set activate <name>   [--dry-run]
cmx set deactivate <name> [--dry-run] [--force]
cmx set delete <name>     [--purge]  # --purge also uninstalls members
cmx set rename <old> <new>
```

- **`--from <source>:<plugin>`** seeds membership from an existing
  `MarketplaceEntry`'s `agents`/`skills` arrays — the one bridge between the
  publisher-side plugin and the consumer-side set.
- **`set add <artifact>`** resolves kind + source from the lockfile for the
  common "group things I already have installed" path. It falls back to
  requiring `skill:name` / `agent:name` or `source:name` disambiguation only
  when the bare name is ambiguous.
- All commands honor `--local` for project scope, consistent with the rest of
  cmx.

## Lifecycle semantics

1. **Reference-counting on shared members.** An artifact may belong to several
   sets. `deactivate A` must **not** uninstall a member that is also held by an
   active set B — it only drops A's claim. The physical uninstall happens only
   when no active set still references the artifact.

2. **Drift guard on deactivate.** Because `deactivate` uninstalls, a member with
   local hand-edits would lose them. Reuse the existing drift detection
   (`gather_install_facts` and the "local edits preserved" skip path in
   cmx-core) to **block-and-warn** on locally-modified members unless `--force`
   is passed — consistent with how `install` already refuses to clobber edits.

3. **Drift is surfaced, not auto-corrected.** If the user manually
   `cmx skill uninstall X` while X is in an active set, cmx does **not** silently
   mutate the set. `set show` and `doctor` report the set as *partially active*.
   No spooky action at a distance; the survey tools tell the truth.

4. **`activate` is idempotent.** Already-installed members are no-ops (existing
   `decide_install` handles this), so `activate` doubles as "repair this set back
   to fully-installed."

## Context-footprint reporting

This is what makes a set more than "uninstall with bookmarks." `set list` and
`set show` report the **context cost** of each set. Every member's `description`
is carried in the `Artifact` struct
([cmx-core/src/types.rs](cmx-core/src/types.rs)), and on Claude Code every
installed skill's description is standing context. So the user can *see* what a
set spends before activating it:

```text
$ cmx set list
NAME            STATE      MEMBERS   FOOTPRINT
rust-work       active     6         ~2.1k chars trigger text
client-ort      inactive   4         ~1.4k chars (not loaded)
blog            inactive   3         ~0.9k chars (not loaded)
```

The footprint is a character (and/or rough token) count of the members' trigger
descriptions — a lightweight, honest proxy for the standing context each set
costs when active.

## doctor integration

Extend the read-only survey ([cmx/src/doctor/](cmx/src/doctor/)) with a
set-consistency check:

- For each **active** set, verify all members are installed.
- For each **inactive** set, verify no members linger installed *solely* on that
  set's behalf.

Report mismatches under the existing issues model, honoring the exit-code-2
contract. Sets become a first-class citizen of cmx's reconciliation story.

## Edge cases and guards

- **Missing source at activation.** If a member's pinned source is no longer
  registered, `activate` reports the member as unresolvable and continues with
  the rest (best-effort, matching `install_many`'s per-target isolation), then
  exits non-zero.
- **Partial activation failure.** One member failing to install does not roll
  back the others; the set is left in whatever state was achieved and `set show`
  reports the gap.
- **Empty set.** `activate`/`deactivate` on an empty set are no-ops that succeed.
- **Deleting an active set.** `delete` without `--purge` removes only the
  definition and leaves members installed (they become ordinary tracked
  artifacts). `--purge` deactivates first, then deletes.

## Phased implementation plan

- **Phase 1 — definitions.** `SetsFile` type + load/save/mutate (clone the
  `sources.json` IO in [cmx-core/src/config/mod.rs](cmx-core/src/config/mod.rs)),
  and `create` / `list` / `show` / `add` / `remove` / `delete` / `rename`. No
  activation yet — useful on its own for curating groupings.
- **Phase 2 — lifecycle.** `activate` / `deactivate` composing the existing
  `install_many` / `uninstall_many`, reference-counting, drift guard,
  `--dry-run` plans.
- **Phase 3 — visibility.** Context-footprint column and the `doctor`
  consistency check.
- **Phase 4 — polish.** `--from <plugin>` seeding, local scope symmetry,
  SPEC.md / CHARTER.md updates, and a refresh of the bundled `cmx` companion
  skill.

The feature is self-contained: no lockfile schema migration, no new platform
work, no changes to the install/uninstall primitives it composes.

## Open decisions (not blocking Phase 1)

- **Noun choice — DECIDED (2026-07-05): `set`.** Considered `profile` / `group`
  / `bundle`; settled on `set`, now shipped in the Phase 1 CLI surface.
- **Footprint units.** Character count is trivial and always available; a rough
  token estimate is friendlier but approximate. Phase 3 can ship chars first and
  add a token estimate later.

---

*Draft started 2026-07-04.*
