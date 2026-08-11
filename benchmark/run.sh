#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scenario_root="$repo_root/benchmark/scenarios"
result_root="$repo_root/benchmark/results"

run_scenario() {
  local name="$1"
  local scenario="$scenario_root/$name"
  local input="$scenario/input"
  local result="$result_root/$name"
  local -a originals

  if [[ ! -d "$scenario" ]]; then
    echo "unknown benchmark scenario: $name" >&2
    return 2
  fi

  shopt -s nullglob
  originals=("$input/original/"*.md)
  shopt -u nullglob
  if [[ ${#originals[@]} -ne 1 ]]; then
    echo "$name must contain exactly one original Markdown document" >&2
    return 2
  fi

  mkdir -p "$result"
  cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cmf -- \
    --root "$input/knowledge-base" \
    assemble "$input/profile.toml" --explain \
    >"$result/assembled.md" 2>"$result/explain.txt"

  python3 "$repo_root/benchmark/score.py" \
    --original "${originals[0]}" \
    --assembled "$result/assembled.md" \
    --explain "$result/explain.txt" \
    --intents "$input/knowledge-base/intents" \
    --expected "$scenario/expected.json" \
    --baseline "$scenario/baseline.json" \
    >"$result/metrics.json"

  echo "$name"
  sed 's/^/  /' "$result/metrics.json"
}

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [scenario]" >&2
  exit 2
fi

if [[ $# -eq 1 ]]; then
  run_scenario "$1"
else
  found=false
  for scenario in "$scenario_root"/*; do
    [[ -d "$scenario" ]] || continue
    found=true
    run_scenario "$(basename "$scenario")"
  done
  if [[ "$found" == false ]]; then
    echo "no benchmark scenarios found" >&2
    exit 2
  fi
fi
