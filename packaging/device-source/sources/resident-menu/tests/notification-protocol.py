#!/usr/bin/env python3
"""Run ONLY through dbus-run-session. No live bus ownership or user state writes."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def main():
    if os.environ.get('ZAMAN_PRIVATE_NOTIFICATION_TEST') != '1':
        raise RuntimeError('Use: dbus-run-session -- env ZAMAN_PRIVATE_NOTIFICATION_TEST=1 python3 tests/notification-protocol.py')
    import gi
    gi.require_version('Gio', '2.0')
    from gi.repository import Gio, GLib
    root = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(root / 'notifications'))
    from transport import request
    with tempfile.TemporaryDirectory(prefix='zaman-notification-test-') as directory:
        os.environ['XDG_RUNTIME_DIR'] = directory
        os.environ['XDG_STATE_HOME'] = directory
        environment = os.environ.copy()
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        def start():
            child = subprocess.Popen(['/usr/bin/python3', str(root / 'tools/zaman-notifications'), 'serve', '--no-renderer'], env=environment)
            for _ in range(100):
                if child.poll() is not None:
                    raise RuntimeError('private notification test server exited')
                try:
                    request({'action':'snapshot'})
                    return child
                except OSError:
                    time.sleep(0.03)
            child.terminate();child.wait(3)
            raise RuntimeError('private notification test server startup timeout')
        child = start()
        try:
            service='org.freedesktop.Notifications'
            def call(method, params, reply):
                return bus.call_sync(service, '/org/freedesktop/Notifications', service, method,
                    params, GLib.VariantType.new(reply), Gio.DBusCallFlags.NO_AUTO_START, 2000, None).unpack()
            assert call('GetCapabilities',None,'(as)') == (['body'],)
            def notify(replaces=0, timeout=600):
                return call('Notify', GLib.Variant('(susssasa{sv}i)', ('Test',replaces,'','Identical','Body',[],{},timeout)), '(u)')[0]
            pid=notify()
            time.sleep(0.4)
            assert notify(pid) == pid
            time.sleep(0.35)
            snapshot=request({'action':'snapshot'})
            assert len(snapshot['records'])==1 and snapshot['records'][0]['live']
            time.sleep(0.5)
            snapshot=request({'action':'snapshot'})
            assert not snapshot['records'][0]['live']
            rid=snapshot['records'][0]['id']
            request({'action':'dismiss','id':rid})
            forever=notify(timeout=0)
            call('CloseNotification',GLib.Variant('(u)',(forever,)),'()')
            snapshot=request({'action':'snapshot'})
            assert not any(r['live'] for r in snapshot['records'])
            # Another process must fail without stealing ownership.
            competing=subprocess.run(['/usr/bin/python3', str(root/'tools/zaman-notifications'), 'serve', '--no-renderer'],env=environment,capture_output=True,timeout=4)
            assert competing.returncode != 0 and b'Another server owns' in competing.stderr
            child.terminate();child.wait(4)
            child=start()
            restored=request({'action':'snapshot'})
            assert len(restored['records'])==2 and not restored['visible']
            assert not any(r['live'] for r in restored['records'])
            event = dict(action='event', source='Dukkan', key='transaction:' + 'a' * 32 + ':1',
                         title='Installed', category='install', state='installed',
                         target='dukkan-transaction:' + 'b' * 32)
            delivered = request(event)
            assert delivered['acceptedEvent'] == event['key']
            transaction_id = delivered['records'][0]['id']
            assert delivered['records'][0]['target'] == event['target']
            child.terminate();child.wait(4)
            child = start()
            repeated = request(event)
            assert repeated['acceptedEvent'] == event['key']
            assert len(repeated['records']) == 3 and not repeated['visible']
            request({'action':'clear', 'ids':[transaction_id]})
            child.terminate();child.wait(4)
            child = start()
            repeated = request(event)
            assert repeated['acceptedEvent'] == event['key']
            assert len(repeated['records']) == 2 and not repeated['visible']
            print('PASS: private D-Bus protocol/history and durable transaction ACK/restart/clear deduplication')
        finally:
            child.terminate();child.wait(4)


if __name__=='__main__':
    try: main()
    except Exception as error:
        print('FAIL: private notification protocol test: '+str(error),file=sys.stderr)
        sys.exit(1)
