#!/usr/bin/env python3
"""Turns the raw screenshots from scripts/guide_rig.py into the guide's pictures.

    python3 scripts/guide_images.py [SHOTS_DIR]     (default target/guide-shots)

Each raw picture (for example live-ride.png) is trimmed of the empty space under
the content, optionally cut to a maximum height, shrunk and written to
docs/images. Needs Pillow (python3 -m pip install pillow). The names below are
the ones docs/user-guide.html uses.
"""
import os
import sys

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(os.path.dirname(HERE), "docs", "images")

# raw name -> (picture name, width, most height of the raw picture kept at the top,
#              least height kept, in the finished picture's pixels)
PICTURES = {
    "live-ride": ("live", 1400, None, 0),
    "live-replay": ("live-replay", 1400, None, 0),
    "readings-detail": ("readings", 1400, 680, 0),
    "flags": ("flags", 1400, 1140, 0),
    "frames": ("frames", 1400, None, 0),
    "explore-signals": ("explore-signals", 1400, None, 0),
    "explore-compare": ("explore-compare", 1400, None, 0),
    "parameters": ("parameters", 1400, 760, 0),
    "events": ("events", 1400, None, 404),
    "phone-live": ("phone-live", 520, None, 0),
    "phone-explore": ("phone-explore", 520, None, 0),
    "phone-more": ("phone-more", 520, 430, 0),
}


def trimmed(image, pad=60, least=0):
    rgb = image.convert("RGB")
    width, height = rgb.size
    background = rgb.getpixel((width - 5, height - 5))
    y = height - 1
    while y > 0 and all(rgb.getpixel((x, y)) == background for x in range(max(0, width // 6), width, 29)):
        y -= 1
    return rgb.crop((0, 0, width, min(height, max(y + pad, least))))


def main():
    shots = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(HERE), "target", "guide-shots")
    os.makedirs(OUT, exist_ok=True)
    for raw, (name, width, keep, least) in PICTURES.items():
        path = os.path.join(shots, raw + ".png")
        if not os.path.isfile(path):
            print(f"skipped {name}: no {raw}.png")
            continue
        image = Image.open(path)
        # The phone pictures are already the right shape; the wide ones are 2x.
        shrink = image.width / width if image.width > width else 1
        image = image if raw.startswith("phone-") else trimmed(image, least=round(least * shrink))
        if keep:
            scale = image.width / 1700 if image.width > 1700 else 1
            image = image.crop((0, 0, image.width, min(image.height, round(keep * scale))))
        if image.width > width:
            image = image.resize((width, round(image.height * width / image.width)), Image.LANCZOS)
        image.convert("RGB").save(os.path.join(OUT, name + ".png"), optimize=True)
        print(name, image.size)


main()
