#!/usr/bin/env python3
"""Drive a game window on an X display through XTest (libXtst via ctypes; no xdotool needed).

usage: xinput.py <display> <step>...
  hold:<key>:<seconds>     press a key (an X keysym name: w, space, Shift_L), wait, release
  move:<dx>:<dy>:<seconds> move the pointer by (dx, dy) in 20 even steps over <seconds>
  click:<button>           press and release a pointer button (1 left, 3 right)
  center                   put the pointer at the screen's centre
  focus                    give the window under the pointer the input focus (there's no window manager)
  wait:<seconds>
Prints one line per step.
"""
import ctypes
import ctypes.util
import sys
import time


def lib(name):
    path = ctypes.util.find_library(name)
    if not path:
        sys.exit(f"xinput: lib{name} not found")
    return ctypes.CDLL(path)


X11, XTST = lib("X11"), lib("Xtst")
X11.XOpenDisplay.restype = ctypes.c_void_p
X11.XOpenDisplay.argtypes = [ctypes.c_char_p]
X11.XStringToKeysym.restype = ctypes.c_ulong
X11.XStringToKeysym.argtypes = [ctypes.c_char_p]
X11.XKeysymToKeycode.restype = ctypes.c_ubyte
X11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
X11.XFlush.argtypes = [ctypes.c_void_p]
for f in ("XDisplayWidth", "XDisplayHeight"):
    getattr(X11, f).argtypes = [ctypes.c_void_p, ctypes.c_int]
X11.XDefaultRootWindow.restype = ctypes.c_ulong
X11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
X11.XQueryPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong] + [ctypes.c_void_p] * 7
X11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
XTST.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
XTST.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
XTST.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
XTST.XTestFakeRelativeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]


def keycode(dpy, name):
    sym = X11.XStringToKeysym(name.encode())
    code = X11.XKeysymToKeycode(dpy, sym) if sym else 0
    if not code:
        sys.exit(f"xinput: no keycode for '{name}'")
    return code


def run(dpy, step):
    kind, *args = step.split(":")
    if kind == "hold":
        code, secs = keycode(dpy, args[0]), float(args[1])
        XTST.XTestFakeKeyEvent(dpy, code, 1, 0)
        X11.XFlush(dpy)
        time.sleep(secs)
        XTST.XTestFakeKeyEvent(dpy, code, 0, 0)
    elif kind == "move":
        dx, dy, secs = int(args[0]), int(args[1]), float(args[2])
        for i in range(20):
            # Even integer steps that sum to exactly (dx, dy).
            sx = dx * (i + 1) // 20 - dx * i // 20
            sy = dy * (i + 1) // 20 - dy * i // 20
            XTST.XTestFakeRelativeMotionEvent(dpy, sx, sy, 0)
            X11.XFlush(dpy)
            time.sleep(secs / 20)
    elif kind == "click":
        button = int(args[0])
        XTST.XTestFakeButtonEvent(dpy, button, 1, 0)
        X11.XFlush(dpy)
        time.sleep(0.05)
        XTST.XTestFakeButtonEvent(dpy, button, 0, 0)
    elif kind == "center":
        XTST.XTestFakeMotionEvent(dpy, 0, X11.XDisplayWidth(dpy, 0) // 2, X11.XDisplayHeight(dpy, 0) // 2, 0)
    elif kind == "focus":
        # No window manager under Xvfb: give the top-level window under the pointer the input focus.
        root, child = ctypes.c_ulong(), ctypes.c_ulong()
        i = ctypes.c_int()
        u = ctypes.c_uint()
        X11.XQueryPointer(dpy, X11.XDefaultRootWindow(dpy), ctypes.byref(root), ctypes.byref(child),
                          ctypes.byref(i), ctypes.byref(i), ctypes.byref(i), ctypes.byref(i), ctypes.byref(u))
        if not child.value:
            sys.exit("xinput: no window under the pointer")
        X11.XSetInputFocus(dpy, child.value, 1, 0)  # RevertToPointerRoot, CurrentTime
        step = f"focus:0x{child.value:x}"
    elif kind == "wait":
        time.sleep(float(args[0]))
    else:
        sys.exit(f"xinput: unknown step '{step}'")
    X11.XFlush(dpy)
    print(f"xinput: {step} at {time.time():.3f}", flush=True)


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    dpy = X11.XOpenDisplay(sys.argv[1].encode())
    if not dpy:
        sys.exit(f"xinput: can't open display {sys.argv[1]}")
    for step in sys.argv[2:]:
        run(dpy, step)


if __name__ == "__main__":
    main()
