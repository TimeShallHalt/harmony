"""Send through the Android keyboard and verify exact delivery on the local fake Discord."""
import argparse
import json
import re
import subprocess
import time
import uuid
from urllib.request import Request, urlopen

PACKAGE = "io.github.timeshallhalt.harmony.android.fixture"
ACTIVITY = "io.github.timeshallhalt.harmony.android.HarmonyActivity"
BOB = "1400000000000000120"
FIXTURE = "http://127.0.0.1:18473"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--adb", default="adb")
    parser.add_argument("--adb-port", type=int, default=5037)
    parser.add_argument("--restart-draft", action="store_true", help="Force-stop the fixture and send the restored text")
    args = parser.parse_args()
    if not re.fullmatch(r"emulator-\d+", args.serial):
        parser.error("Pass the serial of a dedicated Android emulator")
    prefix = [args.adb, "-P", str(args.adb_port), "-s", args.serial]

    def adb(*command):
        return subprocess.check_output([*prefix, *command], text=True, encoding="utf-8").strip()

    if adb("shell", "getprop", "ro.kernel.qemu") != "1":
        raise SystemExit("The selected target did not identify as an emulator")
    if "720x1560" not in adb("shell", "wm", "size").splitlines()[-1] or "280" not in adb("shell", "wm", "density").splitlines()[-1]:
        raise SystemExit("This touch test uses a 720x1560 portrait display at 280 dpi")
    if "tcp:18473 tcp:18473" not in adb("reverse", "--list"):
        raise SystemExit("Forward the fixture port on this emulator first")

    def messages():
        request = Request(f"{FIXTURE}/api/v9/channels/{BOB}/messages?limit=20",
                          headers={"Authorization": "yapfast-test-token"})
        with urlopen(request, timeout=4) as response:
            return json.load(response)

    def touch(x, y):
        resumed = adb("shell", "dumpsys", "activity", "activities")
        if not any("topResumedActivity=" in line and PACKAGE + "/" in line for line in resumed.splitlines()):
            raise SystemExit("The fixture activity must be in the foreground")
        adb("shell", "input", "tap", str(x), str(y))

    def edit_bob():
        adb("shell", "am", "start", "-W", "-n", PACKAGE + "/" + ACTIVITY,
            "-a", "android.intent.action.VIEW", "--es", "conversation", BOB)
        time.sleep(1.2)
        touch(200, 1424)
        time.sleep(0.6)

    messages()
    edit_bob()
    message = "Android keyboard check " + uuid.uuid4().hex[:8]
    adb("shell", "input", "text", message.replace(" ", "%s"))
    if args.restart_draft:
        time.sleep(1.0)
        vault = adb("shell", "run-as", PACKAGE, "cat", "shared_prefs/vault.xml")
        if "message-drafts/" not in vault or message in vault:
            raise SystemExit("The fixture did not save an encrypted draft")
        adb("shell", "am", "force-stop", PACKAGE)
        edit_bob()
    touch(666, 905)
    end = time.monotonic() + 10
    while time.monotonic() < end:
        matches = [m for m in messages() if m.get("content") == message]
        if len(matches) == 1:
            print(f"PASS: exact keyboard text received once in Bob's DM, id {matches[0]['id']}")
            return
        time.sleep(0.2)
    raise SystemExit("The local fixture did not receive the complete text. Check the saved fixture login and keyboard.")


if __name__ == "__main__":
    main()
