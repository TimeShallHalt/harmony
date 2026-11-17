"""Fast source-contract checks; native interaction tests live in src/ui/tests.rs.

Run without the native multimedia toolchain: python -m unittest testing.test_harmony_branding -v
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


def source(path):
    return (ROOT / path).read_text(encoding="utf-8")


class HarmonyBrandingTest(unittest.TestCase):
    def test_window_and_entry_points_present_harmony(self):
        self.assertIn('window-title: "Harmony"', source("ui/yapfast.slint"))
        self.assertIn('text: "Harmony"', source("ui/sign-in.slint"))
        self.assertIn('text: "Sign in to Harmony"', source("ui/sign-in.slint"))
        self.assertIn('"(99+) Harmony"', source("src/ui/shell.rs"))
        self.assertIn('"Open Harmony"', source("src/tray.rs"))
        self.assertIn('w!("Open Harmony")', source("src/tray.rs"))
        self.assertIn('"Harmony".encode_utf16()', source("src/tray.rs"))
        self.assertIn('"Harmony",', source("src/notify.rs"))

    def test_linux_desktop_name_changes_without_renaming_integration(self):
        entry = source("packaging/linux/io.github.TimeShallHalt.Harmony.desktop")
        self.assertIn("Name=Harmony\n", entry)
        for identity in ["Exec=harmony --open-link %u", "Icon=io.github.TimeShallHalt.Harmony", "MimeType=x-scheme-handler/harmony;", "StartupWMClass=io.github.TimeShallHalt.Harmony"]:
            self.assertIn(identity, entry)

    def test_ui_notices_use_harmony(self):
        for relative in ["src/ui.rs", "src/ui/callbacks.rs", "src/ui/refresh.rs", "src/ui/messages.rs", "src/ui/server_settings.rs", "src/keybinds.rs", "src/graphics.rs"]:
            text = source(relative)
            self.assertIsNone(re.search(r'"[^"\n]*Yapfast[^"\n]*"', text), relative)

    def test_session_stopped_notice_uses_harmony(self):
        self.assertIn('Restart Harmony to restore your session.', source('crates/yapfast-core/src/application.rs'))

    def test_caption_uses_shared_metrics_and_live_theme(self):
        bar = source("ui/yapfast/window-bar.slint")
        style = source("ui/yapfast/style.slint")
        window = source("ui/yapfast.slint")
        self.assertIn('text: "Harmony"', bar)
        for token in ["title-height", "caption-width", "caption-icon-size", "title-inset", "title-text-size", "title-drag-threshold"]:
            self.assertIn(f"property <length> {token}:", style)
            self.assertIn(f"ClientStyle.{token}", bar)
        self.assertIn("? ClientStyle.title-height : 0px", window)
        self.assertIn("Theme.rail", bar)
        self.assertIn("Theme.outline", bar)
        self.assertNotRegex(bar, r"#[0-9a-fA-F]{3,8}\b")
        # Preserve native callbacks and keyboard access rather than simulating window state.
        for action in ["move", "minimize", "maximize", "close"]:
            self.assertIn(f"root.{action}-window()", bar)
        self.assertIn("key-pressed(event)", bar)
        self.assertIn("Theme.on-danger", bar)

    def test_user_facing_ui_copy_leaves_no_old_name(self):
        for path in (ROOT / "ui").rglob("*.slint"):
            # Only displayed copy: do not rename components, imports, or attribution.
            for line in path.read_text(encoding="utf-8").splitlines():
                if line.lstrip().startswith("//") or "https://" in line:
                    continue
                for literal in re.findall(r'"([^"\n]*)"', line):
                    if "Yapfast" in literal and not (path.name == "about_updates.slint" and "Based on Yapfast" in literal):
                        self.fail(f"Old display name in {path.relative_to(ROOT)}: {literal}")


if __name__ == "__main__":
    unittest.main()
