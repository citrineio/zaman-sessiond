"""Bounded notification state; no D-Bus, Qt or device dependencies."""
import copy
import json
import os
import re
import tempfile
import time
import uuid
from pathlib import Path

MAX_RECORDS = 100
MAX_AGE = 7 * 86400
MAX_BYTES = 256 * 1024
KINDS = {'network', 'bluetooth', 'audio', 'input', 'battery', 'install', 'success', 'warning', 'error', 'system', 'transfer'}


def clean(value, limit):
    return re.sub(r'[\x00-\x08\x0b-\x1f\x7f]', '', str(value or '')).encode('utf-8', errors='replace')[:limit].decode('utf-8', errors='ignore')


class Model:
    def __init__(self, path, clock=time.time):
        self.path, self.clock = Path(path), clock
        self.generation = uuid.uuid4().hex
        self.records, self.live, self.queue, self.visible = [], {}, [], []
        self.preferences = {'dnd': False, 'steamReminder': True, 'steamStartup': True}
        self.viewers = set()
        self.presenters = set()
        self.closed = []
        self.next_protocol = 1
        self.revision = 0
        self.error = ''
        self.event_times = {}
        self.transaction_receipts = {}
        self._defer_save = False
        self.last_prune = self.clock()
        self.load()

    def load(self):
        try:
            if self.path.stat().st_size > MAX_BYTES:
                raise ValueError('history exceeds size limit')
            data = json.loads(self.path.read_text())
            if not isinstance(data, dict) or data.get('version') != 1 or not isinstance(data.get('records'), list):
                raise ValueError('unsupported notification history')
            receipts = data.get('transactionReceipts', {})
            if not isinstance(receipts, dict) or len(receipts) > 64 or any(
                not re.fullmatch('[a-f0-9]{32}', key) or type(value) is not int or value < 0
                for key, value in receipts.items()):
                raise ValueError('invalid transaction receipts')
            self.transaction_receipts = receipts
            preferences = data.get('preferences', {})
            if not isinstance(preferences, dict):
                raise ValueError('invalid preferences')
            # Preserve a user's disabled Steam alerts when adding the new control.
            if 'steamStartup' not in preferences:
                preferences['steamStartup'] = preferences.get('steamReminder', True)
            self.preferences = {k: preferences.get(k, v) is True
                                for k, v in self.preferences.items()}
            seen = set()
            for raw in data['records'][:MAX_RECORDS]:
                if not isinstance(raw, dict) or not isinstance(raw.get('id'), str) or not re.fullmatch('[a-f0-9]{32}', raw['id']) or raw['id'] in seen:
                    continue
                seen.add(raw['id'])
                stamp = float(raw.get('timestamp', 0))
                if not self.clock() - MAX_AGE <= stamp <= self.clock() + 60:
                    continue
                self.records.append(dict(id=clean(raw['id'], 64), timestamp=stamp,
                    category=raw.get('category') if raw.get('category') in KINDS else 'system',
                    source=clean(raw.get('source'), 64), title=clean(raw.get('title'), 160),
                    body=clean(raw.get('body'), 1024), urgency=2 if raw.get('urgency') == 2 else 1,
                    read=raw.get('read') is True, dismissed=raw.get('dismissed') is True,
                    state=clean(raw.get('state'), 40), target=self.target(raw.get('target')),
                    generation='', protocolId=0, live=False, popup='archived'))
            self.prune()
        except FileNotFoundError:
            pass
        except (ValueError, TypeError, OSError, OverflowError) as error:
            self.records = []
            self.error = 'History could not be loaded: ' + str(error)

    @staticmethod
    def target(value):
        # Only a reserved typed route. Never a command, URI or executable.
        if value == 'transfer-pending':
            return value
        return value if isinstance(value, str) and re.fullmatch(r'dukkan-transaction:[a-f0-9]{32}', value) else ''

    def prune(self):
        cutoff = self.clock() - MAX_AGE
        # Live criticals are acknowledged before archive retention applies.
        keep = [r for r in self.records if r['timestamp'] >= cutoff or r['live'] and r['urgency'] == 2]
        protected = {r['id'] for r in keep if r['live'] and r['urgency'] == 2}
        remaining = MAX_RECORDS - len(protected)
        retained = []
        for r in keep:
            if r['id'] in protected:
                retained.append(r)
            elif remaining > 0:
                retained.append(r)
                remaining -= 1
        self.records = retained
        kept = {r['id'] for r in self.records}
        for pid, r in list(self.live.items()):
            if r['id'] not in kept:
                self.close(pid, 4)
        self.queue = [rid for rid in self.queue if rid in kept]
        self.visible = [rid for rid in self.visible if rid in kept]

    def save(self, strict=False):
        if self._defer_save:
            return
        self.prune()
        data = json.dumps({'version': 1, 'preferences': self.preferences, 'transactionReceipts': self.transaction_receipts,
                           'records': [{k: v for k, v in r.items() if not k.startswith('_')} for r in self.records]},
                          ensure_ascii=False, separators=(',', ':')).encode()
        if len(data) > MAX_BYTES:
            self.error = 'History size limit reached'
            if strict:
                raise OSError(self.error)
            return
        temporary = None
        try:
            self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            fd, temporary = tempfile.mkstemp(prefix='.notifications-', dir=self.path.parent)
            with os.fdopen(fd, 'wb') as out:
                out.write(data)
                out.flush()
                os.fsync(out.fileno())
            os.replace(temporary, self.path)
            directory_fd = os.open(self.path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        except OSError as error:
            self.error = 'History could not be saved: ' + str(error)
            if strict:
                raise
        finally:
            if temporary and os.path.exists(temporary):
                os.unlink(temporary)

    def by_id(self, rid):
        return next((r for r in self.records if r['id'] == rid), None)

    def notify(self, source, title, body='', hints=None, timeout=-1, urgency=1, replaces=0, owner=''):
        hints = hints or {}
        urgency = 2 if urgency == 2 else 1
        r = self.live.get(replaces)
        # Protocol IDs may only replace a live notification of this client.
        if r is not None and r.get('_owner') != owner:
            r = None
        if r is None:
            # Refuse new alerts once every slot is an unacknowledged critical.
            if len(self.live) >= MAX_RECORDS and all(x['urgency'] == 2 for x in self.live.values()):
                raise ValueError('acknowledge critical notifications before accepting more')
            pid = self.next_protocol
            self.next_protocol = pid % 0xffffffff + 1
            r = {'id': uuid.uuid4().hex, 'protocolId': pid, 'generation': self.generation,
                 'read': bool(self.viewers), 'dismissed': False, 'live': True, '_owner': owner}
            self.live[pid] = r
            self.records.insert(0, r)
        else:
            # Includes identical replacements: each Notify resets its deadline.
            self.records.remove(r)
            self.records.insert(0, r)
            r.update(read=bool(self.viewers), dismissed=False)
        kind = clean(hints.get('x-zaman-kind', 'system'), 24)
        r.update(timestamp=self.clock(), category=kind if kind in KINDS else 'system',
                 source=clean(source, 64), title=clean(title, 160) or 'Notification',
                 body=clean(body, 1024), urgency=urgency, state=clean(hints.get('x-zaman-state'), 40),
                 target=self.target(hints.get('x-zaman-target')), live=True)
        timeout = max(-1, min(int(timeout), 86400000))
        duration = 0 if urgency == 2 or timeout == 0 else timeout / 1000 if timeout > 0 else (
            5 if r['state'] in ('error', 'disconnected', 'warning') or kind in ('warning', 'error', 'battery') else 3)
        r['_duration'] = duration
        r['_deadline'] = self.clock() + duration if duration else 0
        rid = r['id']
        self.queue = [i for i in self.queue if i != rid]
        if self.preferences['dnd'] and urgency != 2:
            self.visible = [i for i in self.visible if i != rid]
            r['popup'] = 'suppressed'
        elif rid not in self.visible:
            r['popup'] = 'queued'
            self.queue.append(rid)
            if urgency == 2:
                self.queue.remove(rid)
                self.queue.insert(0, rid)
                # Demote a normal card to history; do not expire or evict criticals.
                if len(self.visible) == 2:
                    normal = next((i for i in reversed(self.visible) if self.by_id(i)['urgency'] != 2), None)
                    if normal:
                        self.visible.remove(normal)
                        self.by_id(normal)['popup'] = 'history'
        self.fill()
        self.revision += 1
        self.save()
        return r['protocolId']

    def fill(self):
        while len(self.visible) < 2 and self.queue:
            rid = self.queue.pop(0)
            r = self.by_id(rid)
            if not r or not r['live'] or r['dismissed']:
                continue
            self.visible.append(rid)
            r['popup'] = 'visible'
            r['_deadline'] = self.clock() + r['_duration'] if r['_duration'] else 0
        # Bound pending presentation; overflow stays in history and obeys its timeout.
        while len(self.queue) > 20:
            normal = next((i for i in self.queue if self.by_id(i)['urgency'] != 2), None)
            if normal is None:
                break  # Total retained/live critical budget is still 100.
            self.queue.remove(normal)
            self.by_id(normal)['popup'] = 'history'

    def close(self, pid, reason):
        r = self.live.pop(pid, None)
        if r is None:
            return
        r.update(live=False, popup='closed')
        r['_deadline'] = 0
        rid = r['id']
        self.visible = [i for i in self.visible if i != rid]
        self.queue = [i for i in self.queue if i != rid]
        self.closed.append((pid, reason))
        self.fill()
        self.revision += 1

    def tick(self):
        changed = False
        for pid, r in list(self.live.items()):
            if r['popup'] != 'queued' and r['_deadline'] and r['_deadline'] <= self.clock():
                self.close(pid, 1)
                changed = True
        if self.clock() - self.last_prune >= 60:
            self.last_prune = self.clock()
            before = len(self.records)
            self.prune()
            if len(self.records) != before:
                self.revision += 1
                changed = True
        if changed:
            self.save()
        return changed

    def dismiss(self, rid):
        r = self.by_id(rid)
        if r is None:
            return
        r.update(dismissed=True, read=True)
        self.close(r['protocolId'], 2) if r['live'] and r['generation'] == self.generation else None
        self.revision += 1

    def command(self, msg, peer='cli'):
        action = msg.get('action')
        if action == 'snapshot':
            return self.snapshot()
        if action == 'dismiss':
            self.dismiss(msg.get('id'))
        elif action == 'dismiss-visible':
            for rid in list(self.visible):
                self.dismiss(rid)
        elif action == 'clear':
            # A captured ID set cannot clear a subsequent arrival.
            supplied = msg.get('ids', [])
            if not isinstance(supplied, list) or not all(isinstance(i, str) for i in supplied):
                raise ValueError('clear requires record IDs')
            ids = set(supplied[:MAX_RECORDS])
            for rid in ids:
                self.dismiss(rid)
            self.records = [r for r in self.records if r['id'] not in ids]
        elif action == 'menu':
            if msg.get('active') is True:
                self.presenters.add(peer)
            else:
                self.presenters.discard(peer)
        elif action == 'view':
            if msg.get('active') is True:
                self.viewers.add(peer)
                for r in self.records:
                    r['read'] = True
            else:
                self.viewers.discard(peer)
        elif action == 'preferences':
            for key in self.preferences:
                if isinstance(msg.get(key), bool):
                    self.preferences[key] = msg[key]
            if self.preferences['dnd']:
                for rid in list(self.visible) + list(self.queue):
                    r = self.by_id(rid)
                    if r and r['urgency'] != 2:
                        if rid in self.visible:
                            self.visible.remove(rid)
                        if rid in self.queue:
                            self.queue.remove(rid)
                        r['popup'] = 'suppressed'
                self.fill()
        elif action != 'snapshot':
            raise ValueError('unknown notification command')
        self.revision += 1
        self.save()
        return self.snapshot()

    def event(self, source, key, title, body='', category='system', state='', target=''):
        # Ordered outbox keys use a persisted high-water mark independent of
        # history retention/dismissal. One atomic file commit precedes the ACK.
        durable = re.fullmatch(r'transaction:([a-f0-9]{32}):([1-9][0-9]{0,15})', key) if source == 'Dukkan' else None
        if durable:
            if self.error.startswith('History could not be loaded:'):
                raise RuntimeError('Repair notification history before acknowledging durable events')
            namespace, sequence = durable[1], int(durable[2])
            previous = self.transaction_receipts.get(namespace, 0)
            if sequence <= previous:
                return
            if sequence != previous + 1:
                raise ValueError('transaction delivery is out of order')
            if namespace not in self.transaction_receipts and len(self.transaction_receipts) >= 64:
                raise ValueError('transaction receipt namespace limit reached')
            before = copy.deepcopy(self.__dict__)
            try:
                self._defer_save = True
                self.notify(source, title, body, {'x-zaman-kind': category,
                    'x-zaman-state': state, 'x-zaman-target': target}, owner='producer:' + source)
                self.transaction_receipts[namespace] = sequence
                self._defer_save = False
                self.save(strict=True)
            except BaseException:
                self.__dict__.clear()
                self.__dict__.update(before)
                raise
            return
        if key.startswith('steam:launch:'):
            if not self.preferences['steamStartup']:
                return
        elif key.startswith('steam:') and not self.preferences['steamReminder']:
            return
        now = self.clock()
        # Typed producer keys; repeated callbacks cannot create another event.
        previous = self.event_times.get((source, key), 0)
        if now - previous < (86400 if key.startswith(('transaction:', 'steam:')) else 5):
            return
        self.event_times[(source, key)] = now
        if len(self.event_times) > 512:
            self.event_times = dict(sorted(self.event_times.items(), key=lambda item: item[1])[-256:])
        return self.notify(source, title, body, {'x-zaman-kind': category, 'x-zaman-state': state,
            'x-zaman-target': target}, owner='producer:' + source)

    def snapshot(self):
        def public(r):
            return {k: v for k, v in r.items() if not k.startswith('_')}
        return {'type': 'notifications', 'generation': self.generation, 'revision': self.revision,
                'records': [public(r) for r in self.records],
                'visible': [public(r) for i in self.visible if (r := self.by_id(i))],
                'unread': sum(not r['read'] for r in self.records),
                'menuOpen': bool(self.presenters),
                'preferences': self.preferences.copy(), 'error': self.error}
