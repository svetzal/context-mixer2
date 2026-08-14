#!/usr/bin/env python3
"""Reusable predicates for deciding whether code exhibits an intent.

Written after the second scenario, not the first. The fx-settlement checks were
mostly "is this name present" and "is this symbol absent", and each one grew its
own traversal. The concurrency scenario needs to ask a different kind of
question — is this call *inside* that construct, does this argument have that
shape — and answering it per-check would have produced eight more bespoke
walks.

What generalizes is the question, not the answer. These are the question forms
that recur:

- `imported_roots`, `references`, `calls` — is a symbol used at all
- `parents`, `within`, `enclosing_function` — is it used *there*
- `decorator_names`, `keyword_map`, `defaults` — what shape does this construct have
- `class_shape` — the several facts every class-level check wants
- `tool_config` — what did the project declare, wherever it declared it

Everything scenario-specific stays in the check that needs it. A constant like
"the fee tier percentages" belongs to one exercise and must not settle here.
"""

import ast
import configparser
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

MUTABLE_LITERAL_TYPES = (ast.List, ast.Dict, ast.Set, ast.ListComp, ast.DictComp, ast.SetComp)

MUTABLE_FACTORIES = {"list", "dict", "set", "bytearray", "defaultdict", "Counter", "OrderedDict"}


def unparse(node):
    """Source text for a node, or an empty string when it cannot be rendered."""
    try:
        return ast.unparse(node)
    except Exception:  # pragma: no cover - defensive on exotic trees
        return ""


class Module:
    """One parsed Python file plus the classification the checks need."""

    def __init__(self, path, root):
        self.path = path
        self.parts = path.relative_to(root).parts
        self.relative = path.relative_to(root).as_posix()
        self.source = path.read_text(encoding="utf-8", errors="replace")
        self.parse_error = None
        self._parents = None
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

    @property
    def parents(self):
        """Child-to-parent map, built once, enabling every containment question."""
        if self._parents is None:
            self._parents = {}
            for parent in ast.walk(self.tree):
                for child in ast.iter_child_nodes(parent):
                    self._parents[child] = parent
        return self._parents

    def imported_roots(self):
        roots = set()
        for node in ast.walk(self.tree):
            if isinstance(node, ast.Import):
                roots.update(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module and node.level == 0:
                roots.add(node.module.split(".")[0])
        return roots

    def imported_names(self):
        """Bare names introduced by `from x import y`, mapped to their origin."""
        names = {}
        for node in ast.walk(self.tree):
            if isinstance(node, ast.ImportFrom) and node.module:
                for alias in node.names:
                    names[alias.asname or alias.name] = f"{node.module}.{alias.name}"
        return names

    def classes(self):
        return [node for node in ast.walk(self.tree) if isinstance(node, ast.ClassDef)]

    def functions(self):
        return [node for node in ast.walk(self.tree) if is_function(node)]

    def calls(self):
        return [node for node in ast.walk(self.tree) if isinstance(node, ast.Call)]

    def label(self, node):
        """A stable, readable identifier for a finding."""
        line = getattr(node, "lineno", 0)
        return f"{self.relative}:{line}"


def is_function(node):
    return isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))


def collect(root):
    """Every Python module in a workspace, excluding tool and environment noise."""
    modules = []
    for path in sorted(root.rglob("*.py")):
        if any(part in SKIP_DIRECTORIES for part in path.relative_to(root).parts):
            continue
        modules.append(Module(path, root))
    return modules


def call_name(node):
    """The dotted callee of a Call, e.g. `asyncio.wait_for` or `AsyncMock`."""
    if not isinstance(node, ast.Call):
        return ""
    return unparse(node.func)


def matches_symbol(dotted, wanted):
    """True when a dotted reference names one of `wanted`.

    Two rules, and the second one is why this is not a substring test. A tail
    match handles the ordinary `asyncio.timeout`. A bare name matching the
    target's last segment handles `from asyncio import timeout`, which is the
    author's business and not the intent's.

    What it must never do is match any `.get` against `client.get` — a
    dictionary lookup is not an HTTP request. So the last-segment rule applies
    only when the reference is a bare name with nothing qualifying it.
    """
    segments = dotted.split(".")
    for candidate in wanted:
        target = candidate.split(".")
        if len(segments) >= len(target) and segments[-len(target) :] == target:
            return True
        if len(segments) == 1 and segments[0] == target[-1]:
            return True
    return False


def calls_to(module, wanted):
    """Every call whose callee tail-matches one of `wanted`."""
    return [node for node in module.calls() if matches_symbol(call_name(node), wanted)]


def references(module, wanted):
    """Every name or attribute reference tail-matching one of `wanted`."""
    found = []
    for node in ast.walk(module.tree):
        if isinstance(node, (ast.Name, ast.Attribute)):
            if matches_symbol(unparse(node), wanted):
                found.append(node)
    return found


def ancestors(module, node):
    """Every enclosing node, innermost first."""
    chain = []
    parents = module.parents
    current = parents.get(node)
    while current is not None:
        chain.append(current)
        current = parents.get(current)
    return chain


def within(module, node, types):
    """Is this node lexically inside one of these node types?"""
    return any(isinstance(item, types) for item in ancestors(module, node))


def enclosing_function(module, node):
    """The innermost function containing a node, or None at module scope."""
    for item in ancestors(module, node):
        if is_function(item):
            return item
    return None


def in_async_context(module, node):
    """Is this node inside an `async def`?"""
    function = enclosing_function(module, node)
    return isinstance(function, ast.AsyncFunctionDef)


def guarded_by_call(module, node, wanted):
    """Is this node inside a `with`/`async with` whose context manager is `wanted`?

    This is the containment question that timeout scopes and resource cleanup
    both reduce to: the construct exists, but it only counts where it encloses
    the operation it is supposed to bound.
    """
    for item in ancestors(module, node):
        if not isinstance(item, (ast.With, ast.AsyncWith)):
            continue
        for handler in item.items:
            if matches_symbol(unparse(handler.context_expr), wanted):
                return True
            if isinstance(handler.context_expr, ast.Call) and matches_symbol(
                call_name(handler.context_expr), wanted
            ):
                return True
    return False


def decorator_names(node):
    """Decorator source text for a definition, outermost call peeled off."""
    names = []
    for item in getattr(node, "decorator_list", []):
        names.append(unparse(item))
        if isinstance(item, ast.Call):
            names.append(unparse(item.func))
    return names


def keyword_map(node):
    """Keyword arguments of a Call, as `name -> source text`."""
    if not isinstance(node, ast.Call):
        return {}
    return {keyword.arg: unparse(keyword.value) for keyword in node.keywords if keyword.arg}


def defaults(node):
    """Every default value expression on a function, positional and keyword-only."""
    if not is_function(node):
        return []
    return [item for item in node.args.defaults + node.args.kw_defaults if item is not None]


def is_mutable_default(node):
    """Does this default expression allocate mutable state shared across calls?"""
    if isinstance(node, MUTABLE_LITERAL_TYPES):
        return True
    if isinstance(node, ast.Call):
        return call_name(node).split(".")[-1] in MUTABLE_FACTORIES
    return False


def class_shape(node):
    """The facts every class-level check wants, gathered once."""
    bases = set()
    for base in node.bases:
        text = unparse(base)
        bases.add(text)
        bases.add(text.rsplit(".", 1)[-1])
    return {
        "bases": bases,
        "decorators": decorator_names(node),
        "keywords": {
            keyword.arg: unparse(keyword.value) for keyword in node.keywords if keyword.arg
        },
        "body": "\n".join(unparse(statement) for statement in node.body),
    }


def tool_config(root, tool):
    """Merged configuration for a tool, from wherever a project declared it.

    `tool` names the pyproject table (`pytest.ini_options`) and the ini section
    heading is derived from its first segment, which covers the layouts Python
    projects actually use.
    """
    settings = {}
    table = tool.split(".")

    pyproject = root / "pyproject.toml"
    if pyproject.is_file():
        try:
            data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError:
            data = {}
        section = data.get("tool", {})
        for key in table:
            section = section.get(key, {}) if isinstance(section, dict) else {}
        if isinstance(section, dict):
            settings.update(section)

    name = table[0]
    for filename, heading in (
        (f"{name}.ini", name),
        ("setup.cfg", f"tool:{name}"),
        ("tox.ini", name),
    ):
        candidate = root / filename
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
    """Normalize an ini-or-TOML setting that may be a list or whitespace string."""
    if value is None:
        return []
    if isinstance(value, str):
        return value.split()
    return [str(item) for item in value]


def declared_requires_python(root):
    pyproject = root / "pyproject.toml"
    if not pyproject.is_file():
        return None
    try:
        data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError:
        return None
    return data.get("project", {}).get("requires-python")


def declared_dependencies(root):
    """Every dependency name a project declared, runtime and grouped."""
    pyproject = root / "pyproject.toml"
    if not pyproject.is_file():
        return set()
    try:
        data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError:
        return set()
    raw = list(data.get("project", {}).get("dependencies", []))
    for group in data.get("dependency-groups", {}).values():
        raw.extend(item for item in group if isinstance(item, str))
    for extra in data.get("project", {}).get("optional-dependencies", {}).values():
        raw.extend(extra)
    names = set()
    for item in raw:
        names.add(item.split("[")[0].split(">")[0].split("<")[0].split("=")[0].split(";")[0].strip())
    return names


def result(followed, signals, evidence):
    """The shape every check returns."""
    return {
        "applicable": True,
        "followed": bool(followed),
        "signals": signals,
        "evidence": evidence,
    }


def not_applicable(signals, evidence):
    """The verdict for an intent this workspace never had occasion to exhibit.

    Some intents are conditional: "mock only owned boundaries" binds code that
    mocks something, and "keep specs beside the module" binds code that has
    specs. Forcing a conditional intent into a boolean scores the absence of the
    condition as a violation, which is how a suite that tested everything
    against a live server got marked down for not specifying its mocks.

    A third state keeps that honest. Not-applicable intents leave the
    denominator rather than failing, so an adherence rate always means "of the
    intents this work had occasion to exhibit".
    """
    return {
        "applicable": False,
        "followed": None,
        "signals": signals,
        "evidence": evidence,
    }
