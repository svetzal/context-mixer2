#!/usr/bin/env bash
# Convenience wrapper around runner.py. Every flag passes straight through.
# The intent atlas is required: --atlas <path>, or CMF_ATLAS in the environment.
#
#   export CMF_ATLAS=~/Work/Projects/Personal/guidelines
#   ./benchmark/exercises/run.sh --agent claude-opus-5 --arm both --trials 3
#   ./benchmark/exercises/run.sh --implementation reference --arm guided --atlas "$CMF_ATLAS"
set -euo pipefail
exec python3 "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/runner.py" "$@"
