#!/usr/bin/env python3
"""Refuse text that is UTF-8 read as a legacy codepage and written back.

An em dash is three bytes of UTF-8. Read those bytes as cp1252 -- which is what
an editor, a terminal or a script does when it is told nothing and guesses -- and
they come back as three separate characters, which then get saved as nine bytes.
The result still parses, still compiles, and still passes every test. It is only
visible to a person reading the file, and by then it has usually spread.

The check works forwards rather than by pattern-matching damage: take the
characters this tree legitimately uses, put each through that exact round trip,
and look for what comes out. Nothing else is guessed at, so a file is either
carrying a known mangling or it is not.
"""

from __future__ import annotations

import sys
from pathlib import Path

# Characters that appear in this tree for real, and are worth catching mangled.
ORIGINALS = "—–‘’“”…·×÷°±≤≥→←✓✗éèêëáàâäíìîïóòôöúùûüñçßαβπ«»"

SKIP_DIRS = {
    ".git",
    "target",
    "node_modules",
    ".dotnet",
    ".node",
    ".cargo",
    "dist",
    "build",
    "gen",
}

# Only text this project authors. A binary or a vendored file is not ours to
# re-encode, and a false positive in one would be a check nobody can satisfy.
SUFFIXES = {
    ".rs",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".css",
    ".html",
    ".json",
    ".toml",
    ".md",
    ".yml",
    ".yaml",
    ".sh",
    ".fish",
    ".py",
}


def manglings() -> dict[str, str]:
    """Each mangled form mapped back to the character it should have been."""
    out: dict[str, str] = {}
    for ch in ORIGINALS:
        try:
            broken = ch.encode("utf-8").decode("cp1252")
        except UnicodeDecodeError:
            # Not every byte is a cp1252 character; those manglings cannot occur.
            continue
        out[broken] = ch
    return out


def main(argv: list[str]) -> int:
    root = Path(argv[1] if len(argv) > 1 else ".")
    table = manglings()

    findings: list[tuple[Path, int, str, str]] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix not in SUFFIXES:
            continue
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for number, line in enumerate(text.splitlines(), start=1):
            for broken, original in table.items():
                if broken in line:
                    findings.append((path, number, broken, original))

    if not findings:
        return 0

    print("Text that was UTF-8, read as cp1252, and written back:")
    print()
    for path, number, broken, original in findings:
        rel = path.relative_to(root) if path.is_relative_to(root) else path
        print(f"  {rel}:{number}: {broken!r} should be {original!r}")
    print()
    print(f"{len(findings)} to fix. Save the file as UTF-8 and put the character back.")
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
