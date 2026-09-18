#!/usr/bin/env python3
"""Validate universal Base README invariants without network dependencies."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlparse

MARKDOWN_LINK_RE = re.compile(r"(?<!!)\[[^\]]+\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
MARKDOWN_IMAGE_RE = re.compile(r"!\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
HTML_IMAGE_RE = re.compile(r"<img\b[^>]*\bsrc=[\"']([^\"']+)[\"']", re.IGNORECASE)
CI_BADGE_RE = re.compile(r"actions/workflows/([^/?#]+\.ya?ml)(?:/badge\.svg|\?[^\s)]*)", re.IGNORECASE)
HEADER_LINK_RE = re.compile(r"<a\b[^>]*\bhref=[\"']#([^\"']+)[\"'][^>]*>", re.IGNORECASE)
NAMED_ANCHOR_RE = re.compile(r"<a\b[^>]*\bname=[\"']([^\"']+)[\"'][^>]*>", re.IGNORECASE)
PLACEHOLDER_RE = re.compile(r"\{\{[^{}]+\}\}")
LOCAL_PATH_RE = re.compile(r"(?<![A-Za-z0-9_])/(?:opt|root)/")


def local_target(raw: str) -> str | None:
    value = unquote(raw.strip())
    parsed = urlparse(value)
    if parsed.scheme or value.startswith("#") or value.startswith("//"):
        return None
    return parsed.path


def validate(root: Path) -> list[str]:
    readme = root / "README.md"
    if not readme.is_file():
        return ["RMD001: README.md is missing"]

    text = readme.read_text(encoding="utf-8")
    findings: list[str] = []
    images = set(MARKDOWN_IMAGE_RE.findall(text)) | set(HTML_IMAGE_RE.findall(text))
    links = set(MARKDOWN_LINK_RE.findall(text))

    for raw in sorted(images):
        target = local_target(raw)
        if target and not (root / target).is_file():
            findings.append(f"RMD003: README.md: missing local image {target}")

    for raw in sorted(links):
        target = local_target(raw)
        if target and not (root / target).exists():
            findings.append(f"RMD002: README.md: missing local link {target}")

    anchors = set(NAMED_ANCHOR_RE.findall(text))
    for target in sorted(set(HEADER_LINK_RE.findall(text))):
        if target not in anchors:
            findings.append(f"RMD004: README.md: missing explicit header anchor {target}")

    if PLACEHOLDER_RE.search(text):
        findings.append("RMD005: README.md: unresolved template placeholder")
    if LOCAL_PATH_RE.search(text):
        findings.append("RMD006: README.md: local filesystem path leaked")

    for workflow in sorted(set(CI_BADGE_RE.findall(text))):
        if not (root / ".github" / "workflows" / workflow).is_file():
            findings.append(f"RMD009: README.md: CI badge references missing workflow {workflow}")

    return findings


def main() -> int:
    findings = validate(Path.cwd())
    if findings:
        print("\n".join(findings))
        return 1
    print("README validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
