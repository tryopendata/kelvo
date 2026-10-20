#!/usr/bin/env python3
"""Check the bundled Linux agent binaries before they ship (plan Milestone C).

Each file must be a 64-bit little-endian ELF for the architecture its name says
(`kelvo-agent-<triple>`), statically linked (no PT_INTERP), and every PT_LOAD segment
must be aligned to at least the largest page size that architecture runs with. aarch64
kernels use 4K, 16K (Asahi) or 64K pages; a segment aligned below the page size fails
to load (`ELF load command address/offset not page-aligned`), so aarch64 needs 0x10000
in practice and this check fails below 0x4000. x86_64 pages are always 4K.

Reads the program headers with struct, so it needs no readelf on the macOS host or CI.

Usage: scripts/check-elf-align.py <binary>...
"""

import struct
import sys
from pathlib import Path

PT_LOAD = 1
PT_INTERP = 3

# e_machine -> (name, triple prefix, minimum PT_LOAD alignment)
MACHINES = {
    183: ("aarch64", "aarch64-", 0x4000),
    62: ("x86_64", "x86_64-", 0x1000),
}


def check(path: Path) -> list[str]:
    data = path.read_bytes()
    if data[:4] != b"\x7fELF":
        return [f"{path}: not an ELF file"]
    if data[4] != 2 or data[5] != 1:
        return [f"{path}: expected a 64-bit little-endian ELF (class {data[4]}, data {data[5]})"]

    # Elf64_Ehdr: e_machine at 18, e_phoff at 32, e_phentsize/e_phnum at 54/56.
    (e_machine,) = struct.unpack_from("<H", data, 18)
    (e_phoff,) = struct.unpack_from("<Q", data, 32)
    e_phentsize, e_phnum = struct.unpack_from("<HH", data, 54)

    machine = MACHINES.get(e_machine)
    if machine is None:
        return [f"{path}: unexpected e_machine {e_machine}"]
    name, prefix, min_align = machine

    errors = []
    triple = path.name.removeprefix("kelvo-agent-")
    if triple != path.name and not triple.startswith(prefix):
        errors.append(f"{path}: is {name}, but the file name says {triple}")

    loads = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        # Elf64_Phdr: p_type (u32), p_flags (u32), then six u64 ending in p_align.
        p_type, _flags, _offset, _vaddr, _paddr, _filesz, _memsz, p_align = struct.unpack_from(
            "<IIQQQQQQ", data, off
        )
        if p_type == PT_INTERP:
            errors.append(f"{path}: has PT_INTERP, so it is dynamically linked")
        if p_type == PT_LOAD:
            loads.append(p_align)
            if p_align < min_align:
                errors.append(
                    f"{path}: PT_LOAD #{len(loads)} aligned {p_align:#x}, below {min_align:#x} for {name}"
                )
    if not loads:
        errors.append(f"{path}: no PT_LOAD segments")
    if not errors:
        aligns = ", ".join(f"{a:#x}" for a in loads)
        print(f"check-elf-align: {path.name}: static {name} ELF, PT_LOAD align {aligns}")
    return errors


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    errors = []
    for arg in argv:
        path = Path(arg)
        if not path.is_file():
            errors.append(f"{path}: does not exist")
            continue
        errors.extend(check(path))
    for err in errors:
        print(f"check-elf-align: {err}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
