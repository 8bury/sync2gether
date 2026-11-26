#!/usr/bin/env python3
import sys
from pathlib import Path

SOURCE_DIRS = ("src", "relay/src", "core/src", "web/src", "tests", "examples")


def comments(src):
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            end = src.find("\n", i)
            end = n if end < 0 else end
            yield i, end
            i = end
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            yield i, j
            i = j
        elif (c == "r" or src.startswith(("br", "cr"), i)) and not (
            i and (src[i - 1].isalnum() or src[i - 1] == "_")
        ):
            j = i + (1 if c == "r" else 2)
            hashes = 0
            while j < n and src[j] == "#":
                hashes, j = hashes + 1, j + 1
            if j < n and src[j] == '"':
                close = '"' + "#" * hashes
                end = src.find(close, j + 1)
                i = n if end < 0 else end + len(close)
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            i = j + 1
        elif c == "'":
            if i + 2 < n and src[i + 1] == "\\":
                end = src.find("'", i + 2)
                i = end + 1
            elif i + 2 < n and src[i + 2] == "'":
                i += 3
            else:
                i += 1
        else:
            i += 1


def allowed(src, spans, k):
    start, end = spans[k]
    while True:
        text = src[start:end]
        if text.startswith("// SAFETY:"):
            return True
        line_start = src.rfind("\n", 0, start) + 1
        if not text.startswith("//") or src[line_start:start].strip() or k == 0:
            return False
        prev_start, prev_end = spans[k - 1]
        between = src[prev_end:line_start]
        prev_line = src.rfind("\n", 0, prev_start) + 1
        if between.strip() or src[prev_line:prev_start].strip():
            return False
        k -= 1
        start, end = spans[k]


def strip(src, spans):
    out, last = [], 0
    for start, end in spans:
        line_start = src.rfind("\n", 0, start) + 1
        line_end = src.find("\n", end)
        line_end = len(src) if line_end < 0 else line_end
        before, after = src[line_start:start], src[end:line_end]
        if not before.strip() and not after.strip():
            out.append(src[last:line_start])
            last = min(line_end + 1, len(src))
        else:
            out.append(src[last:start].rstrip(" \t") if not after.strip() else src[last:start])
            last = end
    out.append(src[last:])
    return "".join(out)


def main():
    fix = "--fix" in sys.argv[1:]
    files = sorted(
        path.as_posix()
        for root in SOURCE_DIRS
        if Path(root).is_dir()
        for path in Path(root).rglob("*.rs")
    )
    found = False
    for name in files:
        path = Path(name)
        src = path.read_text()
        spans = list(comments(src))
        bad = [s for k, s in enumerate(spans) if not allowed(src, spans, k)]
        if not bad:
            continue
        if fix:
            path.write_text(strip(src, bad))
            continue
        found = True
        for start, end in bad:
            line = src.count("\n", 0, start) + 1
            print(f"{name}:{line}: comentário proibido: {src[start:end].splitlines()[0]}")
    if found:
        print("Só `// SAFETY:` em blocos unsafe é permitido. Expresse a intenção no código.")
        sys.exit(1)


main()
