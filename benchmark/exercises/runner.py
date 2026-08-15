#!/usr/bin/env python3
"""Run a behavioural exercise and measure whether assembled guidance changed the work.

The harness takes three inputs:

1. agent and model parameters, named from `agents.toml`
2. a scenario skeleton — a real project and a task written against a fixed
   public contract
3. an AGENTS.md that cmf assembled from a slice of the intent corpus

It produces two things the assembly benchmark cannot: whether the finished code
satisfies a hidden acceptance suite, and whether it exhibits each named intent.
Running the same scenario with and without the guidance is what turns those into
a measurement of the guidance rather than of the model.

This module only *collects*. Turning trials into rates, intervals, and
model-to-model comparisons is `aggregate.py`'s job, so re-analysis never needs a
re-run — which matters, because the scoring rules change more often than the
evidence does.

    ./run.sh --scenario rate-card --agent claude-opus-5 --trials 10 --concurrency 4
    ./run.sh --scenario rate-card --agent codex-gpt-5-6 --trials 10
    python3 aggregate.py --scenario rate-card
"""

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import threading
import time
import tomllib
import xml.etree.ElementTree as ElementTree
from concurrent.futures import ThreadPoolExecutor, as_completed

import adherence

HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent
RUSTFACTS = HERE / "rustfacts"

PROMPT = """Read TASK.md in this directory and implement it.

Work directly in this project. When you are finished the task's definition of
done must hold. Do not ask for confirmation; complete the work."""

PRINT_LOCK = threading.Lock()


def load_agents():
    return tomllib.loads((HERE / "agents.toml").read_text(encoding="utf-8"))["agents"]


def run(command, cwd, env=None, timeout=None):
    started = time.monotonic()
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
            stdin=subprocess.DEVNULL,
            check=False,
        )
        return {
            "command": list(command),
            "exit_code": completed.returncode,
            "seconds": round(time.monotonic() - started, 2),
            "stdout": completed.stdout,
            "stderr": completed.stderr,
            "timed_out": False,
        }
    except subprocess.TimeoutExpired as expired:
        return {
            "command": list(command),
            "exit_code": None,
            "seconds": round(time.monotonic() - started, 2),
            "stdout": expired.stdout or "",
            "stderr": expired.stderr or "",
            "timed_out": True,
        }


# --------------------------------------------------------------------------
# Telemetry: what actually answered
# --------------------------------------------------------------------------


def flag_value(command, *names):
    for index, part in enumerate(command[:-1]):
        if part in names:
            return command[index + 1]
    return None


def parse_telemetry(agent, command, stdout):
    """Resolved model, tokens, and cost, so a comparison names what it compared.

    An agent key is not a model. `--model sonnet` is an alias whose target moves,
    and a session can silently route part of its work to another model. Recording
    the argv is not enough; this reads what the CLI reports it actually used.
    """
    kind = agent.get("telemetry")
    if kind == "claude-json":
        try:
            payload = json.loads(stdout)
        except json.JSONDecodeError:
            return {"parsed": False, "reason": "stdout was not the expected JSON"}
        usage = payload.get("usage") or {}
        models = {
            name: {
                "input_tokens": item.get("inputTokens", 0),
                "output_tokens": item.get("outputTokens", 0),
                "cost_usd": item.get("costUSD", 0.0),
            }
            for name, item in (payload.get("modelUsage") or {}).items()
        }
        primary = max(models, key=lambda name: models[name]["output_tokens"], default=None)
        return {
            "parsed": True,
            "primary_model": primary,
            "models": models,
            "cost_usd": payload.get("total_cost_usd"),
            "input_tokens": usage.get("input_tokens"),
            "output_tokens": usage.get("output_tokens"),
            "cache_read_tokens": usage.get("cache_read_input_tokens"),
            "duration_ms": payload.get("duration_ms"),
            "turns": payload.get("num_turns"),
            "stop_reason": payload.get("stop_reason"),
        }

    if kind == "codex-jsonl":
        # Codex reports usage but not the model or a cost, so the model is the
        # one we asked for and the cost is unknown rather than zero.
        requested = flag_value(command, "--model", "-m")
        usage = {}
        for line in stdout.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") == "turn.completed" and "usage" in event:
                usage = event["usage"]
        return {
            "parsed": bool(usage),
            "primary_model": requested,
            "models": {requested: {"output_tokens": usage.get("output_tokens", 0), "cost_usd": 0.0}}
            if requested
            else {},
            "cost_usd": None,
            "input_tokens": usage.get("input_tokens"),
            "output_tokens": usage.get("output_tokens"),
            "cache_read_tokens": usage.get("cached_input_tokens"),
        }

    if kind == "opencode-jsonl":
        requested = flag_value(command, "--model", "-m")
        totals = {"input": 0, "output": 0, "reasoning": 0, "cache_read": 0}
        cost = 0.0
        steps = 0
        for line in stdout.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") != "step_finish":
                continue
            part = event.get("part") or {}
            tokens = part.get("tokens") or {}
            steps += 1
            totals["input"] += tokens.get("input", 0)
            totals["output"] += tokens.get("output", 0)
            totals["reasoning"] += tokens.get("reasoning", 0)
            totals["cache_read"] += (tokens.get("cache") or {}).get("read", 0)
            cost += part.get("cost") or 0.0
        return {
            "parsed": steps > 0,
            "primary_model": requested,
            "models": {requested: {"output_tokens": totals["output"], "cost_usd": cost}}
            if requested
            else {},
            # Local inference reports zero, which is true at the margin and not
            # the same as the unknown a CLI that reports nothing would give.
            "cost_usd": cost,
            "input_tokens": totals["input"],
            "output_tokens": totals["output"],
            "reasoning_tokens": totals["reasoning"],
            "cache_read_tokens": totals["cache_read"],
            "steps": steps,
        }

    return {"parsed": False, "reason": "agent declares no telemetry format"}


# --------------------------------------------------------------------------
# Inputs
# --------------------------------------------------------------------------


def build_rustfacts():
    """Build the syn-based fact extractor the Rust checks read from."""
    binary = RUSTFACTS / "target" / "release" / "rustfacts"
    outcome = run(
        ["cargo", "build", "--release", "--quiet", "--manifest-path", str(RUSTFACTS / "Cargo.toml")],
        cwd=RUSTFACTS,
        timeout=900,
    )
    if outcome["exit_code"] != 0:
        raise SystemExit(f"could not build rustfacts:\n{outcome['stderr']}")
    return binary


def assemble_guidance(scenario):
    """Input three, assembled once per invocation rather than once per trial.

    The artifact is deterministic for a scenario, and running cargo inside the
    repo contends on one target-directory lock — so N concurrent trials
    assembling the same bytes would serialize on it for no reason.
    """
    outcome = run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(REPO_ROOT / "Cargo.toml"),
            "-p",
            "cmf",
            "--",
            "--root",
            str(scenario / "input" / "knowledge-base"),
            "assemble",
            str(scenario / "input" / "profile.toml"),
            "--explain",
        ],
        cwd=REPO_ROOT,
        timeout=900,
    )
    if outcome["exit_code"] != 0:
        raise SystemExit(f"cmf assemble failed:\n{outcome['stderr']}")
    return outcome["stdout"], outcome["stderr"]


def warm_up(agent, agent_name):
    """Send one throwaway request so trial 1 is not paying a cold model load.

    A local 18-81 GB model takes minutes to load the first time. Without this the
    first trial's duration measures the loader, and a per-trial timeout sized for
    inference will fire before the weights are even resident.
    """
    command = [
        part.replace("{prompt}", "Reply with exactly: OK").replace("{workspace}", str(HERE))
        for part in agent["command"]
    ]
    declared = {
        key: value.replace("{here}", str(HERE)).replace("{workspace}", str(HERE))
        for key, value in agent.get("env", {}).items()
    }
    announce(f"warming {agent_name} (a cold local model can take minutes)")
    outcome = run(command, cwd=HERE, env=os.environ | declared, timeout=1800)
    announce(f"warm-up finished in {outcome['seconds']}s (exit {outcome['exit_code']})")


def selected_from_explain(explanation):
    keys = []
    capturing = False
    for line in explanation.splitlines():
        if line.startswith("selected intents ("):
            capturing = True
            continue
        if capturing:
            if not line.startswith("  "):
                break
            keys.append(line.strip())
    return keys


def isolation_for(agent, run_directory, isolate_home):
    """Describe, and where asked for, enforce separation from ambient config.

    The default deliberately leaves the operator's own installed configuration
    in place. It is identical across both arms, so it cannot manufacture a
    difference between them — it can only raise the control arm's floor and
    understate the lift. `--isolate-agent-home` removes it entirely by pointing
    the agent's configuration home at an empty scratch directory, at the cost of
    whatever credentials live there rather than in the system keychain.
    """
    notes = list(agent.get("isolation_notes", []))
    variable = agent.get("home_env")
    if not isolate_home or not variable:
        notes.append(
            "ambient user configuration was NOT isolated; it is constant across arms "
            "and can only understate guided-arm lift"
        )
        return {}, notes

    scratch = run_directory / "agent-home"
    scratch.mkdir(parents=True, exist_ok=True)
    notes.append(f"{variable} points at an empty scratch home; no ambient config or memory loads")
    return {variable: str(scratch)}, notes


# --------------------------------------------------------------------------
# Test result parsing
# --------------------------------------------------------------------------


def parse_junit(path):
    if not path.is_file():
        return {"collected": 0, "passed": 0, "failed": 0, "errors": 0, "skipped": 0, "failures": []}
    tree = ElementTree.parse(path)
    cases = list(tree.iter("testcase"))
    failures = []
    failed = errors = skipped = 0
    for case in cases:
        name = case.get("name", "?")
        if case.find("failure") is not None:
            failed += 1
            failures.append({"name": name, "kind": "failure"})
        elif case.find("error") is not None:
            errors += 1
            failures.append({"name": name, "kind": "error"})
        elif case.find("skipped") is not None:
            skipped += 1
    return {
        "collected": len(cases),
        "passed": len(cases) - failed - errors - skipped,
        "failed": failed,
        "errors": errors,
        "skipped": skipped,
        "failures": failures,
    }


def parse_cargo_test(output):
    """Read libtest's summary lines, which are the only machine-stable output.

    `cargo test` has no JUnit writer on stable, and one invocation prints one
    summary per test binary, so the counts are summed rather than taken from the
    last line.
    """
    totals = {"collected": 0, "passed": 0, "failed": 0, "errors": 0, "skipped": 0, "failures": []}
    for match in re.finditer(
        r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored", output
    ):
        passed, failed, ignored = (int(group) for group in match.groups())
        totals["passed"] += passed
        totals["failed"] += failed
        totals["skipped"] += ignored
        totals["collected"] += passed + failed + ignored
    totals["failures"] = [
        {"name": name.strip(), "kind": "failure"}
        for name in re.findall(r"^\s{4}(\S+)$", output, re.MULTILINE)
    ]
    return totals


# --------------------------------------------------------------------------
# One trial
# --------------------------------------------------------------------------


def run_trial(job):
    """Materialize a workspace, let the agent work, then score it."""
    scenario = job["scenario"]
    run_directory = job["run_directory"]
    if run_directory.exists():
        shutil.rmtree(run_directory)
    run_directory.mkdir(parents=True)

    workspace = run_directory / "workspace"
    source = scenario / job["implementation"] if job["implementation"] else scenario / "input" / "skeleton"
    shutil.copytree(source, workspace)
    shutil.copy2(scenario / "TASK.md", workspace / "TASK.md")

    agent = job["agent"]
    guidance = {"present": False}
    if job["arm"] == "guided":
        content, explanation = job["guidance"]
        for name in agent.get("guidance_files", ["AGENTS.md"]):
            (workspace / name).write_text(content, encoding="utf-8")
        (run_directory / "guidance.md").write_text(content, encoding="utf-8")
        (run_directory / "explain.txt").write_text(explanation, encoding="utf-8")
        guidance = {
            "present": True,
            "files": agent.get("guidance_files", ["AGENTS.md"]),
            "bytes": len(content.encode("utf-8")),
            "approximate_tokens": -(-len(content) // 4),
            "selected_intents": selected_from_explain(explanation),
        }

    home_env, isolation = isolation_for(agent, run_directory, job["isolate_home"])
    declared = {
        key: value.replace("{here}", str(HERE)).replace("{workspace}", str(workspace))
        for key, value in agent.get("env", {}).items()
    }
    environment = os.environ | home_env | declared

    if job["kind"] == "calibration":
        agent_run = {"skipped": True}
    else:
        command = [
            part.replace("{prompt}", PROMPT).replace("{workspace}", str(workspace))
            for part in agent["command"]
        ]
        agent_run = run(command, cwd=workspace, env=environment, timeout=job["timeout"])
        stdout = agent_run.pop("stdout")
        agent_run["telemetry"] = parse_telemetry(agent, command, stdout)
        (run_directory / "agent-stdout.txt").write_text(stdout, encoding="utf-8")
        (run_directory / "agent-stderr.txt").write_text(agent_run.pop("stderr"), encoding="utf-8")

    expected = job["expected"]
    language = expected.get("language", "python")
    rustfacts = job["rustfacts"]

    extra = {}
    if language == "rust":
        sync = run(["cargo", "fetch", "--quiet"], cwd=workspace, timeout=1800)
        own = run(["cargo", "test", "--quiet"], cwd=workspace, timeout=1800)
        own_results = parse_cargo_test(own["stdout"] + own["stderr"]) | {"exit_code": own["exit_code"]}
        documentation = run(["cargo", "doc", "--no-deps", "--quiet"], cwd=workspace, timeout=1800)
        extra["documentation_build"] = {"exit_code": documentation["exit_code"]}
    else:
        sync = run(["uv", "sync", "--quiet"], cwd=workspace, timeout=1200)
        own_junit = run_directory / "own-tests.xml"
        own = run(
            ["uv", "run", "--project", str(workspace), "pytest", "-q", f"--junit-xml={own_junit}"],
            cwd=workspace,
            timeout=1200,
        )
        own_results = parse_junit(own_junit) | {"exit_code": own["exit_code"]}

    # Adherence is scored before acceptance is staged. A Rust integration test
    # has to live inside the crate to run at all, so copying the hidden suite in
    # first would let it count as the agent's own test layer.
    check_config = dict(expected.get("check_config", {}))
    check_config["baseline_root"] = str(scenario / "input" / "skeleton")
    if rustfacts:
        check_config["rustfacts_binary"] = str(rustfacts)
    report = adherence.score(
        workspace,
        expected["scored_intents"],
        check_config,
        language=language,
        rustfacts=rustfacts,
    )

    if language == "rust":
        staged = workspace / "tests"
        staged.mkdir(exist_ok=True)
        for path in sorted((scenario / "acceptance").glob("*.rs")):
            shutil.copy2(path, staged / path.name)
        acceptance = run(
            ["cargo", "test", "--quiet", "--test", "acceptance", "--", "--test-threads=1"],
            cwd=workspace,
            timeout=1800,
        )
        acceptance_results = parse_cargo_test(acceptance["stdout"] + acceptance["stderr"]) | {
            "exit_code": acceptance["exit_code"]
        }
    else:
        acceptance_directory = run_directory / "acceptance"
        shutil.copytree(scenario / "acceptance", acceptance_directory)
        acceptance_junit = run_directory / "acceptance.xml"
        acceptance = run(
            [
                "uv",
                "run",
                "--project",
                str(workspace),
                "pytest",
                str(acceptance_directory),
                "-q",
                f"--junit-xml={acceptance_junit}",
            ],
            cwd=acceptance_directory,
            timeout=1200,
        )
        acceptance_results = parse_junit(acceptance_junit) | {"exit_code": acceptance["exit_code"]}

    (run_directory / "acceptance-output.txt").write_text(
        acceptance["stdout"] + acceptance["stderr"], encoding="utf-8"
    )

    metrics = {
        "acceptance": acceptance_results,
        "adherence": report["adherence"],
        "agent": {"name": job["agent_name"], "description": agent.get("description", "")},
        "agent_run": agent_run,
        "arm": job["arm"],
        "environment_sync": {"exit_code": sync["exit_code"]},
        "guidance": guidance,
        "implementation": job["implementation"],
        "isolation": isolation,
        # Calibration runs are not observations of an agent and must never enter
        # a rate. `aggregate.py` filters on this.
        "kind": job["kind"],
        "language": language,
        "own_tests": own_results,
        "principles": report["principles"],
        "scenario": job["scenario_name"],
        "task_complete": acceptance_results["failed"] == 0
        and acceptance_results["errors"] == 0
        and acceptance_results["collected"] == expected["acceptance_check_count"],
        "trial": job["trial"],
        "workspace": report["workspace"],
        **extra,
    }
    (run_directory / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return metrics


# --------------------------------------------------------------------------
# Planning and dispatch
# --------------------------------------------------------------------------


def completed_trials(base):
    """Trial numbers that finished. An incomplete directory is not one."""
    if not base.is_dir():
        return set()
    finished = set()
    for path in base.glob("trial-*"):
        if (path / "metrics.json").is_file():
            try:
                finished.add(int(path.name.split("-")[1]))
            except (IndexError, ValueError):
                continue
    return finished


def plan_trials(base, target):
    """Which trial numbers are missing to reach `target`.

    Trials accumulate. Asking for 10 when 6 exist runs 4, and asking again runs
    none — so a crashed sweep resumes by repeating the same command, and a
    sample can be deepened without discarding what it already cost.
    """
    done = completed_trials(base)
    return [index for index in range(1, target + 1) if index not in done]


def announce(message):
    with PRINT_LOCK:
        print(message, file=sys.stderr, flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", default="fx-settlement")
    parser.add_argument("--agent", default="claude-opus-5", help="key in agents.toml")
    parser.add_argument(
        "--arm",
        default="both",
        choices=["guided", "control", "both"],
        help="guided installs the cmf-assembled AGENTS.md; control withholds it",
    )
    parser.add_argument(
        "--trials",
        type=int,
        default=1,
        help="target trials per arm; only the missing ones run",
    )
    parser.add_argument(
        "--fresh",
        action="store_true",
        help="discard this agent's existing trials for the scenario first",
    )
    parser.add_argument(
        "--concurrency",
        type=int,
        default=1,
        help="trials in flight at once; they are independent and workspace-isolated",
    )
    parser.add_argument("--timeout", type=int, default=1800, help="per-trial agent timeout")
    parser.add_argument(
        "--skip-agent",
        action="store_true",
        help="calibration: prepare and score without invoking the agent (the floor)",
    )
    parser.add_argument(
        "--implementation",
        help="calibration: score a checked-in directory, e.g. 'reference' (the ceiling)",
    )
    parser.add_argument(
        "--isolate-agent-home",
        action="store_true",
        help="point the agent's config home at an empty scratch directory; removes ambient "
        "guidance and memory, and requires credentials the agent can still reach",
    )
    arguments = parser.parse_args()

    scenario = HERE / "scenarios" / arguments.scenario
    if not scenario.is_dir():
        raise SystemExit(f"unknown scenario: {arguments.scenario}")

    agents = load_agents()
    if arguments.agent not in agents:
        raise SystemExit(f"unknown agent: {arguments.agent}. known: {', '.join(sorted(agents))}")
    agent = agents[arguments.agent]

    expected = json.loads((scenario / "expected.json").read_text(encoding="utf-8"))
    rustfacts = build_rustfacts() if expected.get("language") == "rust" else None

    calibration = arguments.skip_agent or arguments.implementation
    kind = "calibration" if calibration else "agent"
    out_root = HERE / "results" / arguments.scenario
    if calibration:
        label = arguments.implementation or "skeleton"
        agent_root = out_root / "_calibration" / label
        target = 1
    else:
        agent_root = out_root / arguments.agent
        target = arguments.trials

    arms = ["control", "guided"] if arguments.arm == "both" else [arguments.arm]

    if arguments.fresh and agent_root.exists():
        shutil.rmtree(agent_root)

    guidance = assemble_guidance(scenario) if "guided" in arms else None

    jobs = []
    for arm in arms:
        for index in plan_trials(agent_root / arm, target):
            jobs.append(
                {
                    "agent": agent,
                    "agent_name": arguments.agent if not calibration else f"_calibration:{label}",
                    "arm": arm,
                    "expected": expected,
                    "guidance": guidance,
                    "implementation": arguments.implementation,
                    "isolate_home": arguments.isolate_agent_home,
                    "kind": kind,
                    "run_directory": agent_root / arm / f"trial-{index:02d}",
                    "rustfacts": rustfacts,
                    "scenario": scenario,
                    "scenario_name": arguments.scenario,
                    "timeout": arguments.timeout,
                    "trial": index,
                }
            )

    if not jobs:
        announce(f"nothing to run: {target} trial(s) per arm already complete")
        return 0

    # A hosted API takes parallel trials; one ollama instance serving 18-81 GB
    # weights does not, and two trials wanting different models would measure
    # eviction rather than the models.
    ceiling = agent.get("max_concurrency")
    concurrency = max(1, arguments.concurrency)
    if ceiling and concurrency > ceiling:
        announce(f"clamping concurrency {concurrency} -> {ceiling} ({arguments.agent} declares a limit)")
        concurrency = ceiling

    if agent.get("warmup") and not calibration:
        warm_up(agent, arguments.agent)

    announce(f"{len(jobs)} trial(s) to run at concurrency {concurrency} ({', '.join(arms)})")

    completed = []
    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        futures = {pool.submit(run_trial, job): job for job in jobs}
        for future in as_completed(futures):
            job = futures[future]
            try:
                metrics = future.result()
            except Exception as error:  # a failed trial must not sink the sweep
                announce(f"{job['arm']} trial {job['trial']}: FAILED — {error}")
                continue
            completed.append(metrics)
            announce(
                f"{metrics['arm']} trial {metrics['trial']}: "
                f"acceptance {metrics['acceptance']['passed']}/{metrics['acceptance']['collected']}, "
                f"adherence {metrics['adherence']['followed_count']}"
                f"/{metrics['adherence']['applicable_count']} applicable"
            )

    announce(
        f"\n{len(completed)}/{len(jobs)} trial(s) completed. "
        f"Aggregate with: python3 aggregate.py --scenario {arguments.scenario}"
    )
    json.dump({"completed": len(completed), "attempted": len(jobs)}, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0 if len(completed) == len(jobs) else 1


if __name__ == "__main__":
    sys.exit(main())
