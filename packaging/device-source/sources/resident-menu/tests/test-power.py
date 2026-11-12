import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('action',Path(__file__).resolve().parents[1]/'actions/zaman-menu-session.py')
action=importlib.util.module_from_spec(spec);spec.loader.exec_module(action)


class PowerTests(unittest.TestCase):
    def run_action(self,kind,foreground='library',fail=False):
        calls=[];events=[];now=[0]
        def call(method,signature):
            calls.append(method)
            if method=='ForegroundStatus':return (foreground,'','')
            if method=='Status':return ('Failed' if fail else 'Idle','','','','','logind denied' if fail else '')
            if method=='InstalledGameStatus':return ('0','','','Idle','',False)
            return ()
        def emit(kind,**fields):events.append((kind,fields))
        def pause(dt):now[0]+=dt
        try:action.power_action(call,kind,emit,lambda:now[0],pause,1)
        except RuntimeError as error:return calls,events,str(error)
        self.fail('An acknowledgement must not be reported as completed power-off')
    def test_both_methods_are_one_shot_and_ack_is_not_completion(self):
        for kind,method in [('reboot','Reboot'),('shutdown','Shutdown')]:
            calls,events,error=self.run_action(kind)
            self.assertEqual(calls.count(method),1)
            self.assertEqual(events[0][0],'power-progress')
            self.assertIn('still pending',error)
            self.assertNotIn('ExitGame',calls)
    def test_transfer_prevents_power_request(self):
        calls,events,error=self.run_action('shutdown','transfer')
        self.assertEqual(calls,['ForegroundStatus'])
        self.assertEqual(events,[])
    def test_logind_failure_is_reported_for_retry(self):
        calls,events,error=self.run_action('reboot',fail=True)
        self.assertEqual(calls.count('Reboot'),1)
        self.assertEqual(error,'logind denied')


if __name__=='__main__':unittest.main()
