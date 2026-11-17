"""Writes an invented capture for the user guide: no real scooter data.

    python3 scripts/make_demo_capture.py OUT.log

70 seconds: the key goes on at 3 s, an invented switch (identifier 0x6A1) is on from
22 to 32 s and 44 to 52 s, the scooter rides from 38 to 62 s, and an invented value
(0x6A0) moves all the time. The two invented identifiers are ones no capture of the
scooter has shown, so the guide cannot be read as a claim about a real frame.
"""
import math, struct, random, sys
random.seed(7)
frames = []  # (t_seconds, id, bytes)
def add(t, i, data): frames.append((t, i, bytes(data)))
def u16(v): return struct.pack("<H", int(v) & 0xFFFF)
def i16(v): return struct.pack("<h", int(v))
def u32(v): return struct.pack("<I", int(v) & 0xFFFFFFFF)
def i32(v): return struct.pack("<i", int(v))
T = 70.0
t = 0.0
counter = 0
while t < T:
    key_on = t > 3
    a = max(0.0, t - 38)
    riding = 38 < t < 62
    speed = 0
    if riding:
        speed = 8 + 30 * math.sin((t - 38) / 24 * math.pi) ** 2 + 4 * math.sin(t)
    amps = -(2 + speed * 1.1) if riding else -1.5
    soc = 86 - a / 32 * 2
    volts = 925 - (a / 32) * 14 + amps * 0.22
    if round(t * 20) % 1 == 0:
        mode = 0x21 if key_on else 0x00   # key on, SPORT
        flags = 0x00
        add(t, 0x280, [mode, flags, 0, 0])
        add(t, 0x391, i32(speed * 55) + bytes(4))
    if round(t * 10) % 2 == 0:
        add(t, 0x181, [counter & 0xFF, int(soc), 21, 24] + list(u16(volts)) + list(i16(amps * 10)))
        add(t, 0x189, list(i16(amps * 100)) + list(i16(2310 + a * 6)) + list(i16(2260 + a * 6)) + list(i16(2395 + a * 6)))
        add(t, 0x300, [int(speed), 84 - int(a / 8), 0, 0, 2 if riding else 0])
        add(t, 0x301, list(i16(300 + a * 9)) + list(i16(280 + a * 6)) + list(i16(260)) + [0, 0])
        add(t, 0x297, list(u32(t * 1000 + 40000)) + [0, 0, 0, 0])
        add(t, 0x1AC, list(u32(t * 1000 + 900000)) + [0, 0, 0, 0])
        # an unknown frame: bytes 2-3 are a temperature in 0.1 degrees,
        # byte 4 changes on every frame like a counter
        temp = 250 + 40 * math.sin(t / 9)
        add(t, 0x6A0, [0, 1] + list(i16(temp)) + [counter & 0xFF, 0, 0, 0])
        counter += 1
    if round(t * 10) % 4 == 0:
        # another unknown frame: byte 2 is a switch (two stretches "on")
        on = 22 < t < 32 or 44 < t < 52
        add(t, 0x6A1, [0x01, 0x00, 0x04 if on else 0x00, 0x00, 0x00, 0x00])
        for first, base in ((0x185, 0), (0x186, 4), (0x187, 8), (0x188, 12)):
            cells = [3860 + 8 * ((base + k) % 5) - int(a / 12) for k in range(4 if first != 0x188 else 2)]
            add(t, first, b"".join(u16(c) for c in cells))
    if round(t * 10) % 10 == 0:
        add(t, 0x184, [0x45, 0x82, 0x01, 0x00, 0x04, 0x0A, 0x00, 0x00])
        add(t, 0x190, [0, int(soc * 2), 0x40, 0x0D, int(soc), 96, 0, 0])
        add(t, 0x1AA, list(u16(280)) + list(i16(-80)) + [0, 0, 0, 0])
        add(t, 0x500, [18, 0x58, 0, 0] + list(u32(4812)[:3]) + [0])
        # The battery's clock frame: valid, BCD, and deliberately years behind (18 Jan 2001),
        # so the Battery clock card in the guide shows a difference.
        ticks = 10 * 3600 + 45 * 60 + int(t)
        bcd = lambda v: ((v // 10) << 4) | (v % 10)
        add(t, 0x200, [0x03, bcd(ticks // 3600 % 24), bcd(ticks // 60 % 60), bcd(ticks % 60), 0x18, 0x01, 0x01])
    t = round(t + 0.05, 3)
frames.sort(key=lambda f: f[0])
with open(sys.argv[1] if len(sys.argv) > 1 else "demo.log", "w") as out:
    out.write("# an invented demo capture for the user guide\n")
    for ts, ident, data in frames:
        total = 10 * 3600 + 30 * 60 + ts
        h, rem = divmod(total, 3600); m, s = divmod(rem, 60)
        out.write(f"time:{int(h):02d}:{int(m):02d}:{int(s):02d}.{int(round((s - int(s)) * 1000)):03d} RX: STDID:{ident:05X} data:{' '.join(f'{b:02X}' for b in data)}\n")
print(len(frames), "frames")
