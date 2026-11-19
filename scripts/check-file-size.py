#!/usr/bin/env python3
import subprocess
import sys
from pathlib import Path

MAX_LINES = 950
SOURCE_ROOTS = (("src",), ("relay", "src"))


def is_source_file(path: Path) -> bool:
    return any(path.parts[: len(root)] == root for root in SOURCE_ROOTS)


def production_lines(src: str) -> int:
    lines = src.splitlines()
    for i, line in enumerate(lines):
        if line.strip() != "#[cfg(test)]":
            continue
        j = i + 1
        while j < len(lines) and not lines[j].strip():
            j += 1
        if j < len(lines) and lines[j].lstrip().startswith("mod tests"):
            return i
    return len(lines)


def main():
    limit = MAX_LINES
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == "--limit":
        limit = int(args[1])
    elif args:
        print("Uso: check-file-size.py [--limit N]", file=sys.stderr)
        sys.exit(2)

    files = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "*.rs"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()

    oversized = []
    for name in sorted(set(files)):
        path = Path(name)
        if not path.is_file() or not is_source_file(path):
            continue
        count = production_lines(path.read_text())
        if count > limit:
            oversized.append((count, name))

    if oversized:
        for count, name in sorted(oversized, reverse=True):
            print(f"{name}: {count} linhas de código (limite: {limit})")
        print(
            "Divida módulos grandes em submódulos coesos "
            "(blocos `#[cfg(test)] mod tests` e `tests/` não entram na conta)."
        )
        sys.exit(1)


main()
