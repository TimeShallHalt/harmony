"""Release contracts for Harmony's public, publisher-signed update path."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]

class ReleaseUpdates(unittest.TestCase):
    def test_first_party_update_authority(self):
        source = (ROOT / 'src/harmony_updates.rs').read_text(encoding='utf8')
        self.assertIn('api.github.com/repos/TimeShallHalt/harmony/releases/latest', source)
        self.assertIn('include_str!("../packaging/keys/harmony-update.pub")', source)
        self.assertNotIn('api.github.com/repos/yapfast/yapfast', source)
        public = bytes.fromhex((ROOT / 'packaging/keys/harmony-update.pub').read_text().strip())
        self.assertEqual((len(public), public[0]), (65, 4))

    def test_update_consent_uses_the_desktop_settings_and_full_quit(self):
        about = (ROOT / 'ui/about_updates.slint').read_text(encoding='utf8')
        for label in ('Check now', 'Download update', 'Run Setup when I quit Harmony', 'Copy diagnostics'):
            self.assertIn(label, about)
        main = (ROOT / 'src/ui.rs').read_text(encoding='utf8')
        self.assertIn('adapter.updates.finish()', main)
        setup = (ROOT / 'packaging/windows/harmony-simple.iss').read_text(encoding='utf8')
        self.assertIn('{param:HARMONYWAITPID|}', setup)
        self.assertIn('{param:HARMONYINSTALLED|}', setup)

if __name__ == '__main__':
    unittest.main()
