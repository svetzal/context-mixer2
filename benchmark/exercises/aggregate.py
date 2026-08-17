#!/usr/bin/env python3
"""Turn a pile of trial results into distributions, and compare models.

Reads every `metrics.json` under a results tree and reports, per scenario and
per agent, what fraction of trials exhibited each intent — with an interval, not
just a point. One trial is an anecdote and ten is a small sample; a bare
`0.7000` hides which one you are looking at, so every rate here carries its `n`
and a confidence interval.

Deliberately separate from `runner.py`. Collecting evidence and deciding what it
means are different jobs, and the scoring rules have already changed six times.
Re-analysis must never require re-running an agent.

    python3 aggregate.py                         # everything, all scenarios
    python3 aggregate.py --scenario rate-card    # one scenario, merged into the rest
    python3 aggregate.py --confidence 0.90

A `--scenario` run rewrites only its own scenario in the output file and leaves
the others as they were. Without that, re-analysing one scenario silently
deletes every other scenario's numbers from `comparison.json`, which afterwards
is indistinguishable from those trials never having been run.
"""

import argparse
import json
import math
import pathlib
import sys

# Two-sided normal quantiles. A table rather than an inverse-CDF keeps this
# dependency-free, and these are the only levels anyone asks for.
Z_SCORES = {0.80: 1.2816, 0.90: 1.6449, 0.95: 1.9600, 0.99: 2.5758}


def wilson(successes, total, z):
    """Wilson score interval for a binomial proportion.

    Chosen over the normal approximation because trial counts here are small and
    rates sit near 0 and 1, where the textbook interval produces bounds outside
    [0, 1] and badly wrong coverage.
    """
    if total == 0:
        return {"rate": None, "low": None, "high": None, "successes": 0, "trials": 0}
    rate = successes / total
    denominator = 1 + z * z / total
    center = (rate + z * z / (2 * total)) / denominator
    spread = (z / denominator) * math.sqrt(
        rate * (1 - rate) / total + z * z / (4 * total * total)
    )
    return {
        "rate": round(rate, 4),
        "low": round(max(0.0, center - spread), 4),
        "high": round(min(1.0, center + spread), 4),
        "successes": successes,
        "trials": total,
    }


def newcombe(successes_a, total_a, successes_b, total_b, z):
    """Interval for the difference of two independent proportions.

    Newcombe's hybrid score method, built from the two Wilson intervals. The
    naive difference of two point estimates is what makes a one-trial-per-arm
    "lift" look like a measurement; this says how much of one it is.
    """
    first = wilson(successes_a, total_a, z)
    second = wilson(successes_b, total_b, z)
    if first["rate"] is None or second["rate"] is None:
        return {"estimate": None, "low": None, "high": None, "excludes_zero": False}
    difference = first["rate"] - second["rate"]
    lower = difference - math.sqrt(
        (first["rate"] - first["low"]) ** 2 + (second["high"] - second["rate"]) ** 2
    )
    upper = difference + math.sqrt(
        (first["high"] - first["rate"]) ** 2 + (second["rate"] - second["low"]) ** 2
    )
    lower = max(-1.0, lower)
    upper = min(1.0, upper)
    return {
        "estimate": round(difference, 4),
        "low": round(lower, 4),
        "high": round(upper, 4),
        "excludes_zero": lower > 0 or upper < 0,
    }


def load_trials(roots, scenario=None):
    """Every completed trial across the archive and the working tree.

    The archive is the durable copy and is read first; `results/` may have been
    pruned, cleared, or never existed on this machine. A trial appearing in both
    is counted once, keyed by where it came from rather than by file path.
    """
    trials = []
    invalid = []
    seen = set()
    paths = []
    for root in roots:
        if root and root.is_dir():
            paths.extend(sorted(root.rglob("metrics.json")))
    for path in paths:
        try:
            metrics = json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            continue
        if "principles" not in metrics or "arm" not in metrics:
            continue
        if scenario and metrics.get("scenario") != scenario:
            continue
        # Calibration runs prove the instrument has range; they are not
        # observations of an agent and must never enter a rate. Older results
        # predate the `kind` field, so fall back to how they were produced.
        kind = metrics.get("kind")
        if kind is None:
            kind = (
                "calibration"
                if metrics.get("implementation") or metrics.get("agent_run", {}).get("skipped")
                else "agent"
            )
        if kind != "agent":
            continue
        # An invocation that failed did not measure the model. Counted and
        # reported, never averaged.
        if metrics.get("valid") is False:
            invalid.append(metrics)
            continue
        identity = (
            metrics.get("scenario"),
            metrics.get("agent", {}).get("name"),
            metrics["arm"],
            metrics.get("trial"),
        )
        if identity in seen:
            continue
        seen.add(identity)
        metrics["_path"] = str(path)
        trials.append(metrics)
    if invalid:
        print(
            f"warning: {len(invalid)} trial(s) excluded — the agent invocation failed",
            file=sys.stderr,
        )
    return trials


def tally(trials):
    """Successes and applicable counts per intent, plus task completion."""
    followed = {}
    applicable = {}
    complete = 0
    for metrics in trials:
        complete += int(bool(metrics.get("task_complete")))
        for key, item in metrics["principles"].items():
            if item.get("applicable", True):
                applicable[key] = applicable.get(key, 0) + 1
                followed[key] = followed.get(key, 0) + int(bool(item["followed"]))
            else:
                applicable.setdefault(key, 0)
                followed.setdefault(key, 0)
    return followed, applicable, complete


def describe_arm(trials, z):
    followed, applicable, complete = tally(trials)
    total_followed = sum(followed.values())
    total_applicable = sum(applicable.values())
    return {
        "trials": len(trials),
        "task_completion": wilson(complete, len(trials), z),
        # Pooled across intents. Useful as a headline, misleading on its own —
        # per-intent is where the signal lives.
        "adherence": wilson(total_followed, total_applicable, z),
        "by_intent": {
            key: wilson(followed[key], applicable[key], z) for key in sorted(applicable)
        },
        "not_applicable_trials": {
            key: len(trials) - count for key, count in sorted(applicable.items()) if count < len(trials)
        },
    }


def describe_lift(guided, control, z):
    guided_followed, guided_applicable, guided_complete = tally(guided)
    control_followed, control_applicable, control_complete = tally(control)

    by_intent = {}
    for key in sorted(set(guided_applicable) | set(control_applicable)):
        by_intent[key] = newcombe(
            guided_followed.get(key, 0),
            guided_applicable.get(key, 0),
            control_followed.get(key, 0),
            control_applicable.get(key, 0),
            z,
        )
    return {
        "task_completion": newcombe(
            guided_complete, len(guided), control_complete, len(control), z
        ),
        "adherence": newcombe(
            sum(guided_followed.values()),
            sum(guided_applicable.values()),
            sum(control_followed.values()),
            sum(control_applicable.values()),
            z,
        ),
        "by_intent": by_intent,
        "intents_with_evidence_of_lift": sorted(
            key for key, item in by_intent.items() if item["excludes_zero"] and (item["estimate"] or 0) > 0
        ),
    }


def median(values):
    ordered = sorted(values)
    if not ordered:
        return None
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def describe_models(trials):
    """What actually answered, and what it cost in money and in time."""
    models = {}
    cost = 0.0
    tokens = {"input": 0, "output": 0}
    seconds = 0.0
    durations = []
    for metrics in trials:
        telemetry = metrics.get("agent_run", {}).get("telemetry") or {}
        for name, usage in (telemetry.get("models") or {}).items():
            entry = models.setdefault(name, {"trials": 0, "output_tokens": 0, "cost_usd": 0.0})
            entry["trials"] += 1
            entry["output_tokens"] += usage.get("output_tokens", 0)
            entry["cost_usd"] = round(entry["cost_usd"] + usage.get("cost_usd", 0.0), 4)
        cost += telemetry.get("cost_usd") or 0.0
        tokens["input"] += telemetry.get("input_tokens") or 0
        tokens["output"] += telemetry.get("output_tokens") or 0
        elapsed = metrics.get("agent_run", {}).get("seconds") or 0.0
        seconds += elapsed
        if elapsed:
            durations.append(elapsed)
    return {
        "resolved_models": models,
        "total_cost_usd": round(cost, 4),
        "total_tokens": tokens,
        "total_agent_seconds": round(seconds, 1),
        # Median rather than mean: a single cold start or a retry skews the
        # average, and the typical trial is what a sweep should be planned on.
        "median_agent_seconds": round(median(durations), 1) if durations else None,
        "slowest_agent_seconds": round(max(durations), 1) if durations else None,
    }


def build(roots, scenario, confidence):
    z = Z_SCORES[confidence]
    trials = load_trials(roots, scenario)

    grouped = {}
    for metrics in trials:
        agent = metrics.get("agent", {}).get("name", "unknown")
        key = (metrics.get("scenario", "unknown"), agent, metrics["arm"])
        grouped.setdefault(key, []).append(metrics)

    scenarios = {}
    for (scenario_name, agent, arm), arm_trials in sorted(grouped.items()):
        agents = scenarios.setdefault(scenario_name, {"agents": {}})["agents"]
        entry = agents.setdefault(agent, {"arms": {}, "telemetry": {}})
        entry["arms"][arm] = describe_arm(arm_trials, z)
        entry["telemetry"][arm] = describe_models(arm_trials)

    for scenario_name, block in scenarios.items():
        for agent, entry in block["agents"].items():
            guided = grouped.get((scenario_name, agent, "guided"), [])
            control = grouped.get((scenario_name, agent, "control"), [])
            if guided and control:
                entry["lift"] = describe_lift(guided, control, z)

    for scenario_name, block in scenarios.items():
        block["trials_read"] = sum(
            1 for metrics in trials if metrics.get("scenario", "unknown") == scenario_name
        )

    return {
        "confidence": confidence,
        "scenarios": scenarios,
        "trials_read": len(trials),
    }


def fold_into_existing(destination, report):
    """Merge a scoped run into whatever the destination already holds.

    `--scenario` computes one scenario. Writing that result straight out drops
    every other scenario from the file, and the loss is silent: the next reader
    sees a comparison.json with no fx-settlement in it and cannot tell whether
    those trials were never run or were overwritten by a later rate-card sweep.

    Returns the payload to write and a note for the operator, empty when there
    was nothing to preserve.
    """
    if not destination.exists():
        return report, ""

    try:
        existing = json.loads(destination.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return report, "existing file was unreadable and has been replaced"

    # Intervals computed at different levels must not sit in one file claiming a
    # single `confidence`. The new run wins rather than producing a mixed report.
    if existing.get("confidence") != report["confidence"]:
        return report, (
            f"existing file was written at confidence {existing.get('confidence')}, "
            f"not {report['confidence']} — replaced rather than mixing interval levels"
        )

    kept = {
        name: block
        for name, block in existing.get("scenarios", {}).items()
        if name not in report["scenarios"]
    }
    if not kept:
        return report, ""

    merged = dict(existing)
    merged["scenarios"] = {**kept, **report["scenarios"]}
    merged["trials_read"] = sum(
        block.get("trials_read", 0) for block in merged["scenarios"].values()
    )
    return merged, f"kept {len(kept)} scenario(s) already in the file: {', '.join(sorted(kept))}"


def render(report):
    """A table a person can read, alongside the JSON a script can."""
    lines = []
    level = int(report["confidence"] * 100)
    for scenario_name, block in sorted(report["scenarios"].items()):
        lines.append(f"\n=== {scenario_name} ===")
        for agent, entry in sorted(block["agents"].items()):
            models = set()
            for arm in entry["telemetry"].values():
                models.update(arm["resolved_models"])
            served = ", ".join(sorted(models)) or "not recorded"
            cost = sum(arm["total_cost_usd"] for arm in entry["telemetry"].values())
            wall = sum(arm["total_agent_seconds"] for arm in entry["telemetry"].values())
            lines.append(
                f"\n  {agent}  [served: {served}]  cost ${cost:.2f}  "
                f"agent time {wall / 3600:.1f}h"
            )
            for arm, data in sorted(entry["arms"].items()):
                rate = data["adherence"]
                timing = entry["telemetry"].get(arm, {})
                pace = timing.get("median_agent_seconds")
                pace_text = f"  {pace:.0f}s/trial" if pace else ""
                lines.append(
                    f"    {arm:<8} n={data['trials']:<3} "
                    f"adherence {rate['rate']} [{rate['low']}, {rate['high']}]  "
                    f"complete {data['task_completion']['rate']}{pace_text}"
                )
            lift = entry.get("lift")
            if not lift:
                continue
            overall = lift["adherence"]
            verdict = "excludes 0" if overall["excludes_zero"] else "includes 0 — not distinguishable"
            lines.append(
                f"    lift     {overall['estimate']} "
                f"[{overall['low']}, {overall['high']}] at {level}% — {verdict}"
            )
            moved = lift["intents_with_evidence_of_lift"]
            lines.append(f"    intents with evidence of lift: {len(moved)}")
            for key in moved:
                item = lift["by_intent"][key]
                lines.append(
                    f"      + {key.split('/')[-1]:<45} "
                    f"{item['estimate']} [{item['low']}, {item['high']}]"
                )
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    here = pathlib.Path(__file__).resolve().parent
    parser.add_argument("--archive", type=pathlib.Path, default=here / "archive")
    parser.add_argument("--results", type=pathlib.Path, default=here / "results")
    parser.add_argument("--scenario")
    parser.add_argument("--confidence", type=float, default=0.95, choices=sorted(Z_SCORES))
    parser.add_argument("--out", type=pathlib.Path)
    parser.add_argument("--quiet", action="store_true", help="emit JSON only")
    arguments = parser.parse_args()

    roots = [arguments.archive, arguments.results]
    if not any(root.is_dir() for root in roots):
        raise SystemExit(f"nothing to read at {arguments.archive} or {arguments.results}")

    report = build(roots, arguments.scenario, arguments.confidence)
    destination = arguments.out or arguments.archive / "comparison.json"
    destination.parent.mkdir(parents=True, exist_ok=True)

    payload, note = (report, "")
    if arguments.scenario:
        payload, note = fold_into_existing(destination, report)
    destination.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if not arguments.quiet:
        print(render(report), file=sys.stderr)
        print(f"\nread {report['trials_read']} trial(s) -> {destination}", file=sys.stderr)
        if note:
            print(f"  {note}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
