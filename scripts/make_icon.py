#!/usr/bin/env python3
"""Draws the app icon: a dark rounded square with a green ring and dot.

    python3 scripts/make_icon.py OUT.png [SIZE]      (SIZE defaults to 1024)

Standard library only. The square leaves the usual margin around it, as macOS
app icons do, and the corners are transparent.
"""
import math
import struct
import sys
import zlib

BACKGROUND = (0x10, 0x14, 0x10)
RING = (0x9D, 0xCE, 0x7C)


def clamp(value):
    return max(0.0, min(1.0, value))


def draw(size):
    centre = size / 2
    half = size / 2 - size * 100 / 1024  # the body, with the margin outside it
    radius = half * 0.45
    rows = []
    for y in range(size):
        row = bytearray([0])
        py = y + 0.5 - centre
        for x in range(size):
            px = x + 0.5 - centre
            # Signed distance to a rounded square: negative inside.
            qx = abs(px) - (half - radius)
            qy = abs(py) - (half - radius)
            outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
            distance = outside + min(max(qx, qy), 0.0) - radius
            alpha = clamp(0.5 - distance)
            d = math.hypot(px, py)
            ring = clamp(min(d - 0.51 * half, 0.71 * half - d) + 0.5)
            dot = clamp(0.167 * half - d + 0.5)
            cover = max(ring, dot)
            row += bytes(round(BACKGROUND[i] + (RING[i] - BACKGROUND[i]) * cover) for i in range(3))
            row.append(round(alpha * 255))
        rows.append(bytes(row))
    return b"".join(rows)


def chunk(tag, data):
    body = tag + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    size = int(sys.argv[2]) if len(sys.argv) > 2 else 1024
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(draw(size), 9))
        + chunk(b"IEND", b"")
    )
    with open(sys.argv[1], "wb") as out:
        out.write(png)


main()
