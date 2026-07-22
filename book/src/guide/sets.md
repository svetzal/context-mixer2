# Managing Context Cost with Sets

An installed skill or agent is never free. Every assistant that injects
skill/agent descriptions loads each *installed* artifact's trigger description
on every turn, whether or not it fires. A machine curated to fifty skills
carries fifty descriptions of fixed cost, most of them irrelevant to whatever
you're doing right now.

cmx's usual lever for that cost is install/uninstall — presence on disk. That
lever is too coarse for day-to-day use: uninstalling to reclaim context means
losing the record that you ever wanted the artifact, and reinstalling means
finding it in a source again. **Sets** add a second, cheaper lever — a named
group of installed artifacts you can **activate** and **deactivate** together,
so the context cost of one body of work can be switched on and off as a unit
without losing track of what belongs together.

## Creating a set and adding members

```bash
cmx set create rust-work --desc "Rust craftsmanship + foundry"
cmx set add rust-work rust-craftsperson
```

`add` resolves each name's kind and source from the lock file, so the
artifact must already be installed, and it accepts more than one name per
call. If a name is ambiguous across kinds, disambiguate with `skill:` /
`agent:` — e.g. `cmx set add rust-work skill:foundry`.

```bash
cmx set show rust-work
```

```text
Set 'rust-work' (inactive)
  Rust craftsmanship + foundry
  Footprint: ~1.8k chars (not loaded)
  agent rust-craftsperson (source: guidelines) [installed] ~1.8k chars
```

## Activating and deactivating

`activate` and `deactivate` are plan-then-apply, like the safety model
elsewhere in cmx: the first run shows exactly what would change, and nothing
happens until you pass `--apply`.

```bash
cmx set deactivate rust-work           # preview: what would be uninstalled
cmx set deactivate rust-work --apply   # execute it
```

```text
$ cmx set deactivate rust-work
Plan to deactivate set 'rust-work':
  agent rust-craftsperson: uninstall
    ~/.claude/agents/rust-craftsperson.md
Re-run with --apply to make these changes.
```

Deactivating uninstalls every member **not held by another active set** and
marks the set `inactive` — but it does not forget the set's definition.
Reactivating later reinstalls fresh from each member's pinned source, so
there's never a stale, drifted copy sitting around while the set is off:

```bash
cmx set activate rust-work --apply
```

`activate` is idempotent — safe to re-run any time, including as a repair
after `cmx doctor` flags a set with a missing member.

### Sharing a member across sets

If two sets both claim the same artifact, deactivating one leaves it installed
as long as the other is still active — reference counting, not a plain
uninstall. You never have to worry about one set's cleanup pulling an artifact
out from under a different set that still needs it.

### Local edits block deactivation

A member with local edits (the installed copy has drifted from what cmx
tracked) blocks its own uninstall during `deactivate`, so cmx never silently
discards edited content. Pass `--force` to discard the drift and uninstall it
anyway:

```bash
cmx set deactivate rust-work --apply --force
```

## Reading the context-footprint column

`cmx set list` and `cmx set show` report a `footprint_chars` figure — the
total character count of the set's members' trigger descriptions, i.e. the
context weight the set carries while active:

```bash
cmx set list
```

```text
  Name       State     Members  Footprint
  ---------  --------  -------  ---------
  rust-work  inactive  1        ~1.8k chars (not loaded)
```

Use this to compare sets before deciding what to switch off — a set with a
heavy footprint of low-relevance skills is exactly the kind of thing worth
deactivating while you're working on something unrelated.

## Seeding a set from a plugin

A marketplace plugin already groups related agents/skills for distribution.
`--from-plugin` hands you that same grouping as a starting point for a set,
without installing anything:

```bash
cmx set create rust-work --from-plugin guidelines:rust-toolkit
```

From there the set is yours to curate — add or remove members regardless of
which source or plugin they originally came from.

## Worked example: turning off client work while blogging

Say you have a `client-ort` set of skills specific to one client's codebase,
and you're about to spend the afternoon writing a blog post — those skills'
trigger descriptions are pure overhead for that work:

```bash
cmx set deactivate client-ort --apply   # reclaim the context while blogging
# ... write the post ...
cmx set activate client-ort --apply    # back to client work tomorrow
```

Nothing about the client set's membership was lost in between — only its
on-disk presence, and the context cost that came with it.

## Checking consistency

`cmx doctor` cross-references every set's declared membership against what's
actually installed, and flags the same two situations described above under
[its own reference entry](../reference/commands.md#cmx-doctor): an active set
missing a member, or an inactive set with a member still lingering. Run
`cmx doctor` after any manual install/uninstall work to make sure your sets
still agree with reality.
