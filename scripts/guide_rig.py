#!/usr/bin/env python3
"""Serves the browser app with invented demo data, to take the user guide's screenshots.

    python3 scripts/guide_rig.py [--port 8099] [--shots DIR]

It makes a demo capture (scripts/make_demo_capture.py), plays it at half speed
through a copy of scripts/web_bridge.py, and serves a copy of web/dist (run
scripts/build_web.sh first). The copy of the page can save its canvas: in the
browser pane, run   await window.saveShot("name")   and the picture (twice the
page's size) is written to DIR/name.png. Then scripts/guide_images.py crops them
into docs/images. Nothing in the project is changed, and no real data is used.
Press Ctrl+C to stop.
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

SNIPPET = """<script>
  // Keep the canvas readable after drawing, so a picture can be taken of it.
  const getContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, options) {
    if (String(type).startsWith("webgl")) options = Object.assign({}, options, { preserveDrawingBuffer: true });
    return getContext.call(this, type, options);
  };
  window.saveShot = async (name) => {
    const canvas = document.getElementById("the_canvas_id");
    const r = await fetch("/save/" + name, { method: "POST", body: canvas.toDataURL("image/png") });
    return canvas.width + "x" + canvas.height + " " + r.status;
  };
</script>
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--port", type=int, default=8099)
    parser.add_argument("--shots", default=os.path.join(ROOT, "target", "guide-shots"))
    args = parser.parse_args()
    dist = os.path.join(ROOT, "web", "dist")
    if not os.path.isfile(os.path.join(dist, "index.html")):
        sys.exit("web/dist is missing: run scripts/build_web.sh first.")
    os.makedirs(args.shots, exist_ok=True)

    work = tempfile.mkdtemp(prefix="s01-guide-rig-")
    try:
        shutil.copytree(dist, os.path.join(work, "dist"))
        page = os.path.join(work, "dist", "index.html")
        html = open(page, encoding="utf-8").read()
        if '<script type="module">' not in html:
            sys.exit("web/dist/index.html has changed; update scripts/guide_rig.py.")
        open(page, "w", encoding="utf-8").write(html.replace('<script type="module">', SNIPPET + '<script type="module">', 1))

        bridge = open(os.path.join(HERE, "web_bridge.py"), encoding="utf-8").read()
        old_dist = 'DIST = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "web", "dist")'
        old_views = '        elif path.split("?", 1)[0] == "/views":'
        if old_dist not in bridge or old_views not in bridge:
            sys.exit("scripts/web_bridge.py has changed; update scripts/guide_rig.py.")
        bridge = bridge.replace(old_dist, 'DIST = os.path.join(os.path.dirname(os.path.abspath(__file__)), "dist")')
        save = f'''        elif method == "POST" and path.startswith("/save/"):
            length = int(headers.get("content-length", "0") or 0)
            body = (await reader.readexactly(length)).decode()
            name = re.sub(r"[^A-Za-z0-9_-]", "", path[len("/save/"):])
            data = base64.b64decode(body.split(",", 1)[1])
            open(os.path.join({args.shots!r}, name + ".png"), "wb").write(data)
            print("saved", name, flush=True)
            reply(writer, "204 No Content")
            await writer.drain()
'''
        bridge = bridge.replace(old_views, save + old_views, 1)
        open(os.path.join(work, "bridge.py"), "w", encoding="utf-8").write(bridge)

        demo = os.path.join(work, "demo.log")
        subprocess.run([sys.executable, os.path.join(HERE, "make_demo_capture.py"), demo], check=True)
        print(f"Open http://127.0.0.1:{args.port}  (pictures go to {args.shots})", flush=True)
        subprocess.run(
            [sys.executable, os.path.join(work, "bridge.py"), "--replay", demo, "--speed", "0.5", "--port", str(args.port)]
        )
    except KeyboardInterrupt:
        pass
    finally:
        shutil.rmtree(work, ignore_errors=True)


main()
