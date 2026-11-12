import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'notifications'))
from model import MAX_BYTES, Model


class ModelTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.now = 1000000
        self.model = Model(Path(self.directory.name) / 'history.json', lambda: self.now)
    def alert(self, **kwargs):
        return self.model.notify('Test', 'Hello', **kwargs)
    def advance(self, delta):
        self.now += delta
        self.model.tick()
    def test_default_timeout_and_retained_history(self):
        pid = self.alert()
        self.advance(3.1)
        self.assertEqual(self.model.closed, [(pid, 1)])
        self.assertEqual(len(self.model.records), 1)
        self.assertEqual(self.model.visible, [])
    def test_identical_replacement_restarts_deadline(self):
        pid = self.alert(owner='client')
        self.advance(2)
        self.assertEqual(pid, self.alert(owner='client', replaces=pid))
        self.advance(2)
        self.assertIn(pid, self.model.live)
        self.advance(1.1)
        self.assertNotIn(pid, self.model.live)
        self.assertEqual(len(self.model.records), 1)
    def test_replacement_cannot_take_other_client_id(self):
        pid = self.alert(owner='a')
        self.assertNotEqual(pid, self.alert(owner='b', replaces=pid))
    def test_explicit_timeout_and_persistent(self):
        one = self.alert(timeout=1000)
        forever = self.alert(timeout=0)
        self.advance(1.1)
        self.assertNotIn(one, self.model.live)
        self.advance(100)
        self.assertIn(forever, self.model.live)
    def test_critical_ignores_timeout_and_preempts_normal(self):
        self.alert(timeout=0)
        self.alert(timeout=0)
        critical = self.alert(urgency=2, timeout=1)
        self.advance(100)
        self.assertIn(critical, self.model.live)
        self.assertIn(self.model.live[critical]['id'], self.model.visible)
        self.assertEqual(len(self.model.visible), 2)
    def test_critical_overflow_is_never_expired(self):
        for _ in range(100): self.alert(urgency=2)
        with self.assertRaises(ValueError): self.alert()
        self.advance(1000000)
        self.model.save()
        self.assertEqual(len(self.model.live), 100)
        self.assertEqual(self.model.closed, [])
        self.assertEqual(len(self.model.queue), 98)
    def test_history_pressure_preserves_old_live_critical(self):
        critical = self.alert(urgency=2)
        for _ in range(150): self.alert()
        self.assertEqual(len(self.model.records), 100)
        self.assertIn(critical, self.model.live)
        self.assertIn(self.model.live[critical], self.model.records)
        self.assertLessEqual(len(self.model.queue), 20)
    def test_expired_then_dismissed_sends_no_second_close(self):
        pid = self.alert()
        rid = self.model.live[pid]['id']
        self.advance(4)
        self.model.command({'action':'dismiss', 'id':rid})
        self.assertEqual(self.model.closed, [(pid,1)])
        self.assertTrue(self.model.records[0]['dismissed'])
    def test_dismiss_visible_keeps_history_and_queue(self):
        for _ in range(4): self.alert()
        self.model.command({'action':'dismiss-visible'})
        self.assertEqual(len(self.model.records),4)
        self.assertEqual(sum(r['dismissed'] for r in self.model.records),2)
        self.assertEqual(len(self.model.visible),2)
    def test_captured_clear_preserves_new_arrival(self):
        self.alert()
        ids = [r['id'] for r in self.model.records]
        new = self.alert()
        self.model.command({'action':'clear','ids':ids})
        self.assertEqual([r['protocolId'] for r in self.model.records], [new])
    def test_view_marks_current_and_new_read_without_dismissal(self):
        self.alert(urgency=2)
        self.model.command({'action':'view','active':True}, 'menu')
        self.alert()
        self.assertEqual(self.model.snapshot()['unread'],0)
        self.assertFalse(any(r['dismissed'] for r in self.model.records))
        self.assertEqual(len(self.model.visible),2, 'history viewing leaves current and incoming popups visible')
        self.model.command({'action':'view','active':False},'menu')
        self.advance(4)
        self.assertEqual(len(self.model.visible),1, 'routine toast expires while history is open')
        self.model.command({'action':'view','active':True},'menu')
        self.model.command({'action':'view','active':False},'menu')
        self.assertEqual(len(self.model.visible),1, 'opening and closing history does not replay expired popups')
    def test_dnd_preserves_critical_and_history(self):
        self.model.command({'action':'preferences','dnd':True})
        self.alert()
        critical = self.alert(urgency=2)
        self.assertEqual(len(self.model.visible),1)
        self.assertEqual(self.model.by_id(self.model.visible[0])['protocolId'],critical)
        self.assertEqual(len(self.model.records),2)
    def test_menu_presentation_moves_no_records_and_changes_no_deadlines(self):
        pid=self.alert()
        rid=self.model.live[pid]['id']
        deadline=self.model.live[pid]['_deadline']
        self.model.command({'action':'menu','active':True},'resident')
        self.assertTrue(self.model.snapshot()['menuOpen'])
        self.assertEqual(self.model.visible,[rid])
        self.assertEqual(self.model.live[pid]['_deadline'],deadline)
        self.advance(2)
        self.model.command({'action':'menu','active':False},'resident')
        self.assertFalse(self.model.snapshot()['menuOpen'])
        self.advance(1.1)
        self.assertNotIn(pid,self.model.live)
        self.assertEqual(self.model.closed,[(pid,1)])
    def test_viewing_history_still_fills_queue_and_respects_dnd(self):
        self.model.command({'action':'view','active':True},'resident')
        for _ in range(4):self.alert(timeout=1000)
        self.assertEqual(len(self.model.visible),2)
        self.assertEqual(len(self.model.queue),2)
        self.advance(1.1)
        self.assertEqual(len(self.model.visible),2)
        self.assertEqual(len(self.model.queue),0)
        self.assertEqual(self.model.snapshot()['unread'],0)
        self.model.command({'action':'preferences','dnd':True})
        self.assertEqual(self.model.visible,[])
        self.alert(urgency=2)
        self.assertEqual(len(self.model.visible),1)
    def test_dnd_on_suppresses_queued_routine_popups(self):
        for _ in range(5): self.alert()
        self.model.command({'action':'preferences','dnd':True})
        self.assertEqual(self.model.visible,[])
        self.assertEqual(self.model.queue,[])
    def test_restart_history_is_archived_not_replayed(self):
        self.alert(urgency=2)
        self.model.command({'action':'preferences','dnd':True})
        restarted=Model(self.model.path,lambda:self.now)
        self.assertEqual(len(restarted.records),1)
        self.assertEqual(restarted.live,{})
        self.assertEqual(restarted.visible,[])
        self.assertFalse(restarted.records[0]['live'])
        self.assertEqual(restarted.records[0]['protocolId'],0)
        self.assertTrue(restarted.preferences['dnd'])
        self.assertNotEqual(restarted.generation,self.model.generation)
    def test_corrupt_unknown_and_oversized_history(self):
        for content in ['bad', '{"version":20,"records":[]}', 'x'*(MAX_BYTES+1)]:
            self.model.path.write_text(content)
            recovered=Model(self.model.path,lambda:self.now)
            self.assertEqual(recovered.records,[])
            self.assertTrue(recovered.error)
    def test_unicode_bounds_and_user_owned_atomic_file(self):
        for _ in range(100):
            self.model.notify('زمن'*100, '🦜'*1000, '🦜'*5000)
        self.assertLessEqual(self.model.path.stat().st_size,MAX_BYTES)
        self.assertEqual(self.model.path.stat().st_mode & 0o777,0o600)
        self.assertEqual(len(json.loads(self.model.path.read_text())['records']),100)
        self.assertFalse(any(p.name.startswith('.notifications-') for p in self.model.path.parent.iterdir()))
    def test_event_dedup_and_disabled_steam_reminder(self):
        self.model.event('Dukkan','transaction:one','Installed')
        self.model.event('Dukkan','transaction:one','Installed')
        self.assertEqual(len(self.model.records),1)
        self.model.command({'action':'preferences','steamReminder':False})
        self.model.event('Zaman','steam:one','Shortcut')
        self.assertEqual(len(self.model.records),1)
    def test_bluetooth_event_retains_typed_category(self):
        self.model.event('Zaman','bluetooth:adapter:transition','Bluetooth on','Adapter',category='bluetooth',state='enabled')
        self.assertEqual(self.model.records[0]['category'],'bluetooth')
        self.assertEqual(self.model.records[0]['state'],'enabled')
    def test_queued_deadline_begins_on_presentation(self):
        a=self.alert(timeout=1000)
        b=self.alert(timeout=1000)
        c=self.alert(timeout=1000)
        self.advance(0.9)
        self.model.dismiss(self.model.live[a]['id'])
        self.advance(0.2)
        self.assertIn(c,self.model.live)
        self.advance(0.8)
        self.assertNotIn(c,self.model.live)
    def test_invalid_clear_does_not_change_records(self):
        self.alert()
        with self.assertRaises(ValueError):
            self.model.command({'action':'clear','ids':[{}]})
        self.assertEqual(len(self.model.records),1)
    def test_transfer_target_is_only_a_typed_hint(self):
        self.alert(hints={'x-zaman-target':'sh -c bad'})
        self.assertEqual(self.model.records[0]['target'],'')
        self.alert(hints={'x-zaman-target':'transfer-pending'})
        self.assertEqual(self.model.records[0]['target'],'transfer-pending')


if __name__ == '__main__': unittest.main()
