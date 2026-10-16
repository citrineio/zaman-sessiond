import ast
import concurrent.futures
import importlib.util
import types
from pathlib import Path
import unittest
spec=importlib.util.spec_from_file_location('events',Path(__file__).resolve().parents[1]/'tools/zaman-notification-events.py')
events=importlib.util.module_from_spec(spec);spec.loader.exec_module(events)

def net(name, state, connection='', ssid=None):
    return dict(name=name,state=state,connection=connection,networkName=name if ssid is None else ssid,named=bool(name))

class EventTests(unittest.TestCase):
    def test_startup_is_not_hotplug(self):
        c=events.Changes();self.assertEqual(c.update('controllers','one',{'physical':'Controller'}),[])
        self.assertEqual(c.update('controllers','two',{'physical':'Controller'}),[])
    def test_logical_device_produces_once(self):
        c=events.Changes();c.update('controllers','one',{})
        self.assertEqual(len(c.update('controllers','one',{'physical':'Controller'})),1)
        self.assertEqual(c.update('controllers','one',{'physical':'Controller'}),[])
        self.assertEqual(c.update('controllers','one',{})[0]['state'],'disconnected')
    def test_network_transition_does_not_lose_disconnect(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('Home',100)})
        self.assertEqual(c.update('network','one',{'wlan0':net('',110)}),[])
        self.assertEqual(c.update('network','one',{'wlan0':net('',30)})[0]['state'],'disconnected')
    def test_network_failure_after_connecting(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('',30)})
        self.assertEqual(c.update('network','one',{'wlan0':net('Guest',50)}),[])
        self.assertEqual(c.update('network','one',{'wlan0':net('',120)})[0]['state'],'error')
    def test_battery_hysteresis(self):
        c=events.Changes();c.update('battery','one',{'mac':('Controller',50)})
        self.assertEqual(len(c.update('battery','one',{'mac':('Controller',20)})),1)
        for level in [19,21,18,29,15]:self.assertEqual(c.update('battery','one',{'mac':('Controller',level)}),[])
        self.assertEqual(c.update('battery','one',{'mac':('Controller',40)}),[])
        c.last_sent.clear();self.assertEqual(len(c.update('battery','one',{'mac':('Controller',18)})),1)
    def test_network_changes_while_device_stays_connected(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('Home',100,'home-profile')})
        change=c.update('network','one',{'wlan0':net('Guest',100,'guest-profile')})
        self.assertEqual(change[0]['title'],'Network changed')
        self.assertEqual(change[0]['body'],'Guest')
        self.assertEqual(change[0]['state'],'connected')
        self.assertEqual(c.update('network','one',{'wlan0':net('Guest',100,'guest-profile')}),[])
    def test_ssid_change_on_same_profile_is_detected(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('Home',100,'profile')})
        self.assertEqual(c.update('network','one',{'wlan0':net('Guest',100,'profile')})[0]['body'],'Guest')
    def test_network_transition_preserves_old_identity_until_terminal_state(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('Home',100,'home')})
        self.assertEqual(c.update('network','one',{'wlan0':net('Guest',50,'guest')}),[])
        self.assertEqual(c.update('network','one',{'wlan0':net('Guest',100,'guest')})[0]['title'],'Network changed')
    def test_disconnect_keeps_previous_network_name(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('شبكة المنزل',100,'home')})
        self.assertEqual(c.update('network','one',{'wlan0':net('',30)})[0]['body'],'شبكة المنزل')
    def test_failure_keeps_attempted_network_name(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('',30)})
        c.update('network','one',{'wlan0':net('Guest',50,'guest')})
        self.assertEqual(c.update('network','one',{'wlan0':net('',120)})[0]['body'],'Guest')
    def test_roaming_and_late_metadata_do_not_generate_connection_events(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('Home',100,'home')})
        self.assertEqual(c.update('network','one',{'wlan0':net('Home',100,'home')}),[])
        self.assertEqual(c.update('network','one',{'wlan0':net('',100,'')}),[])
        self.assertEqual(c.update('network','one',{'wlan0':net('Home',100,'home')}),[])
    def test_rapid_network_changes_have_distinct_delivery_keys(self):
        c=events.Changes();c.update('network','one',{'wlan0':net('Home',100,'home')})
        changes=[c.update('network','one',{'wlan0':net(name,100,name)})[0] for name in ['Guest','Home','Guest']]
        self.assertEqual(len({e['key'] for e in changes}),3)
    def test_bluetooth_on_off_on_without_discovery_notifications(self):
        c=events.Changes();self.assertEqual(c.update('bluetooth','one',{'mac':('Adapter',False)}),[])
        changes=[c.update('bluetooth','one',{'mac':('Adapter',powered)})[0] for powered in [True,False,True]]
        self.assertEqual([e['title'] for e in changes],['Bluetooth on','Bluetooth off','Bluetooth on'])
        self.assertEqual([e['state'] for e in changes],['enabled','disabled','enabled'])
        self.assertEqual(len({e['key'] for e in changes}),3)
        self.assertEqual(c.update('bluetooth','one',{'mac':('Adapter',True)}),[])
        self.assertEqual(c.update('bluetooth','restarted',{'mac':('Adapter',False)}),[])
    def test_bluetooth_reader_only_reports_adapter_power(self):
        objects={'/a':{'org.bluez.Adapter1':{'Address':'mac','Alias':'Adapter','Powered':False}},
            '/device':{'org.bluez.Device1':{'Connected':True}},'/unknown':{'org.bluez.Adapter1':{'Alias':'Missing telemetry'}}}
        self.assertEqual(events.bluetooth_adapters(objects),{'mac':('Adapter',False)})
    def test_network_reader_decodes_ssid_and_wired_profile_without_secrets(self):
        data={('/wifi','org.freedesktop.NetworkManager.Device'):{'DeviceType':2,'Interface':'wlan0','State':100,'ActiveConnection':'/wifi-profile'},
            ('/wifi','org.freedesktop.NetworkManager.Device.Wireless'):{'ActiveAccessPoint':'/ap'},
            ('/ap','org.freedesktop.NetworkManager.AccessPoint'):{'Ssid':list('شبكة المنزل'.encode())},
            ('/wifi-profile','org.freedesktop.NetworkManager.Connection.Active'):{'Id':'Custom profile','Uuid':'wifi-id'},
            ('/wired','org.freedesktop.NetworkManager.Device'):{'DeviceType':1,'Interface':'eth0','State':100,'ActiveConnection':'/wired-profile'},
            ('/wired-profile','org.freedesktop.NetworkManager.Connection.Active'):{'Id':'Office LAN','Uuid':'wired-id'}}
        result=events.network_devices(['/wifi','/wired'],lambda path,interface:data[(path,interface)])
        self.assertEqual(result['wlan0']['name'],'شبكة المنزل')
        self.assertEqual(result['wlan0']['connection'],'wifi-id')
        self.assertEqual(result['eth0']['name'],'Office LAN')
    def test_optional_profile_or_ap_disappearing_preserves_device_state(self):
        def properties(path,interface):
            if interface.endswith('.Device'):return {'DeviceType':2,'Interface':'wlan0','State':100,'ActiveConnection':'/gone'}
            raise RuntimeError('object disappeared')
        result=events.network_devices(['/wifi'],properties)
        self.assertEqual(result['wlan0']['state'],100)
        self.assertEqual(result['wlan0']['name'],'Wi-Fi · wlan0')
    def test_continuous_invalidations_cannot_postpone_scan_forever(self):
        tree=ast.parse((Path(__file__).resolve().parents[1]/'tools/zaman-notification-events.py').read_text())
        node=next(n for n in ast.walk(tree) if isinstance(n,ast.FunctionDef) and n.name=='invalidated')
        factory=ast.parse('def factory():\n debounce=0\n return invalidated\n').body[0]
        factory.body.insert(1,node)
        scope={};timers=[];scans=[]
        scope['GLib']=types.SimpleNamespace(timeout_add=lambda delay,callback:timers.append((delay,callback)) or len(timers))
        scope['schedule']=lambda:scans.append(True)
        exec(compile(ast.fix_missing_locations(ast.Module(body=[factory],type_ignores=[])),'<actual invalidation handler>','exec'),scope)
        invalidated=scope['factory']()
        for _ in range(100):invalidated()
        self.assertEqual(len(timers),1);self.assertEqual(timers[0][0],200)
        timers[0][1]();self.assertEqual(scans,[True])
        invalidated();self.assertEqual(len(timers),2)
    def test_signal_during_busy_scan_runs_one_follow_up(self):
        submitted=[];posted=[]
        def submit(work):
            future=concurrent.futures.Future();submitted.append(future);return future
        scheduler=events.ScanScheduler(types.SimpleNamespace(submit=submit),lambda:None,posted.append)
        scheduler.request()
        for _ in range(10):scheduler.request()
        self.assertEqual(len(submitted),1)
        submitted[0].set_result(None);posted.pop(0)()
        self.assertEqual(len(submitted),2)
        submitted[1].set_result(None);posted.pop(0)()
        self.assertEqual(len(submitted),2)

if __name__=='__main__':unittest.main()
