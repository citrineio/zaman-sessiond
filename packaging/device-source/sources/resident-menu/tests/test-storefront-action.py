import importlib.util
from pathlib import Path
import unittest
spec=importlib.util.spec_from_file_location('bridge',Path(__file__).resolve().parents[1]/'actions/zaman-menu-session.py')
bridge=importlib.util.module_from_spec(spec);spec.loader.exec_module(bridge)
class StorefrontTests(unittest.TestCase):
    def run_action(self, action, contexts, fail=False):
        calls=[];events=[];now=[0]
        def call(method,signature,args=None):
            calls.append((method,args))
            if method=='StorefrontAction':
                if fail: raise RuntimeError('ambiguous delivery')
                return ()
            return contexts.pop(0) if len(contexts)>1 else contexts[0]
        def pause(delay):now[0]+=delay
        bridge.storefront_action(call,action,'steam','18446744073709551615',lambda kind,**fields:events.append((kind,fields)),lambda:now[0],pause,1)
        return calls,events
    def test_waits_for_running_and_preserves_exact_generation(self):
        calls,events=self.run_action('launch',[('steam',1,'Starting','zaman',False,''),('steam',1,'Running','zaman',False,'')])
        self.assertEqual(calls[0][1],('launch','steam','18446744073709551615'))
        self.assertEqual(sum(c[0]=='StorefrontAction' for c in calls),1)
        self.assertTrue(events[-1][1]['ok'])
    def test_home_and_close_wait_for_foreground_recovery(self):
        for action in ['home','close']:
            calls,events=self.run_action(action,[('steam',1,'Stopping','zaman',False,''),('',0,'Idle','zaman',False,'')])
            self.assertTrue(events[-1][1]['ok']);self.assertEqual(sum(c[0]=='StorefrontAction' for c in calls),1)
    def test_failed_launch_does_not_report_success(self):
        with self.assertRaisesRegex(RuntimeError,'exited or failed'):
            self.run_action('launch',[('',0,'Idle','zaman',False,'')])
    def test_ambiguous_delivery_is_not_retried(self):
        with self.assertRaisesRegex(RuntimeError,'ambiguous delivery'):
            self.run_action('launch',[],True)
    def test_timeout_does_not_report_success(self):
        with self.assertRaisesRegex(RuntimeError,'still pending'):
            self.run_action('launch',[('steam',1,'Starting','zaman',False,'')])
if __name__=='__main__':unittest.main()
