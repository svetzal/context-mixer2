#!/usr/bin/env python3
"""Re-score archived trials with cmv against the current atlas, without re-running agents.

The agent invocation is the expensive, irreproducible part. Everything after it
— staging the manifest and `cmv.toml`, running the validators over the code the
agent left — is deterministic and repeatable from the workspace source the
archive keeps.

This exists because the scoring has been wrong before and will be again. Seven
corrections so far, and the seventh was the worst kind: the rate-card acceptance
suite used an `unsafe` block, so any crate following
`centralize-curated-lint-policy` and choosing `forbid(unsafe_code)` could not
compile it. Guided trials scored 0/10 on task completion while their code was
perfectly correct, which reads as "the guidance breaks the software" — a false
headline produced entirely by the harness.

Re-scoring recovered the true result from evidence already on disk and cost
nothing. Without the archive it would have cost a day of subscription budget.

Now that the validators live in the atlas, the same recovery is a validator fix
there followed by this. Each rewritten `metrics.json` records the atlas
revision it was scored at, so a correction stays distinguishable from the
evidence it re-scored.

    python3 rescore.py --scenario rate-card --atlas ~/Work/Projects/Personal/guidelines
    python3 rescore.py --scenario rate-card --agent claude-opus-5 --dry-run
    python3 rescore.py --scenario rate-card --compare   # write nothing; show verdict deltas
"""

import argparse
import json
import pathlib
import sys
import tarfile
import tempfile

import scoring

HERE = pathlib.Path(__file__).resolve().parent


def archived_trials(archive, scenario=None, agent=None):
    for path in sorted(archive.rglob("metrics.json")):
        metrics = json.loads(path.read_text(encoding="utf-8"))
        if scenario and metrics.get("scenario") != scenario:
            continue
        if agent and metrics.get("agent", {}).get("name") != agent:
            continue
        if metrics.get("kind") == "calibration" or metrics.get("valid") is False:
            continue
        yield path, metrics


def unstage_acceptance(workspace, scenario_root):
    """Remove the hidden suite a live trial copied into a Rust crate before archiving.

    A live trial scores adherence first and stages `tests/acceptance.rs`
    afterwards, because a Rust integration test has to live inside the crate to
    run and would otherwise count as the agent's own test layer. The archive was
    written after both, so the staged file is in it. Scoring the unpacked
    workspace as it stands would credit the harness's file to the agent; taking
    it out first restores the state the original verdict saw.
    """
    removed = []
    for source in sorted((scenario_root / "acceptance").glob("*.rs")):
        staged = workspace / "tests" / source.name
        if staged.is_file():
            staged.unlink()
            removed.append(str(staged.relative_to(workspace)))
    return removed


def rescore(path, metrics, scenario_root, expected, assembled, atlas, tools, workdir):
    """Rebuild the adherence block for one trial from its archived source."""
    bundle = path.parent / "evidence.tar.gz"
    if not bundle.is_file():
        return None, "no evidence bundle"

    with tarfile.open(bundle) as archive:
        archive.extractall(workdir, filter="data")
    workspace = workdir / "workspace"
    if not workspace.is_dir():
        return None, "bundle carries no workspace"

    unstage_acceptance(workspace, scenario_root)
    scoring.stage_verification(workspace, assembled["manifest_path"], scenario_root, expected)
    cmv_run, report = scoring.check(workspace, atlas, tools)
    scored = scoring.metrics_from_report(report, expected["scored_intents"], cmv_run["exit_code"])
    if scored["harness_faults"]:
        return None, "cmv reached no verdict: " + "; ".join(scored["harness_faults"])

    updated = dict(metrics)
    updated["adherence"] = scored["adherence"]
    updated["principles"] = scored["principles"]
    updated["harness_faults"] = scored["harness_faults"]
    updated["atlas_revision"] = assembled["manifest"]["atlas"].get("revision")
    updated["atlas_moved"] = report["atlas"]["moved"]
    updated["cmv_exit_code"] = cmv_run["exit_code"]
    updated["cmv_report"] = report
    # The old scorer's module partition; nothing reads it and cmv does not
    # produce one.
    updated.pop("workspace", None)
    updated["rescored"] = True
    return updated, None


def verdict_deltas(before, after):
    """Intents whose (applicable, followed) pair moved between two principle blocks."""
    old, new = scoring.verdicts(before), scoring.verdicts(after)
    return {
        key: (old.get(key), new.get(key))
        for key in sorted(set(old) | set(new))
        if old.get(key) != new.get(key)
    }


def describe(pair):
    if pair is None:
        return "absent"
    applicable, followed = pair
    if not applicable:
        return "n/a"
    return "followed" if followed else "violated"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=pathlib.Path, default=HERE / "archive")
    parser.add_argument("--scenario")
    parser.add_argument("--agent")
    parser.add_argument(
        "--atlas",
        help="intent atlas root to assemble and verify against; defaults to $CMF_ATLAS",
    )
    parser.add_argument("--dry-run", action="store_true", help="score, report counts, write nothing")
    parser.add_argument(
        "--compare",
        action="store_true",
        help="score, write nothing, and list per trial which intents changed verdict",
    )
    arguments = parser.parse_args()

    trials = list(archived_trials(arguments.archive, arguments.scenario, arguments.agent))
    if not trials:
        raise SystemExit("no archived trials matched")

    atlas = scoring.atlas_from(arguments.atlas)
    tools = scoring.build_tools()

    # One assembly per scenario: the manifest depends only on the profile and
    # the atlas, never on the trial.
    scenarios = {}
    for _, metrics in trials:
        name = metrics["scenario"]
        if name in scenarios:
            continue
        root = HERE / "scenarios" / name
        expected = json.loads((root / "expected.json").read_text(encoding="utf-8"))
        scoring.snapshot_guard(root, atlas, expected["scored_intents"])
        scratch = pathlib.Path(tempfile.mkdtemp(prefix=f"rescore-{name}-"))
        scenarios[name] = {
            "root": root,
            "expected": expected,
            "assembled": scoring.assemble(root, atlas, tools, scratch / "cmf-manifest.json"),
        }

    changed = 0
    trials_with_deltas = 0
    per_intent = {}
    for path, metrics in trials:
        scenario = scenarios[metrics["scenario"]]
        with tempfile.TemporaryDirectory() as raw:
            updated, error = rescore(
                path,
                metrics,
                scenario["root"],
                scenario["expected"],
                scenario["assembled"],
                atlas,
                tools,
                pathlib.Path(raw),
            )
        label = (
            f"{metrics['scenario']}/{metrics['agent']['name']}/{metrics['arm']}"
            f"/trial-{metrics['trial']:02d}"
        )
        if error:
            print(f"  skip {label}: {error}", file=sys.stderr)
            continue

        before = metrics["adherence"]["followed_count"]
        after = updated["adherence"]["followed_count"]
        deltas = verdict_deltas(metrics.get("principles", {}), updated["principles"])
        moved = f"  <-- {len(deltas)} verdict delta(s)" if deltas else ""
        print(f"  {label}: adherence {before}->{after}{moved}", file=sys.stderr)
        if deltas:
            trials_with_deltas += 1
            for key, (old, new) in deltas.items():
                per_intent[key] = per_intent.get(key, 0) + 1
                if arguments.compare:
                    print(f"      {key}: {describe(old)} -> {describe(new)}", file=sys.stderr)
        if before != after:
            changed += 1
        if not arguments.dry_run and not arguments.compare:
            path.write_text(json.dumps(updated, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if arguments.compare:
        print(
            f"\n{len(trials)} trial(s) compared, {trials_with_deltas} with verdict deltas; "
            "nothing written",
            file=sys.stderr,
        )
        for key, count in sorted(per_intent.items()):
            print(f"  {key}: {count} trial(s)", file=sys.stderr)
    else:
        verb = "would change" if arguments.dry_run else "changed"
        print(f"\n{len(trials)} trial(s) re-scored, {verb} {changed} adherence count(s)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
