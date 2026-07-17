"""haw_plugin — a thin Python binding for the haw plugin contract.

haw dispatches ``haw <name> ...`` to a ``haw-<name>`` executable on PATH,
passing the workspace context as a ``haw.plugin/1`` JSON document on the
``HAW_JSON`` environment variable and on stdin. A plugin prints a
``haw.plugin.report/1`` (for lifecycle phases) or a ``haw.plugin.view/1``
(for TUI render intent) document to stdout.

The JSON Schemas in ``schemas/`` are the source of truth for these shapes;
this module mirrors them. No dependencies beyond the standard library.

Example
-------
    from haw_plugin import Context, Report, Finding

    ctx = Context.from_env()
    rep = Report(plugin="hello", ok=True, summary="hi from python")
    for repo in ctx.repos:
        rep.findings.append(Finding("info", f"saw repo {repo['name']}"))
    rep.emit()
"""

from __future__ import annotations

import json
import os
import sys
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

__all__ = [
    "CONTRACT",
    "REPORT_SCHEMA",
    "VIEW_SCHEMA",
    "Context",
    "Report",
    "Finding",
    "Artifact",
    "view",
]

#: The context schema haw passes to plugins.
CONTRACT = "haw.plugin/1"
#: The report schema plugins emit on stdout for lifecycle phases.
REPORT_SCHEMA = "haw.plugin.report/1"
#: The view schema plugins emit on stdout under render intent.
VIEW_SCHEMA = "haw.plugin.view/1"


@dataclass
class Context:
    """The parsed ``haw.plugin/1`` context handed to the plugin."""

    schema: str = CONTRACT
    root: Optional[str] = None
    stack: Optional[str] = None
    repos: List[Dict[str, Any]] = field(default_factory=list)
    phase: Optional[str] = None
    intent: Optional[str] = None
    #: The full raw document, so callers can read fields not modelled here.
    raw: Dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "Context":
        """Parse a context defensively from an already-decoded dict."""
        if not isinstance(data, dict):
            data = {}
        repos = data.get("repos")
        if not isinstance(repos, list):
            repos = []
        return cls(
            schema=str(data.get("schema", CONTRACT)),
            root=data.get("root"),
            stack=data.get("stack"),
            repos=[r for r in repos if isinstance(r, dict)],
            phase=data.get("phase"),
            intent=data.get("intent"),
            raw=data,
        )

    @classmethod
    def from_env(cls) -> "Context":
        """Read the context from ``HAW_JSON``, falling back to stdin.

        Never raises on malformed input: an unparseable or missing document
        yields a schema-only context (mirroring haw's own fail-open behaviour
        when run outside a workspace).
        """
        body = os.environ.get("HAW_JSON")
        if body is None or body.strip() == "":
            try:
                if not sys.stdin.isatty():
                    body = sys.stdin.read()
            except (OSError, ValueError):
                body = None
        if not body or not body.strip():
            return cls()
        try:
            data = json.loads(body)
        except (ValueError, TypeError):
            return cls()
        return cls.from_dict(data)

    def is_render(self) -> bool:
        """True when haw is asking for a human-readable TUI panel.

        Signalled by ``HAW_RENDER=1`` in the environment or ``intent="render"``
        in the context document.
        """
        if os.environ.get("HAW_RENDER") == "1":
            return True
        return self.intent == "render"


@dataclass
class Artifact:
    """One artifact a plugin produced (``haw.plugin.report/1`` artifacts[])."""

    path: str
    #: Conventionally one of: sbom, signature, provenance, log, report.
    kind: str

    def to_dict(self) -> Dict[str, str]:
        return {"path": self.path, "kind": self.kind}


@dataclass
class Finding:
    """One finding a plugin surfaced (``haw.plugin.report/1`` findings[])."""

    #: One of: info, warn, error.
    level: str
    message: str

    def to_dict(self) -> Dict[str, str]:
        return {"level": self.level, "message": self.message}


@dataclass
class Report:
    """A ``haw.plugin.report/1`` document a plugin emits for a lifecycle phase."""

    plugin: str
    ok: bool
    summary: str = ""
    phase: Optional[str] = None
    artifacts: List[Artifact] = field(default_factory=list)
    findings: List[Finding] = field(default_factory=list)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "schema": REPORT_SCHEMA,
            "plugin": self.plugin,
            "phase": self.phase,
            "ok": self.ok,
            "summary": self.summary,
            "artifacts": [a.to_dict() for a in self.artifacts],
            "findings": [f.to_dict() for f in self.findings],
        }

    def to_json(self) -> str:
        return json.dumps(self.to_dict())

    def emit(self, stream: Any = None) -> None:
        """Print the report as ``haw.plugin.report/1`` JSON to stdout."""
        print(self.to_json(), file=stream or sys.stdout)


def view(title: str, lines: List[str], stream: Any = None) -> None:
    """Print a ``haw.plugin.view/1`` panel document to stdout.

    Use under render intent (``Context.is_render()``) to show a structured
    panel in the haw cockpit.
    """
    doc = {"schema": VIEW_SCHEMA, "title": title, "lines": list(lines)}
    print(json.dumps(doc), file=stream or sys.stdout)
