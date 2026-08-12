#!/usr/bin/env python3
"""Score a cmf benchmark result using deterministic, dependency-free metrics."""

import argparse
import hashlib
import json
import pathlib
import re
import sys


WORD = re.compile(r"[a-z0-9][a-z0-9_?!.-]*", re.IGNORECASE)
SELECTED = re.compile(r"^selected intents \((\d+)\):$", re.MULTILINE)
TOKENS = re.compile(r"^estimated tokens: (\d+)$", re.MULTILINE)
FIELD = re.compile(
    r'^(capability|threat|expectation|strategy|tradeoff) = ("(?:[^"\\]|\\.)*")$',
    re.MULTILINE,
)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def words(text):
    return [match.group(0).casefold() for match in WORD.finditer(text)]


def rounded(value):
    return round(value, 4)


def selected_keys(explanation):
    lines = explanation.splitlines()
    for index, line in enumerate(lines):
        if SELECTED.fullmatch(line):
            keys = []
            for candidate in lines[index + 1 :]:
                if not candidate.startswith("  "):
                    break
                keys.append(candidate.strip())
            return keys
    return []


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--original", required=True, type=pathlib.Path)
    parser.add_argument("--assembled", required=True, type=pathlib.Path)
    parser.add_argument("--explain", required=True, type=pathlib.Path)
    parser.add_argument("--intents", required=True, type=pathlib.Path)
    parser.add_argument("--expected", required=True, type=pathlib.Path)
    parser.add_argument("--baseline", required=True, type=pathlib.Path)
    args = parser.parse_args()

    original_bytes = args.original.read_bytes()
    assembled_bytes = args.assembled.read_bytes()
    original = original_bytes.decode("utf-8")
    assembled = assembled_bytes.decode("utf-8")
    explanation = args.explain.read_text(encoding="utf-8")
    expected = json.loads(args.expected.read_text(encoding="utf-8"))
    baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
    intent_paths = sorted(args.intents.rglob("*.toml"))
    intent_bytes = [path.read_bytes() for path in intent_paths]
    intent_set_hash = sha256(
        b"".join(
            path.relative_to(args.intents).as_posix().encode("utf-8")
            + b"\0"
            + hashlib.sha256(data).hexdigest().encode("ascii")
            + b"\n"
            for path, data in zip(intent_paths, intent_bytes)
        )
    )

    relevant_root = args.intents / expected["relevant_prefix"]
    relevant_paths = sorted(relevant_root.rglob("*.toml"))
    relevant_keys = {
        path.relative_to(args.intents).with_suffix("").as_posix() for path in relevant_paths
    }
    field_values = {
        "capability": [],
        "evidence": [],
        "expectation": [],
        "strategy": [],
        "threat": [],
        "tradeoff": [],
    }
    for path in relevant_paths:
        data = path.read_bytes()
        text = data.decode("utf-8")
        for match in FIELD.finditer(text):
            field_values[match.group(1)].append(json.loads(match.group(2)))

    # Evidence descriptions share TOML basic-string syntax with scalar fields.
    evidence_pattern = re.compile(r'description = ("(?:[^"\\]|\\.)*")')
    field_values["evidence"] = []
    for path in relevant_paths:
        field_values["evidence"].extend(
            json.loads(match.group(1))
            for match in evidence_pattern.finditer(path.read_text(encoding="utf-8"))
        )

    token_match = TOKENS.search(explanation)
    selected = set(selected_keys(explanation))
    estimated_tokens = int(token_match.group(1)) if token_match else 0
    field_metrics = {}
    for name, values in field_values.items():
        retained = sum(value in assembled for value in values)
        field_metrics[name] = {
            "available": len(values),
            "exactly_retained": retained,
            "exact_coverage": rounded(retained / len(values)) if values else 0,
        }
    available_fields = sum(item["available"] for item in field_metrics.values())
    retained_fields = sum(item["exactly_retained"] for item in field_metrics.values())
    semantic_field_coverage = retained_fields / available_fields if available_fields else 0
    true_positives = selected & relevant_keys
    selection_precision = len(true_positives) / len(selected) if selected else 0
    selection_recall = len(true_positives) / len(relevant_keys) if relevant_keys else 0
    original_vocabulary = set(words(original))
    assembled_vocabulary = set(words(assembled))
    shared_vocabulary = original_vocabulary & assembled_vocabulary

    checks = {
        "catalog_intent_count": len(intent_paths) == expected["catalog_intent_count"],
        "catalog_intent_set_sha256": (
            intent_set_hash == expected["catalog_intent_set_sha256"]
        ),
        "original_sha256": sha256(original_bytes) == expected["original_sha256"],
        "relevant_intent_count": len(relevant_paths) == expected["relevant_intent_count"],
    }
    targets = {
        "selection_precision": selection_precision >= expected["minimum_selection_precision"],
        "selection_recall": selection_recall >= expected["minimum_selection_recall"],
        "semantic_field_coverage": (
            semantic_field_coverage >= expected["minimum_semantic_field_coverage"]
        ),
        "strategy_coverage": (
            field_metrics["strategy"]["exact_coverage"]
            >= expected["minimum_strategy_coverage"]
        ),
    }
    metrics = {
        "assembled": {
            "approximate_tokens": estimated_tokens,
            "bytes": len(assembled_bytes),
            "sha256": sha256(assembled_bytes),
            "words": len(words(assembled)),
        },
        "checks": checks,
        "compression_ratio": rounded(len(assembled_bytes) / len(original_bytes)),
        "corpus": {
            "catalog_intent_count": len(intent_paths),
            "catalog_intent_set_sha256": intent_set_hash,
            "original_bytes": len(original_bytes),
            "original_sha256": sha256(original_bytes),
            "original_words": len(words(original)),
            "relevant_intent_count": len(relevant_paths),
        },
        "corpus_valid": all(checks.values()),
        "selection": {
            "false_positives": len(selected - relevant_keys),
            "precision": rounded(selection_precision),
            "recall": rounded(selection_recall),
            "selected_intents": len(selected),
            "true_positives": len(true_positives),
        },
        "semantic_fields": {
            "exact_coverage": rounded(semantic_field_coverage),
            "fields": field_metrics,
        },
        "targets": targets,
        "targets_met": all(targets.values()),
        "vocabulary": {
            "jaccard": rounded(
                len(shared_vocabulary) / len(original_vocabulary | assembled_vocabulary)
            ),
            "original_recall": rounded(len(shared_vocabulary) / len(original_vocabulary)),
        },
    }
    metrics["delta_from_baseline"] = {
        "approximate_tokens": estimated_tokens - baseline["approximate_tokens"],
        "compression_ratio": rounded(
            metrics["compression_ratio"] - baseline["compression_ratio"]
        ),
        "output_bytes": len(assembled_bytes) - baseline["output_bytes"],
        "selection_precision": rounded(
            metrics["selection"]["precision"] - baseline["selection_precision"]
        ),
        "selection_recall": rounded(
            metrics["selection"]["recall"] - baseline["selection_recall"]
        ),
        "semantic_field_coverage": rounded(
            metrics["semantic_fields"]["exact_coverage"]
            - baseline["semantic_field_coverage"]
        ),
        "vocabulary_jaccard": rounded(
            metrics["vocabulary"]["jaccard"] - baseline["vocabulary_jaccard"]
        ),
        "vocabulary_original_recall": rounded(
            metrics["vocabulary"]["original_recall"]
            - baseline["vocabulary_original_recall"]
        ),
    }
    json.dump(metrics, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    if not metrics["corpus_valid"]:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
