import importlib.util
import contextlib
import io
import os
from pathlib import Path
import unittest
from unittest.mock import patch
import types
import sys

spec = importlib.util.spec_from_file_location('action', Path(__file__).resolve().parents[1] / 'actions/zaman-menu-session.py')
action = importlib.util.module_from_spec(spec)
spec.loader.exec_module(action)


def status(state, error='', rom='/games/game.rom'):
    return (state,'nes','retroarch-mesen',rom,'',error)


class Harness:
    def __init__(self, states, foreground='game', fail_exit=False):
        self.states=list(states)
        self.last=None
        self.calls=[]
        self.events=[]
        self.now=0
        self.foreground=foreground
        self.fail_exit=fail_exit
    def call(self, method, signature, args=None):
        self.calls.append(method)
        if method == 'InstalledGameStatus': return ('0','','','Idle','',False)
        if method == 'Status':
            if self.states:
                self.last=self.states.pop(0)
                if isinstance(self.last,Exception): raise self.last
            return self.last
        if method == 'ForegroundStatus': return (self.foreground,'game','test')
        if self.fail_exit: raise RuntimeError('ExitGame call failed')
        return ()
    def emit(self, kind, **fields): self.events.append((kind,fields))
    def pause(self, duration): self.now+=duration
    def run(self, timeout=2):
        action.exit_game(self.call,self.emit,lambda:self.now,self.pause,timeout)


class ExitTests(unittest.TestCase):
    def test_active_waits_for_completion_and_calls_exit_once(self):
        h=Harness([status('Active'),status('Stopping'),status('Stopping'),status('Idle')])
        h.run()
        self.assertEqual(h.calls.count('ExitGame'),1)
        self.assertEqual(h.events[-1][1]['ok'],True)
        self.assertEqual(h.now,1)
    def test_already_idle_is_success_without_exit(self):
        h=Harness([status('Idle')]);h.run()
        self.assertEqual(h.calls,['InstalledGameStatus','Status'])
        self.assertTrue(h.events[-1][1]['ok'])
    def test_already_stopping_waits_without_resending(self):
        h=Harness([status('Stopping'),status('Stopping'),status('Idle')]);h.run()
        self.assertNotIn('ExitGame',h.calls)
    def test_transfer_or_fallback_preemption_does_not_stop_game(self):
        for foreground in ['transfer','menu','library']:
            h=Harness([status('Active')],foreground)
            with self.assertRaisesRegex(RuntimeError,'Another Zaman screen'):h.run()
            self.assertNotIn('ExitGame',h.calls)
    def test_failed_session_reports_error_without_exit(self):
        h=Harness([status('Failed','save failed')])
        with self.assertRaisesRegex(RuntimeError,'save failed'):h.run()
        self.assertNotIn('ExitGame',h.calls)
    def test_shutdown_failure_does_not_report_success(self):
        h=Harness([status('Active'),status('Failed','cleanup failed')])
        with self.assertRaisesRegex(RuntimeError,'cleanup failed'):h.run()
        self.assertEqual(h.calls.count('ExitGame'),1)
        self.assertFalse(any(fields.get('ok') for _,fields in h.events))
    def test_timeout_never_resends_exit(self):
        h=Harness([status('Active'),status('Stopping')])
        with self.assertRaisesRegex(RuntimeError,'still closing'):h.run()
        self.assertEqual(h.calls.count('ExitGame'),1)
    def test_replacement_game_is_never_sent_another_exit(self):
        h=Harness([status('Active'),status('Active',rom='/games/other.rom')])
        with self.assertRaisesRegex(RuntimeError,'session changed'):h.run()
        self.assertEqual(h.calls.count('ExitGame'),1)
    def test_ambiguous_exit_delivery_is_not_retried(self):
        h=Harness([status('Active')],fail_exit=True)
        with self.assertRaisesRegex(RuntimeError,'call failed'):h.run()
        self.assertEqual(h.calls.count('ExitGame'),1)
    def test_owner_loss_is_reported_instead_of_retargeted(self):
        h=Harness([status('Active'),RuntimeError('unique service owner vanished')])
        with self.assertRaisesRegex(RuntimeError,'owner vanished'):h.run()
        self.assertEqual(h.calls.count('ExitGame'),1)

    def test_real_adapter_pins_all_session_calls_to_one_owner(self):
        calls=[]
        statuses=iter([status('Active'),status('Idle')])
        class Variant:
            def __init__(self,*args): self.data=args[-1]
            def unpack(self): return self.data
        class Bus:
            def call_sync(self,destination,path,interface,method,parameters,reply_type,flags,timeout,cancellable):
                calls.append((destination,path,method,flags))
                if method=='GetNameOwner': return Variant((':1.999',))
                if method=='Status': return Variant(next(statuses))
                if method=='InstalledGameStatus': return Variant(('0','','','Idle','',False))
                if method=='ForegroundStatus': return Variant(('game','',''))
                return Variant(())
        gio=types.SimpleNamespace(BusType=types.SimpleNamespace(SESSION=0),
            DBusCallFlags=types.SimpleNamespace(NONE=0,NO_AUTO_START=1),bus_get_sync=lambda *args:Bus())
        glib=types.SimpleNamespace(Variant=Variant,VariantType=types.SimpleNamespace(new=lambda v:v))
        gi=types.ModuleType('gi');gi.require_version=lambda *args:None
        repo=types.ModuleType('gi.repository');repo.Gio=gio;repo.GLib=glib
        output=io.StringIO()
        with patch.dict(sys.modules,{'gi':gi,'gi.repository':repo}), patch.object(action.socket,'socket'), \
                patch.object(sys,'argv',['action.py','exit-game']),patch.dict(os.environ,{'XDG_RUNTIME_DIR':'/nonexistent-test-runtime'}),contextlib.redirect_stdout(output):
            self.assertEqual(action.main(),0)
        self.assertEqual(calls[0][0],'org.freedesktop.DBus')
        self.assertTrue(all(destination==':1.999' and path==action.PATH and flags==1 for destination,path,method,flags in calls[1:]))
        self.assertIn('"ok": true',output.getvalue())

    def test_native_exit_uses_one_generation_bound_close(self):
        calls=[]; events=[]; now=[0]
        states=iter([
            ('18446744073709551615','net.supertuxkart.SuperTuxKart','SuperTuxKart','Running','',True),
            ('18446744073709551615','net.supertuxkart.SuperTuxKart','SuperTuxKart','Stopping','',False),
            ('18446744073709551615','net.supertuxkart.SuperTuxKart','SuperTuxKart','Exited','',False),
        ])
        def call(method, signature, args=None):
            calls.append((method,args))
            if method=='InstalledGameStatus': return next(states)
            if method=='ForegroundStatus': return ('game','','')
            return ()
        action.exit_game(call,lambda kind,**fields:events.append((kind,fields)),
                         lambda:now[0],lambda dt:now.__setitem__(0,now[0]+dt),2)
        self.assertEqual([args for method,args in calls if method=='InstalledGameClose'],
                         [('18446744073709551615',)])
        self.assertTrue(events[-1][1]['ok'])

    def test_native_replacement_is_not_closed(self):
        calls=[]; now=[0]
        states=iter([
            ('7','net.supertuxkart.SuperTuxKart','SuperTuxKart','Running','',True),
            ('8','org.openttd.OpenTTD','OpenTTD','Running','',True),
        ])
        def call(method, signature, args=None):
            calls.append((method,args))
            if method=='InstalledGameStatus': return next(states)
            if method=='ForegroundStatus': return ('game','','')
            return ()
        with self.assertRaisesRegex(RuntimeError, 'changed while closing'):
            action.exit_game(call,lambda *args,**kwargs:None,
                             lambda:now[0],lambda dt:now.__setitem__(0,now[0]+dt),2)
        self.assertEqual([args for method,args in calls if method=='InstalledGameClose'], [('7',)])


if __name__ == '__main__': unittest.main()
