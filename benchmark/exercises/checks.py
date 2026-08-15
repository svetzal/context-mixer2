#!/usr/bin/env python3
"""Per-intent adherence checks, built on the shared predicates.

A check answers one question: does this finished workspace exhibit this intent?
It returns a verdict, the signals behind it, and short evidence strings. It never
asks a model and never reads the agent's transcript.

Checks take `(workspace, config)`. `config` is the scenario's `check_config`
block, which carries the facts only that exercise knows — which literals mark its
business rules, which symbols count as blocking for its domain. Anything that
would have to change per scenario belongs there, not here.
"""

import ast
import re

from predicates import (
    as_patterns,
    baseline_public_items,
    call_name,
    calls_to,
    class_shape,
    decorator_names,
    declared_dependencies,
    declared_requires_python,
    defaults,
    enclosing_function,
    guarded_by_call,
    in_async_context,
    is_function,
    is_mutable_default,
    cargo_manifest,
    keyword_map,
    not_applicable,
    references,
    result,
    rust_calls,
    rust_macros,
    tool_config,
    unparse,
    within,
)

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

GATEWAY_NAME = re.compile(r"(Gateway|Source|Client|Provider|Repository|Adapter|Port|Service)$")

GATEWAY_MODULE = re.compile(
    r"(gateway|client|source|adapter|port|repository|transport|api)", re.IGNORECASE
)

FAKE_NAME = re.compile(r"^(Fake|Stub|Dummy|InMemory|Static|Recording|Canned)")

TASK_GROUP_SYMBOLS = {"asyncio.TaskGroup", "TaskGroup", "anyio.create_task_group"}

LOOSE_TASK_SYMBOLS = {"asyncio.create_task", "asyncio.ensure_future"}

TIMEOUT_SCOPE_SYMBOLS = {
    "asyncio.timeout",
    "asyncio.timeout_at",
    "anyio.fail_after",
    "anyio.move_on_after",
    "async_timeout.timeout",
}

TIMEOUT_CALL_SYMBOLS = {"asyncio.wait_for", "asyncio.waitfor"}

EXECUTOR_SYMBOLS = {
    "asyncio.to_thread",
    "loop.run_in_executor",
    "run_in_executor",
    "anyio.to_thread.run_sync",
    "to_thread.run_sync",
}

RESOURCE_SYMBOLS = {
    "httpx.AsyncClient",
    "httpx.Client",
    "aiohttp.ClientSession",
    "open",
    "socket.socket",
}

CONTEXT_MANAGER_DECORATORS = ("contextmanager", "asynccontextmanager")

UNSPECCED_MOCK_SYMBOLS = {"Mock", "MagicMock", "unittest.mock.Mock", "unittest.mock.MagicMock"}

SPEC_KEYWORDS = {"spec", "spec_set", "autospec", "instance"}


# --------------------------------------------------------------------------
# fx-settlement: structure, naming, and typing
# --------------------------------------------------------------------------


def check_colocated_specs(workspace, config):
    settings = tool_config(workspace.root, "pytest.ini_options")
    patterns = as_patterns(settings.get("python_files"))
    configured = any("_spec" in pattern for pattern in patterns)

    production_directories = {module.path.parent for module in workspace.production}
    colocated = [
        module.relative
        for module in workspace.tests
        if module.is_spec_named and module.path.parent in production_directories
    ]
    stray = [
        module.relative
        for module in workspace.tests
        if module.is_spec_named and module.path.parent not in production_directories
    ]
    in_test_directory = [module.relative for module in workspace.tests if module.in_test_directory]

    evidence = []
    if not workspace.tests:
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
            "specs_outside_production_packages": stray,
            "tests_in_test_directory": in_test_directory,
            "pytest_python_files": patterns,
            "spec_discovery_configured": configured,
        },
        evidence,
    )


def check_bdd_specifications(workspace, config):
    settings = tool_config(workspace.root, "pytest.ini_options")
    describe_classes = []
    should_methods = []
    for module in workspace.tests:
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


def check_native_assertions(workspace, config):
    unittest_users = []
    self_assertions = []
    bare_assertions = 0
    for module in workspace.tests:
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


def check_modern_type_syntax(workspace, config):
    future_annotations = []
    legacy_names = []
    annotated = 0
    builtin_generics = 0
    union_operators = 0

    for module in workspace.modules:
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

        for annotation in _annotations_of(module.tree):
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
            "declared_requires_python": declared_requires_python(workspace.root),
        },
        evidence,
    )


def _annotations_of(tree):
    found = []
    for node in ast.walk(tree):
        if isinstance(node, ast.AnnAssign) and node.annotation is not None:
            found.append(node.annotation)
        elif is_function(node):
            if node.returns is not None:
                found.append(node.returns)
            arguments = node.args
            for argument in arguments.posonlyargs + arguments.args + arguments.kwonlyargs:
                if argument.annotation is not None:
                    found.append(argument.annotation)
    return found


def check_immutable_models(workspace, config):
    models = []
    for module in workspace.production:
        for node in module.classes():
            shape = class_shape(node)
            kind = None
            frozen = False
            mechanism = None
            if "BaseModel" in shape["bases"]:
                kind = "pydantic"
                if shape["keywords"].get("frozen") == "True":
                    frozen, mechanism = True, "class-keyword"
                elif re.search(r"frozen\s*[=:]\s*True", shape["body"]):
                    frozen, mechanism = True, "model_config"
            elif any("dataclass" in decorator for decorator in shape["decorators"]):
                kind = "dataclass"
                if any(
                    "frozen=True" in decorator.replace(" ", "") for decorator in shape["decorators"]
                ):
                    frozen, mechanism = True, "dataclass-frozen"
            elif "NamedTuple" in shape["bases"]:
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
        {"models": models, "mutable_models": mutable},
        evidence,
    )


def check_functional_core(workspace, config):
    rule_pattern = re.compile(config["business_rule_pattern"])
    minimum = config.get("business_rule_minimum_matches", 2)

    pure = []
    effectful = []
    rule_bearing = []
    mixed = []
    for module in workspace.production:
        does_io = bool(module.imported_roots() & IO_MODULE_ROOTS)
        carries_rules = len(set(rule_pattern.findall(module.source))) >= minimum
        (effectful if does_io else pure).append(module.relative)
        if carries_rules:
            rule_bearing.append(module.relative)
        if carries_rules and does_io:
            mixed.append(module.relative)

    isolated = [name for name in rule_bearing if name not in mixed]

    evidence = []
    if not rule_bearing:
        evidence.append("the scenario's business rules were not found in any module")
    if mixed:
        evidence.append(f"{len(mixed)} module(s) hold both the business rules and external I/O")
    if not effectful:
        evidence.append("no module performs the external call")

    return result(
        isolated and not mixed and effectful,
        {
            "pure_modules": pure,
            "effectful_modules": effectful,
            "rule_bearing_modules": rule_bearing,
            "modules_mixing_rules_and_io": mixed,
        },
        evidence,
    )


def check_gateway_mocking(workspace, config):
    rule_pattern = re.compile(config["business_rule_pattern"])
    minimum = config.get("business_rule_minimum_matches", 2)

    patched = []
    for module in workspace.tests:
        for node in module.calls():
            target = _substitution_target(node)
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
    # a module of functions at least as often as in an object.
    gateways = []
    for module in workspace.production:
        for node in module.classes():
            shape = class_shape(node)
            if GATEWAY_NAME.search(node.name) or {"Protocol", "ABC"} & shape["bases"]:
                gateways.append(f"{module.relative}::{node.name}")
    for module in workspace.production:
        if not module.imported_roots() & IO_MODULE_ROOTS:
            continue
        carries_rules = len(set(rule_pattern.findall(module.source))) >= minimum
        if GATEWAY_MODULE.search(module.path.stem) or not carries_rules:
            gateways.append(module.relative)

    fakes = []
    for module in workspace.tests:
        for node in module.classes():
            if FAKE_NAME.match(node.name) or GATEWAY_NAME.search(node.name):
                fakes.append(f"{module.relative}::{node.name}")

    package = config.get("package", "")
    if third_party:
        style = "third-party-patch"
    elif fakes or any(item["target"].startswith(package) for item in patched):
        style = "owned-substitute"
    elif patched:
        style = "other-patch"
    else:
        style = "none"

    evidence = []
    if not workspace.tests:
        evidence.append("no test modules were written, so nothing was substituted either way")
    if third_party:
        evidence.append(f"{len(third_party)} test double(s) target a third-party library")
    if not gateways:
        evidence.append("external calls are not confined to an owned boundary")

    return result(
        workspace.tests and gateways and not third_party,
        {
            "owned_gateways": sorted(set(gateways)),
            "test_fakes": fakes,
            "patch_targets": patched,
            "third_party_patches": third_party,
            "substitution_style": style,
        },
        evidence,
    )


def _substitution_target(node):
    function = node.func
    if isinstance(function, ast.Name) and function.id in {"patch", "setattr"}:
        return function.id
    if isinstance(function, ast.Attribute) and function.attr in {
        "patch",
        "setattr",
        "setitem",
        "object",
    }:
        owner = unparse(function.value)
        if owner.split(".")[0] in {"mock", "mocker", "monkeypatch", "patch", "unittest"}:
            return f"{owner}.{function.attr}"
    return None


def check_domain_errors(workspace, config):
    custom = []
    bare_except = []
    broad_except = []
    generic_raise = []
    documented = []

    for module in workspace.production:
        for node in module.classes():
            bases = class_shape(node)["bases"]
            if bases & {"Exception", "BaseException", "ValueError", "RuntimeError"} or any(
                name.endswith("Error") for name in bases
            ):
                custom.append(f"{module.relative}::{node.name}")
        for node in ast.walk(module.tree):
            if isinstance(node, ast.ExceptHandler):
                if node.type is None:
                    bare_except.append(module.label(node))
                elif unparse(node.type) in {"Exception", "BaseException"}:
                    broad_except.append(module.label(node))
            if isinstance(node, ast.Raise) and node.exc is not None:
                if unparse(node.exc).startswith(("Exception(", "BaseException(")):
                    generic_raise.append(module.label(node))
        for node in module.functions():
            if re.search(r"\bRaises?\b", ast.get_docstring(node) or ""):
                documented.append(f"{module.relative}::{node.name}")

    known = {name.split("::")[-1] for name in custom}
    for module in workspace.production:
        for node in module.classes():
            if f"{module.relative}::{node.name}" in custom:
                continue
            if class_shape(node)["bases"] & known:
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


# --------------------------------------------------------------------------
# probe-fanout: concurrency, cancellation, and resource lifetime
# --------------------------------------------------------------------------


def check_nonblocking_async_io(workspace, config):
    blocking = set(config.get("blocking_symbols", []))

    async_functions = []
    offenders = []
    delegated = []
    for module in workspace.production:
        for node in module.functions():
            if isinstance(node, ast.AsyncFunctionDef):
                async_functions.append(f"{module.relative}::{node.name}")
        for node in calls_to(module, blocking):
            if not in_async_context(module, node):
                continue
            # An awaited call is not blocking, whatever it is named. This is
            # what keeps `await asyncio.sleep(...)` from being read as
            # `time.sleep(...)` when the author imported either one bare.
            if isinstance(module.parents.get(node), ast.Await):
                continue
            record = {"where": module.label(node), "call": call_name(node)}
            # A blocking call handed to an executor is the intent's own remedy,
            # so it is delegation rather than a violation.
            if _delegated_to_executor(module, node):
                delegated.append(record)
            else:
                offenders.append(record)

    awaits = sum(
        1
        for module in workspace.production
        for node in ast.walk(module.tree)
        if isinstance(node, ast.Await)
    )

    evidence = []
    if not async_functions:
        evidence.append("no async functions in production code")
    if offenders:
        evidence.append(f"{len(offenders)} blocking call(s) inside async functions")

    return result(
        async_functions and awaits and not offenders,
        {
            "async_functions": async_functions,
            "await_expressions": awaits,
            "blocking_calls_in_async": offenders,
            "blocking_calls_delegated_to_executor": delegated,
        },
        evidence,
    )


def _delegated_to_executor(module, node):
    for ancestor in [node] + [
        item for item in ast.walk(module.tree) if isinstance(item, ast.Call) and node in ast.walk(item)
    ]:
        if isinstance(ancestor, ast.Call) and any(
            call_name(ancestor).endswith(symbol.split(".")[-1]) for symbol in EXECUTOR_SYMBOLS
        ):
            return True
    return False


def check_structured_concurrency(workspace, config):
    scopes = []
    aggregations = []
    loose = []
    for module in workspace.production:
        for node in calls_to(module, TASK_GROUP_SYMBOLS):
            scopes.append({"where": module.label(node), "call": call_name(node)})
        for node in calls_to(module, {"asyncio.gather"}):
            aggregations.append({"where": module.label(node), "call": call_name(node)})
        for node in calls_to(module, LOOSE_TASK_SYMBOLS):
            # `tg.create_task` inside an `async with asyncio.TaskGroup()` is
            # structured; the same call at large is the orphaning risk.
            if not guarded_by_call(module, node, TASK_GROUP_SYMBOLS):
                loose.append({"where": module.label(node), "call": call_name(node)})

    evidence = []
    if not scopes and not aggregations:
        evidence.append("no TaskGroup or gather; concurrency is unstructured or absent")
    if loose:
        evidence.append(f"{len(loose)} task(s) created outside any task group")

    return result(
        (scopes or aggregations) and not loose,
        {
            "task_group_scopes": scopes,
            "gather_aggregations": aggregations,
            "tasks_outside_a_group": loose,
        },
        evidence,
    )


def check_async_timeouts(workspace, config):
    awaited_operations = config.get("bounded_operations", [])

    scopes = []
    waits = []
    client_timeouts = []
    bounded = []
    unbounded = []
    swallowed = []

    for module in workspace.production:
        for node in calls_to(module, TIMEOUT_SCOPE_SYMBOLS):
            scopes.append({"where": module.label(node), "call": call_name(node)})
        for node in calls_to(module, TIMEOUT_CALL_SYMBOLS):
            waits.append({"where": module.label(node), "call": call_name(node)})
        for node in module.calls():
            if "timeout" in keyword_map(node):
                client_timeouts.append({"where": module.label(node), "call": call_name(node)})

        for node in calls_to(module, awaited_operations):
            record = {"where": module.label(node), "call": call_name(node)}
            if (
                guarded_by_call(module, node, TIMEOUT_SCOPE_SYMBOLS)
                or "timeout" in keyword_map(node)
                or _wrapped_by(module, node, TIMEOUT_CALL_SYMBOLS)
            ):
                bounded.append(record)
            else:
                unbounded.append(record)

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.ExceptHandler):
                continue
            caught = unparse(node.type) if node.type is not None else ""
            catches_cancellation = node.type is None or "CancelledError" in caught or caught in {
                "BaseException",
            }
            if not catches_cancellation:
                continue
            reraises = any(isinstance(inner, ast.Raise) for inner in ast.walk(node))
            if not reraises:
                swallowed.append({"where": module.label(node), "catches": caught or "bare except"})

    has_bound = bool(scopes or waits or client_timeouts)

    evidence = []
    if not has_bound:
        evidence.append("no explicit timeout is applied anywhere")
    if unbounded:
        evidence.append(f"{len(unbounded)} bounded operation(s) run with no timeout in scope")
    if swallowed:
        evidence.append(f"{len(swallowed)} handler(s) catch cancellation without re-raising")

    return result(
        has_bound and not unbounded and not swallowed,
        {
            "timeout_scopes": scopes,
            "wait_for_calls": waits,
            "timeout_arguments": client_timeouts,
            "bounded_operations": bounded,
            "unbounded_operations": unbounded,
            "cancellation_swallowed": swallowed,
        },
        evidence,
    )


def _wrapped_by(module, node, symbols):
    for ancestor in ast.walk(module.tree):
        if not isinstance(ancestor, ast.Call) or ancestor is node:
            continue
        if node in ast.walk(ancestor) and any(
            call_name(ancestor).endswith(symbol.split(".")[-1]) for symbol in symbols
        ):
            return True
    return False


def check_resource_cleanup(workspace, config):
    resources = set(config.get("resource_symbols", [])) | RESOURCE_SYMBOLS

    managed = []
    unmanaged = []
    manual_closes = []
    for module in workspace.production:
        for node in calls_to(module, resources):
            record = {"where": module.label(node), "call": call_name(node)}
            if _is_context_managed(module, node):
                managed.append(record)
            else:
                unmanaged.append(record)
        for node in calls_to(module, {"close", "aclose"}):
            if not within(module, node, (ast.Try,)):
                manual_closes.append({"where": module.label(node), "call": call_name(node)})

    evidence = []
    if not managed and not unmanaged:
        evidence.append("no resource acquisition found")
    if unmanaged:
        evidence.append(f"{len(unmanaged)} resource(s) acquired outside a context manager")
    if manual_closes:
        evidence.append(f"{len(manual_closes)} manual close() with no try/finally around it")

    return result(
        managed and not unmanaged,
        {
            "context_managed_resources": managed,
            "unmanaged_resources": unmanaged,
            "manual_closes_outside_try": manual_closes,
        },
        evidence,
    )


def _is_context_managed(module, node):
    if isinstance(module.parents.get(node), ast.withitem):
        return True
    # A resource acquired inside a try that has a finally, or inside a function
    # that is itself a context manager, is released deterministically too.
    for ancestor in _ancestor_chain(module, node):
        if isinstance(ancestor, ast.Try) and ancestor.finalbody:
            return True
        if is_function(ancestor) and any(
            decorator.endswith(CONTEXT_MANAGER_DECORATORS)
            for decorator in decorator_names(ancestor)
        ):
            return True
    return False


def _ancestor_chain(module, node):
    chain = []
    current = module.parents.get(node)
    while current is not None:
        chain.append(current)
        current = module.parents.get(current)
    return chain


def check_mutable_defaults(workspace, config):
    with_defaults = []
    offenders = []
    sentinels = []
    for module in workspace.modules:
        for node in module.functions():
            values = defaults(node)
            if values:
                with_defaults.append(f"{module.relative}::{node.name}")
            for value in values:
                if is_mutable_default(value):
                    offenders.append(
                        {"where": module.label(node), "function": node.name, "default": unparse(value)}
                    )
                elif isinstance(value, ast.Constant) and value.value is None:
                    sentinels.append(f"{module.relative}::{node.name}")

    evidence = []
    if not with_defaults:
        evidence.append("no function declares a default argument")
    if offenders:
        evidence.append(f"{len(offenders)} mutable default argument(s)")

    return result(
        with_defaults and not offenders,
        {
            "functions_with_defaults": with_defaults,
            "mutable_defaults": offenders,
            "none_sentinels": sentinels,
        },
        evidence,
    )


def check_isolated_async_tests(workspace, config):
    settings = tool_config(workspace.root, "pytest.ini_options")
    dependencies = declared_dependencies(workspace.root)

    async_tests = []
    markers = []
    for module in workspace.tests:
        for node in module.functions():
            if isinstance(node, ast.AsyncFunctionDef):
                async_tests.append(f"{module.relative}::{node.name}")
            for decorator in decorator_names(node):
                if "asyncio" in decorator or "anyio" in decorator:
                    markers.append(f"{module.relative}::{node.name}")

    mode = settings.get("asyncio_mode")
    runner_declared = bool({"pytest-asyncio", "anyio", "pytest-anyio", "trio"} & dependencies)
    configured = bool(mode) or bool(markers)
    warnings_as_errors = any(
        "error" in item for item in as_patterns(settings.get("filterwarnings"))
    )

    evidence = []
    if not async_tests:
        evidence.append("no async test functions")
    if not configured:
        evidence.append("no asyncio_mode setting and no asyncio/anyio marker")
    if not runner_declared:
        evidence.append("no async test runner declared as a dependency")
    if not warnings_as_errors:
        evidence.append("warnings are not treated as errors, so leaked tasks stay quiet")

    return result(
        async_tests and configured and runner_declared,
        {
            "async_tests": async_tests,
            "async_markers": markers,
            "asyncio_mode": mode,
            "async_runner_declared": runner_declared,
            "warnings_as_errors": warnings_as_errors,
        },
        evidence,
    )


def check_parametrized_cases(workspace, config):
    parametrized = []
    test_functions = []
    for module in workspace.tests:
        for node in module.functions():
            if node.name.startswith(("should_", "test_")):
                test_functions.append(f"{module.relative}::{node.name}")
            for decorator in decorator_names(node):
                if "parametrize" in decorator:
                    parametrized.append(f"{module.relative}::{node.name}")
                    break

    evidence = []
    if not parametrized:
        evidence.append("no parametrized specifications; repeated cases are spelled out one by one")

    return result(
        bool(parametrized),
        {
            "parametrized_tests": sorted(set(parametrized)),
            "test_functions": test_functions,
        },
        evidence,
    )


def check_interface_checked_mocks(workspace, config):
    unspecced = []
    specced = []
    async_doubles = []
    fakes = []
    for module in workspace.tests:
        for node in module.calls():
            name = call_name(node)
            tail = name.split(".")[-1]
            if tail in {"Mock", "MagicMock"}:
                record = {"where": module.label(node), "call": name}
                if SPEC_KEYWORDS & set(keyword_map(node)):
                    specced.append(record)
                else:
                    unspecced.append(record)
            elif tail in {"AsyncMock"}:
                async_doubles.append({"where": module.label(node), "call": name})
            elif tail in {"create_autospec"}:
                specced.append({"where": module.label(node), "call": name})
        for node in module.classes():
            if FAKE_NAME.match(node.name):
                fakes.append(f"{module.relative}::{node.name}")

    signals = {
        "specced_mocks": specced,
        "unspecced_mocks": unspecced,
        "async_mocks": async_doubles,
        "hand_written_fakes": fakes,
    }

    # Conditional intent. It binds a suite that substitutes something; a suite
    # that drives everything against the real collaborator has no doubles to
    # spec and is not in breach of anything.
    if not (specced or unspecced or async_doubles or fakes):
        return not_applicable(
            signals,
            ["no test doubles of any kind; nothing to check an interface against"],
        )

    evidence = []
    if unspecced:
        evidence.append(f"{len(unspecced)} Mock construction(s) with no interface spec")

    return result(not unspecced, signals, evidence)


CHECKS = {
    "craftsperson/python/colocated-module-specifications": check_colocated_specs,
    "craftsperson/python/deterministic-resource-cleanup": check_resource_cleanup,
    "craftsperson/python/functional-core-imperative-shell": check_functional_core,
    "craftsperson/python/gateway-only-mocking": check_gateway_mocking,
    "craftsperson/python/graceful-async-timeouts-and-cancellation": check_async_timeouts,
    "craftsperson/python/immutable-domain-models": check_immutable_models,
    "craftsperson/python/interface-checked-mocks": check_interface_checked_mocks,
    "craftsperson/python/isolated-async-tests": check_isolated_async_tests,
    "craftsperson/python/native-modern-type-syntax": check_modern_type_syntax,
    "craftsperson/python/native-pytest-assertions": check_native_assertions,
    "craftsperson/python/no-shared-mutable-defaults": check_mutable_defaults,
    "craftsperson/python/nonblocking-async-io": check_nonblocking_async_io,
    "craftsperson/python/parametrized-behavior-cases": check_parametrized_cases,
    "craftsperson/python/readable-bdd-specifications": check_bdd_specifications,
    "craftsperson/python/specific-domain-errors": check_domain_errors,
    "craftsperson/python/structured-concurrent-lifetimes": check_structured_concurrency,
}


# --------------------------------------------------------------------------
# rate-card: Rust
#
# Same question forms, different substrate. Rust moves several of them: test
# scope is an attribute rather than a directory, so production and test code
# share a file; panicking is a macro, not a call; and substituting a
# collaborator is implementing a trait, not patching a name.
# --------------------------------------------------------------------------

PANIC_MACROS = {"panic", "unreachable", "todo", "unimplemented"}

FALLIBLE_SHORTCUTS = {"unwrap", "expect"}

PRINT_MACROS = {"println", "print", "eprintln", "eprint", "dbg"}

TRACING_MACROS = {"trace", "debug", "info", "warn", "error", "event", "span", "info_span"}

DOCUMENTED_KINDS = {"fn", "struct", "enum", "trait", "mod", "type", "const"}


def check_rust_result_errors(workspace, config):
    production = [module for module in workspace.modules if module.is_source]

    shortcuts = rust_calls(production, FALLIBLE_SHORTCUTS, in_test=False)
    panics = rust_macros(production, PANIC_MACROS, in_test=False)
    fallible_api = [
        f"{module.relative}::{item['name']}"
        for module in production
        for item in module.production_items("fn")
        if item["visibility"].startswith("pub") and "Result" in (item.get("return_type") or "")
    ]

    evidence = []
    if not fallible_api:
        evidence.append("no public function returns a Result")
    if shortcuts:
        evidence.append(f"{len(shortcuts)} unwrap/expect outside test code")
    if panics:
        evidence.append(f"{len(panics)} panicking macro(s) outside test code")

    return result(
        fallible_api and not shortcuts and not panics,
        {
            "public_fallible_functions": fallible_api,
            "unwrap_or_expect_in_production": shortcuts,
            "panicking_macros_in_production": panics,
        },
        evidence,
    )


def check_rust_gateway_traits(workspace, config):
    production = [module for module in workspace.modules if module.is_source]
    effectful = {module.relative for module in production if module.performs_io()}

    declared = {}
    for module in production:
        for item in module.production_items("trait"):
            declared[item["name"]] = module.relative

    boundary_impls = []
    for module in production:
        for item in module.production_items("impl"):
            implemented = item.get("impl_trait")
            if implemented and implemented in declared:
                boundary_impls.append(
                    {
                        "where": module.label(item),
                        "trait": implemented,
                        "type": item.get("impl_type"),
                        "at_effect_boundary": module.relative in effectful,
                    }
                )

    # What the intent asks is that the core depend on the contract rather than on
    # a concrete client. Which file the trait is declared in is not that
    # question: a trait and its implementation living together is ordinary Rust.
    # So the test is whether some module that performs no I/O names the trait.
    consumers = {
        name: [
            module.relative
            for module in production
            if module.relative not in effectful
            and module.relative != where
            and name in module.source
        ]
        for name, where in declared.items()
    }
    depended_upon = {name: where for name, where in consumers.items() if where}

    evidence = []
    if not declared:
        evidence.append("no gateway trait is declared")
    if not boundary_impls:
        evidence.append("no concrete type implements a declared gateway trait")
    elif not depended_upon:
        evidence.append("no effect-free module depends on a gateway trait")

    return result(
        bool(boundary_impls and depended_upon),
        {
            "declared_traits": declared,
            "effect_free_consumers": consumers,
            "effectful_modules": sorted(effectful),
            "trait_implementations": boundary_impls,
        },
        evidence,
    )


def check_rust_fakes(workspace, config):
    production = [module for module in workspace.modules if module.is_source]
    declared = {
        item["name"]
        for module in production
        for item in module.production_items("trait")
    }

    # An in-memory implementation of an owned contract is a fake wherever it
    # lives. Requiring it to sit in test scope would fail a crate that ships one
    # as a testing affordance, which is a normal and good thing to do.
    fakes = []
    for module in workspace.modules:
        for item in module.items:
            if item["kind"] != "impl" or item.get("impl_trait") not in declared:
                continue
            in_tests = item["in_test_scope"] or module.is_test
            named_as_double = bool(FAKE_NAME.match(item.get("impl_type") or ""))
            if not (in_tests or named_as_double):
                continue
            fakes.append(
                {
                    "where": module.label(item),
                    "trait": item["impl_trait"],
                    "type": item.get("impl_type"),
                    "in_test_scope": in_tests,
                }
            )

    has_tests = any(module.is_test for module in workspace.modules) or any(
        module.has_inline_test_module() for module in production
    )

    signals = {"owned_traits": sorted(declared), "test_fakes": fakes}

    if not has_tests:
        return not_applicable(signals, ["no tests, so nothing was substituted either way"])
    if not declared:
        return not_applicable(
            signals, ["no owned trait to substitute; the gateway intent covers that"]
        )

    evidence = []
    if not fakes:
        evidence.append("tests substitute nothing that implements an owned trait")

    return result(bool(fakes), signals, evidence)


def check_rust_test_layers(workspace, config):
    production = [module for module in workspace.modules if module.is_source]

    inline = [module.relative for module in production if module.has_inline_test_module()]
    integration = [module.relative for module in workspace.modules if module.is_test]
    doctests = [
        f"{module.relative}::{item['name']}"
        for module in production
        for item in module.items
        if "```" in item.get("doc", "")
    ]

    evidence = []
    if not inline:
        evidence.append("no inline #[cfg(test)] module for unit-level behaviour")
    if not integration:
        evidence.append("no tests/*.rs for wiring and cross-module behaviour")

    return result(
        inline and integration,
        {
            "inline_unit_modules": inline,
            "integration_test_files": integration,
            "documented_examples": doctests,
        },
        evidence,
    )


def check_rust_lint_policy(workspace, config):
    manifest = cargo_manifest(workspace.root)
    lints = manifest.get("lints", {})
    declared = sorted(lints.keys())

    crate_level = [
        attribute
        for module in workspace.modules
        for attribute in module.crate_attrs
        if attribute.startswith(("deny", "warn", "allow", "forbid"))
    ]

    evidence = []
    if not lints:
        evidence.append("Cargo.toml declares no [lints] policy")
        if crate_level:
            evidence.append(
                f"{len(crate_level)} crate-level lint attribute(s) instead of a central policy"
            )

    return result(
        bool(lints),
        {
            "cargo_lint_tables": declared,
            "cargo_lints": {key: sorted(value) for key, value in lints.items() if isinstance(value, dict)},
            "crate_level_lint_attributes": crate_level,
        },
        evidence,
    )


def check_rust_functional_core(workspace, config):
    rule_pattern = re.compile(config["business_rule_pattern"])
    minimum = config.get("business_rule_minimum_matches", 2)
    production = [module for module in workspace.modules if module.is_source]

    pure = []
    effectful = []
    rule_bearing = []
    mixed = []
    for module in production:
        does_io = module.performs_io()
        carries_rules = len(set(rule_pattern.findall(module.source))) >= minimum
        (effectful if does_io else pure).append(module.relative)
        if carries_rules:
            rule_bearing.append(module.relative)
        if carries_rules and does_io:
            mixed.append(module.relative)

    isolated = [name for name in rule_bearing if name not in mixed]

    evidence = []
    if not rule_bearing:
        evidence.append("the scenario's business rules were not found in any module")
    if mixed:
        evidence.append(f"{len(mixed)} module(s) hold both the business rules and I/O")
    if not effectful:
        evidence.append("no module performs the external read")

    return result(
        isolated and not mixed and effectful,
        {
            "pure_modules": pure,
            "effectful_modules": effectful,
            "rule_bearing_modules": rule_bearing,
            "modules_mixing_rules_and_io": mixed,
        },
        evidence,
    )


def check_rust_tracing(workspace, config):
    production = [module for module in workspace.modules if module.is_source]

    prints = rust_macros(production, PRINT_MACROS, in_test=False)
    tracing_declared = any(
        any(use.startswith("tracing") for use in module.uses) for module in production
    ) or "tracing" in cargo_manifest(workspace.root).get("dependencies", {})
    emissions = rust_macros(production, TRACING_MACROS, in_test=False) if tracing_declared else []
    structured = [item for item in emissions if "=" in item["arguments"]]
    instrumented = [
        f"{module.relative}::{item['name']}"
        for module in production
        for item in module.production_items()
        if any("instrument" in attribute for attribute in item.get("attrs", []))
    ]

    evidence = []
    if not tracing_declared:
        evidence.append("the tracing ecosystem is not used")
    if prints:
        evidence.append(f"{len(prints)} print macro(s) in production code")
    if tracing_declared and not structured:
        evidence.append("tracing emits interpolated strings rather than named fields")

    return result(
        tracing_declared and structured and not prints,
        {
            "print_macros": prints,
            "tracing_declared": tracing_declared,
            "tracing_emissions": emissions,
            "structured_emissions": structured,
            "instrumented_functions": instrumented,
        },
        evidence,
    )


def check_rust_public_docs(workspace, config):
    production = [module for module in workspace.modules if module.is_source]

    # `pub mod card;` carries no doc comment of its own; rustdoc takes the
    # module's documentation from the `//!` at the top of `card.rs`. Treating the
    # declaration as undocumented would fail idiomatic Rust.
    documented_modules = {
        module.relative.rsplit("/", 1)[-1].removesuffix(".rs")
        for module in production
        if module.crate_doc.strip()
    }

    # The skeleton is stripped of doc comments so it carries no precedent, which
    # would otherwise make every arm inherit its undocumented items as
    # violations. Score what the agent produced, not what it was handed.
    inherited = baseline_public_items(config)

    public = []
    undocumented = []
    for module in production:
        for item in module.production_items():
            if item["kind"] not in DOCUMENTED_KINDS:
                continue
            if not item["visibility"].startswith("pub"):
                continue
            label = f"{module.relative}::{item['name']}"
            if label in inherited:
                continue
            public.append(label)
            if item.get("doc", "").strip():
                continue
            if item["kind"] == "mod" and item["name"] in documented_modules:
                continue
            undocumented.append(label)

    crate_docs = [module.relative for module in production if module.crate_doc.strip()]
    signals_inherited = sorted(inherited)

    evidence = []
    if not public:
        evidence.append("the crate exposes no public items")
    if undocumented:
        evidence.append(f"{len(undocumented)} public item(s) carry no doc comment")
    if not crate_docs:
        evidence.append("no crate-level documentation")

    return result(
        public and not undocumented,
        {
            "public_items": public,
            "undocumented_public_items": undocumented,
            "crate_documented_files": crate_docs,
            "inherited_from_skeleton": signals_inherited,
        },
        evidence,
    )


CHECKS.update(
    {
        "craftsperson/rust/centralize-curated-lint-policy": check_rust_lint_policy,
        "craftsperson/rust/compile-public-documentation": check_rust_public_docs,
        "craftsperson/rust/isolate-functional-core-from-effects": check_rust_functional_core,
        "craftsperson/rust/prefer-fakes-at-boundaries": check_rust_fakes,
        "craftsperson/rust/put-gateways-at-effect-boundaries": check_rust_gateway_traits,
        "craftsperson/rust/use-purpose-specific-test-layers": check_rust_test_layers,
        "craftsperson/rust/use-results-for-recoverable-library-errors": check_rust_result_errors,
        "craftsperson/rust/use-structured-tracing": check_rust_tracing,
    }
)
