#!/bin/sh
# Fixture validator: passes when --workspace names a directory holding a
# Cargo.toml, proving the workspace argument reached it; exits 3 otherwise.
set -eu
workspace=""
while [ $# -gt 0 ]; do
  case "$1" in
    --workspace) workspace="$2"; shift 2 ;;
    --config) shift 2 ;;
    *) shift ;;
  esac
done
if [ ! -f "$workspace/Cargo.toml" ]; then
  echo "no Cargo.toml in workspace $workspace" >&2
  exit 3
fi
printf '%s\n' '{"applicable": true, "followed": true, "signals": {"gateway_traits": ["Filesystem", "ProcessRunner"]}, "evidence": ["every effectful call crosses a gateway trait"]}'
