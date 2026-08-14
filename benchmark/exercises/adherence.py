#!/usr/bin/env python3
"""Score whether an agent's output followed named intents, using only the code.

Every check is deterministic and reads the finished workspace. No check asks an
LLM whether guidance was followed, and no check inspects the agent's transcript
— what the agent said it would do is not evidence that it did.

Each intent reports `followed`, the raw signals behind that verdict, and short
evidence strings. A false verdict with visible signals is the point: it is what
distinguishes "the guidance never arrived" from "the guidance arrived and was
partly applied".
"""

import argparse
import ast
import configparser
import json
import pathlib
import re
import sys
import tomllib

SKIP_DIRECTORIES = {
    ".git",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".venv",
    "__pycache__",
    "build",
    "dist",
    "node_modules",
    "venv",
}

TEST_DIRECTORY_NAMES = {"test", "tests"}

LEGACY_TYPING_NAMES = {
    "Dict",
    "FrozenSet",
    "List",
    "Optional",
    "Set",
    "Tuple",
    "Type",
    "Union",
}

BUILTIN_GENERIC_NAMES = {"dict", "frozenset", "list", "set", "tuple", "type"}

IO_MODULE_ROOTS = {
    "aiohttp",
    "http",
    "httpx",
    "os",
    "pathlib",
    "psycopg",
    "requests",
    "shutil",
    "socket",
    "sqlite3",
    "subprocess",
    "urllib",
}

THIRD_PARTY_PATCH_ROOTS = {
    "aiohttp",
    "http",
    "httpx",
    "requests",
    "socket",
    "urllib",
}

GATEWAY_NAME = re.compile(
    r"(Gateway|Source|Client|Provider|Repository|Adapter|Port|Service)$"
)

GATEWAY_MODULE = re.compile(
    r"(gateway|client|source|adapter|port|repository|transport|api)", re.IGNORECASE
)

FAKE_NAME = re.compile(r"^(Fake|Stub|Dummy|InMemory|Static|Recording|Canned)")

TIER_RATE = re.compile(r"0\.025|0\.015|0\.008|\b2\.5\b|\b1\.5\b|\b0\.8\b")

PATCH_FUNCTIONS = {"patch", "setattr", "setitem", "object"}


class Module:
    """One parsed Python file plus the classification the checks need."""

    def __init__(self, path, root):
        self.path = path
        self.parts = path.relative_to(root).parts
        self.relative = path.relative_to(root).as_posix()
        self.source = path.read_text(encoding="utf-8", errors="replace")
        self.parse_error = None
        try:
            self.tree = ast.parse(self.source)
        except SyntaxError as error:
            self.tree = ast.Module(body=[], type_ignores=[])
            self.parse_error = f"{self.relative}: {error}"

    @property
    def is_spec_named(self):
        return self.path.name.endswith("_spec.py")

    @property
    def in_test_directory(self):
        # Deliberately the workspace-relative parts. An absolute path can pick
        # up a "tests" component from wherever the run directory happens to be.
        return any(part in TEST_DIRECTORY_NAMES for part in self.parts)

    @property
    def is_test(self):
        name = self.path.name
        return (
            self.is_spec_named
            or name.startswith("test_")
            or name.endswith("_test.py")
            or name == "conftest.py"
            or self.in_test_directory
        )

    def imported_roots(self):
        roots = set()
        for node in ast.walk(self.tree):
            if isinstance(node, ast.Import):
                roots.update(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module and node.level == 0:
                roots.add(node.module.split(".")[0])
        return roots

    def classes(self):
        return [node for node in ast.walk(self.tree) if isinstance(node, ast.ClassDef)]

    def functions(self):
        return [
            node
            for node in ast.walk(self.tree)
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        ]


def collect(root):
    modules = []
    for path in sorted(root.rglob("*.py")):
        if any(part in SKIP_DIRECTORIES for part in path.relative_to(root).parts):
            continue
        modules.append(Module(path, root))
    return modules


def unparse(node):
    try:
        return ast.unparse(node)
    except Exception:  # pragma: no cover - defensive on exotic trees
        return ""


def base_names(node):
    names = set()
    for base in node.bases:
        text = unparse(base)
        names.add(text)
        names.add(text.rsplit(".", 1)[-1])
    return names


def pytest_settings(root):
    """Merge pytest discovery settings from wherever the project declared them."""
    settings = {}

    pyproject = root / "pyproject.toml"
    if pyproject.is_file():
        try:
            data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError:
            data = {}
        section = data.get("tool", {}).get("pytest", {}).get("ini_options", {})
        settings.update({key: value for key, value in section.items()})

    for name, heading in (("pytest.ini", "pytest"), ("setup.cfg", "tool:pytest"), ("tox.ini", "pytest")):
        candidate = root / name
        if not candidate.is_file():
            continue
        parser = configparser.ConfigParser()
        try:
            parser.read(candidate, encoding="utf-8")
        except configparser.Error:
            continue
        if parser.has_section(heading):
            settings.update(dict(parser.items(heading)))

    return settings


def as_patterns(value):
    if value is None:
        return []
    if isinstance(value, str):
        return value.split()
    return [str(item) for item in value]


def result(followed, signals, evidence):
    return {
        "followed": bool(followed),
        "signals": signals,
        "evidence": evidence,
    }


def check_colocated_specs(root, production, tests):
    settings = pytest_settings(root)
    patterns = as_patterns(settings.get("python_files"))
    configured = any("_spec" in pattern for pattern in patterns)

    production_directories = {module.path.parent for module in production}
    colocated = [
        module.relative
        for module in tests
        if module.is_spec_named and module.path.parent in production_directories
    ]
    stray_spec = [
        module.relative
        for module in tests
        if module.is_spec_named and module.path.parent not in production_directories
    ]
    in_test_directory = [module.relative for module in tests if module.in_test_directory]

    evidence = []
    if not tests:
        evidence.append("no test modules were written")
    if in_test_directory:
        evidence.append(f"{len(in_test_directory)} test module(s) live in a separate tests directory")
    if colocated:
        evidence.append(f"{len(colocated)} spec module(s) sit beside the module they specify")
    if not configured:
        evidence.append("pytest discovery is not configured for *_spec.py")

    return result(
        colocated and not in_test_directory and configured,
        {
            "colocated_specs": colocated,
            "specs_outside_production_packages": stray_spec,
            "tests_in_test_directory": in_test_directory,
            "pytest_python_files": patterns,
            "spec_discovery_configured": configured,
        },
        evidence,
    )


def check_bdd_specifications(root, production, tests):
    settings = pytest_settings(root)
    describe_classes = []
    should_methods = []
    for module in tests:
        for node in module.classes():
            if node.name.startswith("Describe"):
                describe_classes.append(f"{module.relative}::{node.name}")
        for node in module.functions():
            if node.name.startswith("should_"):
                should_methods.append(f"{module.relative}::{node.name}")

    class_patterns = as_patterns(settings.get("python_classes"))
    function_patterns = as_patterns(settings.get("python_functions"))
    configured = any("Describe" in item for item in class_patterns) and any(
        "should" in item for item in function_patterns
    )

    evidence = []
    if not describe_classes:
        evidence.append("no Describe grouping classes")
    if not should_methods:
        evidence.append("no should_* behaviour names")
    if describe_classes and should_methods and not configured:
        evidence.append("Describe/should naming is used but pytest is not configured to collect it")

    return result(
        describe_classes and should_methods,
        {
            "describe_classes": describe_classes,
            "should_methods": should_methods,
            "pytest_python_classes": class_patterns,
            "pytest_python_functions": function_patterns,
            "naming_discovery_configured": configured,
        },
        evidence,
    )


def check_native_assertions(root, production, tests):
    unittest_users = []
    self_assertions = []
    bare_assertions = 0
    for module in tests:
        if "unittest" in module.imported_roots():
            unittest_users.append(module.relative)
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Assert):
                bare_assertions += 1
            if (
                isinstance(node, ast.Attribute)
                and isinstance(node.value, ast.Name)
                and node.value.id == "self"
                and node.attr.startswith("assert")
            ):
                self_assertions.append(f"{module.relative}::{node.attr}")

    evidence = []
    if unittest_users:
        evidence.append(f"unittest imported in {len(unittest_users)} test module(s)")
    if self_assertions:
        evidence.append(f"{len(self_assertions)} TestCase-style assertion call(s)")
    if not bare_assertions:
        evidence.append("no plain assert statements")

    return result(
        bare_assertions and not unittest_users and not self_assertions,
        {
            "plain_assert_statements": bare_assertions,
            "unittest_modules": unittest_users,
            "testcase_assertions": self_assertions,
        },
        evidence,
    )


def check_modern_type_syntax(root, production, tests):
    future_annotations = []
    legacy_names = []
    annotated = 0
    builtin_generics = 0
    union_operators = 0

    for module in production + tests:
        for node in ast.walk(module.tree):
            if isinstance(node, ast.ImportFrom):
                if node.module == "__future__" and any(
                    alias.name == "annotations" for alias in node.names
                ):
                    future_annotations.append(module.relative)
                if node.module == "typing":
                    for alias in node.names:
                        if alias.name in LEGACY_TYPING_NAMES:
                            legacy_names.append(f"{module.relative}::typing.{alias.name}")
            if (
                isinstance(node, ast.Attribute)
                and isinstance(node.value, ast.Name)
                and node.value.id == "typing"
                and node.attr in LEGACY_TYPING_NAMES
            ):
                legacy_names.append(f"{module.relative}::typing.{node.attr}")

        for annotation in annotations_of(module.tree):
            annotated += 1
            for node in ast.walk(annotation):
                if (
                    isinstance(node, ast.Subscript)
                    and isinstance(node.value, ast.Name)
                    and node.value.id in BUILTIN_GENERIC_NAMES
                ):
                    builtin_generics += 1
                if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
                    union_operators += 1

    requires_python = declared_requires_python(root)

    evidence = []
    if future_annotations:
        evidence.append(f"from __future__ import annotations in {len(future_annotations)} module(s)")
    if legacy_names:
        evidence.append(f"{len(legacy_names)} legacy typing alias reference(s)")
    if not annotated:
        evidence.append("no type annotations at all")

    return result(
        annotated and not future_annotations and not legacy_names,
        {
            "annotations": annotated,
            "builtin_generic_uses": builtin_generics,
            "union_operator_uses": union_operators,
            "future_annotations_modules": future_annotations,
            "legacy_typing_references": legacy_names,
            "declared_requires_python": requires_python,
        },
        evidence,
    )


def annotations_of(tree):
    found = []
    for node in ast.walk(tree):
        if isinstance(node, ast.AnnAssign) and node.annotation is not None:
            found.append(node.annotation)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if node.returns is not None:
                found.append(node.returns)
            arguments = node.args
            for argument in (
                arguments.posonlyargs + arguments.args + arguments.kwonlyargs
            ):
                if argument.annotation is not None:
                    found.append(argument.annotation)
    return found


def declared_requires_python(root):
    pyproject = root / "pyproject.toml"
    if not pyproject.is_file():
        return None
    try:
        data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError:
        return None
    return data.get("project", {}).get("requires-python")


def check_immutable_models(root, production, tests):
    models = []
    for module in production:
        for node in module.classes():
            bases = base_names(node)
            decorators = [unparse(item) for item in node.decorator_list]
            keywords = {
                keyword.arg: unparse(keyword.value) for keyword in node.keywords
            }
            body = "\n".join(unparse(statement) for statement in node.body)

            kind = None
            frozen = False
            mechanism = None
            if "BaseModel" in bases:
                kind = "pydantic"
                if keywords.get("frozen") == "True":
                    frozen, mechanism = True, "class-keyword"
                elif re.search(r"frozen\s*[=:]\s*True", body):
                    frozen, mechanism = True, "model_config"
            elif any("dataclass" in decorator for decorator in decorators):
                kind = "dataclass"
                if any("frozen=True" in decorator.replace(" ", "") for decorator in decorators):
                    frozen, mechanism = True, "dataclass-frozen"
            elif "NamedTuple" in bases:
                kind, frozen, mechanism = "namedtuple", True, "namedtuple"

            if kind:
                models.append(
                    {
                        "name": f"{module.relative}::{node.name}",
                        "kind": kind,
                        "frozen": frozen,
                        "mechanism": mechanism,
                    }
                )

    mutable = [model["name"] for model in models if not model["frozen"]]

    evidence = []
    if not models:
        evidence.append("no domain model classes found")
    if mutable:
        evidence.append(f"{len(mutable)} mutable domain model(s)")

    return result(
        models and not mutable,
        {
            "models": models,
            "mutable_models": mutable,
        },
        evidence,
    )


def check_functional_core(root, production, tests):
    pure = []
    effectful = []
    fee_logic = []
    mixed = []
    for module in production:
        does_io = bool(module.imported_roots() & IO_MODULE_ROOTS)
        carries_rules = len(set(TIER_RATE.findall(module.source))) >= 2
        if does_io:
            effectful.append(module.relative)
        else:
            pure.append(module.relative)
        if carries_rules:
            fee_logic.append(module.relative)
        if carries_rules and does_io:
            mixed.append(module.relative)

    isolated = [name for name in fee_logic if name not in mixed]

    evidence = []
    if not fee_logic:
        evidence.append("the tiered fee rules were not found in any module")
    if mixed:
        evidence.append(f"{len(mixed)} module(s) hold both the fee rules and external I/O")
    if not effectful:
        evidence.append("no module performs the rate lookup")

    return result(
        isolated and not mixed and effectful,
        {
            "pure_modules": pure,
            "effectful_modules": effectful,
            "rule_bearing_modules": fee_logic,
            "modules_mixing_rules_and_io": mixed,
        },
        evidence,
    )


def check_gateway_mocking(root, production, tests):
    patched = []
    for module in tests:
        for node in ast.walk(module.tree):
            if not isinstance(node, ast.Call):
                continue
            target = call_target_name(node)
            if target is None:
                continue
            for argument in node.args[:1]:
                text = (
                    argument.value
                    if isinstance(argument, ast.Constant) and isinstance(argument.value, str)
                    else unparse(argument)
                )
                if text:
                    patched.append({"module": module.relative, "call": target, "target": text})

    third_party = [
        item
        for item in patched
        if item["target"].split(".")[0].strip("'\"") in THIRD_PARTY_PATCH_ROOTS
    ]

    # A gateway is a boundary, not a class. Python code wraps external calls in
    # a module of functions at least as often as in an object, and the intent
    # asks for "a thin, logic-free gateway" without prescribing either.
    gateways = []
    for module in production:
        for node in module.classes():
            bases = base_names(node)
            if GATEWAY_NAME.search(node.name) or {"Protocol", "ABC"} & bases:
                gateways.append(f"{module.relative}::{node.name}")

    for module in production:
        if not module.imported_roots() & IO_MODULE_ROOTS:
            continue
        named_boundary = GATEWAY_MODULE.search(module.path.stem)
        carries_rules = len(set(TIER_RATE.findall(module.source))) >= 2
        if named_boundary or not carries_rules:
            gateways.append(module.relative)

    fakes = []
    for module in tests:
        for node in module.classes():
            if FAKE_NAME.match(node.name) or GATEWAY_NAME.search(node.name):
                fakes.append(f"{module.relative}::{node.name}")

    if third_party:
        style = "third-party-patch"
    elif fakes or any(item["target"].startswith("fxsettle") for item in patched):
        style = "owned-substitute"
    elif patched:
        style = "other-patch"
    else:
        style = "none"

    evidence = []
    if not tests:
        evidence.append("no test modules were written, so nothing was substituted either way")
    if third_party:
        evidence.append(f"{len(third_party)} test double(s) target a third-party library")
    if not gateways:
        evidence.append("external calls are not confined to an owned boundary")

    return result(
        tests and gateways and not third_party,
        {
            "owned_gateways": sorted(set(gateways)),
            "test_fakes": fakes,
            "patch_targets": patched,
            "third_party_patches": third_party,
            "substitution_style": style,
        },
        evidence,
    )


def call_target_name(node):
    function = node.func
    if isinstance(function, ast.Name) and function.id in {"patch", "setattr"}:
        return function.id
    if isinstance(function, ast.Attribute) and function.attr in PATCH_FUNCTIONS:
        owner = unparse(function.value)
        if owner.split(".")[0] in {"mock", "mocker", "monkeypatch", "patch", "unittest"}:
            return f"{owner}.{function.attr}"
    return None


def check_domain_errors(root, production, tests):
    custom = []
    bare_except = []
    broad_except = []
    generic_raise = []
    documented = []

    for module in production:
        for node in module.classes():
            bases = base_names(node)
            if bases & {"Exception", "BaseException", "ValueError", "RuntimeError"} or any(
                name.endswith("Error") for name in bases
            ):
                custom.append(f"{module.relative}::{node.name}")
        for node in ast.walk(module.tree):
            if isinstance(node, ast.ExceptHandler):
                if node.type is None:
                    bare_except.append(f"{module.relative}:{node.lineno}")
                elif unparse(node.type) in {"Exception", "BaseException"}:
                    broad_except.append(f"{module.relative}:{node.lineno}")
            if isinstance(node, ast.Raise) and node.exc is not None:
                raised = unparse(node.exc)
                if raised.startswith(("Exception(", "BaseException(")):
                    generic_raise.append(f"{module.relative}:{node.lineno}")
        for node in module.functions():
            docstring = ast.get_docstring(node) or ""
            if re.search(r"\bRaises?\b", docstring):
                documented.append(f"{module.relative}::{node.name}")

    # Custom classes may subclass a project base class rather than Exception
    # directly, so a second pass promotes anything inheriting a known custom.
    known = {name.split("::")[-1] for name in custom}
    for module in production:
        for node in module.classes():
            if f"{module.relative}::{node.name}" in custom:
                continue
            if base_names(node) & known:
                custom.append(f"{module.relative}::{node.name}")

    evidence = []
    if not custom:
        evidence.append("no domain exception classes were defined")
    if bare_except:
        evidence.append(f"{len(bare_except)} bare except clause(s)")
    if generic_raise:
        evidence.append(f"{len(generic_raise)} generic Exception raise(s)")
    if not documented:
        evidence.append("no function documents the exceptions it raises")

    return result(
        custom and not bare_except and not generic_raise,
        {
            "domain_exceptions": sorted(set(custom)),
            "bare_except_clauses": bare_except,
            "broad_except_clauses": broad_except,
            "generic_raises": generic_raise,
            "functions_documenting_raises": documented,
        },
        evidence,
    )


CHECKS = {
    "craftsperson/python/colocated-module-specifications": check_colocated_specs,
    "craftsperson/python/functional-core-imperative-shell": check_functional_core,
    "craftsperson/python/gateway-only-mocking": check_gateway_mocking,
    "craftsperson/python/immutable-domain-models": check_immutable_models,
    "craftsperson/python/native-modern-type-syntax": check_modern_type_syntax,
    "craftsperson/python/native-pytest-assertions": check_native_assertions,
    "craftsperson/python/readable-bdd-specifications": check_bdd_specifications,
    "craftsperson/python/specific-domain-errors": check_domain_errors,
}


def score(workspace, scored_intents):
    modules = collect(workspace)
    production = [module for module in modules if not module.is_test]
    tests = [module for module in modules if module.is_test]

    principles = {}
    for key in scored_intents:
        check = CHECKS.get(key)
        if check is None:
            principles[key] = result(False, {}, [f"no deterministic check implements {key}"])
            continue
        principles[key] = check(workspace, production, tests)

    followed = [key for key, item in principles.items() if item["followed"]]
    return {
        "adherence": {
            "followed": sorted(followed),
            "followed_count": len(followed),
            "scored_count": len(scored_intents),
            "rate": round(len(followed) / len(scored_intents), 4) if scored_intents else 0,
            "violated": sorted(set(scored_intents) - set(followed)),
        },
        "principles": principles,
        "workspace": {
            "parse_errors": [module.parse_error for module in modules if module.parse_error],
            "production_modules": sorted(module.relative for module in production),
            "test_modules": sorted(module.relative for module in tests),
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", required=True, type=pathlib.Path)
    parser.add_argument("--expected", required=True, type=pathlib.Path)
    arguments = parser.parse_args()

    expected = json.loads(arguments.expected.read_text(encoding="utf-8"))
    report = score(arguments.workspace, expected["scored_intents"])
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
