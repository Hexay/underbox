# Sourced by tools/diff/*.sh: where real engine2 artifacts and run copies live. See docs/differential.md.
REAL_ENGINE2_CACHE=${REAL_ENGINE2_CACHE:-${UNDERBOX_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/underbox}/real-engine2}
REAL_ENGINE2_RUNS=${REAL_ENGINE2_RUNS:-$REAL_ENGINE2_CACHE-runs}
