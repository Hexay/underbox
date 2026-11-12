"""Names for captured textures from the seeded core's `.vtex_c` (docs/differential-gpu.md, "Textures").

The capture layer hashes each uploaded mip as the sum (mod 2^64) of zlib CRC-32s of its block rows, each row
seeded by the CRC-32 of (layer, z, row, x) as four little-endian uint32 (tools/diff/gpu/uploads.cpp). Hashing
mip 0 of a `.vtex_c` the same way names the file an engine uploaded unchanged (real uploads block-compressed
data as stored; ours decodes it, so ours' textures are named by their debug label instead).
"""
import os
import re
import struct
import zlib

BLOCK = {1: 8, 27: 8, 23: 8, 25: 8, 2: 16, 19: 16, 20: 16, 21: 16, 24: 16, 26: 16}
TEXEL = {5: 2, 8: 2, 22: 2, 4: 4, 28: 4, 6: 4, 9: 4, 11: 4, 31: 4, 7: 8, 10: 8, 12: 8, 13: 12, 14: 16}
IMAGE_FILE = {15, 16, 17, 18, 29, 30}
CUBE, VOLUME = 0x10, 0x20


def u32(b, at):
    return struct.unpack_from("<I", b, at)[0]


def data_block(b):
    """(offset, length) of a resource container's DATA block, or None."""
    if len(b) < 16 or struct.unpack_from("<H", b, 4)[0] != 12:
        return None
    table, count = 8 + u32(b, 8), u32(b, 12)
    for i in range(count):
        e = table + i * 12
        if b[e:e + 4] == b"DATA" and e + 12 <= len(b):
            return e + 4 + u32(b, e + 4), u32(b, e + 8)
    return None


def info(b):
    """(width, height, layers, mips, format) from a VTEX DATA header in b, or None."""
    blk = data_block(b)
    if not blk or blk[0] + 40 > len(b):
        return None
    d = b[blk[0]:]
    if struct.unpack_from("<H", d, 0)[0] != 1:
        return None
    flags, w, h, depth = struct.unpack_from("<H", d, 2)[0], *struct.unpack_from("<3H", d, 20)
    if d[26] in IMAGE_FILE or flags & VOLUME:
        return None
    return w, h, max(1, depth) * (6 if flags & CUBE else 1), max(1, d[27]), d[26]


def read(path):
    """(width, height, layers, mips, format, mip 0 bytes) of a .vtex_c, or None."""
    with open(path, "rb") as f:
        b = f.read()
    i, blk = info(b), data_block(b)
    if not i or not blk:
        return None
    w, hgt, layers, mips, fmt = i
    off, n = blk
    d = b[off:off + n]
    method, sizes = None, []
    first = 32 + u32(d, 32)
    for i in range(u32(d, 36)):
        at = first + i * 12
        start = at + 4 + u32(d, at + 4)
        if u32(d, at) == 4 and u32(d, start) in (1, 2):
            method = u32(d, start)
            sizes = [u32(d, start + 4 + u32(d, start + 4) + k * 4) for k in range(u32(d, start + 8))]
    at = off + n
    for level in range(mips - 1, -1, -1):
        raw = level_bytes(fmt, w >> level, hgt >> level, layers)
        stored = sizes[level] if level < len(sizes) and sizes[level] < raw else raw
        if level == 0:
            data = b[at:at + stored]
            if stored < raw:
                if method != 2:
                    return None  # LZ4: not in the standard library, and real's core uses zstd
                from compression import zstd
                data = zstd.decompress(data)
            return w, hgt, layers, mips, fmt, data
        at += stored


def level_bytes(fmt, w, h, layers):
    w, h = max(1, w), max(1, h)
    if fmt in BLOCK:
        layers = 4 if 1 < layers < 4 else layers
        return ((w + 3) // 4) * ((h + 3) // 4) * BLOCK[fmt] * layers
    return w * h * layers * TEXEL.get(fmt, 1)


def content_hash(fmt, w, h, layers, data):
    bw, bpb = (4, BLOCK[fmt]) if fmt in BLOCK else (1, TEXEL.get(fmt, 1))
    rows, row = (h + bw - 1) // bw, ((w + bw - 1) // bw) * bpb
    total = 0
    for layer in range(layers):
        for y in range(rows):
            at = (layer * rows + y) * row
            total += zlib.crc32(data[at:at + row], zlib.crc32(struct.pack("<4I", layer, 0, y, 0)))
    return f"{total & (2 ** 64 - 1):016x}"


def source_name(rel):
    """A compiled texture's source, as ours labels it: `x_tga_1234abcd.generated.vtex_c` -> `x.tga`."""
    m = re.match(r"(.*)_([a-z0-9]+)_[0-9a-f]+\.generated\.vtex(?:_c)?$", rel)
    return f"{m.group(1)}.{m.group(2)}" if m else re.sub(r"\.vtex_c$", ".vtex", rel)


def index(core, wanted):
    """{content hash: [source names]} for the core textures whose (w, h, layers, mips) is in wanted; flat
    defaults (white, flat normal) are byte-identical across many files, hence a list."""
    out = {}
    if not core or not os.path.isdir(core):
        return out
    for root, _, files in os.walk(core):
        for f in files:
            if not f.endswith(".vtex_c"):
                continue
            path = os.path.join(root, f)
            try:
                with open(path, "rb") as fh:
                    i = info(fh.read(65536))
                t = read(path) if i and i[:4] in wanted else None
            except (OSError, struct.error, ValueError, IndexError):
                continue
            if t:
                w, h, layers, mips, fmt, data = t
                out.setdefault(content_hash(fmt, w, h, layers, data), set()).add(source_name(os.path.relpath(path, core)))
    return {h: sorted(names) for h, names in out.items()}
