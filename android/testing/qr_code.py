"""Check live QR issuance on a signed-out emulator after choosing Show QR code."""
import argparse
import re
import subprocess
import time

import cv2
import numpy as np


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--adb", default="adb")
    parser.add_argument("--adb-port", type=int, default=5037)
    args = parser.parse_args()
    if not re.fullmatch(r"emulator-\d+", args.serial):
        parser.error("Pass the serial of a dedicated emulator")
    adb = [args.adb, "-P", str(args.adb_port), "-s", args.serial]

    def shell(*command):
        return subprocess.check_output([*adb, "shell", *command], text=True, encoding="utf-8")

    if shell("getprop", "ro.kernel.qemu").strip() != "1":
        raise SystemExit("The selected target did not identify as an emulator")

    def check_foreground():
        resumed = shell("dumpsys", "activity", "activities")
        if not any(("topResumedActivity=" in line or "mResumedActivity:" in line)
                   and "io.github.timeshallhalt.harmony.android/" in line for line in resumed.splitlines()):
            raise SystemExit("Open the live Harmony APK and choose Link Discord, then Show QR code")

    detector = cv2.QRCodeDetector()
    deadline = time.monotonic() + 50
    while time.monotonic() < deadline:
        check_foreground()
        png = subprocess.check_output([*adb, "exec-out", "screencap", "-p"])
        frame = cv2.imdecode(np.frombuffer(png, dtype=np.uint8), cv2.IMREAD_GRAYSCALE)
        if frame is not None:
            url, _, _ = detector.detectAndDecode(frame)
            if re.fullmatch(r"https://discord\.com/ra/[A-Za-z0-9_-]{1,128}", url):
                check_foreground()
                shell("input", "keyevent", "KEYCODE_BACK")
                print("PASS: Discord issued a QR link. Cancelled the attempt before approval.")
                return
        time.sleep(0.4)
    raise SystemExit("The live APK did not display a Discord QR link within 50 seconds")


if __name__ == "__main__":
    main()
