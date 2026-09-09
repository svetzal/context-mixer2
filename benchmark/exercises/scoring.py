"""Score a finished workspace through cmf and cmv, and map the report to metrics.

The harness used to own its adherence checks. They now live in the intent atlas
as validators beside the records they verify, and cmv runs them; this module is
how the benchmark consumes that instead of re-implementing it:

1. `assemble` — cmf assembles the scenario's profile against the atlas and
   writes the compile manifest: exactly which intents, at which checksums, from
   which atlas revision, went into the guidance.
2. `stage_verification` — a trial workspace, guided or control, receives that
   manifest at `.context-mixer/cmf-manifest.json` and a `cmv.toml` carrying the
   scenario's `check_config` per scored intent, so cmv finds everything where a
   real project would keep it.
3. `check` — `cmv check --json` over the workspace.
4. `metrics_from_report` — cmv's per-intent states mapped onto the `adherence`
   and `principles` blocks `aggregate.py` reads, unchanged in shape.

cmf and cmv are built once per invocation from this repository (`build_tools`)
and then invoked as plain binaries, so trials running concurrently never contend
for cargo's target-directory lock. Nothing here reads the agent's transcript or
asks a model; every verdict comes from a validator parsing the code.
"""

import json
import os
import pathlib
import shutil
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent

MANIFEST_RELATIVE = pathlib.Path(".context-mixer") / "cmf-manifest.json"
CMV_CONFIG_NAME = "cmv.toml"

# A validator's own work takes well under a second. The Rust validators, though,
# build their fact extractor into a content-addressed cache the first time a
# given helper source is seen, and a release build of a syn binary is minutes,
# not seconds. cmv's default of 60s would report that first run as unchecked.
VALIDATOR_TIMEOUT_SECONDS = 900

# States cmv reports per intent. The first three are verdicts; the last two mean
# no verdict was reached, which for this benchmark is always a harness fault,
# because every scored intent ships a validator for its scenario's language.
VERDICT_STATES = {"pass", "fail", "not_applicable"}


def run_tool(command, cwd=None, env=None, timeout=1800):
    """Run cmf, cmv, or cargo and capture what it said.

    These are short-lived deterministic tools with their own timeouts (cmv
    bounds each validator), so the process-group handling `runner.run` needs for
    agent CLIs and local inference servers does not apply here.
    """
    return subprocess.run(
        [str(part) for part in command],
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


# --------------------------------------------------------------------------
# Locating the inputs
# --------------------------------------------------------------------------


def atlas_from(argument):
    """Resolve the intent atlas root from `--atlas`, falling back to `CMF_ATLAS`."""
    raw = argument or os.environ.get("CMF_ATLAS")
    if not raw:
        raise SystemExit(
            "no intent atlas given: pass --atlas <path> or set CMF_ATLAS to the atlas "
            "checkout (the guidelines repository) whose validators score the trials"
        )
    atlas = pathlib.Path(raw).expanduser().resolve()
    if not (atlas / "intents").is_dir():
        raise SystemExit(f"{atlas} does not look like an intent atlas: no intents/ directory")
    return atlas


def build_tools():
    """Build cmf and cmv from this repository once and return their binary paths.

    Built with cargo, located through `cargo metadata` so a `CARGO_TARGET_DIR`
    override is honoured, and invoked directly afterwards. This is the one way
    the harness reaches either tool; the debug profile is fine because the
    scoring is deterministic and its cost is in the validators, not in cmv.
    """
    manifest = REPO_ROOT / "Cargo.toml"
    build = run_tool(
        ["cargo", "build", "--quiet", "--manifest-path", manifest, "-p", "cmf", "-p", "cmv"],
        cwd=REPO_ROOT,
    )
    if build.returncode != 0:
        raise SystemExit(f"could not build cmf and cmv:\n{build.stderr}")
    metadata = run_tool(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--manifest-path", manifest],
        cwd=REPO_ROOT,
    )
    if metadata.returncode != 0:
        raise SystemExit(f"could not read cargo metadata:\n{metadata.stderr}")
    target = pathlib.Path(json.loads(metadata.stdout)["target_directory"]) / "debug"
    return {"cmf": target / "cmf", "cmv": target / "cmv"}


def snapshot_guard(scenario, atlas, scored_intents):
    """Abort unless every scored record in the snapshot matches the atlas byte for byte.

    The scenario's `input/knowledge-base/` is the fixture that documents which
    guidance the exercise measures. Scoring now reads the live atlas, so the
    snapshot no longer feeds anything — but if the atlas's copy of a scored
    record has changed, the guidance the agent receives has changed too, and
    trials collected before and after are not measuring the same thing. That
    is a decision for whoever maintains the scenario, not something a run
    should paper over.
    """
    snapshot = scenario / "input" / "knowledge-base"
    drifted = []
    for key in scored_intents:
        relative = pathlib.Path("intents") / f"{key}.toml"
        ours, theirs = snapshot / relative, atlas / relative
        if not theirs.is_file():
            drifted.append(f"{key} (no record at this key in the atlas)")
        elif not ours.is_file():
            drifted.append(f"{key} (no record at this key in the snapshot)")
        elif ours.read_bytes() != theirs.read_bytes():
            drifted.append(key)
    if drifted:
        raise SystemExit(
            f"snapshot drift: {len(drifted)} scored record(s) in {snapshot} differ from "
            f"{atlas}:\n  " + "\n  ".join(drifted) + "\n"
            "The benchmark's meaning has changed. Refresh the snapshot deliberately (and "
            "note it in the scenario's provenance) or point --atlas at the revision the "
            "snapshot was taken from."
        )


# --------------------------------------------------------------------------
# cmf: the manifest
# --------------------------------------------------------------------------


def assemble(scenario, atlas, tools, manifest_path):
    """cmf assembles the scenario's profile against the atlas and writes the manifest.

    Returns the artifact content, cmf's `--explain` provenance, and the parsed
    manifest. The profile stays the scenario's own `input/profile.toml`; only
    the records come from the atlas.
    """
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    outcome = run_tool(
        [
            tools["cmf"],
            "--root",
            atlas,
            "assemble",
            scenario / "input" / "profile.toml",
            "--explain",
            "--manifest",
            manifest_path,
        ],
        cwd=REPO_ROOT,
    )
    if outcome.returncode != 0:
        raise SystemExit(f"cmf assemble failed:\n{outcome.stderr}")
    return {
        "content": outcome.stdout,
        "explanation": outcome.stderr,
        "manifest": json.loads(manifest_path.read_text(encoding="utf-8")),
        "manifest_path": manifest_path,
    }


# --------------------------------------------------------------------------
# Staging: what cmv reads from the workspace
# --------------------------------------------------------------------------


def stage_verification(workspace, manifest_path, scenario, expected):
    """Give a workspace the manifest and `cmv.toml` cmv needs to verify it."""
    destination = workspace / MANIFEST_RELATIVE
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(manifest_path, destination)
    (workspace / CMV_CONFIG_NAME).write_text(
        render_cmv_config(scenario, expected), encoding="utf-8"
    )


def render_cmv_config(scenario, expected):
    """The `cmv.toml` for a trial: one `[intent."<key>"]` table per scored intent.

    Each table is the scenario's `check_config` — the facts only this exercise
    knows, which cmv hands to the validator as its `--config` document — plus
    `baseline_root`, the untouched skeleton, whose public items the
    documentation validator subtracts so that only the agent's own work is
    scored. No `ecosystems` override: the atlas's sensors detect the language
    from the skeleton's `Cargo.toml` or `pyproject.toml`, as they would in a
    real project.
    """
    config = dict(expected.get("check_config", {}))
    config["baseline_root"] = str((scenario / "input" / "skeleton").resolve())
    lines = [
        "# Written by the benchmark harness for this trial, from the scenario's",
        "# expected.json; not hand-authored. Read by cmv, never by the agent.",
        f"validator_timeout_seconds = {VALIDATOR_TIMEOUT_SECONDS}",
        "",
    ]
    for key in expected["scored_intents"]:
        lines.append(f'[intent."{key}"]')
        lines.extend(f"{name} = {toml_literal(value)}" for name, value in sorted(config.items()))
        lines.append("")
    return "\n".join(lines)


def toml_literal(value):
    """Render one `check_config` value as TOML.

    For the types `check_config` holds — strings, numbers, booleans, and flat
    lists of those — JSON's literal syntax is valid TOML, escapes included, so
    `json.dumps` does the work. Anything nested would need a table of its own,
    which no check has ever asked for; refuse rather than emit something cmv
    will misread.
    """
    flat = (str, int, float, bool)
    if isinstance(value, flat):
        return json.dumps(value)
    if isinstance(value, list) and all(isinstance(item, flat) for item in value):
        return json.dumps(value)
    raise TypeError(
        f"check_config values must be scalars or flat lists, not {type(value).__name__}: {value!r}"
    )


# --------------------------------------------------------------------------
# cmv: the verdicts
# --------------------------------------------------------------------------


def check(workspace, atlas, tools):
    """`cmv check --json` over a staged workspace.

    Returns what cmv did and its parsed report. Exit 0 and 1 are both verdicts
    (1 means a required intent was not held — the expected result for a control
    arm); anything else, or unparseable stdout, is no report, and the caller
    records the trial as measuring nothing.
    """
    outcome = run_tool(
        [tools["cmv"], "check", "--root", workspace, "--atlas", atlas, "--json"],
        cwd=workspace,
    )
    report = None
    if outcome.returncode in (0, 1):
        try:
            report = json.loads(outcome.stdout)
        except json.JSONDecodeError:
            report = None
    return {
        "command": [str(part) for part in outcome.args],
        "exit_code": outcome.returncode,
        "stderr": outcome.stderr[-4000:],
    }, report


def metrics_from_report(report, scored_intents, exit_code):
    """Map cmv's report onto the `adherence` and `principles` blocks the aggregator reads.

    Per scored intent, cmv's state becomes the three-valued verdict the harness
    has always recorded: `pass` is applicable and followed, `fail` is applicable
    and not followed, `not_applicable` is neither — the condition never arose and
    the intent leaves the denominator. `unchecked` and `unguided` are not
    verdicts. Every scored intent has a validator for its scenario's language,
    so either one means the harness, not the agent, failed; such a trial is
    reported as a `harness_fault` and must not enter a rate as a violation.

    `signals`, `evidence`, and `locations` pass through from the validator so a
    `false` still shows what was and was not found.
    """
    outcomes = {item["key"]: item for item in (report or {}).get("intents", [])}
    principles = {}
    faults = []
    if report is None:
        faults.append(f"cmv produced no report (exit {exit_code})")
    for key in scored_intents:
        outcome = outcomes.get(key)
        if outcome is None:
            principles[key] = principle_from({"state": "missing", "reason": "not in the cmv report"})
            if report is not None:
                faults.append(f"{key}: not in the cmv report")
            continue
        entry = principle_from(outcome)
        principles[key] = entry
        if entry["state"] not in VERDICT_STATES:
            faults.append(f"{key}: {entry['state']} — {entry.get('reason', 'no reason given')}")

    followed = sorted(key for key, item in principles.items() if item["followed"] is True)
    violated = sorted(key for key, item in principles.items() if item["followed"] is False)
    applicable_count = len(followed) + len(violated)
    return {
        "adherence": {
            "followed": followed,
            "followed_count": len(followed),
            "applicable_count": applicable_count,
            "scored_count": len(scored_intents),
            "not_applicable": sorted(
                key for key, item in principles.items() if item["state"] == "not_applicable"
            ),
            # The denominator is what this work had occasion to exhibit, not
            # every intent in the slice. A conditional intent whose condition
            # never arose is not a failure to follow it.
            "rate": round(len(followed) / applicable_count, 4) if applicable_count else 0,
            "violated": violated,
            "unchecked": sorted(
                key for key, item in principles.items() if item["state"] not in VERDICT_STATES
            ),
        },
        "principles": principles,
        "harness_faults": faults,
    }


def principle_from(outcome):
    """One intent's cmv outcome as the per-intent record `aggregate.py` tallies."""
    state = outcome["state"]
    entry = {
        "applicable": state in {"pass", "fail"},
        "followed": {"pass": True, "fail": False}.get(state),
        "signals": outcome.get("signals") or {},
        "evidence": outcome.get("evidence") or [],
        "locations": outcome.get("locations") or [],
        "state": state,
    }
    if outcome.get("reason"):
        entry["reason"] = outcome["reason"]
    return entry


def verdicts(principles):
    """The `(applicable, followed)` pair per intent — what a re-score compares."""
    return {
        key: (item.get("applicable", True), item.get("followed"))
        for key, item in principles.items()
    }


def warm_validators(scenario, atlas, tools, manifest_path, expected, scratch):
    """Run cmv once over the untouched skeleton before any trial does.

    Two reasons. The Rust validators build their fact extractor into a
    content-addressed cache on first use, with no lock around the build, so N
    concurrent first trials would each start a release build of the same crate;
    one sequential run pays that once. And a run whose validators cannot answer
    — the atlas moved, an entry point is missing, cmv cannot resolve something —
    should say so now, not after an hour of agent time has produced trials that
    all score as harness faults.
    """
    workspace = scratch / "validator-warmup"
    if workspace.exists():
        shutil.rmtree(workspace)
    shutil.copytree(scenario / "input" / "skeleton", workspace)
    stage_verification(workspace, manifest_path, scenario, expected)
    cmv_run, report = check(workspace, atlas, tools)
    faults = metrics_from_report(report, expected["scored_intents"], cmv_run["exit_code"])[
        "harness_faults"
    ]
    shutil.rmtree(workspace, ignore_errors=True)
    if faults:
        raise SystemExit(
            "validators cannot score this scenario:\n  "
            + "\n  ".join(faults)
            + ("\n" + cmv_run["stderr"] if cmv_run["stderr"] else "")
        )
    return report
