"""Opt-in REAL-input/GetWindowRect regression on an owned, account-free probe.

Run only when no other desktop automation or user mouse activity is occurring:
    python testing/test_window_chrome_native.py --run-real-input
Add --legacy to exercise the old winit bridge against the identical hit targets.
Never attaches to, closes, or sends app input to an existing Harmony instance.
"""
from pathlib import Path
import argparse
import ctypes as C
from ctypes import wintypes as W
import json
import os
import queue
import subprocess
import sys
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-real-input", action="store_true")
    parser.add_argument("--legacy", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if sys.platform != "win32" or C.sizeof(C.c_void_p) != 8:
        parser.error("requires 64-bit Windows")
    if not args.run_real_input:
        parser.error("requires --run-real-input: this test temporarily controls the physical mouse")
    root = Path(__file__).resolve().parents[1]
    scratch = Path(os.environ["LOCALAPPDATA"]) / "hermes/cache/scratch/harmony-chrome-tests"
    scratch.mkdir(parents=True, exist_ok=True)
    if not args.skip_build:
        subprocess.run([sys.executable, str(root / "testing/run_window_chrome_tests.py"), "--build-probe"], check=True)
    u = C.WinDLL("user32", use_last_error=True)
    u.SetProcessDpiAwarenessContext.argtypes = [W.HANDLE]
    u.SetProcessDpiAwarenessContext(W.HANDLE(-4))
    signatures = {
        "GetWindowRect": ([W.HWND, C.POINTER(W.RECT)], W.BOOL),
        "GetClientRect": ([W.HWND, C.POINTER(W.RECT)], W.BOOL),
        "ClientToScreen": ([W.HWND, C.POINTER(W.POINT)], W.BOOL),
        "GetWindowThreadProcessId": ([W.HWND, C.POINTER(W.DWORD)], W.DWORD),
        "SetWindowPos": ([W.HWND, W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.UINT], W.BOOL),
        "SetForegroundWindow": ([W.HWND], W.BOOL),
        "GetForegroundWindow": ([], W.HWND),
        "WindowFromPoint": ([W.POINT], W.HWND),
        "GetAsyncKeyState": ([C.c_int], C.c_short),
        "SetCursorPos": ([C.c_int, C.c_int], W.BOOL),
        "GetCursorPos": ([C.POINTER(W.POINT)], W.BOOL),
        "GetDpiForWindow": ([W.HWND], W.UINT),
        "IsIconic": ([W.HWND], W.BOOL),
        "IsZoomed": ([W.HWND], W.BOOL),
        "ShowWindow": ([W.HWND, C.c_int], W.BOOL),
        "PostMessageW": ([W.HWND, W.UINT, C.c_size_t, C.c_ssize_t], W.BOOL),
    }
    for name, (parameters, result) in signatures.items():
        getattr(u, name).argtypes = parameters
        getattr(u, name).restype = result
    class MouseInput(C.Structure):
        _fields_ = [("dx", W.LONG), ("dy", W.LONG), ("data", W.DWORD), ("flags", W.DWORD), ("time", W.DWORD), ("extra", C.c_size_t)]
    class Payload(C.Union):
        _fields_ = [("mouse", MouseInput), ("padding", C.c_byte * 32)]
    class Input(C.Structure):
        _fields_ = [("type", W.DWORD), ("payload", Payload)]
    assert C.sizeof(Input) == 40
    u.SendInput.argtypes = [W.UINT, C.POINTER(Input), C.c_int]
    u.SendInput.restype = W.UINT
    def button(flags):
        item = Input(0, Payload(mouse=MouseInput(0, 0, 0, flags, 0, 0)))
        assert u.SendInput(1, C.byref(item), C.sizeof(item)) == 1, C.get_last_error()
    assert u.GetAsyncKeyState(1) >= 0, "refusing to start while somebody holds the left mouse button"
    original_cursor = W.POINT()
    assert u.GetCursorPos(C.byref(original_cursor))
    original_foreground = u.GetForegroundWindow()
    process = None
    hwnd = None
    pressed = False
    checks = []
    error_path = scratch / ("probe-legacy-stderr.log" if args.legacy else "probe-stderr.log")
    try:
        with error_path.open("w", encoding="utf-8") as errors:
            process = subprocess.Popen([str(scratch / "window-chrome-probe.exe"), *(["--legacy"] if args.legacy else [])], stdout=subprocess.PIPE, stderr=errors, text=True)
            assert process.stdout is not None
            output = process.stdout
            ready = queue.Queue()
            threading.Thread(target=lambda: ready.put(output.readline()), daemon=True).start()
            line = ready.get(timeout=10)
            state = json.loads(line)
            assert state["pid"] == process.pid
            hwnd = state["hwnd"]
            def owned():
                owner = W.DWORD()
                u.GetWindowThreadProcessId(hwnd, C.byref(owner))
                assert owner.value == process.pid and process.poll() is None, "probe HWND/PID no longer belongs to the child"
            def rect():
                owned()
                value = W.RECT()
                assert u.GetWindowRect(hwnd, C.byref(value))
                return [value.left, value.top, value.right, value.bottom]
            def client():
                owned()
                value, origin = W.RECT(), W.POINT()
                assert u.GetClientRect(hwnd, C.byref(value))
                assert u.ClientToScreen(hwnd, C.byref(origin))
                return origin.x, origin.y, value.right, value.bottom
            def foreground():
                owned()
                u.SetForegroundWindow(hwnd)
                time.sleep(0.15)
                assert u.GetForegroundWindow() == hwnd, "refusing input because the probe is not foreground"
            def reset():
                owned()
                u.ShowWindow(hwnd, 9)
                assert u.SetWindowPos(hwnd, None, 300, 160, 1000, 700, 0x14)
                time.sleep(0.2)
                foreground()
            def drag(name, x, y, dx, dy, expected):
                nonlocal pressed
                before = rect()
                foreground()
                assert u.SetCursorPos(round(x), round(y))
                time.sleep(0.15)
                assert u.WindowFromPoint(W.POINT(round(x), round(y))) == hwnd, "drag start is obscured by another window"
                button(2)
                pressed = True
                time.sleep(0.25)
                assert u.GetAsyncKeyState(1) < 0, "REAL left button was not held"
                # Cross the Slint threshold, let native initiation run, THEN move.
                assert u.SetCursorPos(round(x + (5 if dx > 0 else -5 if dx < 0 else 0)), round(y + (5 if dy > 0 else -5 if dy < 0 else 0)))
                time.sleep(0.25)
                for step in range(1, 7):
                    assert u.SetCursorPos(round(x + dx * step / 6), round(y + dy * step / 6))
                    time.sleep(0.06)
                button(4)
                pressed = False
                time.sleep(0.2)
                after = rect()
                delta = [after[i] - before[i] for i in range(4)]
                success = all((abs(d) <= 2 if sign == 0 else d * sign > 2) for d, sign in zip(delta, expected))
                item = dict(name=name, before=before, after=after, delta=delta, passed=success)
                checks.append(item)
                print(json.dumps(item), flush=True)
            reset()
            for name, dx, dy in [("title drag", 90, 45), ("repeated title drag", -70, -35)]:
                x, y, width, height = client()
                scale = u.GetDpiForWindow(hwnd) / 96
                drag(name, x + 220 * scale, y + 16 * scale, dx, dy, [1 if dx > 0 else -1, 1 if dy > 0 else -1] * 2)
            for name, side, dx, dy, expected in [
                ("east", (1, .5), 30, 0, [0, 0, 1, 0]),
                ("north", (.5, 0), 0, -24, [0, -1, 0, 0]),
                ("north-east", (1, 0), 30, -24, [0, -1, 1, 0]),
                ("north-west", (0, 0), -30, -24, [-1, -1, 0, 0]),
                ("south", (.5, 1), 0, 24, [0, 0, 0, 1]),
                ("south-east", (1, 1), 30, 24, [0, 0, 1, 1]),
                ("south-west", (0, 1), -30, 24, [-1, 0, 0, 1]),
                ("west", (0, .5), -30, 0, [-1, 0, 0, 0]),
            ]:
                reset()
                x, y, width, height = client()
                drag(name, x + 2 + side[0] * (width - 4), y + 2 + side[1] * (height - 4), dx, dy, expected)
            def click_caption(offset):
                nonlocal pressed
                foreground()
                x, y, width, height = client()
                scale = u.GetDpiForWindow(hwnd) / 96
                point = W.POINT(round(x + width - offset * scale), round(y + 16 * scale))
                assert u.SetCursorPos(point.x, point.y)
                time.sleep(0.15)
                assert u.WindowFromPoint(point) == hwnd
                button(2)
                pressed = True
                time.sleep(0.15)
                button(4)
                pressed = False
                time.sleep(0.4)
            click_caption(54)
            checks.append(dict(name="maximize after modal loops", passed=bool(u.IsZoomed(hwnd))))
            click_caption(54)
            checks.append(dict(name="restore after modal loops", passed=not bool(u.IsZoomed(hwnd))))
            click_caption(90)
            checks.append(dict(name="minimize after modal loops", passed=bool(u.IsIconic(hwnd))))
            u.ShowWindow(hwnd, 9)
            time.sleep(0.25)
            click_caption(18)
            try:
                process.wait(timeout=3)
                closed = process.returncode == 0
            except subprocess.TimeoutExpired:
                closed = False
            checks.append(dict(name="CloseRequested after modal loops", passed=closed))
    finally:
        if pressed:
            button(4)
        if process and process.poll() is None:
            owner = W.DWORD()
            if hwnd:
                u.GetWindowThreadProcessId(hwnd, C.byref(owner))
                if owner.value == process.pid:
                    u.PostMessageW(hwnd, 0x10, 0, 0)
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.terminate()  # Only the child we launched, never a discovered app.
                process.wait(timeout=2)
        u.SetCursorPos(original_cursor.x, original_cursor.y)
        if original_foreground:
            u.SetForegroundWindow(original_foreground)
        report = dict(legacy=args.legacy, checks=checks)
        (scratch / ("native-motion-legacy.json" if args.legacy else "native-motion.json")).write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))
    assert checks and all(item["passed"] for item in checks), "native window geometry or caption regression; see the report above"
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
