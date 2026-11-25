"""Fast source-contract regressions; native behavior lives in theme_pack_ui_tests.rs.
The pure Rust parser/filesystem tests run separately without a Cargo build.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]

def text(path):
    return (ROOT / path).read_text(encoding="utf-8")

class NativePackContracts(unittest.TestCase):
    def test_appearance_actions_are_reachable_and_bound(self):
        ui = text("ui/settings.slint")
        root = text("ui/yapfast.slint")
        callbacks = text("src/ui/callbacks.rs")
        for action in ["import-theme-pack", "export-theme-pack", "reload-theme-pack", "reset-theme-pack", "reset-theme-logo", "edit-theme-pack"]:
            self.assertIn("callback " + action, ui)
            self.assertIn("callback " + action, root)
            self.assertIn("root." + action + "();", root)
            self.assertIn("on_" + action.replace("-", "_"), callbacks)
        self.assertIn("rfd::AsyncFileDialog", text("src/ui/appearance.rs"))

    def test_overrides_have_live_native_bindings_and_clean_defaults(self):
        style = text("ui/yapfast/style.slint")
        for metric in ["title-height", "title-inset", "rail-tile", "sidebar-width", "header-height", "account-height", "profile-width", "members-width", "composer-height", "composer-padding", "composer-gutter", "message-gap", "message-group-gap", "channel-row-height", "text-size", "title-text-size"]:
            self.assertIn("in-out property <length> pack-" + metric + ": -1px;", style)
            self.assertIn("self.pack-" + metric + " >= 0px", style)
        chrome = text("ui/yapfast/window-bar.slint")
        self.assertIn("source: ClientStyle.title-logo", chrome)
        self.assertIn("root.move-window();", chrome)
        theme = text("ui/theme.slint")
        self.assertIn("self.reduced-motion ? 0ms", theme)
        self.assertIn("Theme.popup-ms * 1ms", theme)

    def test_pack_palette_copy_names_its_real_base(self):
        source = text("src/ui/appearance.rs")
        new_theme = source.split("fn new_theme(", 1)[1].split("fn edit_theme(", 1)[0]
        self.assertIn("saved.pack.base.as_str()", new_theme)
        self.assertIn("start.id == current", new_theme)

    def test_restore_and_persistence_share_the_same_resolver(self):
        appearance = text("src/ui/appearance.rs")
        cancel = appearance.split("fn cancel_theme(", 1)[1].split("///", 1)[0]
        self.assertIn("restore_appearance(window)", cancel)
        self.assertNotIn("theme::chosen", cancel)
        root = text("src/ui.rs")
        self.assertIn("deserialize_with = \"theme_pack::deserialize_saved\"", root)
        self.assertIn("fn try_write", root)
        self.assertIn("theme_pack::write_atomic", root)
        self.assertIn("self.sidebar_width.unwrap_or", re.sub(r"\s+", "", root))
        self.assertIn("root.text-size == 0 ? 14px", text("ui/yapfast.slint"))

if __name__ == "__main__":
    unittest.main()
