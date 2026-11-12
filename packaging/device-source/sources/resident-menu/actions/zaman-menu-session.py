#!/usr/bin/env python3
"""One exit action on the session bus; sessiond owns emulator shutdown."""
import json
import os
import re
from pathlib import Path
import socket
import sys
import time

SERVICE = 'com.kawnelectro.Zaman.Session1'
PATH = '/com/kawnelectro/Zaman/Session1'
INTERFACE = SERVICE
STATUS_SIGNATURE = '(ssssss)'
INSTALLED_SIGNATURE = '(sssssb)'


def emit(kind, **fields):
    print(json.dumps({'type': kind, **fields}), flush=True)


def log(message):
    try:
        with (Path(os.environ['XDG_RUNTIME_DIR']) / 'zaman-menu-timing.log').open('a') as stream:
            stream.write(f'pid={os.getpid()} [menu-exit] {message}\n')
    except (KeyError, OSError):
        pass


def exit_game(call, emit_event=emit, clock=time.monotonic, pause=time.sleep, timeout=120):
    """call is already pinned to one unique sessiond bus owner."""
    installed = call('InstalledGameStatus', INSTALLED_SIGNATURE)
    if installed[3] in ('Starting', 'Running', 'Stopping', 'CleanupFailed'):
        return exit_installed_game(call, installed, emit_event, clock, pause, timeout)
    deadline = clock() + timeout
    initial = call('Status', STATUS_SIGNATURE)
    identity = initial[1:4]
    if initial[0] == 'Idle':
        emit_event('exit-result', ok=True, message='The game has already closed.')
        return
    if initial[0] not in ('Active', 'Stopping'):
        raise RuntimeError(initial[5] or 'There is no active game session to close.')
    if initial[0] == 'Active':
        foreground = call('ForegroundStatus', '(sss)')[0]
        if foreground != 'game':
            raise RuntimeError('Another Zaman screen has control. Return to the game before trying again.')
        # Never resend this call after an error/timeout: its delivery may already
        # have succeeded. A subsequent explicit retry reads Stopping and waits.
        call('ExitGame', '()')
        log('ExitGame accepted')
    emit_event('exit-progress', message='Closing game…')
    while clock() < deadline:
        status = call('Status', STATUS_SIGNATURE)
        if status[1:4] != identity:
            raise RuntimeError('The game session changed while closing. No further exit request was sent.')
        if status[0] == 'Idle':
            log('game finished; sessiond reports Idle')
            emit_event('exit-result', ok=True, message='Game closed.')
            return
        if status[0] == 'Failed':
            raise RuntimeError(status[5] or 'The game could not be closed normally.')
        if status[0] not in ('Active', 'Stopping'):
            raise RuntimeError('Unexpected game state: ' + status[0])
        pause(0.5)
    raise RuntimeError('The game is still closing. Check its state before trying again.')


def exit_installed_game(call, initial, emit_event=emit, clock=time.monotonic,
                        pause=time.sleep, timeout=120):
    generation, app = initial[:2]
    if initial[3] in ('Starting', 'Running', 'CleanupFailed'):
        if initial[3] != 'CleanupFailed':
            foreground = call('ForegroundStatus', '(sss)')[0]
            if foreground != 'game':
                raise RuntimeError('Another Zaman screen has control. Return to the game before trying again.')
        # One generation-bound request to the same unique D-Bus owner.
        call('InstalledGameClose', '()', (generation,))
        log('InstalledGameClose accepted for ' + app)
    emit_event('exit-progress', message='Closing game…')
    deadline = clock() + timeout
    while clock() < deadline:
        state = call('InstalledGameStatus', INSTALLED_SIGNATURE)
        if state[:2] != (generation, app):
            raise RuntimeError('The installed game changed while closing. No further exit request was sent.')
        if state[3] == 'Exited':
            emit_event('exit-result', ok=True, message='Game closed.')
            return
        if state[3] in ('Failed', 'CleanupFailed'):
            raise RuntimeError(state[4] or 'The installed game could not be cleaned up.')
        if state[3] not in ('Starting', 'Running', 'Stopping'):
            raise RuntimeError('Unexpected installed game state: ' + state[3])
        pause(0.5)
    raise RuntimeError('The game is still closing. Check its state before trying again.')


def power_action(call, kind, emit_event=emit, clock=time.monotonic, pause=time.sleep, timeout=120):
    method = {'reboot': 'Reboot', 'shutdown': 'Shutdown'}[kind]
    foreground = call('ForegroundStatus', '(sss)')[0]
    if foreground not in ('library', 'game', 'storefront'):
        raise RuntimeError('Close Transfer or the other foreground screen first.')
    # Only sessiond may stop the game and request power from logind. A successful
    # method reply acknowledges acceptance; it does not prove the system is off.
    call(method, '()')
    log(method + ' accepted')
    emit_event('power-progress', message='Restarting Zaman…' if kind == 'reboot' else 'Shutting down Zaman…')
    deadline = clock() + timeout
    while clock() < deadline:
        state = call('Status', STATUS_SIGNATURE)
        if state[0] == 'Failed':
            raise RuntimeError(state[5] or 'The power request failed.')
        installed = call('InstalledGameStatus', INSTALLED_SIGNATURE)
        if installed[3] == 'CleanupFailed':
            raise RuntimeError(installed[4] or 'Installed game cleanup failed; power was not completed.')
        pause(0.5)
    raise RuntimeError('The power request is still pending. Check the session state before trying again.')


def storefront_action(call, action, id, generation, emit_event=emit, clock=time.monotonic, pause=time.sleep, timeout=20):
    if action not in ('launch', 'home', 'close') or id not in ('dukkan', 'steam'):
        raise RuntimeError('Unknown storefront action.')
    # Exactly one mutation, even when the reply is ambiguous or times out.
    call('StorefrontAction', '()', (action, id, generation))
    deadline = clock() + timeout
    while clock() < deadline:
        context = call('StorefrontStatus', '(stssbs)')
        if action == 'home' and not context[0]: break
        if action == 'close' and not context[0]: break
        if action == 'launch' and context[0] == id and context[2] == 'Running': break
        if context[0] == id and context[2] == 'Failed':
            raise RuntimeError(context[5] or 'The storefront could not start.')
        # Failure/exit restores Home. Report it instead of waiting indefinitely.
        if action == 'launch' and not context[0]:
            raise RuntimeError('The storefront exited or failed during startup. Check the sessiond journal.')
        pause(0.1)
    else:
        raise RuntimeError('The storefront request is still pending. Check its status before retrying.')
    emit_event('store-result', ok=True, message='')


def transaction_request(action, job_id):
    if not re.fullmatch(r'[a-f0-9]{32}', job_id):
        raise ValueError('Invalid transaction target')
    path = Path(os.environ['XDG_RUNTIME_DIR']) / 'dukkan-transactions/control.sock'
    with socket.socket(socket.AF_UNIX) as peer:
        peer.settimeout(4)
        peer.connect(str(path))
        peer.sendall((json.dumps({'action': action, 'id': job_id}) + '\n').encode())
        data = bytearray()
        while b'\n' not in data:
            block = peer.recv(65536)
            if not block or len(data) > 2 * 1024 * 1024:
                raise RuntimeError('Incomplete transaction service reply')
            data.extend(block)
        result = json.loads(data.split(b'\n', 1)[0])
        if result.get('type') != 'transactions' or result.get('job', {}).get('id') != job_id:
            raise RuntimeError(result.get('error', 'Transaction target is unavailable'))
        return result['job']


def main():
    lock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        is_store = len(sys.argv) in (5, 6) and sys.argv[1] == 'storefront'
        is_emulator = len(sys.argv) == 2 and sys.argv[1] in ('emulator-menu', 'emulator-menu-capability')
        if not is_store and not is_emulator and (len(sys.argv) != 2 or sys.argv[1] not in ('exit-game', 'reboot', 'shutdown')):
            raise RuntimeError('Expected exit-game, emulator-menu, emulator-menu-capability, reboot or shutdown.')
        runtime = os.environ['XDG_RUNTIME_DIR']
        if sys.argv[1] != 'emulator-menu-capability':
            lock.bind('\0zaman-menu-exit:' + runtime)
        import gi
        gi.require_version('Gio', '2.0')
        from gi.repository import Gio, GLib
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        owner = bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                              'org.freedesktop.DBus', 'GetNameOwner', GLib.Variant('(s)', (SERVICE,)),
                              GLib.VariantType.new('(s)'), Gio.DBusCallFlags.NONE, 2000, None).unpack()[0]

        def call(method, signature, args=None):
            # A service restart invalidates the action; never stop a replacement
            # daemon's game because the well-known bus name was reused.
            input_value = GLib.Variant('(sss)' if len(args) == 3 else '(s)', args) if args is not None else None
            return bus.call_sync(owner, PATH, INTERFACE, method, input_value,
                                 GLib.VariantType.new(signature),
                                 Gio.DBusCallFlags.NO_AUTO_START, 5000, None).unpack()

        if is_store:
            if len(sys.argv) == 6:
                if sys.argv[2:5] != ['launch', 'dukkan', '0']:
                    raise ValueError('Transaction navigation must launch Dukkan')
                transaction_request('get', sys.argv[5])
                storefront_action(call, *sys.argv[2:5], emit_event=lambda *args, **kwargs: None)
                transaction_request('navigate', sys.argv[5])
                emit('store-result', ok=True, message='')
            else:
                storefront_action(call, *sys.argv[2:])
        elif sys.argv[1] == 'emulator-menu-capability':
            emit('emulator-menu-capability', available=bool(call('EmulatorMenuAvailable', '(b)')[0]))
        elif sys.argv[1] == 'emulator-menu':
            # sessiond owns emulator registry, focus validation and injection.
            call('EmulatorMenu', '()')
            emit('emulator-menu-result', ok=True, message='')
        elif sys.argv[1] == 'exit-game':
            exit_game(call)
        else:
            power_action(call, sys.argv[1])
        return 0
    except Exception as error:
        message = str(error)
        if 'the Zaman system menu is not open' in message:
            message = 'Sessiond needs the included ExitGame API update. Build and deploy it, then try again.'
        log('failed: ' + message.replace('\n', ' '))
        kind = ('store-result' if len(sys.argv) > 1 and sys.argv[1] == 'storefront'
                else 'emulator-menu-capability' if len(sys.argv) > 1 and sys.argv[1] == 'emulator-menu-capability'
                else 'emulator-menu-result' if len(sys.argv) > 1 and sys.argv[1] == 'emulator-menu'
                else 'exit-result')
        emit(kind, ok=False, available=False, message=message)
        return 1
    finally:
        lock.close()


if __name__ == '__main__':
    sys.exit(main())
