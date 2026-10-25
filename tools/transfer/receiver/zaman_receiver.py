#!/usr/bin/env python3
"""LocalSend protocol v2.2 receiver and same-user controller inbox API.

No Flutter runtime, pip packages, credentials in logs, or network-shell routes.
Reception is disabled on every service start until enabled from the menu.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import ipaddress
import os
from pathlib import Path
import secrets
import signal
import socket
import socketserver
import ssl
import struct
import subprocess
import threading
import time
from urllib.parse import parse_qs, urlsplit

from zaman_inbox import Inbox, Registry, TERMINAL

GROUP = "224.0.0.167"
DEFAULT_PORT = 53317
SERVICE = "com.kawnelectro.Zaman.Session1"
OBJECT = "/com/kawnelectro/Zaman/Session1"
CHUNK = 1024 * 1024


def local_interfaces():
    """Linux IPv4 LAN interfaces, without a route, subprocess or network probe.

    IFF_BROADCAST excludes point-to-point VPN links. Retain a LAN address even
    when multicast is unavailable so manual sending remains usable.
    """
    interfaces = []
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        for index, name in socket.if_nameindex():
            if name.startswith(("docker", "veth", "virbr", "br-")):
                continue
            request = struct.pack("256s", name.encode()[:15])
            try:
                flags = struct.unpack_from("H", fcntl.ioctl(probe, 0x8913, request), 16)[0]
                if not flags & 0x1 or flags & 0x8 or not flags & 0x2:
                    continue
                address = socket.inet_ntoa(fcntl.ioctl(probe, 0x8915, request)[20:24])
                ip = ipaddress.IPv4Address(address)
                if ip.is_loopback or ip.is_unspecified or ip.is_multicast:
                    continue
                interfaces.append(dict(interface=name, index=index, address=address,
                                       multicast=bool(flags & 0x1000)))
            except OSError:
                # Interface removal/address changes can race enumeration.
                continue
    return sorted(interfaces, key=lambda entry: (entry["index"], entry["address"]))


class ProtocolError(Exception):
    def __init__(self, code, message):
        self.code = code
        super().__init__(message)


def bus_call(method):
    output = subprocess.check_output([
        "busctl", "--user", "--json=short", "--timeout=2", "call",
        SERVICE, OBJECT, SERVICE, method], timeout=3, stderr=subprocess.DEVNULL)
    return json.loads(output)["data"]


def safe_to_publish():
    """Fail closed. Reception never needs this check or an input/launch lease.

    This is a deferral check, not a global lifecycle lock. Publication only adds
    immutable, uniquely named files; it never changes any running game's files.
    """
    try:
        status = bus_call("Status")
        foreground = bus_call("ForegroundStatus")
        installed = bus_call("InstalledGameStatus")
        store = bus_call("StorefrontStatus")
        return (len(status) == 6 and status[0] == "Idle"
                and len(foreground) == 3 and foreground[0] in {"library", "menu"}
                and foreground[1] in {"", "library"}
                and len(installed) == 6 and installed[3] not in {"Starting","Running","Stopping","CleanupFailed"}
                and len(store) == 6 and not store[4]
                and store[2] not in {"Starting","Stopping","CleanupFailed"})
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        return False


def safe_to_import():
    """Artwork may run over the menu; add games only after it closes."""
    if not safe_to_publish():
        return False
    try:
        foreground = bus_call("ForegroundStatus")
        return len(foreground) == 3 and foreground[0] == "library"
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        return False


def notify(row):
    runtime = Path(os.environ.get("XDG_RUNTIME_DIR", "/run/user/" + str(os.getuid())))
    phase = row["phase"]
    labels = {"complete":"Added to your library", "ready":"Game ready to add", "needs-destination":"Choose a destination folder",
              "needs-files":"Import needs attention", "failed":"Transfer failed",
              "scrape-input":"Artwork update needs your choice", "scrape-complete":"Artwork & info updated", "scrape-failed":"Artwork update needs attention"}
    if phase not in labels:
        return True
    scraping = phase.startswith("scrape-")
    warning=row.get("artwork_warning","") if phase in {"ready","complete"} else ""
    title = "Ready to add without artwork" if phase == "ready" and warning else labels[phase]
    body = row["name"][:160]
    if row.get("folderOnly") and phase in {"ready", "complete"}:
        title = "Ready to transfer to folder" if phase == "ready" else "Transferred to folder"
        body += "\nClose the menu to finish saving the file." if phase == "ready" else "\nSaved in " + row.get("destination", str(Path(row["target"]).parent))
    elif phase == "ready":
        body += "\nClose the menu to update your library."
        if warning: body += " You can find artwork later in Artwork & Game Info."
    elif phase == "complete":
        body += "\nYour game is ready to play."
        if warning: body += "\n" + warning
    message = dict(action="event", source="Zaman Artwork & Game Info" if scraping else "Zaman Transfer",
                   key="transfer:" + row["id"] + ":" + phase,
                   title=title, body=body, category="transfer",
                   state=phase, target="scrape-pending:"+row["id"] if scraping else "transfer-pending")
    try:
        with socket.socket(socket.AF_UNIX) as peer:
            peer.settimeout(2)
            peer.connect(str(runtime / "zaman-notifications/control.sock"))
            peer.sendall((json.dumps(message) + "\n").encode())
            with peer.makefile("rb") as f:
                line = f.readline(256 * 1024 + 1)
            return len(line) <= 256 * 1024 and json.loads(line).get("type") != "error"
    except (OSError, ValueError):
        return False


class Receiver:
    def __init__(self, inbox, alias="Zaman", port=DEFAULT_PORT, fingerprint="", theme=None):
        self.inbox, self.alias, self.port = inbox, alias, port
        self.fingerprint, self.theme = fingerprint, Path(theme) if theme else None
        self.lock = threading.RLock()
        self.until = 0.0
        self.pin = ""
        self.sessions = {}
        self.failures = {}
        self.stopping = threading.Event()
        self.changed = threading.Event()
        self.discovery_error = ""
        self.addresses = []
        self.discovery_ready = False
        self.repair_metadata = True
        self.scraper = None
        self.target_signature = None
        self.target_error = ""
        self.unregistered_folders = []
        self.target_lock = threading.RLock()

    def info(self, announce=None):
        data = dict(alias=self.alias, version="2.2", deviceModel="Zaman",
                    deviceType="headless", fingerprint=self.fingerprint,
                    port=self.port, protocol="https", download=False)
        if announce is not None:
            data["announce"] = announce
        return data

    def enabled(self):
        return self.until > time.monotonic()

    def prepare(self, payload, ip, pin):
        with self.lock:
            if not self.enabled():
                raise ProtocolError(403, "Enable receiving from Zaman Transfer first")
            now = time.monotonic()
            self.failures = {k:v for k,v in self.failures.items() if v[1] > now - 60}
            count = self.failures.get(ip, (0, now))[0]
            if count >= 5:
                raise ProtocolError(429, "Try again later")
            if not secrets.compare_digest(str(pin), self.pin):
                self.failures[ip] = (count + 1, now)
                raise ProtocolError(401, "PIN required")
            self.expire()
            if any(not s["cancelled"].is_set() and any(self.inbox.get(i)["phase"] in {"offered","receiving"} for i in s["items"].values()) for s in self.sessions.values()):
                raise ProtocolError(409, "Another sender is transferring")
            info = payload.get("info", {})
            if not isinstance(info, dict):
                raise ProtocolError(400, "Invalid sender info")
            try:
                batch, items = self.inbox.offer(payload.get("files"), info.get("alias", "LocalSend"))
            except (ValueError, TypeError) as e:
                raise ProtocolError(400, str(e)) from e
            sid = secrets.token_urlsafe(32)
            tokens = {fid: secrets.token_urlsafe(32) for fid in items}
            self.sessions[sid] = dict(items=items, tokens=tokens, ip=ip, batch=batch,
                                     cancelled=threading.Event(), touched=now, inflight=set(), connections={})
            self.changed.set()
            return dict(sessionId=sid, files=tokens)

    def claim_upload(self, query, ip):
        sid, fid, token = (query.get(k, [""])[0] for k in ("sessionId","fileId","token"))
        with self.lock:
            s = self.sessions.get(sid)
            if s is None or s["ip"] != ip or s["cancelled"].is_set() or fid not in s["tokens"] or not secrets.compare_digest(s["tokens"][fid], token):
                raise ProtocolError(403, "Invalid transfer credentials")
            if fid in s["inflight"] or self.inbox.get(s["items"][fid])["phase"] != "offered":
                raise ProtocolError(409, "File already started")
            s["inflight"].add(fid)
            s["touched"] = time.monotonic()
            return s, fid, s["items"][fid]

    def cancel_session(self, sid, ip=None):
        with self.lock:
            s = self.sessions.get(sid)
            if not s or (ip is not None and s["ip"] != ip):
                raise ProtocolError(403, "Unknown sender session")
            s["cancelled"].set()
            for connection in s["connections"].values():
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
            for ident in s["items"].values():
                if self.inbox.get(ident)["phase"] == "offered":
                    self.inbox.update(ident, phase="cancelled", error="Sender session cancelled")
            self.changed.set()

    def expire(self):
        with self.lock:
            now = time.monotonic()
            for sid, s in list(self.sessions.items()):
                # In-flight files have their own stall timeout. Unsent files
                # cannot leave a batch waiting forever after sender loss.
                if not s["inflight"] and now - s["touched"] > 120:
                    for ident in s["items"].values():
                        if self.inbox.get(ident)["phase"] == "offered":
                            self.inbox.update(ident, phase="failed", error="Sender stopped before sending this file")
                    del self.sessions[sid]

    def control(self, msg):
        action = msg.get("action")
        with self.lock:
            if action == "enable":
                self.refresh_targets(force=True)
                self.until = time.monotonic() + 900
                self.pin = f"{secrets.randbelow(1000000):06d}"
                self.failures.clear()
                self.changed.set()
            elif action == "disable":
                self.until = 0
                self.pin = ""
            elif action == "choose":
                self.inbox.choose(msg.get("id"), msg.get("system"))
            elif action == "cancel":
                row = self.inbox.get(msg.get("id"))
                if row["phase"] == "scraping" and self.scraper:
                    self.scraper.cancel(row["scrape_job"])
                self.inbox.cancel(msg.get("id"))
            elif action == "retry":
                self.inbox.retry(msg.get("id"))
            elif action == "dismiss-history":
                ident = msg.get("id")
                if not isinstance(ident, str) or not ident:
                    raise ValueError("Choose a finished transfer record")
                self.inbox.dismiss_history(ident)
            elif action == "clear-history":
                self.inbox.dismiss_history()
            elif action == "import-scrape":
                if msg.get("decision") == "skip":
                    self.inbox.skip_scrape(msg.get("id"))
                elif msg.get("decision") == "scrape" and self.scraper:
                    self.scraper.start(None,msg.get("mode","cached"),msg.get("review",False),msg.get("id"))
                else:
                    raise ValueError("Choose scraping or import without scraping")
            elif action.startswith("scrape-"):
                if not self.scraper:raise ValueError("Scraping is unavailable")
                if action == "scrape-scan":self.scraper.scan()
                elif action == "scrape-start":self.scraper.start(msg.get("folder"),msg.get("mode","cached"),msg.get("review",False))
                elif action == "scrape-answer":self.scraper.answer(msg.get("id"),msg.get("prompt"),msg.get("answer"))
                elif action == "scrape-cancel":self.scraper.cancel(msg.get("id"))
                elif action == "scrape-retry":self.scraper.retry(msg.get("id"))
                elif action == "scrape-dismiss":self.scraper.dismiss(msg.get("id"))
                elif action == "scrape-clear":self.scraper.dismiss()
                else:raise ValueError("Unknown scraping action")
            elif action == "cancel-session":
                batch = msg.get("batch")
                found = [sid for sid,s in self.sessions.items() if s["batch"] == batch]
                if not found:
                    raise ValueError("Sender session already ended")
                for sid in found:
                    self.cancel_session(sid)
            elif action != "snapshot":
                raise ValueError("Unknown Transfer command")
            return self.snapshot()

    def snapshot(self):
        jobs = self.inbox.snapshot()
        return dict(type="transfer", enabled=self.enabled(),
                    seconds=max(0, int(self.until-time.monotonic())),
                    pin=self.pin if self.enabled() else "", alias=self.alias,
                    port=self.port, discoveryError=self.discovery_error,
                    addresses=list(self.addresses), discoveryReady=self.discovery_ready,
                    targetRefreshVersion=2, folderTargetsVersion=1, targetError=self.target_error,
                    unregisteredFolders=[],
                    folderTargets=[dict(id=sid, name=self.inbox.registry.target_names()[sid])
                                   for sid in self.inbox.registry.folder_targets],
                    registryRoots=[str(root) for root in self.inbox.registry.roots],
                    jobs=jobs, historyCount=self.inbox.history_count(),
                    pending=sum(j["phase"] not in TERMINAL for j in jobs),
                    scraper=self.scraper.snapshot() if self.scraper else dict(available=False,error="Scraping unavailable",folders=[],jobs=[]),
                    systems=[dict(id=sid,name=name) for sid,name in sorted(self.inbox.registry.target_names().items())])

    def signal_refresh(self):
        rows = [r for r in self.inbox.rows() if r["refresh"]]
        scrape_refresh = self.scraper and self.scraper.needs_refresh()
        if not rows and not scrape_refresh:
            return
        if not self.theme or not (self.theme / "theme.qml").is_file():
            return
        directory = self.theme / "rom-library-refresh"
        if directory.is_symlink() or self.theme.is_symlink():
            raise ValueError("Unsafe ROM refresh directory")
        directory.mkdir(exist_ok=True)
        path = directory / (secrets.token_hex(16) + ".signal")
        path.touch(exist_ok=False)
        for old in directory.glob("*.signal"):
            if old != path:
                old.unlink()
        for row in rows:
            self.inbox.update(row["id"], refresh=0)
        if scrape_refresh:self.scraper.refreshed()

    def refresh_targets(self, force=False):
        with self.target_lock:
            self._refresh_targets(force)

    def _refresh_targets(self, force):
        """Called by the existing worker once per second, outside snapshot I/O."""
        previous = self.inbox.registry
        try:
            signature = Registry.signature(previous.roots, previous.rom_root)
            if not force and signature == self.target_signature:
                return
            registry = Registry(previous.roots, previous.rom_root)
            # An editor/importer may change files during loading. Retry next tick
            # rather than advertise a registry assembled across two versions.
            if Registry.signature(previous.roots, previous.rom_root) != signature:
                return
            first = self.target_signature is None
            self.inbox.replace_registry(registry, revalidate=first or previous.systems != registry.systems
                                        or previous.folder_targets != registry.folder_targets)
            if self.scraper:
                self.scraper.scan()
            self.target_signature = signature
            self.target_error = ""
        except (OSError, ValueError, KeyError, TypeError) as error:
            # Folder reception does not depend on a well-formed emulator registry.
            try:
                with self.inbox.lock:
                    old_folders = previous.folder_targets
                    previous.scan_folders()
                    self.inbox.replace_registry(previous, revalidate=old_folders != previous.folder_targets)
                if self.scraper:
                    self.scraper.scan()
            except OSError:
                pass
            message = "System definitions could not refresh. Existing ROM folders remain available."
            if not self.target_error:
                print("Transfer target refresh:", str(error)[:200], flush=True)
            self.target_error = message

    def work_once(self, safe=safe_to_import):
        self.refresh_targets()
        self.expire()
        rows = self.inbox.rows()
        for batch in {r["batch"] for r in rows if r["phase"] == "validating"}:
            self.inbox.validate_batch(batch)
        rows = self.inbox.rows()
        for row in rows:
            if row["phase"]=="needs-scrape":
                if self.scraper:self.scraper.auto_import(row["id"])
                else:self.inbox.update(row["id"],phase="ready",scrape_choice="skip",error="",artwork_warning="Artwork lookup is unavailable. You can add artwork later from Artwork & Game Info.")
        rows = self.inbox.rows()
        has_work = self.repair_metadata or any(r["phase"] in {"ready","importing"} or r["refresh"] for r in rows) or (self.scraper and self.scraper.needs_refresh())
        can_publish = has_work and safe()
        for row in rows:
            if row["phase"] in {"ready","importing"} and can_publish:
                try:
                    self.inbox.publish(row["id"])
                except Exception as error:
                    self.inbox.update(row["id"], phase="needs-files", error=str(error))
        if can_publish:
            # Rebuilding is idempotent and repairs the metadata/DB crash boundary.
            self.repair_metadata = True
            self.inbox.write_metadata()
            self.repair_metadata = False
            self.signal_refresh()
        # Completion is announced only after metadata and the refresh signal
        # have been written successfully. Failed writes retry on the next tick.
        for current in self.inbox.rows():
            if current["phase"] == "complete" and (self.repair_metadata or current["refresh"] and self.theme is not None):
                continue
            current["folderOnly"] = bool(current["system"]) and not self.inbox.registry.can_launch(current["system"], current["name"])
            current["destination"] = str(self.inbox.registry.rom_root / current["system"]) if current["system"] else ""
            if not (current["hidden"] and current["phase"] in TERMINAL) and current["phase"] != current["notified"] and notify(current):
                self.inbox.update(current["id"], notified=current["phase"])

    def worker(self):
        while not self.stopping.wait(1):
            try:
                self.work_once()
            except Exception as error:
                print("Transfer worker:", type(error).__name__, str(error)[:200], flush=True)

    def discovery_session(self, interfaces):
        """Recreate memberships when interfaces, addresses or indices change."""
        multicast = [entry for entry in interfaces if entry["multicast"]]
        if not multicast:
            raise OSError("No multicast-capable LAN interface")
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP) as udp:
            udp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            udp.bind(("", self.port))
            joined = []
            failures = []
            for entry in multicast:
                membership = (socket.inet_aton(GROUP) + socket.inet_aton(entry["address"])
                              + struct.pack("=I", entry["index"]))
                try:
                    udp.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, membership)
                    joined.append(entry)
                except OSError as error:
                    failures.append(error)
            if not joined:
                raise failures[0]
            udp.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
            udp.settimeout(1)
            self.discovery_ready = True
            self.discovery_error = ""
            last = 0
            checked = time.monotonic()
            retry_memberships_at = checked + 15

            def announce(value):
                payload = json.dumps(self.info(value)).encode()
                sent = 0
                for entry in joined:
                    try:
                        udp.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF,
                                       socket.inet_aton(entry["address"]))
                        udp.sendto(payload, (GROUP, self.port))
                        sent += 1
                    except OSError:
                        continue
                if not sent:
                    raise OSError("Discovery could not send on any LAN interface")

            while not self.stopping.is_set():
                now = time.monotonic()
                if now - checked >= 2:
                    current = local_interfaces()
                    self.addresses = current
                    if current != interfaces:
                        return
                    checked = now
                    # Retry a failed membership without disrupting good interfaces.
                    if failures and now >= retry_memberships_at:
                        return
                if self.enabled() and (self.changed.is_set() or now - last >= 15):
                    announce(True)
                    last = now
                    self.changed.clear()
                try:
                    data, address = udp.recvfrom(16385)
                    if len(data) > 16384 or not self.enabled():
                        continue
                    message = json.loads(data)
                    if message.get("announce") is True and message.get("fingerprint") != self.fingerprint:
                        announce(False)
                except socket.timeout:
                    continue
                except (ValueError, AttributeError):
                    continue

    def discovery(self):
        delay = 1
        while not self.stopping.is_set():
            self.discovery_ready = False
            try:
                self.addresses = local_interfaces()
                if not self.addresses:
                    self.discovery_error = "No LAN address yet. Connect to Wi-Fi or Ethernet; discovery retries automatically."
                    if self.stopping.wait(2):
                        break
                    continue
                self.discovery_session(self.addresses)
                delay = 1
            except OSError as error:
                self.discovery_ready = False
                self.discovery_error = "Automatic discovery unavailable; retrying. Use a LAN address below for manual sending."
                print("Transfer discovery retry:", str(error)[:200], flush=True)
                if self.stopping.wait(delay):
                    break
                delay = min(delay * 2, 8)


class HttpServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    def __init__(self, address, receiver, context):
        self.receiver, self.context = receiver, context
        self.slots = threading.BoundedSemaphore(12)
        super().__init__(address, Handler)

    def process_request(self, request, address):
        if not self.slots.acquire(blocking=False):
            request.close()
            return
        try:
            super().process_request(request, address)
        except Exception:
            self.slots.release()
            raise

    def process_request_thread(self, request, address):
        try:
            request.settimeout(60)
            try:
                tls = self.context.wrap_socket(request, server_side=True)
            except (OSError, ssl.SSLError):
                request.close()
                return
            super().process_request_thread(tls, address)
        finally:
            self.slots.release()

    def handle_error(self, request, address):
        # HTTP query strings contain per-file credentials; never log them.
        pass


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def setup(self):
        self.request.settimeout(60)
        super().setup()

    def log_message(self, *_):
        pass

    def reply(self, code, body=None):
        raw = b"" if body is None else json.dumps(body).encode()
        self.close_connection = True
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(raw)

    def body(self, maximum=128 * 1024):
        if self.headers.get("Transfer-Encoding"):
            raise ProtocolError(400, "JSON metadata requires Content-Length")
        lengths = self.headers.get_all("Content-Length", [])
        if len(lengths) != 1 or not lengths[0].isdigit():
            raise ProtocolError(400, "Invalid content length")
        count = int(lengths[0])
        if not 0 < count <= maximum:
            raise ProtocolError(400, "Metadata too large or empty")
        data = self.rfile.read(count)
        obj = json.loads(data)
        if not isinstance(obj, dict):
            raise ProtocolError(400, "Expected JSON object")
        return obj

    def chunks(self, size):
        transfer = self.headers.get("Transfer-Encoding", "").lower()
        lengths = self.headers.get_all("Content-Length", [])
        if transfer:
            if transfer != "chunked" or lengths:
                raise ProtocolError(400, "Unsupported upload framing")
            total = 0
            while True:
                line = self.rfile.readline(256)
                if not line.endswith(b"\r\n"):
                    raise ProtocolError(400, "Invalid chunk header")
                try:
                    n = int(line.split(b";",1)[0], 16)
                except ValueError as e:
                    raise ProtocolError(400, "Invalid chunk length") from e
                if n < 0 or total + n > size:
                    raise ProtocolError(400, "Chunk exceeds advertised size")
                if n == 0:
                    # Bounded trailer support, not a second HTTP request.
                    for _ in range(64):
                        trailer = self.rfile.readline(4096)
                        if trailer == b"\r\n":
                            return
                        if not trailer.endswith(b"\r\n"):
                            break
                    raise ProtocolError(400, "Invalid chunk trailer")
                remaining = n
                while remaining:
                    b = self.rfile.read(min(CHUNK, remaining))
                    if not b:
                        raise ProtocolError(400, "Short chunk")
                    remaining -= len(b)
                    total += len(b)
                    yield b
                if self.rfile.read(2) != b"\r\n":
                    raise ProtocolError(400, "Invalid chunk terminator")
        else:
            if len(lengths) != 1 or not lengths[0].isdigit() or int(lengths[0]) != size:
                raise ProtocolError(400, "Upload size differs from advertised size")
            remaining = size
            while remaining:
                b = self.rfile.read(min(CHUNK, remaining))
                if not b:
                    raise ProtocolError(400, "Upload ended early")
                remaining -= len(b)
                yield b

    def do_GET(self):
        if urlsplit(self.path).path == "/api/localsend/v2/info":
            self.reply(200, self.server.receiver.info())
        else:
            self.reply(404)

    def do_POST(self):
        receiver = self.server.receiver
        route = urlsplit(self.path)
        query = parse_qs(route.query)
        try:
            if route.path == "/api/localsend/v2/register":
                self.body()
                self.reply(200, receiver.info())
            elif route.path == "/api/localsend/v2/prepare-upload":
                self.reply(200, receiver.prepare(self.body(), self.client_address[0], query.get("pin",[""])[0]))
            elif route.path == "/api/localsend/v2/upload":
                session, fid, ident = receiver.claim_upload(query, self.client_address[0])
                with receiver.lock:
                    session["connections"][fid] = self.connection
                try:
                    receiver.inbox.receive(ident, self.chunks(receiver.inbox.get(ident)["size"]),
                                           lambda:session["cancelled"].is_set() or receiver.stopping.is_set())
                finally:
                    with receiver.lock:
                        session["inflight"].discard(fid)
                        session["connections"].pop(fid, None)
                        session["touched"] = time.monotonic()
                self.reply(200)
            elif route.path == "/api/localsend/v2/cancel":
                receiver.cancel_session(query.get("sessionId",[""])[0], self.client_address[0])
                self.reply(200)
            else:
                self.reply(404)
        except ProtocolError as error:
            self.reply(error.code, {"message": str(error)})
        except (ValueError, KeyError, TypeError, OSError) as error:
            self.reply(422 if "checksum" in str(error).lower() else 400,
                       {"message":str(error)[:200]})


class ControlHandler(socketserver.StreamRequestHandler):
    def handle(self):
        uid = struct.unpack("3i", self.request.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[1]
        if uid != os.getuid():
            return
        self.request.settimeout(3)
        try:
            line = self.rfile.readline(8193)
            if len(line) > 8192 or not line.endswith(b"\n"):
                raise ValueError("Invalid control request")
            msg = json.loads(line)
            if not isinstance(msg, dict):
                raise ValueError("Expected a control object")
            result = self.server.receiver.control(msg)
        except (ValueError, TypeError, OSError) as e:
            result = dict(type="error", error=str(e))
        self.wfile.write((json.dumps(result) + "\n").encode())


class ControlServer(socketserver.ThreadingUnixStreamServer):
    daemon_threads = True


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--state", type=Path, required=True)
    p.add_argument("--rom-root", type=Path, default=Path("/srv/zaman/roms"))
    p.add_argument("--registry-roots", default="/usr/share/zaman:/etc/zaman")
    p.add_argument("--config-root", type=Path, default=Path(os.environ.get("XDG_CONFIG_HOME",str(Path.home()/".config"))))
    p.add_argument("--port", type=int, default=DEFAULT_PORT)
    args = p.parse_args()
    os.umask(0o077)
    args.state.mkdir(mode=0o700, parents=True, exist_ok=True)
    lock = (args.state / "receiver.lock").open("a")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    cert, key = args.state / "receiver.crt", args.state / "receiver.key"
    if not cert.is_file() or not key.is_file():
        raise RuntimeError("Receiver certificate is missing; run deploy.py prepare first")
    fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert(cert.read_text())).hexdigest()
    registry = Registry(args.registry_roots.split(os.pathsep), args.rom_root)
    root = args.config_root / "pegasus-frontend"
    inbox = Inbox(args.state, registry, root / "metafiles", require_scrape=True)
    receiver = Receiver(inbox, port=args.port, fingerprint=fingerprint, theme=root/"themes/zaman")
    from zaman_scraper import Scraper
    receiver.scraper = Scraper(inbox,safe_to_publish,notify)
    receiver.scraper.begin()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(cert, key)
    http = HttpServer(("0.0.0.0",args.port), receiver, context)
    runtime = Path(os.environ.get("XDG_RUNTIME_DIR","/run/user/"+str(os.getuid()))) / "zaman-transfer"
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    if runtime.is_symlink() or runtime.stat().st_uid != os.getuid():
        raise RuntimeError("Unsafe runtime directory")
    os.chmod(runtime,0o700)
    control_path = runtime / "control.sock"
    control_path.unlink(missing_ok=True)
    control = ControlServer(str(control_path), ControlHandler)
    control.receiver = receiver
    os.chmod(control_path,0o600)
    threads = [threading.Thread(target=fn, daemon=True) for fn in
               (http.serve_forever, control.serve_forever, receiver.discovery, receiver.worker)]
    for thread in threads:
        thread.start()
    def stop(*_):
        receiver.stopping.set()
    signal.signal(signal.SIGTERM,stop)
    signal.signal(signal.SIGINT,stop)
    print("Zaman Transfer receiver ready; receiving is disabled",flush=True)
    receiver.stopping.wait()
    receiver.scraper.close()
    http.shutdown()
    control.shutdown()
    http.server_close()
    control.server_close()
    control_path.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
