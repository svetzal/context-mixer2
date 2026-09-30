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
if [ -f "$workspace/src/violation.marker" ]; then
  printf '%s\n' '{"applicable":true,"followed":false,"evidence":["business rule and I/O are coupled"],"locations":[{"path":"src/violation.marker"}]}'
else
  printf '%s\n' '{"applicable":true,"followed":true,"evidence":["business rules are isolated"]}'
fi
