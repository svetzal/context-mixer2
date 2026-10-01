#!/usr/bin/env python3
"""Check the reference modules with rustc, without executing project code.

These checks cover the documented reference layout, not arbitrary Rust projects.
A normal compilation establishes valid source. Restricted compilation removes
std from the core and shell while retaining the shell's project-owned Gateway.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def compile_source(source, restricted, directory):
    path = directory / "contract.rs"
    path.write_text(("#![no_std]\n" if restricted else "") + source)
    return subprocess.run(
        ["rustc", "--crate-name", "reference_contract", "--crate-type", "lib",
         "--edition", "2024", "--emit", "metadata", "--out-dir", str(directory), str(path)],
        capture_output=True, text=True, check=False,
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=["core", "gateway"])
    parser.add_argument("--workspace", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)
    args = parser.parse_args()
    json.loads(args.config.read_text())
    src = args.workspace / "src"
    if args.kind == "core":
        source = "pub mod core {\n" + (src / "core.rs").read_text() + "\n}\n"
        location = "src/core.rs"
    else:
        source = "extern crate alloc;\n"
        for name in ["gateway", "shell"]:
            source += "pub mod " + name + " {\n" + (src / (name + ".rs")).read_text() + "\n}\n"
        source += ("pub fn contract(g: &dyn gateway::Gateway, path: &str) -> alloc::string::String {"
                   "shell::load(g, path)}\n")
        location = "src/shell.rs"
    with tempfile.TemporaryDirectory(prefix="reference-check-") as directory:
        directory = Path(directory)
        normal = compile_source(source, False, directory)
        if normal.returncode:
            print("reference source does not compile: " + normal.stderr, file=sys.stderr)
            return 2
        restricted = compile_source(source, True, directory)
    followed = restricted.returncode == 0
    print(json.dumps({
        "applicable": True,
        "followed": followed,
        "signals": {"valid_source": True, "restricted_compilation": followed},
        "evidence": ["reference module compiles without std" if followed
                     else "reference module requires std outside its allowed interface"],
        "locations": [{"path": location}],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(2)
