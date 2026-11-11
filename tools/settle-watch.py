"""Read a boot's live log on stdin; exit 0 once a line matched UNTIL, GRACE s have passed since,
and the last SETTLE presented frames share one hash. Exits 1 if the log ends first.
usage: tail -F log | python3 -I tools/settle-watch.py <until-regex> <grace-s> <settle-n>
"""
import re
import sys
import time

until, grace, settle = re.compile(sys.argv[1]), float(sys.argv[2]), int(sys.argv[3])
frame = re.compile(r"frame: \S+ #\d+ \S+ ([0-9a-f]+)")
start, last, run = None, None, 0
for line in sys.stdin:
    if start is None:
        if until.search(line):
            start = time.monotonic()
        continue
    m = frame.search(line)
    if not m:
        continue
    run = run + 1 if m.group(1) == last else 1
    last = m.group(1)
    if run >= settle and time.monotonic() - start >= grace:
        sys.exit(0)
sys.exit(1)
