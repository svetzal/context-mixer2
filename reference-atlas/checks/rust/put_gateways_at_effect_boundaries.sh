#!/bin/sh
set -eu
exec python3 "$(dirname "$0")/source_contract.py" gateway "$@"
