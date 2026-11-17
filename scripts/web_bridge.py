#!/usr/bin/env python3
"""Serves the browser version of the S01 monitor and feeds it the CAN bus.

Standard library only. The page is served over HTTP; the live feed is a
WebSocket at /ws that carries slcan text, the same lines a CANable prints,
so the browser runs the same decoder as the desktop app.

  scripts/web_bridge.py --serial /dev/cu.usbmodemXXXX   live bus from a CANable
  scripts/web_bridge.py --replay recording.log      play a capture, on a loop

Then open http://localhost:8080 . This is a stand-in for the ESP32, which will
serve the same page, the same /ws feed and the same /views (saved views) from
the scooter.
"""
import argparse
import asyncio
import base64
import hashlib
import mimetypes
import os
import re
import struct
import sys
import time

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
DIST = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "web", "dist")
LINE = re.compile(r"time:(\d+):(\d+):(\d+)\.(\d+)\s+RX:\s+STDID:([0-9A-Fa-f]+)\s+data:([0-9A-Fa-f ]*)")

clients = set()
serial_fd = None

# The saved views, kept in memory here (the ESP32 keeps them in flash). The
# limits and answers match the firmware's /views.
VIEWS_MAX = 6000
VIEWS_HEADER = b"S01 views 1"
views = b""


def ws_frame(payload, opcode=1):
    head = bytes([0x80 | opcode])
    n = len(payload)
    if n < 126:
        head += bytes([n])
    elif n < 65536:
        head += bytes([126]) + struct.pack(">H", n)
    else:
        head += bytes([127]) + struct.pack(">Q", n)
    return head + payload


async def read_frame(reader):
    """One client frame: (opcode, payload)."""
    b1, b2 = await reader.readexactly(2)
    opcode = b1 & 0x0F
    length = b2 & 0x7F
    if length == 126:
        (length,) = struct.unpack(">H", await reader.readexactly(2))
    elif length == 127:
        (length,) = struct.unpack(">Q", await reader.readexactly(8))
    mask = await reader.readexactly(4) if b2 & 0x80 else b"\0\0\0\0"
    data = bytearray(await reader.readexactly(length))
    for i in range(length):
        data[i] ^= mask[i % 4]
    return opcode, bytes(data)


def broadcast(text):
    # Noise on a serial line decodes to characters that are not ASCII. Turn
    # them into "?" (the app ignores such a line) instead of failing and
    # dropping everything that arrived with them.
    frame = ws_frame(text.encode("ascii", "replace"))
    for writer in list(clients):
        try:
            writer.write(frame)
        except Exception:
            clients.discard(writer)


def to_serial(text):
    if serial_fd is not None:
        try:
            os.write(serial_fd, text.encode("ascii", "replace"))
        except BlockingIOError:
            pass  # the adapter is not ready for more; a read request is simply dropped


async def serve_static(writer, path, gzip_ok):
    path = path.split("?", 1)[0]
    if path == "/":
        path = "/index.html"
    full = os.path.normpath(os.path.join(DIST, path.lstrip("/")))
    if not full.startswith(os.path.normpath(DIST)) or not os.path.isfile(full):
        writer.write(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        return
    with open(full, "rb") as f:
        body = f.read()
    kind = mimetypes.guess_type(full)[0] or "application/octet-stream"
    if full.endswith(".wasm"):
        kind = "application/wasm"
    # Send the pre-compressed file when the browser accepts it.
    head = f"HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\n"
    gz = full + ".gz"
    if full.endswith(".wasm") and os.path.isfile(gz) and gzip_ok:
        with open(gz, "rb") as f:
            body = f.read()
        head += "Content-Encoding: gzip\r\n"
    head += f"Content-Length: {len(body)}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    writer.write(head.encode() + body)


def reply(writer, status, body=b""):
    writer.write(
        f"HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\n"
        f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode() + body
    )


async def serve_views(reader, writer, method, headers):
    global views
    if method == "GET":
        reply(writer, "200 OK", views)
    elif method == "PUT":
        length = int(headers.get("content-length", "0") or 0)
        if length > VIEWS_MAX:
            reply(writer, "413 Payload Too Large", b"too big")
            return
        body = await reader.readexactly(length)
        if body and not body.startswith(VIEWS_HEADER):
            reply(writer, "400 Bad Request", b"not a views list")
            return
        views = body
        print(f"saved views: {len(body)} bytes", flush=True)
        reply(writer, "204 No Content")
    else:
        reply(writer, "405 Method Not Allowed")


async def handle(reader, writer):
    try:
        request = await reader.readuntil(b"\r\n\r\n")
        lines = request.decode("latin-1").split("\r\n")
        method, path, _ = lines[0].split(" ", 2)
        headers = {}
        for line in lines[1:]:
            if ":" in line:
                k, v = line.split(":", 1)
                headers[k.strip().lower()] = v.strip()
        if path.split("?", 1)[0] == "/ws" and headers.get("upgrade", "").lower() == "websocket":
            accept = base64.b64encode(hashlib.sha1((headers["sec-websocket-key"] + GUID).encode()).digest()).decode()
            writer.write(
                (
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                    f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
                ).encode()
            )
            await writer.drain()
            clients.add(writer)
            print(f"browser connected ({len(clients)} open)", flush=True)
            try:
                while True:
                    opcode, data = await read_frame(reader)
                    if opcode == 8:
                        break
                    if opcode == 9:
                        writer.write(ws_frame(data, 10))
                    elif opcode in (1, 2):
                        to_serial(data.decode("ascii", "replace"))
            finally:
                clients.discard(writer)
                print(f"browser left ({len(clients)} open)", flush=True)
        elif path.split("?", 1)[0] == "/views":
            await serve_views(reader, writer, method, headers)
            await writer.drain()
        else:
            await serve_static(writer, path, "gzip" in headers.get("accept-encoding", ""))
            await writer.drain()
    except (asyncio.IncompleteReadError, ConnectionError):
        pass
    finally:
        try:
            writer.close()
        except Exception:
            pass


def parse_capture(path):
    """(seconds, slcan line) for every frame in a dump or recording log."""
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for text in f:
            m = LINE.match(text.strip())
            if not m:
                continue
            h, mi, s, ms, ident, data = m.groups()
            t = int(h) * 3600 + int(mi) * 60 + int(s) + int(ms) / 1000
            payload = data.replace(" ", "")
            out.append((t, f"t{int(ident, 16):03X}{len(payload) // 2}{payload}\r"))
    return out


async def replay(path, speed):
    frames = parse_capture(path)
    if not frames:
        sys.exit(f"No frames found in {path}")
    print(f"replaying {len(frames)} frames from {os.path.basename(path)} at {speed}x, looping", flush=True)
    while True:
        start = time.monotonic()
        base = frames[0][0]
        batch = []
        for t, line in frames:
            due = (t - base) / speed
            wait = due - (time.monotonic() - start)
            if wait > 0.004 and batch:
                broadcast("".join(batch))
                batch = []
            if wait > 0.004:
                await asyncio.sleep(wait)
            batch.append(line)
        if batch:
            broadcast("".join(batch))
        await asyncio.sleep(1.0)


def open_serial(path):
    import termios

    fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    attrs[0] = 0
    attrs[1] = 0
    attrs[2] = termios.CLOCAL | termios.CREAD | termios.CS8
    attrs[3] = 0
    attrs[4] = termios.B115200
    attrs[5] = termios.B115200
    attrs[6][termios.VMIN] = 0
    attrs[6][termios.VTIME] = 0
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    termios.tcflush(fd, termios.TCIOFLUSH)
    return fd


async def live(path):
    global serial_fd
    serial_fd = open_serial(path)
    for command in (b"C\r", b"S5\r", b"O\r"):
        os.write(serial_fd, command)
        await asyncio.sleep(0.1)
    print(f"reading {path}", flush=True)
    loop = asyncio.get_running_loop()

    def readable():
        try:
            data = os.read(serial_fd, 4096)
        except BlockingIOError:
            return
        if data and clients:
            broadcast(data.decode("ascii", "replace"))

    loop.add_reader(serial_fd, readable)
    while True:
        await asyncio.sleep(3600)


async def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--serial", help="CANable serial port")
    source.add_argument("--replay", help="capture log to play")
    parser.add_argument("--speed", type=float, default=1.0, help="replay speed")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--host", default="127.0.0.1", help="use 0.0.0.0 to reach it from a phone")
    args = parser.parse_args()
    server = await asyncio.start_server(handle, args.host, args.port)
    print(f"open http://{args.host}:{args.port}", flush=True)
    task = replay(args.replay, args.speed) if args.replay else live(args.serial)
    async with server:
        await asyncio.gather(server.serve_forever(), task)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
