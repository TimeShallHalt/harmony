"""Update 1 release scope: no executable updater or update controls."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]

class NoUpdaterRelease(unittest.TestCase):
    def test_release_has_no_updater_entrypoints_or_preferences(self):
        for name in ('src/main.rs', 'src/ui.rs', 'src/ui/callbacks.rs',
                     'src/autostart.rs', 'src/protocol.rs', 'src/desktop_entry.rs'):
            text = (ROOT / name).read_text(encoding='utf-8')
            for forbidden in ('harmony_update', 'updater_info', 'handoff_on_full_exit'):
                self.assertNotIn(forbidden, text, name)
        self.assertFalse((ROOT / 'src/harmony_update').exists())
        self.assertFalse((ROOT / 'scripts/harmony_bootstrap.rs').exists())

    def test_about_has_only_build_information_and_copy(self):
        text = (ROOT / 'ui/about_updates.slint').read_text(encoding='utf-8')
        self.assertIn('Updater not included in this build.', text)
        self.assertIn('Copy diagnostics', text)
        for forbidden in ('callback check', 'callback download', 'callback arm',
                          'callback preferences-changed', 'SettingsSwitch', 'Install on full exit'):
            self.assertNotIn(forbidden, text)

if __name__ == '__main__':
    unittest.main()
