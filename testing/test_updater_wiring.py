"""Supplemental source wiring contracts, not a desktop execution substitute."""
from pathlib import Path
import re
import unittest
ROOT = Path(__file__).resolve().parents[1]
class UpdaterWiring(unittest.TestCase):
    def test_settings_route_is_available_in_both_layouts(self):
        settings = (ROOT / 'ui/settings.slint').read_text()
        nav = (ROOT / 'ui/yapfast/settings-nav.slint').read_text()
        root = (ROOT / 'ui/yapfast.slint').read_text()
        self.assertGreaterEqual(len(re.findall(r'text: "About & Updates".*id: 9', settings)), 2)
        self.assertIn('text: "About & Updates"', nav)
        self.assertIn('root.settings-section == 9: AboutUpdatesSettings', root)
    def test_production_bindings_and_persistence_are_registered(self):
        ui = (ROOT / 'src/ui.rs').read_text()
        callbacks = (ROOT / 'src/ui/callbacks.rs').read_text()
        self.assertIn('mod updater_info;', ui)
        self.assertIn('harmony_updates: crate::harmony_update::controller::Preferences', ui)
        self.assertIn('super::updater_info::bind(window, adapter)', callbacks)
        self.assertIn('updater_info::startup', ui)
        self.assertIn('handoff_on_full_exit', ui)
        for path in ['src/autostart.rs','src/protocol.rs','src/desktop_entry.rs']:
            self.assertIn('harmony_update::controller::normal_entry()', (ROOT/path).read_text())
    def test_native_helper_dispatch_precedes_tls_and_app_runtime(self):
        main = (ROOT / 'src/main.rs').read_text()
        self.assertLess(main.index('harmony_update::cli_main'), main.index('yapfast_core::init_tls()?'))
    def test_no_updater_callback_can_quit_or_restart(self):
        source = (ROOT / 'src/ui/updater_info.rs').read_text()
        self.assertNotIn('quit_event_loop', source)
        self.assertNotIn('invoke_restart', source)
        self.assertIn('invoke_copy_text', source)
        self.assertNotIn('Repeated', source)
if __name__ == '__main__':
    unittest.main()
