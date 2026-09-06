#!/bin/sh
# Fixture validator: fails, echoing the --config document back as signals to
# prove the per-intent block of cmv.toml reached it.
set -eu
config=""
while [ $# -gt 0 ]; do
  case "$1" in
    --workspace) shift 2 ;;
    --config) config="$2"; shift 2 ;;
    *) shift ;;
  esac
done
printf '{"applicable": true, "followed": false, "signals": %s, "evidence": ["business rules live beside I/O in src/main.rs"], "locations": [{"path": "src/main.rs", "line": 12}, {"path": "src/rates.rs"}]}\n' "$(cat "$config")"
