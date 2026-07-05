#!/usr/bin/env bash
#
# install.sh — install cmx and cmf locally from this checkout via `cargo install`.
#
# cmx is installed WITH the `llm` feature so LLM-backed commands (`cmx skill
# info` summaries, `cmx diff`) work; it pulls tokio + mojentic and needs the
# configured gateway's credentials (e.g. OPENAI_API_KEY) at runtime.
# cmf stays lean (default features).
#
# Run from a checkout at the version you want installed (the tagged commit, or
# `main` at the same version). `--force` overwrites previously installed binaries.

set -euo pipefail

# Resolve the workspace root (directory containing this script) so the script
# works regardless of the caller's current directory.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing cmx (with llm feature)..."
cargo install --path "$SCRIPT_DIR/cmx" --features llm --force

echo "Installing cmf (lean)..."
cargo install --path "$SCRIPT_DIR/cmf" --force

ZSH_COMPLETION_DIR="${CMX_ZSH_COMPLETION_DIR:-$HOME/.zfunc}"
ZSH_COMPLETION_PATH="$ZSH_COMPLETION_DIR/_cmx"

if [[ -d "$ZSH_COMPLETION_DIR" ]]; then
  completion_tmp="$(mktemp)"
  if cmx completions zsh > "$completion_tmp"; then
    mv "$completion_tmp" "$ZSH_COMPLETION_PATH"
    echo "Installed zsh completions: $ZSH_COMPLETION_PATH"
  else
    rm -f "$completion_tmp"
    echo "Skipped zsh completions: run 'cmx completions zsh > $ZSH_COMPLETION_PATH' manually."
  fi
else
  echo "Skipped zsh completions: $ZSH_COMPLETION_DIR does not exist."
  echo "Run 'cmx completions zsh > ~/.zfunc/_cmx' after creating that directory."
fi

echo
echo "Installed:"
cmx --version
cmf --version
