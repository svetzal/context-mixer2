#!/usr/bin/env bash
# Convenience wrapper around runner.py. Every flag passes straight through.
#
#   ./benchmark/exercises/run.sh --agent claude-opus-5 --arm both --trials 3
#   ./benchmark/exercises/run.sh --implementation reference --arm guided
set -euo pipefail
exec python3 "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/runner.py" "$@"
