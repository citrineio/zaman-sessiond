import json
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'notifications'))
from model import Model


class SteamNotices(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / 'history.json'
        self.model = Model(self.path)

    def test_startup_can_be_disabled_without_disabling_shortcut(self):
        self.model.command({'action': 'preferences', 'steamStartup': False})
        self.model.event('Zaman', 'steam:launch:one', 'Launching Steam')
        self.model.event('Zaman', 'steam:shortcut:one', 'Start + Select')
        self.assertEqual([r['title'] for r in self.model.records], ['Start + Select'])

    def test_shortcut_can_be_disabled_without_disabling_startup(self):
        self.model.command({'action': 'preferences', 'steamReminder': False})
        self.model.event('Zaman', 'steam:launch:one', 'Launching Steam')
        self.model.event('Zaman', 'steam:shortcut:one', 'Start + Select')
        self.assertEqual([r['title'] for r in self.model.records], ['Launching Steam'])

    def test_preferences_persist_independently(self):
        self.model.command({'action': 'preferences', 'steamStartup': False, 'steamReminder': True})
        self.assertEqual(Model(self.path).preferences, {'dnd': False, 'steamReminder': True, 'steamStartup': False})

    def test_previous_disabled_steam_preference_is_respected(self):
        self.path.write_text(json.dumps({'version': 1, 'records': [], 'preferences': {'steamReminder': False}}))
        self.assertFalse(Model(self.path).preferences['steamStartup'])

    def test_dnd_suppresses_both_routine_popups(self):
        self.model.command({'action': 'preferences', 'dnd': True})
        self.model.event('Zaman', 'steam:launch:one', 'Launching Steam')
        self.model.event('Zaman', 'steam:shortcut:one', 'Start + Select')
        self.assertEqual(self.model.visible, [])
        self.assertEqual(len(self.model.records), 2)

    def test_launch_errors_are_not_disabled_by_steam_notices(self):
        self.model.command({'action': 'preferences', 'steamStartup': False, 'steamReminder': False})
        self.model.event('Zaman', 'steam-launch-error:one', 'Steam launch needs attention', category='error')
        self.assertEqual(len(self.model.records), 1)


if __name__ == '__main__':
    unittest.main()
