#!/usr/bin/env python3
"""Find out what makes the motor controller fault when the event log is read.

    python3 scripts/event_log_test.py TEST [--send] [--port PATH]

A user reports that reading the battery's event log makes the Sevcon
controller show error 0xF481 and shut down until the scooter is switched off
and on (see CAN_NOTES.md, `0x1ED` / `0x1EE`). A recording shows the
controller's fault bytes (0x411 bytes 2-3) change during a log read. This script
runs the read in small steps, one named test at a time, and records every frame
it receives and every frame it sends, with times, so the step that causes the
fault can be told from the ones that do not.

TEST
  quiet    Listen for 20 s. Sends nothing. Checks the set-up and shows what a
           healthy bus looks like. It asks the adapter for listen-only mode. If
           the adapter does not support that and shows nothing, add --ack to
           open it as can_dump.py does (it then acknowledges frames, but sends
           no message of its own).
  first    Send the first request (an empty 0x1ED frame) once. The battery
           answers with ten records. No window request, no long burst.
  window   Send one window request (0x1ED with four bytes) and nothing before it.
           The index comes from --window-index (the monitor sends the index
           minus one, then the index). To try other numbers, give both with
           --window-first and --window-second. The battery may not answer.
  full     What the monitor does today: the first request, then the window
           request after the tenth record. This is the one that faults.
  setclock THIS ONE WRITES. It sends the battery clock the computer's date and
           time, in the one "Set current time" frame (0x201, seven bytes: 04, then hour, minute, second, day,
           month and year as BCD). It needs --send and also --write-clock. It
           then shows the clock before and after. Used on 9 Nov 2026 to see what
           the battery makes of a date in November.
  paged    The whole log, a few records at a time. The first request (the
           newest ten), then window requests (start, count) one after another
           with a pause between them, each for --page-size records (10), down to
           record 0. The last page asks for only the records that remain: asking
           for more makes the battery wrap round to index 4095 and count down
           from there (seen on 9 Nov 2026). A window request with the numbers (start, count) brings
           `count` records from `start` downwards (seen on 9 Nov 2026). It
           stops at the first sign of a fault, so at most one more page is sent
           after the controller reacts.

Safety.
  * Without --send, a test that transmits only prints what it would send and
    exits. Nothing is opened.
  * With --send it counts down for 10 s first (Ctrl+C cancels), then sends.
  * It can send only the read requests above, which the monitor already sends:
    an empty 0x1ED frame and a 0x1ED frame with four bytes. Anything else is
    refused in the code, so a mistake cannot write a setting or send a control
    frame. The one exception is the clock frame of the setclock test, which is
    allowed in that test only, and only in its exact form. (The adapter set-up
    commands C, S5 and O or L are not CAN frames.)
  * It closes the adapter on every way out, including Ctrl+C and closing the
    Terminal window.

Before a test: the scooter on its stand with the wheel off the ground, key on,
as on 2 Nov. Nothing else may hold the serial port (close idf.py monitor and the
monitor app). After a test that faults, switch the scooter off and on again.

The dates of the records that come back are decoded too, the way the monitor shows
them (year/month/day hour:minute:second, each byte as a plain decimal number),
with the raw bytes beside them and a warning for a month outside 1-12. This is
for the report that October to December show as months 00 to 02 on one scooter.

The log goes to the log directory as eventtest-DATE-TEST.log. Lines that start "# TX" are
what was sent. The monitor can replay it (Frames, Replay a capture).
"""

import argparse
import glob
import os
import re
import select
import signal
import sys
import time
from datetime import datetime

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from can_dump import frame_line, open_fd, port_holders, stamp  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

FIRST_REQUEST = b"t1ED0\r"
WINDOW_REQUEST = re.compile(rb"^t1ED4[0-9A-F]{8}\r$")
SETUP_COMMANDS = {b"C\r", b"S5\r", b"O\r", b"L\r"}
# The clock-set frame: id 0x201, seven bytes, 04 then six BCD bytes (every nibble 0-9).
CLOCK_SET = re.compile(rb"^t201704(?:[0-9]{2}){6}\r$")

QUIET_SECONDS = 20.0
BEFORE_SECONDS = 5.0
AFTER_SECONDS = 12.0
AFTER_FAULT_SECONDS = 6.0
RECORD_WAIT_SECONDS = 10.0
SILENCE_SECONDS = 3.0
LONGEST_FULL_SECONDS = 25.0
COUNTDOWN_SECONDS = 10

# What is watched for a change. Each is (identifier, first byte, last byte, name).
WATCHED = [
    (0x411, 0, 1, "0x411 bytes 0-1 (motor status word)"),
    (0x411, 2, 3, "0x411 bytes 2-3 (highest active fault)"),
    (0x701, 0, 0, "0x701 byte 0 (Sevcon operating mode)"),
]


def pair_line(first, second):
    """A window request with its two 16-bit numbers (low byte first)."""
    first &= 0xFFFF
    second &= 0xFFFF
    data = bytes([first & 0xFF, first >> 8, second & 0xFF, second >> 8])
    return b"t1ED4" + data.hex().upper().encode() + b"\r"


def window_line(index):
    """The window request the monitor sends after the tenth record: the index
    minus one, then the index."""
    return pair_line(index - 1, index)


def clock_line(moment):
    """The clock-set frame for this moment: 04, hour, minute, second, day, month,
    year (two digits), each written as its decimal text read as hex, which gives the BCD."""
    digits = f"{moment.hour:02d}{moment.minute:02d}{moment.second:02d}{moment.day:02d}{moment.month:02d}{moment.year % 100:02d}"
    return ("t201704" + digits + "\r").encode()


def allowed(data, clock=False):
    if clock and CLOCK_SET.match(data):
        return True
    return data in SETUP_COMMANDS or data == FIRST_REQUEST or bool(WINDOW_REQUEST.match(data))


def bcd(value):
    return (value >> 4) * 10 + (value & 15)


def clock_text(frame):
    """A 0x200 clock frame as 'HH:MM:SS DD/MM/YY (flag)'."""
    if len(frame) < 7:
        return "short frame"
    return "%02d:%02d:%02d %02d/%02d/%02d (flag %02X)" % (
        bcd(frame[1]), bcd(frame[2]), bcd(frame[3]), bcd(frame[4]), bcd(frame[5]), bcd(frame[6]), frame[0])


def parse_rx(raw):
    """(identifier, payload) of a received standard frame line, else None."""
    text = raw.split(b"\r", 1)[0].decode("ascii", "replace").strip()
    if len(text) < 5 or text[0] != "t":
        return None
    try:
        ident = int(text[1:4], 16)
        dlc = int(text[4], 16)
        payload = bytes.fromhex(text[5 : 5 + dlc * 2])
    except ValueError:
        return None
    if len(payload) != dlc:
        return None
    return ident, payload


class Test:
    def __init__(self, path, log, name, window_index, acknowledge=False, window_pair=None,
                 page_size=10, page_gap=1.0, max_pages=40, allow_clock=False):
        self.path = path
        self.log = log
        self.name = name
        self.window_index = window_index
        self.acknowledge = acknowledge
        self.window_pair = window_pair
        self.allow_clock = allow_clock and name == "setclock"
        self.clock_frames = []  # (monotonic, the computer's time, the 0x200 frame)
        self.clock_sent_for = None
        self.page_size = page_size
        self.page_gap = page_gap
        self.max_pages = max_pages
        self.pages = []  # (monotonic when sent, start, records asked for, records that came)
        self.stopped_for = None
        self.fd = None
        self.buf = b""
        self.stop = False
        self.t0 = time.monotonic()
        self.sent = []  # (monotonic, text)
        self.event_frames = []  # monotonic times of 0x1EE frames
        self.partial = 0
        self.records = 0
        self.last_index = None
        self.first_frame = None
        self.dated = []  # (index, the first frame of each whole record)
        self.values = {}  # watched name -> last value (hex text)
        self.first_values = {}
        self.changes = []  # (monotonic, name, before, after)
        self.window_sent = False
        self.seen_411 = False

    # --- output ---------------------------------------------------------
    def write(self, text, echo=True):
        self.log.write(text + "\n")
        self.log.flush()
        if echo:
            print(text, flush=True)

    def since(self, moment):
        return moment - self.t0

    # --- adapter --------------------------------------------------------
    def open_port(self):
        self.fd = open_fd(self.path)
        self.buf = b""

    def close_port(self):
        if self.fd is not None:
            try:
                os.write(self.fd, b"C\r")
            except OSError:
                pass
            try:
                os.close(self.fd)
            except OSError:
                pass
            self.fd = None

    def tx(self, data):
        if not allowed(data, self.allow_clock):
            raise SystemExit(f"Refusing to send {data!r}: only the two event-log read requests may be sent.")
        os.write(self.fd, data)
        shown = data.decode("ascii").replace("\r", "\\r")
        now = time.monotonic()
        is_frame = data not in SETUP_COMMANDS
        if is_frame:
            self.sent.append((now, shown))
            self.write(f"# TX {stamp()} {shown}")
        else:
            self.write(f"# >> {shown}", echo=False)

    # --- receiving ------------------------------------------------------
    def pump(self, seconds, until=None):
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
            if until is not None and until():
                return True
        return False

    def take(self):
        while True:
            bell = self.buf.find(b"\x07")
            cr = self.buf.find(b"\r")
            if bell != -1 and (cr == -1 or bell < cr):
                self.buf = self.buf[bell + 1 :]
                self.write("# << bell (the adapter refused a command)")
                continue
            if cr == -1:
                return
            line = self.buf[: cr + 1]
            self.buf = self.buf[cr + 1 :]
            if line == b"\r":
                continue
            shown = frame_line(line)
            if shown is None:
                text = line.decode("ascii", "replace").replace("\r", "\\r")
                self.write(f"# << {text}", echo=False)
                continue
            self.write(shown, echo=False)
            parsed = parse_rx(line)
            if parsed is not None:
                self.on_frame(*parsed)

    def on_frame(self, ident, data):
        now = time.monotonic()
        if ident == 0x411:
            self.seen_411 = True
        if ident == 0x200 and len(data) >= 7:
            self.clock_frames.append((now, datetime.now(), bytes(data)))
        for watched_id, first, last, name in WATCHED:
            if ident == watched_id and len(data) > last:
                value = data[first : last + 1].hex().upper()
                before = self.values.get(name)
                self.values[name] = value
                self.first_values.setdefault(name, value)
                if before is not None and before != value:
                    self.changes.append((now, name, before, value))
                    after_tx = ""
                    if self.sent:
                        after_tx = f", {now - self.sent[-1][0]:.2f} s after the last frame sent"
                    self.write(f"*** CHANGE {name}: {before} -> {value} at +{self.since(now):.2f} s{after_tx}")
        if ident == 0x1EE:
            self.event_frames.append(now)
            self.partial += 1
            if self.partial == 1:
                self.first_frame = data
            if self.partial == 3:
                self.partial = 0
                self.records += 1
                if self.first_frame is not None and len(self.first_frame) >= 2:
                    self.last_index = (self.first_frame[0] >> 4) + (self.first_frame[1] << 4)
                    self.dated.append((self.last_index, bytes(self.first_frame)))

    # --- the tests ------------------------------------------------------
    def start_adapter(self, listen_only):
        self.open_port()
        time.sleep(2.0)  # the adapter restarts when the port is opened
        self.tx(b"C\r")
        self.pump(0.4)
        self.tx(b"S5\r")
        self.pump(0.4)
        self.tx(b"L\r" if listen_only else b"O\r")
        self.pump(0.4)
        mode = "listen only" if listen_only else "open, acknowledging frames"
        self.write(f"# adapter {mode}, 250 kbit/s")

    def run(self):
        quiet = self.name == "quiet" and not self.acknowledge
        self.write(f"# event_log_test {self.name} {self.path} started {datetime.now().isoformat(timespec='milliseconds')}", echo=False)
        self.start_adapter(listen_only=quiet)
        self.t0 = time.monotonic()
        if self.name == "quiet":
            self.write(f"# listening for {QUIET_SECONDS:.0f} s, sending nothing")
            self.pump(QUIET_SECONDS)
            return
        self.write(f"# listening for {BEFORE_SECONDS:.0f} s before anything is sent")
        self.pump(BEFORE_SECONDS)
        if self.name == "first":
            self.tx(FIRST_REQUEST)
            self.pump(AFTER_SECONDS)
        elif self.name == "window":
            self.tx(pair_line(*self.window_pair) if self.window_pair else window_line(self.window_index))
            self.window_sent = True
            self.pump(AFTER_SECONDS)
        elif self.name == "setclock":
            moment = datetime.now()
            self.clock_sent_for = moment
            self.tx(clock_line(moment))
            self.pump(AFTER_SECONDS)
        elif self.name == "paged":
            self.run_paged()
        elif self.name == "full":
            self.tx(FIRST_REQUEST)
            got = self.pump(RECORD_WAIT_SECONDS, until=lambda: self.records >= 10)
            if got:
                self.tx(window_line(self.last_index))
                self.window_sent = True
                # Until the stream stops, and then a little longer.
                started = time.monotonic()
                self.pump(
                    LONGEST_FULL_SECONDS,
                    until=lambda: self.event_frames
                    and time.monotonic() - self.event_frames[-1] > SILENCE_SECONDS
                    and time.monotonic() - started > 1.0,
                )
            else:
                self.write(f"# only {self.records} records came, so no window request was sent")
            self.pump(AFTER_FAULT_SECONDS)

    def fault_raised(self):
        return any("fault" in name and after != "0000" for _moment, name, _before, after in self.changes)

    def run_paged(self):
        """The newest ten, then (start, count) pages down to record 0, with a pause."""
        self.tx(FIRST_REQUEST)
        if not self.pump(RECORD_WAIT_SECONDS, until=lambda: self.records >= 10):
            self.stopped_for = f"only {self.records} records came to the first request"
            return
        # The pause also leaves time for the fault bytes (every 0.5 s) to show a reaction.
        self.pump(self.page_gap)
        lowest = self.last_index
        for _page in range(self.max_pages):
            if self.fault_raised():
                self.stopped_for = "the controller reported a fault"
                break
            if lowest is None or lowest <= 0:
                break
            start = lowest - 1
            asked = min(self.page_size, start + 1)
            before = self.records
            sent_at = time.monotonic()
            self.tx(pair_line(start, asked))
            self.pump(RECORD_WAIT_SECONDS, until=lambda: self.records >= before + asked)
            came = self.records - before
            self.pages.append((sent_at, start, asked, came))
            if came != asked:
                self.stopped_for = f"page from {start} gave {came} records, not {asked}"
                break
            lowest = self.last_index
            self.pump(self.page_gap)
        else:
            self.stopped_for = "the page limit was reached"
        self.pump(AFTER_FAULT_SECONDS)

    def summary(self):
        lines = ["", f"== Result of the '{self.name}' test =="]
        if not self.seen_411:
            lines.append("No 0x411 frames were seen. Is the scooter on, and the adapter connected to the bus?")
        for name, value in self.first_values.items():
            lines.append(f"At the start: {name} = {value}")
        for moment, shown in self.sent:
            lines.append(f"Sent {shown} at +{self.since(moment):.2f} s")
        if self.name == "setclock" and self.sent:
            sent_at = self.sent[0][0]
            before = [item for item in self.clock_frames if item[0] < sent_at]
            after = [item for item in self.clock_frames if item[0] >= sent_at + 0.3]
            def line(label, item):
                return f"{label}: the battery clock reads {clock_text(item[2])} when the computer reads {item[1].strftime('%H:%M:%S %d/%m/%y')}"
            if before:
                lines.append(line("Just before", before[-1]))
            if self.clock_sent_for:
                lines.append("Sent for: " + self.clock_sent_for.strftime("%H:%M:%S %d/%m/%y"))
            if after:
                lines.append(line("Just after", after[0]))
                lines.append(line("At the end", after[-1]))
            else:
                lines.append("No clock frame came after the frame was sent.")
        if self.name == "paged":
            asked = sum(item[2] for item in self.pages)
            lines.append(f"Pages sent after the first request: {len(self.pages)}, for {asked} records")
            lines.append(f"Records in all: {self.records}")
            indexes = [item[0] for item in self.dated]
            if indexes:
                complete = indexes == list(range(indexes[0], indexes[0] - len(indexes), -1))
                lines.append(
                    f"Index {indexes[0]} down to {indexes[-1]}: "
                    + ("in order, with none missing or repeated" if complete else "NOT a clean run (a gap, a repeat or wrong order)")
                )
                lines.append("The whole log was read." if indexes[-1] == 0 else "The log was not read down to record 0.")
            if self.stopped_for:
                lines.append(f"Stopped because: {self.stopped_for}")
        if self.event_frames:
            first, last = self.event_frames[0], self.event_frames[-1]
            lines.append(
                f"0x1EE frames: {len(self.event_frames)} ({self.records} whole records), "
                f"from +{self.since(first):.2f} s to +{self.since(last):.2f} s"
            )
            if self.sent:
                lines.append(f"The first came {first - self.sent[0][0]:.2f} s after the first frame sent")
        elif self.sent:
            lines.append("No 0x1EE frame came back.")
        if self.changes:
            lines.append("Changes in what was watched:")
            for moment, name, before, after in self.changes:
                lines.append(f"  +{self.since(moment):.2f} s  {name}: {before} -> {after}")
            raised = [c for c in self.changes if "fault" in c[1] and c[3] != "0000"]
            if raised:
                lines.append("The controller reported a fault. Switch the scooter off and on again before the next test.")
            else:
                lines.append("A watched value changed (for example a fault was cleared).")
        elif self.seen_411:
            lines.append("Nothing watched changed.")
        lines.append("The fault bytes arrive every 0.5 s, so a time here is right to about half a second.")
        lines.extend(date_lines(self.dated))
        return lines


def date_text(first):
    """The date as the monitor shows it: bytes 2 to 7, each as a decimal number."""
    padded = bytes(first) + bytes(8 - len(first))
    return "%02d/%02d/%02d %02d:%02d:%02d" % tuple(padded[2:8])


def date_problems(first):
    """What is not a possible date or time in the six bytes, if anything."""
    padded = bytes(first) + bytes(8 - len(first))
    year, month, day, hour, minute, second = padded[2:8]
    found = []
    if not 1 <= month <= 12:
        found.append(f"month {month}")
    if not 1 <= day <= 31:
        found.append(f"day {day}")
    if hour > 23:
        found.append(f"hour {hour}")
    if minute > 59:
        found.append(f"minute {minute}")
    if second > 59:
        found.append(f"second {second}")
    return found


def date_lines(dated, show=12):
    """A table of the records' dates, and a count of what months were seen."""
    if not dated:
        return []
    lines = ["", "Event-log dates (as the monitor shows them; the raw first frame is on the right):"]
    rows = dated if len(dated) <= show else dated[: show - 3] + [None] + dated[-3:]
    for row in rows:
        if row is None:
            lines.append(f"  ... {len(dated) - (show - 3) - 3} more ...")
            continue
        index, first = row
        problems = date_problems(first)
        note = "   <-- " + ", ".join(problems) if problems else ""
        lines.append(f"  #{index:<4} {date_text(first)}   {bytes(first).hex(' ').upper()}{note}")
    months = {}
    for _index, first in dated:
        padded = bytes(first) + bytes(8 - len(first))
        months[(padded[2], padded[3])] = months.get((padded[2], padded[3]), 0) + 1
    seen = ", ".join(f"{year:02d}/{month:02d} x{count}" for (year, month), count in sorted(months.items()))
    lines.append(f"Year/month seen: {seen}")
    odd = [item for item in dated if date_problems(item[1])]
    if odd:
        lines.append(f"{len(odd)} of {len(dated)} records have a date that cannot be real.")
    return lines


def find_port(path):
    if path:
        return path
    ports = sorted(glob.glob("/dev/cu.usbmodem*"))
    for candidate in ports:
        if not port_holders(candidate):
            return candidate
    if ports:
        sys.exit("Every usbmodem port is already open. Close the monitor app and any serial monitor.")
    sys.exit("No /dev/cu.usbmodem device is connected. Use --port to name the adapter.")


def plan_text(name, window_index, pair=None, page_size=10, page_gap=1.0, max_pages=40):
    if name == "quiet":
        return ["Listen for 20 s. Nothing is sent."]
    if name == "setclock":
        moment = datetime.now()
        return [
            "THIS TEST WRITES TO THE BATTERY. It sets the battery clock, once.",
            f"Send {clock_line(moment).decode().strip()}  (0x201, seven bytes: 04, then {moment.strftime('%H:%M:%S %d/%m/%y')} "
            "as BCD hour, minute, second, day, month, year; the time is taken again at the moment of sending)",
            "Nothing else is ever sent.",
        ]
    lines = []
    if name in ("first", "full", "paged"):
        lines.append(f"Send {FIRST_REQUEST.decode().strip()}  (an empty 0x1ED frame: the first request)")
    if name == "paged":
        lines.append(
            f"Then, one page at a time with {page_gap:g} s between them (at most {max_pages} pages), a 0x1ED frame with "
            f"four bytes: the next index down, then {page_size}. Each page brings {page_size} records "
            f"({page_size * 3} frames). It stops at the first fault."
        )
    if name == "window" and pair:
        lines.append(
            f"Send {pair_line(*pair).decode().strip()}  (a 0x1ED frame with four bytes: the numbers {pair[0]} and "
            f"{pair[1]}; sent on its own)"
        )
    elif name == "window":
        lines.append(
            f"Send {window_line(window_index).decode().strip()}  (a 0x1ED frame with four bytes: the window request "
            f"for index {window_index}; sent on its own)"
        )
    if name == "full":
        lines.append("Then, after the tenth record, send the window request for that record's index, as the monitor does.")
    lines.append("Nothing else is ever sent.")
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("test", choices=["quiet", "first", "window", "full", "paged", "setclock"])
    parser.add_argument("--send", action="store_true", help="really transmit (without it a sending test only shows its plan)")
    parser.add_argument("--port", help="the adapter's serial port (default: the first free /dev/cu.usbmodem*)")
    parser.add_argument("--write-clock", action="store_true", help="setclock test: also needed, because this test writes")
    parser.add_argument("--ack", action="store_true", help="quiet test: open the adapter in normal mode, not listen-only")
    parser.add_argument("--page-size", type=int, default=10, help="paged test: records per page (default 10)")
    parser.add_argument("--page-gap", type=float, default=1.0, help="paged test: seconds between pages (default 1.0)")
    parser.add_argument("--max-pages", type=int, default=40, help="paged test: most pages to send (default 40)")
    parser.add_argument("--window-first", type=int, help="window test: the first number (use with --window-second)")
    parser.add_argument("--window-second", type=int, help="window test: the second number (use with --window-first)")
    parser.add_argument("--window-index", type=int, default=132, help="index for the window test (default 132)")
    parser.add_argument("--countdown", type=int, default=COUNTDOWN_SECONDS, help="seconds to wait before sending")
    parser.add_argument("--log-dir", default=os.path.join(ROOT, "captures"), help="where the log goes (default: captures)")
    args = parser.parse_args()
    if not 1 <= args.window_index <= 0xFFFF:
        sys.exit("--window-index must be between 1 and 65535.")
    if not 1 <= args.page_size <= 30:
        sys.exit("--page-size must be between 1 and 30.")
    if args.page_gap < 0.6:
        sys.exit("--page-gap must be at least 0.6 s, so a reaction can be seen before the next page.")
    if not 1 <= args.max_pages <= 100:
        sys.exit("--max-pages must be between 1 and 100.")
    pair = None
    if args.window_first is not None or args.window_second is not None:
        if args.test != "window" or args.window_first is None or args.window_second is None:
            sys.exit("--window-first and --window-second go together, and only with the window test.")
        if not (0 <= args.window_first <= 0xFFFF and 0 <= args.window_second <= 0xFFFF):
            sys.exit("The window numbers must be between 0 and 65535.")
        pair = (args.window_first, args.window_second)

    print(f"Test '{args.test}'. What it does:")
    for line in plan_text(args.test, args.window_index, pair, args.page_size, args.page_gap, args.max_pages):
        print("  " + line)
    if args.test != "quiet" and not args.send:
        print("\nThis is only the plan. Nothing was opened or sent. Add --send to run it.")
        return
    if args.test == "setclock" and not args.write_clock:
        print("\nThis test writes the battery clock. Add --write-clock as well as --send to run it. Nothing was sent.")
        return

    path = find_port(args.port)
    started = datetime.now()
    log_path = os.path.join(args.log_dir, started.strftime(f"eventtest-%Y%m%d-%H%M-{args.test}.log"))
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"\nAdapter: {path}\nLog:     {log_path}")

    if args.test != "quiet":
        print(f"\nSending in {args.countdown} s. Press Ctrl+C now to cancel.")

        def cancel(_signum, _frame):
            raise KeyboardInterrupt

        # Set explicitly: a shell can start a program with Ctrl+C switched off.
        for name in ("SIGINT", "SIGTERM", "SIGHUP"):
            signal.signal(getattr(signal, name), cancel)
        try:
            for left in range(args.countdown, 0, -1):
                print(f"  {left}", flush=True)
                time.sleep(1)
        except KeyboardInterrupt:
            print("Cancelled. Nothing was sent.")
            return

    with open(log_path, "w", encoding="utf-8") as log:
        test = Test(path, log, args.test, args.window_index, acknowledge=args.ack, window_pair=pair,
                    page_size=args.page_size, page_gap=args.page_gap, max_pages=args.max_pages,
                    allow_clock=args.write_clock)

        def handle_stop(_signum, _frame):
            test.stop = True

        for name in ("SIGINT", "SIGTERM", "SIGHUP"):
            signal.signal(getattr(signal, name), handle_stop)
        try:
            test.run()
        finally:
            test.close_port()
            for line in test.summary():
                test.write(("# " + line) if line else "#", echo=False)
                print(line)
            test.write("# closed", echo=False)
    print(f"\nLog saved: {log_path}")


if __name__ == "__main__":
    main()
