To run: unzip the folder, then double-click s01-monitor.exe.
Windows 10 or 11, 64-bit. Nothing to install.

User-Guide.pdf in this folder explains every screen, step by step. Start
there. What changed in this version, and in every version before it, is in
CHANGELOG.md (the newest version is first).

This build was made on a Mac with a cross-compiler. An earlier build was tried
on Windows and worked.

Windows may say "Windows protected your PC" because the program is not
signed. Click "More info", then "Run anyway".

If the window does not open or is blank, the graphics driver may not support
OpenGL. Update the graphics driver and try again.

The version number is shown at the bottom of the left menu (or in the "..."
menu in a narrow window).


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
   open in Excel.
4. Saved views: on the Live or Readings screen choose "All", turn items off,
   name the view and press "Save view". It is still there next time. "Delete
   view" removes it (it asks first). They are kept in
   %APPDATA%\Silence S01\views.txt (paste that into the Explorer address bar).
5. Explore screen, with a replayed capture playing: "Signals" reads any bytes
   of any frame as a number and draws them, so you can try a guess and keep it
   if the line follows. "Compare two moments" lists the bytes that changed
   between two markers (press Mark before and after something happens).


Try it with a scooter
---------------------
You need a CANable adapter with slcan firmware. Plug it in, pick its port
(for example COM3) in the box at the top, and press Connect. The app mostly
listens. After it connects it sends one request for the pack ID, and it can switch the scooter's debug mode on, but only when you
use the Debug mode switch on the Frames or Flags screen. It is off every time you
start, so by default the app only listens. When you switch it on, the app sends it
at most twice (and not at all if the scooter already reports it as on), and
switches it off again when you switch it off or disconnect. That is the only
message it sends besides read requests, which it sends when you press a Read
button. The one other thing it can write is the battery's clock,
and only when you press Set battery clock and confirm.
The adapter may need its Windows driver installed first.

Reading the Parameters and the Event log: if the scooter does not answer, a
parameter read asks again twice and then stops, saying the battery or the ECU
stopped answering. An event-log read ends by itself a moment after the last
record. If the connection to the adapter stops unexpectedly, the app
disconnects and says so; press Connect to start again.
