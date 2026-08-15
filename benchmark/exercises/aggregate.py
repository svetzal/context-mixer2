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
    python3 aggregate.py --scenario rate-card    # one scenario
    python3 aggregate.py --confidence 0.90
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


def load_trials(results_root, scenario=None):
    """Every completed trial under a results tree, newest layout only."""
    trials = []
    for path in sorted(results_root.rglob("metrics.json")):
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
        metrics["_path"] = str(path)
        trials.append(metrics)
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


def describe_models(trials):
    """What actually answered, which an agent key alone does not tell you."""
    models = {}
    cost = 0.0
    tokens = {"input": 0, "output": 0}
    seconds = 0.0
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
        seconds += metrics.get("agent_run", {}).get("seconds") or 0.0
    return {
        "resolved_models": models,
        "total_cost_usd": round(cost, 4),
        "total_tokens": tokens,
        "total_agent_seconds": round(seconds, 1),
    }


def build(results_root, scenario, confidence):
    z = Z_SCORES[confidence]
    trials = load_trials(results_root, scenario)

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

    return {
        "confidence": confidence,
        "scenarios": scenarios,
        "trials_read": len(trials),
    }


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
            lines.append(f"\n  {agent}  [served: {served}]  cost ${cost:.2f}")
            for arm, data in sorted(entry["arms"].items()):
                rate = data["adherence"]
                lines.append(
                    f"    {arm:<8} n={data['trials']:<3} "
                    f"adherence {rate['rate']} [{rate['low']}, {rate['high']}]  "
                    f"complete {data['task_completion']['rate']}"
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
    parser.add_argument("--results", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parent / "results")
    parser.add_argument("--scenario")
    parser.add_argument("--confidence", type=float, default=0.95, choices=sorted(Z_SCORES))
    parser.add_argument("--out", type=pathlib.Path)
    parser.add_argument("--quiet", action="store_true", help="emit JSON only")
    arguments = parser.parse_args()

    if not arguments.results.is_dir():
        raise SystemExit(f"no results directory at {arguments.results}")

    report = build(arguments.results, arguments.scenario, arguments.confidence)
    destination = arguments.out or arguments.results / "comparison.json"
    destination.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if not arguments.quiet:
        print(render(report), file=sys.stderr)
        print(f"\nread {report['trials_read']} trial(s) -> {destination}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
