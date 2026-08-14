#!/usr/bin/env python3
"""Score whether an agent's output followed named intents, using only the code.

Every check is deterministic and reads the finished workspace. No check asks an
LLM whether guidance was followed, and no check inspects the agent's transcript
— what the agent said it would do is not evidence that it did.

Each intent reports `followed`, the raw signals behind that verdict, and short
evidence strings. A false verdict with visible signals is the point: it is what
distinguishes "the guidance never arrived" from "the guidance arrived and was
partly applied".

The checks themselves live in `checks.py` and the traversals they share live in
`predicates.py`. This module only decides what to run and reports the result.
"""

import argparse
import json
import pathlib
import sys

from checks import CHECKS
from predicates import collect, result


class Workspace:
    """A finished workspace, partitioned into production and test modules."""

    def __init__(self, root):
        self.root = root
        self.modules = collect(root)
        self.production = [module for module in self.modules if not module.is_test]
        self.tests = [module for module in self.modules if module.is_test]

    def inventory(self):
        return {
            "parse_errors": [module.parse_error for module in self.modules if module.parse_error],
            "production_modules": sorted(module.relative for module in self.production),
            "test_modules": sorted(module.relative for module in self.tests),
        }


def score(root, scored_intents, config=None):
    """Run every scored intent's check against a finished workspace."""
    workspace = Workspace(root)
    config = config or {}

    principles = {}
    for key in scored_intents:
        check = CHECKS.get(key)
        if check is None:
            principles[key] = result(
                False, {}, [f"no deterministic check implements {key}"]
            )
            continue
        principles[key] = check(workspace, config)

    followed = [key for key, item in principles.items() if item["followed"]]
    applicable = [key for key, item in principles.items() if item.get("applicable", True)]
    violated = [key for key in applicable if key not in followed]
    return {
        "adherence": {
            "followed": sorted(followed),
            "followed_count": len(followed),
            "applicable_count": len(applicable),
            "scored_count": len(scored_intents),
            "not_applicable": sorted(set(scored_intents) - set(applicable)),
            # The denominator is what this work had occasion to exhibit, not
            # every intent in the slice. A conditional intent whose condition
            # never arose is not a failure to follow it.
            "rate": round(len(followed) / len(applicable), 4) if applicable else 0,
            "violated": sorted(violated),
        },
        "principles": principles,
        "workspace": workspace.inventory(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", required=True, type=pathlib.Path)
    parser.add_argument("--expected", required=True, type=pathlib.Path)
    arguments = parser.parse_args()

    expected = json.loads(arguments.expected.read_text(encoding="utf-8"))
    report = score(
        arguments.workspace,
        expected["scored_intents"],
        expected.get("check_config", {}),
    )
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
