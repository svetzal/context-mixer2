#!/usr/bin/env python3
"""Run a behavioural exercise and measure whether assembled guidance changed the work.

The harness takes three inputs:

1. agent and model parameters, named from `agents.toml`
2. a scenario skeleton — a real project and a task written against a fixed
   public contract
3. an AGENTS.md that cmf assembled from a slice of the intent corpus

It produces two things that the assembly benchmark cannot: whether the finished
code satisfies a hidden acceptance suite, and whether it exhibits each named
intent. Running the same scenario with and without the guidance is what turns
those into a measurement of the guidance rather than of the model.
"""

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import tomllib
import xml.etree.ElementTree as ElementTree

import adherence

HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent

PROMPT = """Read TASK.md in this directory and implement it.

Work directly in this project. When you are finished the task's definition of
done must hold. Do not ask for confirmation; complete the work."""


def load_agents():
    return tomllib.loads((HERE / "agents.toml").read_text(encoding="utf-8"))["agents"]


def run(command, cwd, env=None, timeout=None, stdin_devnull=True):
    started = time.monotonic()
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
            stdin=subprocess.DEVNULL if stdin_devnull else None,
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


def assemble_guidance(scenario, destination):
    """Input three: the cmf-assembled slice under test."""
    knowledge_base = scenario / "input" / "knowledge-base"
    profile = scenario / "input" / "profile.toml"
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
            str(knowledge_base),
            "assemble",
            str(profile),
            "--explain",
        ],
        cwd=REPO_ROOT,
    )
    if outcome["exit_code"] != 0:
        raise SystemExit(f"cmf assemble failed:\n{outcome['stderr']}")
    (destination / "explain.txt").write_text(outcome["stderr"], encoding="utf-8")
    (destination / "guidance.md").write_text(outcome["stdout"], encoding="utf-8")
    return outcome["stdout"], outcome["stderr"]


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


def run_trial(
    scenario,
    scenario_name,
    agent_name,
    agent,
    arm,
    trial,
    out_root,
    timeout,
    skip_agent,
    implementation,
    isolate_home,
):
    run_directory = out_root / agent_name / arm / f"trial-{trial:02d}"
    if run_directory.exists():
        shutil.rmtree(run_directory)
    run_directory.mkdir(parents=True)

    workspace = run_directory / "workspace"
    source = scenario / implementation if implementation else scenario / "input" / "skeleton"
    shutil.copytree(source, workspace)
    shutil.copy2(scenario / "TASK.md", workspace / "TASK.md")

    guidance = {"present": False}
    if arm == "guided":
        content, explanation = assemble_guidance(scenario, run_directory)
        for name in agent.get("guidance_files", ["AGENTS.md"]):
            (workspace / name).write_text(content, encoding="utf-8")
        guidance = {
            "present": True,
            "files": agent.get("guidance_files", ["AGENTS.md"]),
            "bytes": len(content.encode("utf-8")),
            "approximate_tokens": -(-len(content) // 4),
            "selected_intents": selected_from_explain(explanation),
        }

    home_env, isolation = isolation_for(agent, run_directory, isolate_home)
    environment = os.environ | home_env | agent.get("env", {})

    if skip_agent or implementation:
        agent_run = {"skipped": True}
    else:
        command = [
            part.replace("{prompt}", PROMPT).replace("{workspace}", str(workspace))
            for part in agent["command"]
        ]
        agent_run = run(command, cwd=workspace, env=environment, timeout=timeout)
        (run_directory / "agent-stdout.txt").write_text(agent_run.pop("stdout"), encoding="utf-8")
        (run_directory / "agent-stderr.txt").write_text(agent_run.pop("stderr"), encoding="utf-8")

    sync = run(["uv", "sync", "--quiet"], cwd=workspace, timeout=600)

    own_junit = run_directory / "own-tests.xml"
    own = run(
        ["uv", "run", "--project", str(workspace), "pytest", "-q", f"--junit-xml={own_junit}"],
        cwd=workspace,
        timeout=600,
    )
    own_results = parse_junit(own_junit) | {"exit_code": own["exit_code"]}

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
        timeout=600,
    )
    acceptance_results = parse_junit(acceptance_junit) | {"exit_code": acceptance["exit_code"]}
    (run_directory / "acceptance-output.txt").write_text(
        acceptance["stdout"] + acceptance["stderr"], encoding="utf-8"
    )

    expected = json.loads((scenario / "expected.json").read_text(encoding="utf-8"))
    report = adherence.score(
        workspace, expected["scored_intents"], expected.get("check_config", {})
    )

    metrics = {
        "acceptance": acceptance_results,
        "adherence": report["adherence"],
        "agent": {"name": agent_name, "description": agent.get("description", "")},
        "agent_run": agent_run,
        "arm": arm,
        "environment_sync": {"exit_code": sync["exit_code"]},
        "guidance": guidance,
        "implementation": implementation,
        "isolation": isolation,
        "own_tests": own_results,
        "principles": report["principles"],
        "scenario": scenario_name,
        "task_complete": acceptance_results["failed"] == 0
        and acceptance_results["errors"] == 0
        and acceptance_results["collected"] == expected["acceptance_check_count"],
        "trial": trial,
        "workspace": report["workspace"],
    }
    (run_directory / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return metrics


def summarize(runs):
    by_arm = {}
    for metrics in runs:
        bucket = by_arm.setdefault(
            metrics["arm"], {"trials": 0, "complete": 0, "followed": {}, "applicable": {}}
        )
        bucket["trials"] += 1
        bucket["complete"] += int(metrics["task_complete"])
        for key, item in metrics["principles"].items():
            counts = int(item.get("applicable", True))
            bucket["applicable"][key] = bucket["applicable"].get(key, 0) + counts
            bucket["followed"][key] = bucket["followed"].get(key, 0) + int(bool(item["followed"]))

    summary = {}
    for arm, bucket in by_arm.items():
        trials = bucket["trials"]
        applicable_total = sum(bucket["applicable"].values())
        summary[arm] = {
            "trials": trials,
            "task_completion_rate": round(bucket["complete"] / trials, 4),
            # Rates are per intent over the trials where that intent applied, so
            # a conditional intent never leaves the denominator inflated.
            "adherence_rate": round(sum(bucket["followed"].values()) / applicable_total, 4)
            if applicable_total
            else 0,
            "followed_by_intent": {
                key: round(count / bucket["applicable"][key], 4)
                if bucket["applicable"][key]
                else None
                for key, count in sorted(bucket["followed"].items())
            },
            "not_applicable_trials": {
                key: trials - applied
                for key, applied in sorted(bucket["applicable"].items())
                if applied < trials
            },
        }

    if "guided" in summary and "control" in summary:
        summary["lift"] = {
            "task_completion_rate": round(
                summary["guided"]["task_completion_rate"]
                - summary["control"]["task_completion_rate"],
                4,
            ),
            "adherence_rate": round(
                summary["guided"]["adherence_rate"] - summary["control"]["adherence_rate"], 4
            ),
            "by_intent": {
                key: round(value - (summary["control"]["followed_by_intent"].get(key) or 0), 4)
                for key, value in summary["guided"]["followed_by_intent"].items()
                if value is not None
            },
        }
    return summary


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
    parser.add_argument("--trials", type=int, default=1)
    parser.add_argument("--timeout", type=int, default=1800, help="per-trial agent timeout")
    parser.add_argument(
        "--skip-agent",
        action="store_true",
        help="prepare and score without invoking the agent; validates the harness",
    )
    parser.add_argument(
        "--implementation",
        help="score a checked-in directory instead of running an agent, e.g. 'reference'",
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

    arms = ["control", "guided"] if arguments.arm == "both" else [arguments.arm]
    out_root = HERE / "results" / arguments.scenario
    runs = []
    for arm in arms:
        for trial in range(1, arguments.trials + 1):
            metrics = run_trial(
                scenario,
                arguments.scenario,
                arguments.agent,
                agent,
                arm,
                trial,
                out_root,
                arguments.timeout,
                arguments.skip_agent,
                arguments.implementation,
                arguments.isolate_agent_home,
            )
            runs.append(metrics)
            print(
                f"{arm} trial {trial}: "
                f"acceptance {metrics['acceptance']['passed']}/{metrics['acceptance']['collected']}, "
                f"adherence {metrics['adherence']['followed_count']}"
                f"/{metrics['adherence']['applicable_count']} applicable",
                file=sys.stderr,
            )

    summary = summarize(runs)
    (out_root / "summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    json.dump(summary, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
