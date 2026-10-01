#!/usr/bin/env python3
"""Check public top-level Python functions for parameter and return annotations."""
import argparse
import ast
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--workspace", type=Path, required=True)
parser.add_argument("--config", type=Path, required=True)
args = parser.parse_args()
json.loads(args.config.read_text())
locations = []
functions = 0
for path in sorted((args.workspace / "src").rglob("*.py")):
    tree = ast.parse(path.read_text())
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and not node.name.startswith("_"):
            functions += 1
            parameters = node.args.posonlyargs + node.args.args + node.args.kwonlyargs
            parameters += [arg for arg in (node.args.vararg, node.args.kwarg) if arg is not None]
            if node.returns is None or any(arg.annotation is None for arg in parameters):
                locations.append({"path": str(path.relative_to(args.workspace)), "line": node.lineno})
print(json.dumps({"applicable": functions > 0, "followed": not locations,
                  "signals": {"public_functions": functions}, "locations": locations}, sort_keys=True))
