#!/usr/bin/env python3
"""Check nonblank authored Rust production lines; keep legacy ceilings explicit.

Rustfmt puts top-level module closing braces in column zero. Only an adjacent,
exact #[cfg(test)] module is excluded; other cfgs and test-only helpers count
conservatively. Code following a test module is counted too. This is a file-size
check, not a Rust parser; Clippy owns function measurement.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

LIMIT = 500
TEST_MODULE = re.compile(r"(?:pub(?:\([^)]*\))? )?mod\s+(\w+)\s*([;{])\s*$")
VENDOR = "crates/kelvo-collect/src/macos/vendor/"


def production_lines(source: str) -> int:
    lines = source.splitlines()
    count = 0
    index = 0
    while index < len(lines):
        if lines[index] == "#[cfg(test)]" and index + 1 < len(lines):
            module_index = index + 1
            while module_index < len(lines) and lines[module_index].startswith("#["):
                module_index += 1
            module = TEST_MODULE.fullmatch(lines[module_index]) if module_index < len(lines) else None
            if module:
                index = module_index + 1
                if module[2] == "{":
                    # A nested block's closing brace is indented by rustfmt.
                    while index < len(lines) and lines[index] != "}":
                        index += 1
                    if index == len(lines):
                        raise ValueError("test module lacks a rustfmt-aligned closing brace")
                    index += 1
                continue
        count += bool(lines[index].strip())
        index += 1
    return count


def source_paths(root: Path) -> list[Path]:
    paths = sorted(root.glob("crates/*/src/**/*.rs"))
    paths += sorted(root.glob("crates/*/examples/**/*.rs"))
    paths += sorted(root.glob("src-tauri/src/**/*.rs"))
    paths += sorted(root.glob("src-tauri/examples/**/*.rs"))
    paths += list(root.glob("src-tauri/build.rs"))
    test_paths: set[Path] = set()
    for path in paths:
        source = path.read_text()
        declarations = re.finditer(
            r"^#\[cfg\(test\)\]\n(?P<attrs>(?:#\[[^\n]+\]\n)*)(?:pub(?:\([^)]*\))? )?mod (?P<name>\w+);",
            source, re.MULTILINE,
        )
        for declaration in declarations:
            name = declaration["name"]
            explicit = re.search(r'#\[path = "([^"]+)"\]', declaration["attrs"])
            base = path.parent if path.name in {"mod.rs", "lib.rs", "main.rs"} else path.with_suffix("")
            if explicit:
                test_paths.add(path.parent / explicit[1])
            else:
                test_paths.add(base / f"{name}.rs")
                test_paths.update((base / name).rglob("*.rs"))
    return [p for p in paths if p not in test_paths and not p.relative_to(root).as_posix().startswith(VENDOR)]


def read_ceilings(path: Path) -> dict[str, int]:
    ceilings: dict[str, int] = {}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        entry = line.partition("#")[0].strip()
        if not entry:
            continue
        name, ceiling = entry.split()
        value = int(ceiling)
        if name in ceilings or value <= LIMIT:
            raise ValueError(f"{path}:{number}: duplicate or invalid ceiling")
        ceilings[name] = value
    return ceilings


def check(root: Path, ceilings: dict[str, int]) -> tuple[list[str], int]:
    errors = []
    seen = set()
    paths = source_paths(root)
    for path in paths:
        name = path.relative_to(root).as_posix()
        seen.add(name)
        count = production_lines(path.read_text())
        ceiling = ceilings.get(name, LIMIT)
        if count > ceiling:
            errors.append(f"{name}: {count} production lines exceed {ceiling}; split by responsibility")
        if name in ceilings and count <= LIMIT:
            errors.append(f"{name}: {count} production lines; remove the obsolete ceiling")
    for name in sorted(ceilings.keys() - seen):
        errors.append(f"{name}: ceiling has no authored production file; remove it")
    return errors, len(paths)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        ceilings = read_ceilings(root / "scripts/architecture/rust-size-ceilings.txt")
        errors, count = check(root, ceilings)
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    for error in errors:
        print(error, file=sys.stderr)
    print(f"Checked {count} authored Rust files against {LIMIT} lines and {len(ceilings)} legacy ceilings.")
    return bool(errors)


if __name__ == "__main__":
    sys.exit(main())
