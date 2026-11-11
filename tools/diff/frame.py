"""Compare two rendered frames (ours vs real engine2) and write PNGs, a diff image and metrics.

usage: python3 -I tools/diff/frame.py <ours.ppm|.xwd> <real.ppm|.xwd> <out-dir> [threshold=16]

Inputs are binary PPM (our UNDERBOX_FRAMES dump) or XWD (Xvfb's -fbdir screen; the window is
cropped from its top-left at the other frame's size). Writes ours.png, real.png, diff.png (per-pixel
max channel error, x4) and metrics.txt: per-channel mean abs error, the share of pixels whose max
channel error exceeds the threshold, mean luma per side, and SSIM on luma over 8x8 blocks.
Standard library only.
"""
import os
import struct
import sys

sys.dont_write_bytecode = True
sys.path.insert(0,os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from ppm2png import read_ppm, write_png  # noqa: E402


def read_xwd(path):
    data = open(path, "rb").read()
    # Xvfb writes the header in host byte order; version 7 tells which.
    head = struct.unpack(">25I", data[:100])
    if head[1] != 7:
        head = struct.unpack("<25I", data[:100])
    size, fmt, depth, w, h = head[0], head[2], head[3], head[4], head[5]
    bpp, bpl, ncolors = head[11], head[12], head[19]
    if fmt != 2 or bpp != 32 or depth < 24:
        sys.exit(f"{path}: want a 32-bit ZPixmap XWD (format {fmt}, {bpp} bpp, depth {depth})")
    big = head[7] == 1
    pix = memoryview(data)[size + ncolors * 12:]
    rgb = bytearray(w * h * 3)
    r, g, b = (1, 2, 3) if big else (2, 1, 0)
    for y in range(h):
        row = pix[y * bpl:y * bpl + w * 4]
        o = y * w * 3
        rgb[o:o + w * 3:3] = row[r::4]
        rgb[o + 1:o + w * 3:3] = row[g::4]
        rgb[o + 2:o + w * 3:3] = row[b::4]
    return w, h, bytes(rgb)


def load(path):
    return read_xwd(path) if path.endswith(".xwd") or "Xvfb_screen" in path else read_ppm(path)


def crop(img, w, h):
    iw, ih, rgb = img
    if (iw, ih) == (w, h):
        return img
    if iw < w or ih < h:
        sys.exit(f"frame {iw}x{ih} is smaller than {w}x{h}")
    return w, h, b"".join(rgb[y * iw * 3:(y * iw + w) * 3] for y in range(h))


def luma(rgb):
    return [(54 * r + 183 * g + 19 * b) >> 8 for r, g, b in zip(rgb[0::3], rgb[1::3], rgb[2::3])]


def ssim(a, b, w, h, n=8):
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    total, count = 0.0, 0
    for by in range(0, h - n + 1, n):
        for bx in range(0, w - n + 1, n):
            xs = [a[(by + j) * w + bx + i] for j in range(n) for i in range(n)]
            ys = [b[(by + j) * w + bx + i] for j in range(n) for i in range(n)]
            k = len(xs)
            mx, my = sum(xs) / k, sum(ys) / k
            vx = sum((x - mx) ** 2 for x in xs) / k
            vy = sum((y - my) ** 2 for y in ys) / k
            cv = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / k
            total += ((2 * mx * my + c1) * (2 * cv + c2)) / ((mx * mx + my * my + c1) * (vx + vy + c2))
            count += 1
    return total / max(count, 1)


def main():
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    ours, real = load(sys.argv[1]), load(sys.argv[2])
    out, thr = sys.argv[3], int(sys.argv[4]) if len(sys.argv) > 4 else 16
    w, h = min(ours[0], real[0]), min(ours[1], real[1])
    ours, real = crop(ours, w, h), crop(real, w, h)
    a, b = ours[2], real[2]
    os.makedirs(out, exist_ok=True)
    write_png(os.path.join(out, "ours.png"), w, h, a)
    write_png(os.path.join(out, "real.png"), w, h, b)
    err = [abs(x - y) for x, y in zip(a, b)]
    mae = [sum(err[c::3]) / (w * h) for c in range(3)]
    worst = [max(t) for t in zip(err[0::3], err[1::3], err[2::3])]
    over = sum(1 for e in worst if e > thr) / (w * h)
    write_png(os.path.join(out, "diff.png"), w, h, bytes(min(255, e * 4) for e in worst for _ in range(3)))
    la, lb = luma(a), luma(b)
    lines = [
        f"size {w}x{h}",
        f"mae_rgb {mae[0]:.2f} {mae[1]:.2f} {mae[2]:.2f}",
        f"over_{thr} {100 * over:.2f}%",
        f"luma_mean ours {sum(la) / len(la):.1f} real {sum(lb) / len(lb):.1f}",
        f"ssim8 {ssim(la, lb, w, h):.4f}",
    ]
    open(os.path.join(out, "metrics.txt"), "w").write("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
