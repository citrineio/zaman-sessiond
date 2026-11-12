"""GIO owns the notification bus; the reused Quickshell renderer never does."""
import asyncio
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import struct
import subprocess
import sys
import threading
import uuid

from model import Model
from transport import LIMIT, socket_path

XML = '''<node><interface name="org.freedesktop.Notifications">
<method name="GetCapabilities"><arg direction="out" type="as"/></method>
<method name="GetServerInformation"><arg direction="out" type="s"/><arg direction="out" type="s"/><arg direction="out" type="s"/><arg direction="out" type="s"/></method>
<method name="Notify"><arg direction="in" type="s"/><arg direction="in" type="u"/><arg direction="in" type="s"/><arg direction="in" type="s"/><arg direction="in" type="s"/><arg direction="in" type="as"/><arg direction="in" type="a{sv}"/><arg direction="in" type="i"/><arg direction="out" type="u"/></method>
<method name="CloseNotification"><arg direction="in" type="u"/></method>
<signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
</interface></node>'''
SERVICE = 'org.freedesktop.Notifications'
PATH = '/org/freedesktop/Notifications'


def serve(renderer=True):
    try:
        import gi
        gi.require_version('Gio', '2.0')
        from gi.repository import Gio, GLib
    except ImportError as error:
        raise RuntimeError('Python GIO bindings unavailable; preflight failed. Install nothing automatically.') from error
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    # DO_NOT_QUEUE. No replacing another desktop notification server.
    reply = connection.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
        'org.freedesktop.DBus', 'RequestName', GLib.Variant('(su)', (SERVICE, 4)),
        GLib.VariantType.new('(u)'), Gio.DBusCallFlags.NONE, 2000, None).unpack()[0]
    if reply != 1:
        raise RuntimeError('Another server owns org.freedesktop.Notifications; inspect busctl --user status ' + SERVICE)
    state_path = Path(os.environ.get('XDG_STATE_HOME', str(Path.home() / '.local/state'))) / 'zaman/notifications.json'
    model = Model(state_path)
    lock = threading.RLock()
    loop = asyncio.new_event_loop()
    peers = {}
    renderer_status = {}
    ready = threading.Event()
    startup_error = []
    stopping = threading.Event()
    renderer_process = None

    def snapshot():
        data = model.snapshot()
        data['renderer'] = {k:v for k,v in renderer_status.items() if k != 'peer'}
        return data

    def publish():
        with lock:
            pending, model.closed = model.closed, []
            data = snapshot()
        for pid, reason in pending:
            connection.emit_signal(None, PATH, SERVICE, 'NotificationClosed', GLib.Variant('(uu)', (pid, reason)))
        def enqueue():
            for queue in tuple(peers.values()):
                # A lagging UI gets the latest state, never an unbounded backlog.
                if queue.full():
                    queue.get_nowait()
                queue.put_nowait(data)
        loop.call_soon_threadsafe(enqueue)

    def method(conn, sender, object_path, interface, name, params, invocation):
        try:
            with lock:
                args = params.unpack()
                if name == 'GetCapabilities':
                    result = GLib.Variant('(as)', (['body'],))
                elif name == 'GetServerInformation':
                    result = GLib.Variant('(ssss)', ('Zaman', 'Kawn Electro', '1.0', '1.2'))
                elif name == 'Notify':
                    app, replaces, icon, title, body, actions, hints, timeout = args
                    pid = model.notify(app, title, body, hints, timeout, hints.get('urgency', 1), replaces, sender)
                    result = GLib.Variant('(u)', (pid,))
                elif name == 'CloseNotification':
                    r = model.live.get(args[0])
                    if r is not None and r.get('_owner') == sender:
                        model.close(args[0], 3)
                        model.save()
                    result = GLib.Variant('()', ())
                else:
                    raise ValueError('unknown method')
            invocation.return_value(result)
            publish()
        except Exception as error:
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', str(error))

    connection.register_object(PATH, Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0], method, None, None)

    async def peer(reader, writer):
        identity = uuid.uuid4().hex
        watch = False
        try:
            credentials = writer.get_extra_info('socket').getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
            if struct.unpack('3i', credentials)[1] != os.getuid() or len(peers) >= 8:
                return
            async def send(data):
                writer.write((json.dumps(data, ensure_ascii=False, separators=(',', ':')) + '\n').encode())
                await asyncio.wait_for(writer.drain(), 2)
            first = await asyncio.wait_for(reader.readline(), 2)
            msg = json.loads(first)
            if not isinstance(msg, dict):
                raise ValueError('expected an object')
            watch = msg.get('action') == 'watch'
            if watch:
                queue = asyncio.Queue(maxsize=1)
                peers[identity] = queue
                with lock:
                    initial = snapshot()
                await send(initial)
                async def updates():
                    while True:
                        await send(await queue.get())
                async def commands():
                    while line := await reader.readline():
                        await dispatch(json.loads(line), identity)
                tasks = [asyncio.create_task(updates()), asyncio.create_task(commands())]
                try:
                    await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
                finally:
                    for task in tasks:
                        task.cancel()
                    await asyncio.gather(*tasks, return_exceptions=True)
            else:
                await send(await dispatch(msg, identity))
        except (ValueError, OSError, asyncio.TimeoutError) as error:
            try:
                writer.write((json.dumps({'type': 'error', 'error': str(error)}) + '\n').encode())
                await asyncio.wait_for(writer.drain(), 1)
            except (OSError, asyncio.TimeoutError):
                pass
        finally:
            peers.pop(identity, None)
            with lock:
                if renderer_status.get('peer') == identity:
                    renderer_status.clear()
                was_viewing = identity in model.viewers
                model.viewers.discard(identity)
                was_presenting = identity in model.presenters
                model.presenters.discard(identity)
            if was_viewing or was_presenting:
                publish()
            writer.close()
            await writer.wait_closed()

    async def dispatch(msg, identity):
        if not isinstance(msg, dict):
            raise ValueError('expected an object')
        with lock:
            if msg.get('action') == 'renderer-status':
                if identity not in peers:
                    raise ValueError('renderer status requires a watched connection')
                output = str(msg.get('output',''))[:100]
                present = msg.get('outputPresent') is True
                changed = renderer_status.get('output') != output or renderer_status.get('outputPresent') != present
                renderer_status.update(peer=identity, output=output, outputPresent=present, updatedAt=model.clock())
                if changed:
                    GLib.idle_add(lambda: (publish(), False)[1])
                return snapshot()  # Heartbeats do not rewrite history or churn UI models.
            if msg.get('action') == 'event':
                # Same-UID local producers, never executable actions.
                if not isinstance(msg.get('key'), str):
                    raise ValueError('event requires a deduplication key')
                model.event(str(msg.get('source', 'Zaman'))[:64], msg['key'][:160],
                    msg.get('title', 'Notification'), msg.get('body', ''), msg.get('category', 'system'),
                    msg.get('state', ''), msg.get('target', ''))
                result = snapshot()
                if str(msg.get('source')) == 'Dukkan' and __import__('re').fullmatch(r'transaction:[a-f0-9]{32}:[1-9][0-9]{0,15}', msg['key']):
                    result['acceptedEvent'] = msg['key']
            else:
                model.command(msg, identity)
                result = snapshot()
        GLib.idle_add(lambda: (publish(), False)[1])
        return result

    async def run_socket():
        path = Path(socket_path())
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        if path.parent.is_symlink() or path.parent.stat().st_uid != os.getuid():
            raise RuntimeError('unsafe notification runtime directory')
        os.chmod(path.parent, 0o700)
        if path.exists():
            path.unlink()
        server = await asyncio.start_unix_server(peer, str(path), limit=8192)
        os.chmod(path, 0o600)
        ready.set()
        async with server:
            await server.serve_forever()

    def socket_thread():
        asyncio.set_event_loop(loop)
        try:
            loop.run_until_complete(run_socket())
        except Exception as error:
            startup_error.append(error)
            ready.set()
            if not stopping.is_set():
                GLib.idle_add(main.quit)

    main = GLib.MainLoop()
    thread = threading.Thread(target=socket_thread, daemon=True)
    thread.start()
    if not ready.wait(3) or startup_error:
        raise RuntimeError('notification IPC startup failed: ' + str(startup_error))
    if renderer:
        executable = shutil.which('qs') or shutil.which('quickshell')
        if not executable:
            raise RuntimeError('Quickshell is missing; run preflight')
        renderer_process = subprocess.Popen([executable, '--no-duplicate', '-p', str(Path(__file__).parent.parent / 'notification-shell.qml')],
            env={**os.environ, 'ZAMAN_NOTIFICATION_CLIENT': str(Path(__file__).parent.parent / 'tools/zaman-notifications'),
                 'QT_QPA_PLATFORM': 'wayland', 'QS_NO_RELOAD_POPUP': '1'})
    fatal = []
    def tick():
        if renderer_process and renderer_process.poll() is not None:
            fatal.append('toast renderer exited; inspect service logs')
            main.quit()
            return False
        with lock:
            changed = model.tick()
        if changed:
            publish()
        return True
    GLib.timeout_add(100, tick)
    def stop():
        main.quit()
        return False
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGTERM, stop)
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGINT, stop)
    print('Zaman notifications ready; generation=' + model.generation, flush=True)
    try:
        main.run()
    finally:
        stopping.set()
        if renderer_process:
            renderer_process.terminate()
            try:
                renderer_process.wait(2)
            except subprocess.TimeoutExpired:
                renderer_process.kill()
        with lock:
            model.save()
        Path(socket_path()).unlink(missing_ok=True)
    if fatal or startup_error:
        raise RuntimeError(str(fatal or startup_error))
