#!/usr/bin/env python3
"""Open the SH-C31B the way CANViewer does and print every CAN frame it receives."""

import glob
import os
import select
import signal
import subprocess
import sys
import termios
import time
from datetime import datetime

SERIAL_PAUSE_S = 2.0
COMMAND_WAIT_S = 0.4


def stamp():
    now = datetime.now()
    return now.strftime("%H:%M:%S.") + f"{now.microsecond // 1000:03d}"


def find_port():
    ports = sorted(glob.glob("/dev/cu.usbmodem*"))
    if not ports:
        sys.exit("No /dev/cu.usbmodem device is connected.")
    return ports


def port_holders(path):
    result = subprocess.run(["lsof", path], capture_output=True, text=True)
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def open_fd(path):
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


def frame_line(raw):
    text = raw.split(b"\r", 1)[0].decode("ascii", "replace").strip()
    if len(text) < 5 or text[0] not in "tTrR":
        return None
    extended = text[0] in "TR"
    remote = text[0] in "rR"
    id_len = 8 if extended else 3
    if len(text) < 1 + id_len + 1:
        return None
    try:
        ident = int(text[1 : 1 + id_len], 16)
        dlc = int(text[1 + id_len], 16)
    except ValueError:
        return None
    data_hex = text[2 + id_len :]
    if dlc > 8 or (not remote and len(data_hex) < dlc * 2):
        return None
    try:
        payload = bytes.fromhex(data_hex[: dlc * 2]) if not remote else b""
    except ValueError:
        return None
    kind = "EXTID" if extended else "STDID"
    width = 8 if extended else 5
    data = " ".join(f"{byte:02X}" for byte in payload)
    if remote:
        data = "RTR"
    return f"time:{stamp()} RX: {kind}:{ident:0{width}X} data:{data}"


class Dump:
    def __init__(self, path, log):
        self.path = path
        self.log = log
        self.fd = None
        self.buf = b""
        self.stop = False

    def write(self, text, echo=True):
        self.log.write(text + "\n")
        self.log.flush()
        if echo:
            print(text, flush=True)

    def tx(self, data):
        os.write(self.fd, data)
        shown = data.decode("ascii", "replace").replace("\r", "\\r")
        self.write(f"# >> {shown}", echo=False)

    def open_port(self):
        self.fd = open_fd(self.path)
        self.buf = b""

    def close_port(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None

    def take(self):
        while self.buf:
            bell = self.buf.find(b"\x07")
            cr = self.buf.find(b"\r")
            if bell != -1 and (cr == -1 or bell < cr):
                self.buf = self.buf[bell + 1 :]
                self.write("bell")
                continue
            if cr == -1:
                return
            line = self.buf[: cr + 1]
            self.buf = self.buf[cr + 1 :]
            if line == b"\r":
                self.write("# << \\r", echo=False)
                continue
            shown = frame_line(line)
            if shown is not None:
                self.write(shown)
            else:
                text = line.decode("ascii", "replace").replace("\r", "\\r")
                self.write(f"# << {text}", echo=False)

    def pump(self, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline and not self.stop:
            timeout = min(0.05, max(0.0, deadline - time.monotonic()))
            readable, _, _ = select.select([self.fd], [], [], timeout)
            if self.fd in readable:
                try:
                    chunk = os.read(self.fd, 4096)
                except BlockingIOError:
                    chunk = b""
                if chunk:
                    self.buf += chunk
            self.take()

    def command(self, data):
        self.tx(data)
        self.pump(COMMAND_WAIT_S)

    def run(self):
        self.open_port()
        time.sleep(SERIAL_PAUSE_S)
        self.command(b"C\r")
        self.command(b"S5\r")
        self.command(b"O\r")
        self.write("# open, acknowledging, receiving")
        while not self.stop:
            self.pump(0.2)
        self.command(b"C\r")
        self.close_port()
        self.write("# closed")


def main():
    path = None
    for candidate in find_port():
        holders = port_holders(candidate)
        if holders:
            print(f"{candidate} is in use:\n{holders}", file=sys.stderr)
            continue
        path = candidate
        break
    if path is None:
        sys.exit("Every usbmodem port is already open.")

    captures = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "captures")
    os.makedirs(captures, exist_ok=True)
    started = datetime.now()
    log_path = os.path.join(captures, started.strftime("dump-%Y%m%d-%H%M.log"))
    print(log_path, flush=True)

    with open(log_path, "w", encoding="utf-8") as log:
        dump = Dump(path, log)
        dump.write(
            f"# dump {path} 115200 8N1 S5 250 kbit/s started {started.isoformat(timespec='milliseconds')}",
            echo=False,
        )

        def handle_stop(_signum, _frame):
            dump.stop = True

        signal.signal(signal.SIGINT, handle_stop)
        try:
            dump.run()
        finally:
            if dump.fd is not None:
                try:
                    os.write(dump.fd, b"C\r")
                except OSError:
                    pass
                dump.close_port()


if __name__ == "__main__":
    main()
