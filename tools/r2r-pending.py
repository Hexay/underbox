#!/usr/bin/env python3
"""List the IL-only managed dlls in a directory that still need ReadyToRun (tools/r2r-managed.sh).

usage: r2r-pending.py <dir>             print the pending dlls' paths
       r2r-pending.py --failed <dir>    record the dlls still IL-only as failed, so they aren't retried

An assembly's own bytes decide, not its mtime: a no-op `dotnet build` copies IL dlls back over R2R
ones with their old mtimes. A failure is keyed by size and mtime, so a rebuilt dll is tried again.
"""
import os
import struct
import sys

IL_MACHINE = 0x14C  # AnyCPU IL; crossgen2's output carries the target's machine (xor'd per OS)
STAMP = ".underbox-r2r-failed"


def il_only(path):
    try:
        with open(path, "rb") as f:
            head = f.read(4096)
        pe = struct.unpack_from("<I", head, 0x3C)[0]
        if head[pe:pe + 4] != b"PE\0\0":
            return False
        machine = struct.unpack_from("<H", head, pe + 4)[0]
        opt = pe + 24
        magic = struct.unpack_from("<H", head, opt)[0]
        cli = opt + (224 if magic == 0x20B else 208)  # data directory 14, the CLI header
        return machine == IL_MACHINE and struct.unpack_from("<I", head, cli)[0] != 0
    except (OSError, struct.error):
        return False


def signature(path):
    st = os.stat(path)
    return f"{os.path.basename(path)} {st.st_size} {st.st_mtime_ns}"


def main():
    record = sys.argv[1] == "--failed"
    d = sys.argv[2] if record else sys.argv[1]
    stamp = os.path.join(d, STAMP)
    try:
        failed = set(open(stamp).read().splitlines())
    except OSError:
        failed = set()
    il = [os.path.join(d, n) for n in sorted(os.listdir(d)) if n.endswith(".dll") and il_only(os.path.join(d, n))]
    if record:
        with open(stamp, "w") as f:
            f.write("".join(signature(p) + "\n" for p in il))
    else:
        print("\n".join(p for p in il if signature(p) not in failed))


if __name__ == "__main__":
    main()
