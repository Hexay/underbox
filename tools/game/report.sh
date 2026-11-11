# Report helpers for tools/game.sh (sourced). `check` lines make up the PASS/FAIL summary.
FAILED=0

summary() { grep -iE 'exception|error|panick' "$1" | grep -vE 'stub:|GAME:|Error \||achievements|127\.0\.0\.1:9' | sed -E 's/^[0-9/]+ ?[0-9:.]+ //' | cut -c1-200 | sort | uniq -c | sort -rn | head -n "$2" || true; }

report_game() {
    echo "== $2 compile: $(grep -c "Compile of .* Failed" "$1" || true) failed, $(grep -cE ' Error \| ' "$1" || true) errors"
    grep -E ' Error \| ' "$1" | sed -E 's/^.* Error \| //; s/ - \/.*//' | cut -c1-200 | sort -u | head -n 10 || true
    echo "== $2 missing components:"; grep -oE 'Missing Component: .*' "$1" | cut -c1-140 | sort | uniq -c | sort -rn | head -n 10 || true
    echo "== $2 packages refused: $(grep -c 'Package not found:' "$1" || true), HTTP stopped at the dead proxy: $(grep -c '127\.0\.0\.1:9' "$1" || true)"
    echo "== $2 markers"; grep -oE 'GAME:(START|AWAKE|TRACE|SETTLED|DROP|STANDING|PLAYER_STANDING|COMMAND|END).*' "$1" | cut -c1-200 || true
    echo "== $2 contents"; grep -oE 'GAME:(RESOURCES|COMPONENTS|PHYSICS).*' "$1" | cut -c1-400 || true
    echo "== $2 last tick"; grep -oE 'GAME:TICK.*' "$1" | tail -n 1 | cut -c1-300 || true
    echo "== $2 errors"; summary "$1" 12
    echo "== $2 stubs hit: $(grep -oE 'stub: [A-Za-z0-9_]+' "$1" | sort -u | wc -l)"
}

# check <label> <regex> <file>: PASS with the first matching line as evidence, else FAIL.
check() {
    line=$(grep -m1 -oE -- "$2" "$3" 2>/dev/null | cut -c1-220 || true)
    if [ -n "$line" ]; then echo "PASS $1: $line"; else echo "FAIL $1: no /$2/ in $(basename -- "$3")"; FAILED=1; fi
}

# refute <label> <regex> <file>: FAIL with the first matching line, else PASS.
refute() {
    line=$(grep -m1 -oE -- "$2" "$3" 2>/dev/null | cut -c1-220 || true)
    if [ -z "$line" ]; then echo "PASS $1"; else echo "FAIL $1: $line"; FAILED=1; fi
}

compile_clean() {
    n=$(grep -cE 'Compile of .* Failed| Error \| ' "$1" || true)
    if [ "$n" = 0 ]; then echo "PASS $2 compiles: no compile errors"; else echo "FAIL $2 compiles: $n compile error lines"; FAILED=1; fi
}
