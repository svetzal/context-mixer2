"""Real-binary multi-profile lifecycle probe over a private reference atlas."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

repo, work, stage = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
atlas, project, home = work / "atlas", work / "project", work / "home"
tool_home = Path.home()
env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"), XDG_CACHE_HOME=str(home / ".cache"), RUSTUP_HOME=str(tool_home / ".rustup"), CARGO_HOME=str(tool_home / ".cargo"))
CORE = "craftsperson/rust/isolate-functional-core"
GATEWAY = "craftsperson/rust/put-gateways-at-effect-boundaries"
DOCS = "craftsperson/rust/compile-public-documentation"


def run(binary, *args):
    cmd = [str(repo / "target/debug" / binary), *map(str, args)]
    result = subprocess.run(cmd, cwd=project, env=env, text=True, capture_output=True)
    print("$", " ".join(cmd), "\nexit:", result.returncode, "\nstdout:\n", result.stdout, "\nstderr:\n", result.stderr, flush=True)
    return result


def git(*args):
    git_env = dict(env, GIT_AUTHOR_NAME="Proof", GIT_AUTHOR_EMAIL="proof@example.invalid", GIT_COMMITTER_NAME="Proof", GIT_COMMITTER_EMAIL="proof@example.invalid")
    result = subprocess.run(["git", *args], cwd=atlas, env=git_env, text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


if stage == "prepare":
    shutil.copytree(repo / "reference-atlas", atlas)
    shutil.copytree(atlas / "cases/compliant", project)
    home.mkdir()
    profile_path = atlas / "profiles/rust-shipping.toml"
    original = profile_path.read_text()
    agent = original.replace(f'  "{CORE}",\n', "")
    assert agent != original
    profile_path.write_text(agent)
    gateway_path = atlas / "intents/craftsperson/rust/put-gateways-at-effect-boundaries.toml"
    gateway_record = gateway_path.read_text()
    without_relation = gateway_record.replace('relations = [\n  { type = "related-to", target = "craftsperson/rust/isolate-functional-core" },\n]\n', "")
    assert without_relation != gateway_record
    gateway_path.write_text(without_relation)
    git("init", "-q")
    git("add", ".")
    git("commit", "-qm", "Agent selects gateway and documentation")
    first = git("rev-parse", "HEAD")
    assert run("cmf", "--root", atlas, "install", "rust-shipping", "--local", "--platform", "codex", "--apply").returncode == 0
    skill = original.replace('id = "rust-shipping"', 'id = "rust-skill"').replace('name = "AGENTS"', 'name = "rust-skill"').replace('surface = "agent"', 'surface = "skill"')
    skill = skill.replace(f'  "{GATEWAY}",\n', "").replace(f'  "{DOCS}",\n', "")
    (atlas / "profiles/rust-skill.toml").write_text(skill)
    git("add", ".")
    git("commit", "-qm", "Skill selects functional core")
    second = git("rev-parse", "HEAD")
    assert first != second
    assert run("cmf", "--root", atlas, "install", "rust-skill", "--local", "--platform", "codex", "--apply").returncode == 0
    manifest = json.loads((project / ".context-mixer/cmf-manifest.json").read_text())
    entries = manifest["artifacts"]
    assert manifest["schema"] == 2 and len(entries) == 2
    assert [(e["artifact"]["name"], e["artifact"]["surface"], e["atlas"]["revision"]) for e in entries] == [("AGENTS", "agent", first), ("rust-skill", "skill", second)]
    assert [[i["key"] for i in e["intents"]] for e in entries] == [[DOCS, GATEWAY], [CORE]]
    assert all(i["id"] and i["checksum"].startswith("sha256:") for e in entries for i in e["intents"])
    print("independent pins, identities, selected keys, and obligations confirmed", flush=True)
elif stage in ("rejecting", "corrected"):
    source = atlas / "cases" / ("violation" if stage == "rejecting" else "compliant") / "src/core.rs"
    shutil.copyfile(source, project / "src/core.rs")
    json_result = run("cmv", "check", "--json", "--atlas", atlas)
    human_result = run("cmv", "check", "--atlas", atlas)
    expected = 1 if stage == "rejecting" else 0
    assert json_result.returncode == human_result.returncode == expected
    report = json.loads(json_result.stdout)
    assert report["summary"]["exit_code"] == expected
    entries = report["artifacts"]
    assert [e["artifact"]["name"] for e in entries] == ["AGENTS", "rust-skill"]
    assert [e["check"]["manifest"]["atlas"]["revision"] for e in entries] == [git("rev-list", "--max-parents=0", "HEAD"), git("rev-parse", "HEAD")]
    assert [[i["key"] for i in e["check"]["intents"]] for e in entries] == [[DOCS, GATEWAY], [CORE]]
    assert [e["check"]["summary"]["exit_code"] for e in entries] == [0, expected]
    assert entries[0]["check"]["intents"][1]["required"] is True
    assert entries[0]["check"]["intents"][1]["state"] == "pass"
    assert entries[1]["check"]["intents"][0]["required"] is True
    assert (entries[1]["check"]["intents"][0]["state"] == "fail") == (stage == "rejecting")
    assert "AGENTS (Agent)" in human_result.stdout and "rust-skill (Skill)" in human_result.stdout
    assert run("cmv", "check", "--json", "--atlas", atlas).stdout == json_result.stdout
    assert run("cmv", "check", "--atlas", atlas).stdout == human_result.stdout
    print(f"{stage} artifact-attributed JSON and human reports confirmed", flush=True)
    if stage == "rejecting":
        sys.exit(1)
else:
    raise ValueError(stage)
