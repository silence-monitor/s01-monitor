Ubuntu 22.04 or later (or another Debian-based Linux with glibc 2.35 or
later), 64-bit Intel or AMD (amd64).

To install: open a terminal in the folder that holds the .deb file and run

    sudo apt install ./s01-monitor-@VERSION@-linux-amd64.deb

Then start "Silence S01 monitor" from the applications menu, or run
s01-monitor in a terminal. To remove it: sudo apt remove s01-monitor

This file, User-Guide.pdf (every screen, step by step: start there) and
CHANGELOG.md (what changed in this version and every version before it, newest
first) were installed in /usr/share/doc/s01-monitor/.

This package was made on a Mac with a cross-compiler. It has not yet been run
on Ubuntu, so reports are welcome. It is not in any apt archive and is not
signed; apt will say the file is not from a repository, which is expected.

The version number is shown at the bottom of the left menu (or in the "..."
menu in a narrow window).


If the window does not open
---------------------------
The app draws with OpenGL or Vulkan. A normal Ubuntu desktop has what it
needs. A bare virtual machine may not have graphics acceleration; Mesa's
software drawing works, but slowly. It runs on both X11 and Wayland.

If Open or Save does nothing, the file dialogs need the desktop portal:
install xdg-desktop-portal and the one for your desktop (on Ubuntu with GNOME
that is xdg-desktop-portal-gnome).


Connecting a CANable adapter (read this before you try a scooter)
-----------------------------------------------------------------
The adapter appears as /dev/ttyACM0 (or ttyACM1 and so on) once it is plugged
in. Two things commonly stop it working on Ubuntu:

1. Permission. Ordinary users may not open serial ports until they are in the
   "dialout" group. Run
       sudo usermod -aG dialout $USER
   then log out and in again (a restart is the sure way). Check with the
   command "groups".
2. ModemManager. Ubuntu's modem service sometimes opens a new serial device
   and sends it text, which can confuse the adapter. If the port is listed but
   nothing comes through, or it disconnects at once, try stopping it while you
   use the adapter:
       sudo systemctl stop ModemManager
   (it starts again after a restart). If that is the cause, you can stop it
   from starting at all with: sudo systemctl disable --now ModemManager
   (only do that if you do not use a mobile broadband modem).

Pick the port in the box at the top of the app and press Connect.


Try it with no scooter
----------------------
1. Press Simulate. The dashboard fills with sample data. Press Disconnect to
   stop. (In a narrow window Simulate is in the menu marked "...".)
2. If you have a capture log (the Record button on the Frames screen makes
   one while connected), open the Frames screen, press "Replay a capture..."
   and choose it. It plays the recording back with no scooter. Try Pause, the
   speed box, and the Mark button.
   It can also play files from some other tools (candump, SavvyCAN, Vector
   ASC). That support is experimental; the User Guide says how it works.
3. Try the export buttons: Readings screen, the chart on the Live screen,
   Frames, Flags, Parameters and Event log. Each saves a .csv file you can
   open in a spreadsheet program.
4. Saved views: on the Live or Readings screen choose "All", turn items off,
   name the view and press "Save view". It is still there next time. "Delete
   view" removes it (it asks first). They are kept in
   ~/.config/silence-s01/views.txt
5. Explore screen, with a replayed capture playing: "Signals" reads any bytes
   of any frame as a number and draws them, so you can try a guess and keep it
   if the line follows. "Compare two moments" lists the bytes that changed
   between two markers (press Mark before and after something happens).


Try it with a scooter
---------------------
You need a CANable adapter with slcan firmware, wired to the scooter's CAN
bus. The app mostly listens. After it connects it sends one request for the
pack ID. It can switch the scooter's debug mode on, but only when you use the
Debug mode switch on the Frames or Flags screen. It is off every time you
start, so by default the app only listens. When you switch it on, the app
sends it at most twice (and not at all if the scooter already reports it as
on), and switches it off again when you switch it off or disconnect. That is
the only message it sends besides read requests, which it sends when you press
a Read button. The one other thing it can write is the battery's clock, and
only when you press Set battery clock and confirm.

Reading the Parameters and the Event log: if the scooter does not answer, a
parameter read asks again twice and then stops, saying the battery or the ECU
stopped answering. An event-log read ends by itself a moment after the last
record. If the connection to the adapter stops unexpectedly, the app
disconnects and says so; press Connect to start again.
