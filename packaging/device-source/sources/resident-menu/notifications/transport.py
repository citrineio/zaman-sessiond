"""User-owned, bounded JSON-lines IPC shared by QML and command line tools."""
import json
import os
import socket
from pathlib import Path

LIMIT = 256 * 1024


def socket_path():
    runtime = os.environ.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid()))
    return str(Path(runtime) / 'zaman-notifications' / 'control.sock')


def connect():
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(2)
    sock.connect(socket_path())
    return sock


def request(message):
    with connect() as sock:
        sock.sendall((json.dumps(message) + '\n').encode())
        with sock.makefile('rb') as file:
            line = file.readline(LIMIT + 1)
        if len(line) > LIMIT or not line.endswith(b'\n'):
            raise ValueError('invalid notification service reply')
        reply = json.loads(line)
        if reply.get('type') == 'error':
            raise ValueError(reply.get('error', 'notification request failed'))
        return reply
