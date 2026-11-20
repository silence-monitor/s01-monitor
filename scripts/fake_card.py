"""A stand-in for the unit's SD card, for web_bridge.py.

It answers the same endpoints as the firmware (firmware/main/sd_card.cpp) with
the same text, so the browser app's SD card screen can be tried with no
hardware: /info, /time and /sd/status, /sd/list, /sd/file (GET and PUT),
/sd/delete, /sd/mkdir, /sd/format, /sd/eject, /sd/mount, /sd/record, /sd/auto, /sd/mark.

The "card" is a folder on this computer. Recording writes the frames the bridge
is playing into /recordings in the app's own capture format, as the unit does.

Standard library only. Not the firmware: it checks paths the same way, but it
does not model every timing.
"""
import asyncio
import os
import re
import shutil
import time
import urllib.parse

CAPACITY = 15_931_539_456
TOTAL = 15_900_000_000
NAME_FORBIDDEN = set('\\/:*?"<>|')
CHUNK = 4096


def name_ok(name):
    if not name or len(name) > 100 or name[0] == " " or name[-1] in " .":
        return False
    return all(0x20 <= ord(c) <= 0x7E and c not in NAME_FORBIDDEN for c in name)


def path_ok(path):
    if not path or path[0] != "/" or len(path) > 200:
        return False
    if path == "/":
        return True
    return all(name_ok(part) for part in path[1:].split("/"))


def date_text(timestamp):
    if timestamp < 1577836800:  # before 2020: no clock was set
        return ""
    return time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(timestamp))


class FakeCard:
    def __init__(self, root, state="ready"):
        self.root = os.path.abspath(root)
        os.makedirs(self.root, exist_ok=True)
        self.initial = state
        self.state = state
        self.message = ""
        self.busy = "none"
        self.done = 0
        self.total = 0
        self.want_record = False
        self.hold = False
        self.auto = False
        self.clock = False
        self.offset = 0
        self.rec_file = ""
        self.rec_frames = 0
        self.rec_bytes = 0
        self.rec_handle = None
        self.sequence = 0
        self.users = 0

    # -- helpers ---------------------------------------------------------------

    def full(self, path):
        return os.path.normpath(os.path.join(self.root, path.lstrip("/")))

    def used(self):
        total = 0
        for folder, _dirs, files in os.walk(self.root):
            for name in files:
                total += os.path.getsize(os.path.join(folder, name))
        return total

    def local_ms(self):
        return int(time.time() * 1000) + (self.offset if self.clock else 0)

    # -- the recorder ----------------------------------------------------------

    def log(self, text):
        """Called with each slcan text the bridge broadcasts."""
        if not (self.want_record and self.state == "ready"):
            return
        lines = []
        for frame in text.replace("\r", "\n").split("\n"):
            m = re.fullmatch(r"t([0-9A-Fa-f]{3})([0-8])([0-9A-Fa-f]*)", frame.strip())
            if m:
                lines.append(m)
        if not lines:
            return
        if self.rec_handle is None:
            os.makedirs(self.full("/recordings"), exist_ok=True)
            if self.clock:
                name = "rec-" + time.strftime("%Y%m%d-%H%M%S", time.gmtime(self.local_ms() / 1000)) + ".log"
            else:
                self.sequence += 1
                name = f"rec-{self.sequence:04d}.log"
            self.rec_file = "/recordings/" + name
            self.rec_handle = open(self.full(self.rec_file), "a")
            self.rec_handle.write("# recording from the unit's SD card, 250 kbit/s\n")
            self.rec_frames = 0
            self.rec_bytes = 0
            print(f"[card] recording to {self.rec_file}", flush=True)
        ms = self.local_ms()
        stamp = time.strftime("%H:%M:%S", time.gmtime(ms / 1000)) + f".{ms % 1000:03d}"
        for m in lines:
            data = " ".join(m.group(3)[i:i + 2].upper() for i in range(0, len(m.group(3)), 2))
            line = f"time:{stamp} RX: STDID:{int(m.group(1), 16):05X} data:{data}\n"
            self.rec_handle.write(line)
            self.rec_frames += 1
            self.rec_bytes += len(line)
        self.rec_handle.flush()

    def stop_recording(self):
        if self.rec_handle is not None:
            self.rec_handle.close()
            self.rec_handle = None

    @property
    def recording(self):
        return self.rec_handle is not None

    # -- the endpoints -----------------------------------------------------------

    async def serve(self, reader, writer, method, target, headers, reply):
        url = urllib.parse.urlsplit(target)
        route = url.path
        query = {k: v[0] for k, v in urllib.parse.parse_qs(url.query, keep_blank_values=True).items()}
        origin = headers.get("origin")
        if route != "/info" and origin and origin.split("://", 1)[-1] != headers.get("host", ""):
            reply(writer, "403 Forbidden", b"not from this device's own page")
            return
        if route == "/info":
            reply(writer, "200 OK", b"version bridge\nhardware web_bridge.py fake card\nsd 1\n")
        elif route == "/time" and method == "POST":
            try:
                seconds = int(query.get("local", ""))
                assert 1_700_000_000 <= seconds <= 4_102_444_800
            except (ValueError, AssertionError):
                reply(writer, "400 Bad Request", b"not a usable time")
                return
            offset = seconds * 1000 - int(time.time() * 1000)
            if not self.clock or abs(offset - self.offset) > 2000:
                self.stop_recording()  # the times would jump: a new file is started
            self.offset, self.clock = offset, True
            reply(writer, "204 No Content")
        elif route == "/sd/status":
            reply(writer, "200 OK", self.status_text().encode())
        elif route == "/sd/list":
            await self.list(writer, query.get("path", ""), reply)
        elif route == "/sd/file" and method == "GET":
            await self.download(writer, query.get("path", ""), reply)
        elif route == "/sd/file" and method == "PUT":
            await self.upload(reader, writer, query.get("path", ""), headers, reply)
        elif route == "/sd/delete" and method == "POST":
            await self.delete(writer, query.get("path", ""), reply)
        elif route == "/sd/mkdir" and method == "POST":
            self.mkdir(writer, query.get("path", ""), reply)
        elif route == "/sd/format" and method == "POST":
            await self.format(writer, reply)
        elif route == "/sd/eject" and method == "POST":
            self.stop_recording()
            self.want_record, self.hold = False, True
            self.state, self.message = "ejected", "Ejected. It is safe to take the card out."
            reply(writer, "204 No Content")
        elif route == "/sd/mount" and method == "POST":
            self.state = self.initial if self.initial != "ejected" else "ready"
            self.message = ""
            self.hold = False
            reply(writer, "204 No Content")
        elif route == "/sd/record" and method == "POST":
            if query.get("on") == "1":
                if self.state != "ready":
                    reply(writer, "409 Conflict", b"there is no card ready to record to")
                    return
                self.want_record, self.hold = True, False
            else:
                self.want_record, self.hold = False, True
                self.stop_recording()
            reply(writer, "204 No Content")
        elif route == "/sd/mark" and method == "POST":
            text = query.get("text")
            if text is None or len(text.encode()) > 63 or any(ord(c) < 0x20 for c in text):
                reply(writer, "400 Bad Request", b"no text, or too long (at most 63 characters)")
            elif self.rec_handle is None:
                reply(writer, "409 Conflict", b"nothing is being recorded")
            else:
                ms = self.local_ms()
                stamp = time.strftime("%H:%M:%S", time.gmtime(ms / 1000)) + f".{ms % 1000:03d}"
                self.rec_handle.write(f"# MARK {stamp} {text.strip()}\n")
                self.rec_handle.flush()
                reply(writer, "204 No Content")
        elif route == "/sd/auto" and method == "POST":
            self.auto = query.get("on") == "1"
            reply(writer, "204 No Content")
        else:
            reply(writer, "404 Not Found", b"no such endpoint")

    def status_text(self):
        if self.auto and not self.hold and self.state == "ready" and not self.want_record:
            self.want_record = True
        ready = self.state == "ready"
        used = self.used() if ready else 0
        card = ("SD16G", "SDHC", CAPACITY) if self.state in ("ready", "formatting") else ("", "", 0)
        return (
            f"state {self.state}\nmessage {self.message}\ncard_name {card[0]}\ncard_type {card[1]}\n"
            f"card_bytes {card[2]}\ntotal_bytes {TOTAL if ready else 0}\nfree_bytes {TOTAL - used if ready else 0}\n"
            f"space_known {1 if ready else 0}\nbusy {self.busy}\nbusy_done {self.done}\nbusy_total {self.total}\n"
            f"recording {1 if self.recording else 0}\nrecord_wanted {1 if self.want_record else 0}\n"
            f"record_file {self.rec_file}\nrecord_frames {self.rec_frames}\nrecord_bytes {self.rec_bytes}\n"
            f"record_lost 0\nauto {1 if self.auto else 0}\nclock {1 if self.clock else 0}\nheap_free 91000\n"
        )

    def not_ready(self, writer, reply):
        reply(writer, "503 Service Unavailable", b"the card is not ready")

    async def list(self, writer, path, reply):
        if not path_ok(path):
            reply(writer, "400 Bad Request", b"bad path")
        elif self.state != "ready":
            self.not_ready(writer, reply)
        elif not os.path.isdir(self.full(path)):
            reply(writer, "404 Not Found", b"no such folder")
        else:
            lines = [f"path {path}"]
            names = sorted(os.listdir(self.full(path)))
            shown = 0
            for name in names:
                if not name_ok(name):
                    continue
                if shown >= 300:
                    lines.append("more 1")
                    break
                p = os.path.join(self.full(path), name)
                directory = os.path.isdir(p)
                lines.append(f"{'d' if directory else 'f'}\t{name}\t{0 if directory else os.path.getsize(p)}\t{date_text(os.path.getmtime(p))}")
                shown += 1
            reply(writer, "200 OK", ("\n".join(lines) + "\n").encode())

    async def download(self, writer, path, reply):
        if not path_ok(path):
            reply(writer, "400 Bad Request", b"bad path")
        elif self.state != "ready":
            self.not_ready(writer, reply)
        elif not os.path.isfile(self.full(path)):
            reply(writer, "404 Not Found", b"no such file")
        elif self.recording and path == self.rec_file:
            reply(writer, "409 Conflict", b"this file is being recorded; stop the recording first")
        else:
            size = os.path.getsize(self.full(path))
            name = path.rsplit("/", 1)[-1]
            writer.write(
                (
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n"
                    f'Content-Disposition: attachment; filename="{name}"\r\nContent-Length: {size}\r\n'
                    "Cache-Control: no-store\r\nConnection: close\r\n\r\n"
                ).encode()
            )
            self.busy, self.done, self.total = "download", 0, size
            with open(self.full(path), "rb") as f:
                while True:
                    block = f.read(CHUNK * 16)
                    if not block:
                        break
                    writer.write(block)
                    await writer.drain()
                    self.done += len(block)
                    await asyncio.sleep(0.01)
            self.busy = "none"

    async def upload(self, reader, writer, path, headers, reply):
        length = int(headers.get("content-length", "0") or 0)
        if not path_ok(path) or path == "/":
            reply(writer, "400 Bad Request", b"bad path")
            return
        parent = os.path.dirname(self.full(path))
        problem = None
        if self.state != "ready":
            problem = ("503 Service Unavailable", b"the card is not ready")
        elif self.busy != "none":
            problem = ("409 Conflict", b"busy with another job")
        elif not os.path.isdir(parent):
            problem = ("404 Not Found", b"that folder does not exist")
        elif os.path.isdir(self.full(path)) or (self.recording and path == self.rec_file):
            problem = ("409 Conflict", b"that name is a folder or a recording in progress")
        elif length + CHUNK > TOTAL - self.used():
            problem = ("507 Insufficient Storage", b"not enough free space on the card")
        if problem:
            reply(writer, *problem)
            return
        self.busy, self.done, self.total = "upload", 0, length
        part = self.full(path) + ".part"
        try:
            with open(part, "wb") as f:
                while self.done < length:
                    block = await reader.read(min(CHUNK * 4, length - self.done))
                    if not block:
                        raise ConnectionError("cut off")
                    f.write(block)
                    self.done += len(block)
                    await asyncio.sleep(0.005)
            os.replace(part, self.full(path))
            reply(writer, "204 No Content")
        except ConnectionError:
            if os.path.exists(part):
                os.remove(part)
        finally:
            self.busy = "none"

    async def delete(self, writer, path, reply):
        if not path_ok(path):
            reply(writer, "400 Bad Request", b"bad path")
        elif path == "/":
            reply(writer, "400 Bad Request", b"the whole card cannot be deleted this way; use Format")
        elif self.state != "ready":
            self.not_ready(writer, reply)
        elif not os.path.exists(self.full(path)):
            reply(writer, "404 Not Found", b"no such file or folder")
        elif self.recording and (self.rec_file == path or self.rec_file.startswith(path.rstrip("/") + "/")):
            reply(writer, "409 Conflict", b"a recording is being written there; stop it first")
        else:
            self.busy = "delete"
            await asyncio.sleep(0.3)
            target = self.full(path)
            shutil.rmtree(target) if os.path.isdir(target) else os.remove(target)
            self.busy = "none"
            reply(writer, "204 No Content")

    def mkdir(self, writer, path, reply):
        if not path_ok(path) or path == "/":
            reply(writer, "400 Bad Request", b"bad path")
        elif self.state != "ready":
            self.not_ready(writer, reply)
        elif not os.path.isdir(os.path.dirname(self.full(path))):
            reply(writer, "404 Not Found", b"the folder above does not exist")
        elif os.path.exists(self.full(path)):
            reply(writer, "409 Conflict", b"that name is already used")
        else:
            os.mkdir(self.full(path))
            reply(writer, "204 No Content")

    async def format(self, writer, reply):
        if self.want_record or self.recording:
            reply(writer, "409 Conflict", b"stop the recording first")
        elif self.state not in ("ready", "nofs"):
            self.not_ready(writer, reply)
        else:
            self.state, self.busy = "formatting", "format"
            self.message = "Formatting the card..."
            asyncio.get_running_loop().create_task(self.finish_format())
            reply(writer, "204 No Content")  # answered at once, as the unit does; the page follows the status

    async def finish_format(self):
        await asyncio.sleep(3)
        for name in os.listdir(self.root):
            p = os.path.join(self.root, name)
            shutil.rmtree(p) if os.path.isdir(p) else os.remove(p)
        self.state, self.busy, self.message = "ready", "none", "Formatted."
