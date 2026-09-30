#!/bin/sh
set -eu
workspace=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --workspace) workspace="$2"; shift 2 ;;
    --config) shift 2 ;;
    *) exit 2 ;;
  esac
done
if [ -f "$workspace/Cargo.toml" ]; then
  printf '%s\n' '{"applicable":true,"followed":true,"evidence":["gateway boundary declared"]}'
else
  printf '%s\n' '{"applicable":false,"followed":false,"evidence":["no Rust package"]}'
fi
