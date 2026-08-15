#!/usr/bin/env python3
"""Re-score archived trials against the current checks, without re-running agents.

The agent invocation is the expensive, irreproducible part. Everything after it
— staging a hidden suite, running it, parsing code for intents — is
deterministic and repeatable from the workspace source the archive keeps.

This exists because the scoring has been wrong before and will be again. Seven
corrections so far, and the seventh was the worst kind: the rate-card acceptance
suite used an `unsafe` block, so any crate following
`centralize-curated-lint-policy` and choosing `forbid(unsafe_code)` could not
compile it. Guided trials scored 0/10 on task completion while their code was
perfectly correct, which reads as "the guidance breaks the software" — a false
headline produced entirely by the harness.

Re-scoring recovered the true result from evidence already on disk and cost
nothing. Without the archive it would have cost a day of subscription budget.

    python3 rescore.py --scenario rate-card                 # everything
    python3 rescore.py --scenario rate-card --agent claude-opus-5
    python3 rescore.py --scenario rate-card --dry-run
"""

import argparse
import json
import pathlib
import shutil
import sys
import tarfile
import tempfile

import adherence
import runner

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


def rescore(path, metrics, scenario_root, rustfacts, workdir):
    """Rebuild acceptance and adherence for one trial from its archived source."""
    bundle = path.parent / "evidence.tar.gz"
    if not bundle.is_file():
        return None, "no evidence bundle"

    with tarfile.open(bundle) as archive:
        archive.extractall(workdir, filter="data")
    workspace = workdir / "workspace"
    if not workspace.is_dir():
        return None, "bundle carries no workspace"

    expected = json.loads((scenario_root / "expected.json").read_text(encoding="utf-8"))
    language = expected.get("language", "python")

    check_config = dict(expected.get("check_config", {}))
    check_config["baseline_root"] = str(scenario_root / "input" / "skeleton")
    if rustfacts:
        check_config["rustfacts_binary"] = str(rustfacts)

    report = adherence.score(
        workspace,
        expected["scored_intents"],
        check_config,
        language=language,
        rustfacts=rustfacts,
    )

    # Acceptance is staged after adherence, exactly as a live trial does it.
    if language == "rust":
        staged = workspace / "tests"
        staged.mkdir(exist_ok=True)
        for source in sorted((scenario_root / "acceptance").glob("*.rs")):
            shutil.copy2(source, staged / source.name)
        card = workdir / "rate-card.tsv"
        card.write_text("", encoding="utf-8")
        import os

        outcome = runner.run(
            ["cargo", "test", "--quiet", "--test", "acceptance", "--", "--test-threads=1"],
            cwd=workspace,
            env=os.environ | {"RATECARD_PATH": str(card)},
            timeout=1800,
        )
        acceptance = runner.parse_cargo_test(outcome["stdout"] + outcome["stderr"])
    else:
        directory = workdir / "acceptance"
        shutil.copytree(scenario_root / "acceptance", directory)
        junit = workdir / "acceptance.xml"
        runner.run(["uv", "sync", "--quiet"], cwd=workspace, timeout=1200)
        outcome = runner.run(
            [
                "uv", "run", "--project", str(workspace), "pytest",
                str(directory), "-q", f"--junit-xml={junit}",
            ],
            cwd=directory,
            timeout=1200,
        )
        acceptance = runner.parse_junit(junit)
    acceptance["exit_code"] = outcome["exit_code"]

    updated = dict(metrics)
    updated["acceptance"] = acceptance
    updated["adherence"] = report["adherence"]
    updated["principles"] = report["principles"]
    updated["workspace"] = report["workspace"]
    updated["task_complete"] = (
        acceptance["failed"] == 0
        and acceptance["errors"] == 0
        and acceptance["collected"] == expected["acceptance_check_count"]
    )
    updated["rescored"] = True
    return updated, None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=pathlib.Path, default=HERE / "archive")
    parser.add_argument("--scenario")
    parser.add_argument("--agent")
    parser.add_argument("--dry-run", action="store_true")
    arguments = parser.parse_args()

    trials = list(archived_trials(arguments.archive, arguments.scenario, arguments.agent))
    if not trials:
        raise SystemExit("no archived trials matched")

    scenarios = {name for _, metrics in trials for name in [metrics["scenario"]]}
    rustfacts = None
    if any(
        json.loads((HERE / "scenarios" / name / "expected.json").read_text()).get("language")
        == "rust"
        for name in scenarios
    ):
        rustfacts = runner.build_rustfacts()

    changed = 0
    for path, metrics in trials:
        scenario_root = HERE / "scenarios" / metrics["scenario"]
        with tempfile.TemporaryDirectory() as raw:
            updated, error = rescore(path, metrics, scenario_root, rustfacts, pathlib.Path(raw))
        label = (
            f"{metrics['scenario']}/{metrics['agent']['name']}/{metrics['arm']}"
            f"/trial-{metrics['trial']:02d}"
        )
        if error:
            print(f"  skip {label}: {error}", file=sys.stderr)
            continue

        before = (metrics["acceptance"]["passed"], metrics["adherence"]["followed_count"])
        after = (updated["acceptance"]["passed"], updated["adherence"]["followed_count"])
        moved = "  <-- changed" if before != after else ""
        print(
            f"  {label}: acceptance {before[0]}->{after[0]}, "
            f"adherence {before[1]}->{after[1]}{moved}",
            file=sys.stderr,
        )
        if before != after:
            changed += 1
        if not arguments.dry_run:
            path.write_text(json.dumps(updated, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    verb = "would change" if arguments.dry_run else "changed"
    print(f"\n{len(trials)} trial(s) re-scored, {verb} {changed}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
