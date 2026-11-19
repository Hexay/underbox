#!/usr/bin/env python3
"""Writes the README's peak-memory chart, one SVG per GitHub theme, into docs/img/.

Numbers are the medians of the latest ours-vs-real sweep in docs/differential-perf.md; update
SWEEP and rerun after a new sweep: `python3 tools/readme_charts.py`.
"""

from pathlib import Path

SWEEP = "2026-11-19"
# (scenario, side, underbox MiB, real engine2 MiB)
PEAK_RSS = [
    ("pile", "server", 500, 545),
    ("probe", "server", 541, 629),
    ("showroom", "client", 1253, 3781),
    ("skinned", "client", 1298, 3873),
]

THEMES = {
    "light": dict(surface="#fcfcfb", ink="#0b0b0b", ink2="#52514e", muted="#898781",
                  grid="#e1e0d9", axis="#c3c2b7", ours="#2a78d6", real="#eb6834"),
    "dark": dict(surface="#1a1a19", ink="#f0efec", ink2="#c3c2b7", muted="#898781",
                 grid="#2c2c2a", axis="#383835", ours="#3987e5", real="#d95926"),
}

WIDTH, LEFT, PLOT_W, TOP = 720, 150, 470, 92
BAR, GAP, BAND = 20, 2, 66
X_MAX, TICK = 4000, 1000
FONT = 'system-ui, -apple-system, "Segoe UI", Helvetica, Arial, sans-serif'


def x_of(mib):
    return LEFT + mib * PLOT_W / X_MAX


def bar(y, mib, fill):
    """Rounded at the data end only, square at the baseline."""
    x0, x1, r = LEFT, x_of(mib), 4
    return (f'<path d="M{x0},{y} H{x1 - r:.1f} Q{x1:.1f},{y} {x1:.1f},{y + r} V{y + BAR - r} '
            f'Q{x1:.1f},{y + BAR} {x1 - r:.1f},{y + BAR} H{x0} Z" fill="{fill}"/>')


def text(x, y, s, fill, size=13, anchor="start", weight=400):
    return (f'<text x="{x:.1f}" y="{y}" fill="{fill}" font-size="{size}" font-weight="{weight}" '
            f'text-anchor="{anchor}">{s}</text>')


def chart(t):
    plot_h = BAND * len(PEAK_RSS)
    height = TOP + plot_h + 64
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
           f'viewBox="0 0 {WIDTH} {height}" font-family=\'{FONT}\' role="img" '
           f'aria-label="Peak memory in MiB, underbox against real engine2, per scenario">',
           f'<rect width="{WIDTH}" height="{height}" rx="8" fill="{t["surface"]}"/>',
           text(24, 34, "Peak memory (MiB)", t["ink"], 16, weight=600),
           text(24, 56, "Lower is better. Median of 5 runs per engine.", t["ink2"])]
    for i, (label, colour) in enumerate([("underbox", t["ours"]), ("real engine2", t["real"])]):
        lx = 24 + i * 120
        out += [f'<rect x="{lx}" y="68" width="12" height="12" rx="2" fill="{colour}"/>',
                text(lx + 18, 79, label, t["ink2"])]
    for v in range(0, X_MAX + 1, TICK):
        x = x_of(v)
        out += [f'<line x1="{x:.1f}" y1="{TOP}" x2="{x:.1f}" y2="{TOP + plot_h}" '
                f'stroke="{t["axis"] if v == 0 else t["grid"]}" stroke-width="1"/>',
                text(x, TOP + plot_h + 20, f"{v:,}", t["muted"], 12, "middle")]
    for i, (name, side, ours, real) in enumerate(PEAK_RSS):
        y = TOP + i * BAND + (BAND - 2 * BAR - GAP) / 2
        out += [text(LEFT - 14, y + BAR - 4, name, t["ink"], 14, "end", 600),
                text(LEFT - 14, y + 2 * BAR + GAP - 4, side, t["muted"], 12, "end"),
                bar(y, ours, t["ours"]), bar(y + BAR + GAP, real, t["real"]),
                text(x_of(ours) + 6, y + BAR - 5, f"{ours:,}", t["ink"], 12, weight=600),
                text(x_of(real) + 6, y + 2 * BAR + GAP - 5, f"{real:,}", t["ink2"], 12)]
    note = f"Sweep of {SWEEP}. Same managed build on both engines; clients at 1024×768 on lavapipe."
    out.append(text(24, height - 16, note, t["muted"], 11))
    out.append("</svg>\n")
    return "\n".join(out)


def main():
    out = Path(__file__).resolve().parent.parent / "docs" / "img"
    out.mkdir(parents=True, exist_ok=True)
    for name, theme in THEMES.items():
        (out / f"peak-memory-{name}.svg").write_text(chart(theme))
    print(f"wrote peak-memory-{{light,dark}}.svg in {out}")


if __name__ == "__main__":
    main()
