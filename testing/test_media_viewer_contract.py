"""Supplemental source checks; native viewer behavior is in media_viewer_tests.rs.

Run: python -m unittest testing.test_media_viewer_contract -v
These checks are not a replacement for compiling/running the native tests.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


def viewer_source():
    text = (ROOT / "ui/messages.slint").read_text(encoding="utf-8")
    return text.split("export component ImageViewer inherits Rectangle {", 1)[1].split(
        "export component VideoViewer", 1
    )[0]


class MediaViewerContractTest(unittest.TestCase):
    def test_zoom_controls_have_real_actions_and_accessible_names(self):
        viewer = viewer_source()
        for label, action in [
            ("Zoom in (+)", "zoom-in"),
            ("Zoom out (-)", "zoom-out"),
            ("Fit image to window", "fit-image"),
            ("Actual size (100%)", "actual-size"),
            ("Reset view (0)", "reset-view"),
        ]:
            with self.subTest(label=label):
                self.assertIn(f'label: "{label}"', viewer)
                self.assertIn(f"root.{action}();", viewer)
                self.assertRegex(viewer, rf"public function {action}\(")
        self.assertIn('event.text == "+"', viewer)
        self.assertIn('event.text == "-"', viewer)
        self.assertIn('event.text == "0"', viewer)

    def test_actual_size_uses_decoded_pixels_not_source_metadata(self):
        viewer = viewer_source()
        for contract in [
            "root.viewer-image.width",
            "root.viewer-image.height",
            "Decoded ",
            " · Preview",
        ]:
            self.assertTrue(contract in viewer, f"Missing decoded-size contract: {contract}")
        self.assertNotIn("pixel-width: root.viewer-width > 0", viewer)
        self.assertNotIn("pixel-height: root.viewer-height > 0", viewer)

    def test_wheel_zoom_keeps_the_pointer_anchor(self):
        viewer = viewer_source()
        for contract in [
            "function zoom-at(",
            "scroll-event(event)",
            "event.delta-y",
            "self.mouse-x - viewport.width / 2",
            "self.mouse-y - viewport.height / 2",
            "anchor-x + (previous-x - anchor-x) * ratio",
            "anchor-y + (previous-y - anchor-y) * ratio",
        ]:
            self.assertTrue(contract in viewer, f"Missing anchored-wheel contract: {contract}")

    def test_shift_arrows_pan_without_replacing_navigation(self):
        viewer = viewer_source()
        self.assertTrue("if event.modifiers.shift" in viewer, "Shift-arrow pan is missing")
        self.assertTrue("root.pan-by(-48px, 0px)" in viewer, "Rightward keyboard pan is missing")
        self.assertIn("root.step-image(-1)", viewer)
        self.assertIn("root.step-image(1)", viewer)
        self.assertIn("Key.Escape", viewer)
        self.assertIn("root.close-image()", viewer)

    def test_navigation_targets_have_the_same_bounds_as_their_visible_buttons(self):
        viewer = viewer_source()
        self.assertTrue("size: 48px; height: 48px; glyph: 28px" in viewer,
                        "Standalone navigation buttons need an explicit 48px height")

    def test_animation_bindings_and_event_driven_rendering_are_preserved(self):
        viewer = viewer_source()
        self.assertIn("still: root.viewer-image; frames: root.viewer-frames; delays: root.viewer-delays", viewer)
        self.assertIn("out property <image> displayed-frame: shown.source", viewer)
        self.assertNotIn("animation-tick(", viewer)
        self.assertNotIn("Timer {", viewer)
        self.assertNotIn("Motion.playing =", viewer)

    def test_viewer_chrome_uses_only_semantic_theme_colors(self):
        viewer = viewer_source()
        self.assertNotRegex(viewer, r"#[0-9a-fA-F]{3,8}\b")
        for token in ["Theme.sunken", "Theme.panel", "Theme.text", "Theme.secondary", "Theme.raised"]:
            self.assertIn(token, viewer)

    def test_pan_is_clamped_in_a_stationary_clipped_viewport(self):
        viewer = viewer_source()
        for contract in [
            "public function pan-by(",
            "out property <length> pan-x: clamp(",
            "out property <length> pan-y: clamp(",
            "pan-limit-x: max(0px,",
            "pan-limit-y: max(0px,",
            "+ root.pan-x",
            "+ root.pan-y",
            "pan-touch := TouchArea",
            "moved =>",
            "root.pan-by(",
            "PointerEventKind.cancel",
        ]:
            self.assertTrue(contract in viewer, f"Missing clamped-pan contract: {contract}")


if __name__ == "__main__":
    unittest.main()
