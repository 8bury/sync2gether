#!/usr/bin/env bash
# Empacota sync2gether.app e o .dmg no macOS, incluindo libmpv e dependências.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

arch=${1:?Uso: bash scripts/package-macos.sh arm64|x86_64 [target-triple]}
target=${2:-}
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
target_dir="${CARGO_TARGET_DIR:-target}"
if [[ -n "$target" ]]; then
  binary="$target_dir/$target/release/sync2gether"
else
  binary="$target_dir/release/sync2gether"
fi

test -x "$binary"
test "$("$binary" --version)" = "sync2gether $version"
help=$("$binary" --help)
[[ "$help" != *--demo* ]]

brew_prefix=$(brew --prefix)
mpv_prefix=$(brew --prefix mpv)
libmpv="$mpv_prefix/lib/libmpv.2.dylib"
test -f "$libmpv"

output="$PWD/target/dist"
work="$PWD/target/packaging/macos-$arch"
app="$work/sync2gether.app"
dmg_stage="$work/dmg"
dmg_out="$output/sync2gether-${version}-macos-${arch}.dmg"

rm -rf "$work"
mkdir -p "$output" "$app/Contents/MacOS" "$app/Contents/Frameworks" "$dmg_stage"

install -m755 "$binary" "$app/Contents/MacOS/sync2gether"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/Contents/MacOS/sync2gether" 2>/dev/null || true

python3 - "$app/Contents/MacOS/sync2gether" "$libmpv" "$app/Contents/Frameworks" "$brew_prefix" <<'PY'
import os
import shutil
import subprocess
import sys
from collections import deque
from pathlib import Path

exe = Path(sys.argv[1])
root_lib = Path(sys.argv[2])
frameworks = Path(sys.argv[3])
brew_prefix = Path(sys.argv[4])


def is_system(dep: str) -> bool:
    return dep.startswith("/System/") or dep.startswith("/usr/lib/")


def otool_deps(path: Path) -> list[str]:
    out = subprocess.check_output(["otool", "-L", str(path)], text=True)
    deps = []
    for line in out.splitlines()[1:]:
        dep = line.strip().split(" (", 1)[0]
        if dep:
            deps.append(dep)
    return deps


def otool_rpaths(path: Path) -> list[str]:
    out = subprocess.check_output(["otool", "-l", str(path)], text=True)
    lines = out.splitlines()
    rpaths = []
    for i, line in enumerate(lines):
        if "cmd LC_RPATH" in line:
            for j in range(i + 1, min(i + 6, len(lines))):
                if "path " in lines[j]:
                    rpaths.append(lines[j].strip().split("path ", 1)[1].split(" (", 1)[0])
                    break
    return rpaths


def resolve_dep(dep: str, source_real: Path, rpaths: list[str]) -> Path:
    if dep.startswith("@loader_path/"):
        candidate = source_real.parent / dep.removeprefix("@loader_path/")
        if candidate.exists():
            return candidate
    if dep.startswith("@rpath/"):
        rel = dep.removeprefix("@rpath/")
        for rp in rpaths:
            base = Path(rp.replace("@loader_path", str(source_real.parent)))
            candidate = base / rel
            if candidate.exists():
                return candidate
        for base in [source_real.parent, brew_prefix / "lib"]:
            candidate = base / rel
            if candidate.exists():
                return candidate
    if dep.startswith("/"):
        candidate = Path(dep)
        if candidate.exists():
            return candidate
    candidate = brew_prefix / "lib" / Path(dep).name
    if candidate.exists():
        return candidate
    raise FileNotFoundError(f"Não foi possível localizar dependência {dep} de {source_real}")


copied: dict[str, Path] = {}
queue: deque[tuple[str, Path]] = deque()


def enqueue(dep_name: str, src: Path) -> None:
    if dep_name in copied:
        return
    real = src.resolve()
    dst = frameworks / dep_name
    shutil.copy2(real, dst)
    os.chmod(dst, 0o755)
    copied[dep_name] = real
    queue.append((dep_name, real))


enqueue("libmpv.2.dylib", root_lib)

for dep in otool_deps(exe):
    if is_system(dep) or dep.startswith("@executable_path/"):
        continue
    resolved = resolve_dep(dep, exe.resolve(), otool_rpaths(exe))
    name = Path(dep).name
    enqueue(name, resolved)
    subprocess.check_call(
        ["install_name_tool", "-change", dep, f"@executable_path/../Frameworks/{name}", str(exe)]
    )

while queue:
    name, real_src = queue.popleft()
    dst = frameworks / name
    rpaths = otool_rpaths(real_src)
    deps = otool_deps(dst)
    subprocess.check_call(["install_name_tool", "-id", f"@loader_path/{name}", str(dst)])
    for dep in deps:
        if is_system(dep) or dep == f"@loader_path/{name}":
            continue
        if Path(dep).name == name and (dep.startswith("/") or dep.startswith("@")):
            continue
        resolved = resolve_dep(dep, real_src, rpaths)
        dep_name = Path(dep).name
        enqueue(dep_name, resolved)
        subprocess.check_call(
            ["install_name_tool", "-change", dep, f"@loader_path/{dep_name}", str(dst)]
        )

leaked = []
for target_bin in [exe, *sorted(frameworks.glob("*.dylib"))]:
    for dep in otool_deps(target_bin):
        if dep.startswith("/opt/homebrew/") or dep.startswith("/usr/local/"):
            leaked.append(f"{target_bin.name} -> {dep}")
if leaked:
    raise SystemExit("Dependências externas restantes:\n" + "\n".join(leaked))

print(f"Empacotadas {len(copied)} bibliotecas em Contents/Frameworks.")
PY

for dylib in "$app"/Contents/Frameworks/*.dylib; do
  codesign --force --sign - "$dylib"
done
codesign --force --sign - "$app/Contents/MacOS/sync2gether"
codesign --force --sign - "$app"

test "$("$app/Contents/MacOS/sync2gether" --version)" = "sync2gether $version"

cp -R "$app" "$dmg_stage/"
ln -s /Applications "$dmg_stage/Applications"
for attempt in 1 2 3 4 5; do
  rm -f "$dmg_out"
  if hdiutil create \
    -volname "sync2gether $version" \
    -srcfolder "$dmg_stage" \
    -ov \
    -format UDZO \
    "$dmg_out"; then
    break
  fi
  if [[ "$attempt" -eq 5 ]]; then
    exit 1
  fi
  sleep 3
done
test -s "$dmg_out"
