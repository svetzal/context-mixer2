# rate-card exercise

A fulfilment crate prices outbound parcels against a rate card that operations
edits and redeploys without a code change. The task is to read that card, pick
the weight band, apply surcharges, and fail in four distinguishable ways.

This is the third exercise and the stress test. The first two shared a language;
this one changes the parser out from under every check to find out which parts of
the design were about *guidance* and which were about Python.

| The task forces | The intent it exposes |
| --- | --- |
| four named failure modes on a fallible read | `use-results-for-recoverable-library-errors` |
| a rate card read from the filesystem | `put-gateways-at-effect-boundaries` |
| pricing that must be testable without a file | `prefer-fakes-at-boundaries`, `isolate-functional-core-from-effects` |
| band arithmetic plus cross-module wiring | `use-purpose-specific-test-layers` |
| a published API surface | `compile-public-documentation` |
| a diagnostic record per quote | `use-structured-tracing` |
| a crate anyone will lint | `centralize-curated-lint-policy` |

## What survived the language change

The four question forms all did. *Is a symbol used* became `rust_calls` and
`rust_macros`; *is it used there* became the `in_test_scope` flag each fact
carries; *what shape is this construct* became item visibility, return types,
and trait/impl pairs; *what did the project declare* became `cargo_manifest`
beside `tool_config`.

So did everything structural: the check signature, the three-state verdict, the
`check_config` split, the reference-and-skeleton calibration discipline, and the
rule that a check parses code rather than asking a model.

## What did not

The traversals, entirely. Python's `ast` has no counterpart reachable from
Python, so `rustfacts/` — a small `syn` binary — emits the facts as JSON and the
checks read that instead of a tree.

Three assumptions baked into the Python checks turned out to be Python's, not
the intents':

- **Test scope is a directory.** In Rust it is an attribute. `#[cfg(test)] mod
  tests` puts unit tests inside the file they exercise, so the
  production/test partition is per *item*, not per module, and every Rust check
  consults `in_test_scope` rather than a path.
- **Panicking is a call.** `panic!`, `todo!`, and `unreachable!` are macros, and
  a checker that only walks call expressions sees none of them.
- **Substituting a collaborator is patching a name.** In Rust it is implementing
  a trait, so the fake is found by matching `impl` items in test scope against
  traits declared in production — a relationship between two definitions rather
  than a string argument to `patch`.

One check has no Python analogue at all. `put-gateways-at-effect-boundaries`
asks where a trait is *declared* relative to where it is *implemented*: the
contract belongs with the core and the concrete gateway belongs at the edge. A
trait declared in the same module that performs the I/O has not moved the
boundary anywhere, and the check says so.

## What the first calibration run corrected

The reference solution failed two checks on its first pass, and both were the
harness's fault.

`compile-public-documentation` counted every `pub mod card;` as an undocumented
public item. Rust puts a module's documentation in the module file as `//!`, not
on the declaration, so the check was demanding something no idiomatic crate
does.

More seriously, the hidden acceptance suite would not compile against the
reference, because the reference declares `unsafe_code = "deny"` and the suite
needs `set_var`. A hidden suite that the agent's own configuration can break is
not a hidden suite — the Python scenarios pin `pytest.ini` for exactly this
reason and the Rust one had no equivalent. It now carries
`#![allow(unsafe_code, unused_unsafe)]`, which an in-source attribute can assert
over a command-line `-D`.

## Known gaps

`use-structured-tracing` recognizes named fields by looking for `=` in the macro
arguments. That is a token-level test, not a parse of the tracing macro grammar,
and a format string containing an `=` would satisfy it.

The acceptance suite runs with `--test-threads=1` because `RATECARD_PATH` is
process-global. A crate that caches the rate card at first use would pass
acceptance while behaving wrongly for a long-lived process; nothing here
detects that.
