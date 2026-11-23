#!/usr/bin/env python3
"""Checks a .deb made by scripts/release.sh linux, using only the standard library.

    python3 scripts/check_deb.py FILE.deb PROGRAM VERSION EPOCH USERNAME

It opens the package the way dpkg would (an ar archive holding debian-binary,
control.tar.* and data.tar.*) and stops with a message if:
  - the members are not the three expected ones, in that order
  - the control fields are wrong (package, version, architecture, maintainer)
    or mention the user name
  - the files are not exactly the expected list, or one is not owned by root,
    has the wrong mode, or has a time other than EPOCH
  - the program inside is not the one that was built (PROGRAM)
  - the README or CHANGELOG inside has a "What to tell me" section, the user
    name or a home folder path
Prints the control file and the file list when everything is right.
"""
import io
import os
import sys
import tarfile

EXPECTED = {
    "usr/bin/s01-monitor": 0o755,
    "usr/share/applications/s01-monitor.desktop": 0o644,
    "usr/share/icons/hicolor/256x256/apps/s01-monitor.png": 0o644,
    "usr/share/doc/s01-monitor/copyright": 0o644,
    "usr/share/doc/s01-monitor/README.txt": 0o644,
    "usr/share/doc/s01-monitor/CHANGELOG.md": 0o644,
    "usr/share/doc/s01-monitor/User-Guide.pdf": 0o644,
}


def fail(message):
    sys.exit(f"STOPPED: {message}")


def read_ar(path):
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"!<arch>\n":
        fail("the file is not an ar archive.")
    members, at = [], 8
    while at < len(data):
        header = data[at:at + 60]
        if len(header) < 60 or header[58:60] != b"`\n":
            fail("the ar archive is damaged.")
        name = header[0:16].decode().strip().rstrip("/")
        size = int(header[48:58].decode())
        members.append((name, data[at + 60:at + 60 + size]))
        at += 60 + size + (size % 2)
    return members


def main():
    if len(sys.argv) != 6:
        sys.exit(__doc__)
    deb, program, version, epoch, user = sys.argv[1:]
    epoch = int(epoch)
    members = read_ar(deb)
    names = [name for name, _ in members]
    if len(names) != 3 or names[0] != "debian-binary" or not names[1].startswith("control.tar") or not names[2].startswith("data.tar"):
        fail(f"the package holds {names}, not debian-binary, control.tar.* and data.tar.*.")
    if members[0][1] != b"2.0\n":
        fail("debian-binary is not 2.0.")

    control_tar = tarfile.open(fileobj=io.BytesIO(members[1][1]))
    control_names = sorted(m.name.lstrip("./") for m in control_tar.getmembers() if m.isfile())
    if control_names not in (["control"], ["control", "md5sums"]):
        fail(f"the control archive holds {control_names}, not control (and md5sums).")
    control = control_tar.extractfile("./control" if "./control" in control_tar.getnames() else "control").read().decode()
    fields = {}
    for line in control.splitlines():
        if line and not line[0].isspace() and ":" in line:
            key, _, value = line.partition(":")
            fields[key] = value.strip()
    want = {"Package": "s01-monitor", "Version": version, "Architecture": "amd64",
            "Maintainer": "S01 Monitor contributors <noreply@example.invalid>"}
    for key, value in want.items():
        if fields.get(key) != value:
            fail(f"control says {key}: {fields.get(key)!r}, not {value!r}.")
    if user.lower() in control.lower() or "/Use" "rs/" in control:
        fail("the control file mentions the user name or a home folder.")

    data_tar = tarfile.open(fileobj=io.BytesIO(members[2][1]))
    files = {}
    for m in data_tar.getmembers():
        path = m.name.lstrip("./")
        if m.isdir():
            if (m.uid, m.gid) != (0, 0) or m.uname not in ("", "root") or m.gname not in ("", "root"):
                fail(f"folder {path} is not owned by root.")
            continue
        if not m.isfile():
            fail(f"{path} is not a plain file.")
        files[path] = m
        if (m.uid, m.gid) != (0, 0) or m.uname not in ("", "root") or m.gname not in ("", "root"):
            fail(f"{path} is not owned by root ({m.uname}:{m.gname}).")
        if m.mtime != epoch:
            fail(f"{path} has time {m.mtime}, not {epoch}.")
    if sorted(files) != sorted(EXPECTED):
        fail(f"the package holds {sorted(files)}, not {sorted(EXPECTED)}.")
    for path, mode in EXPECTED.items():
        if files[path].mode & 0o7777 != mode:
            fail(f"{path} has mode {oct(files[path].mode & 0o7777)}, not {oct(mode)}.")

    def content(path):
        return data_tar.extractfile(files[path]).read()

    with open(program, "rb") as f:
        if content("usr/bin/s01-monitor") != f.read():
            fail("the program in the package is not the one that was built.")
    for path in ("usr/share/doc/s01-monitor/README.txt", "usr/share/doc/s01-monitor/CHANGELOG.md",
                 "usr/share/applications/s01-monitor.desktop", "usr/share/doc/s01-monitor/copyright"):
        text = content(path).decode()
        if "what to tell me" in text.lower():
            fail(f"{path} has a 'What to tell me' section.")
        if user.lower() in text.lower() or "/Use" "rs/" in text:
            fail(f"{path} mentions the user name or a home folder.")
    if b"@VERSION@" in content("usr/share/doc/s01-monitor/README.txt"):
        fail("the README still has its @VERSION@ placeholder.")
    if not content("usr/share/icons/hicolor/256x256/apps/s01-monitor.png").startswith(b"\x89PNG"):
        fail("the icon is not a PNG.")
    if not content("usr/share/doc/s01-monitor/User-Guide.pdf").startswith(b"%PDF"):
        fail("the guide is not a PDF.")

    print(control.rstrip())
    print()
    for path in sorted(files):
        print(f"  {oct(files[path].mode & 0o7777)[2:]}  root:root  {files[path].size:>9}  /{path}")
    print(f"\nPackage OK: {os.path.getsize(deb)} bytes, {len(files)} files.")


if __name__ == "__main__":
    main()
