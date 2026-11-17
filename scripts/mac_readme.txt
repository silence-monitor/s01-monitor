To open: unzip the folder, drag "Silence S01.app" to Applications (or leave it
where it is) and double-click it.
Apple silicon Macs only (M1 and later), macOS 11 or later. Nothing to install.

User-Guide.pdf in this folder explains every screen, step by step. Start
there. What changed in this version, and in every version before it, is in
CHANGELOG.md (the newest version is first).

The app is not signed by Apple or notarised, so the first time macOS says it
cannot check it. Right-click (or Control-click) the app, choose Open, then
Open again. If macOS still refuses, open System Settings, Privacy & Security,
scroll down and press "Open Anyway". This is needed only once.

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
   open in Numbers or Excel.
4. Saved views: on the Live or Readings screen choose "All", turn items off,
   name the view and press "Save view". It is still there next time. "Delete
   view" removes it (it asks first). They are kept in
   ~/Library/Application Support/Silence S01/views.txt
5. Explore screen, with a replayed capture playing: "Signals" reads any bytes
   of any frame as a number and draws them, so you can try a guess and keep it
   if the line follows. "Compare two moments" lists the bytes that changed
   between two markers (press Mark before and after something happens).


Try it with a scooter
---------------------
You need a CANable adapter with slcan firmware. Plug it in, pick its port
(it shows up as a name starting /dev/cu.usbmodem) in the box at the top, and
press Connect. The app mostly listens. After it connects it sends one request
for the pack ID. It can switch the scooter's debug
mode on, but only when you use the Debug mode switch on the Frames or Flags
screen. It is off every time you start, so by default the app only listens. When
you switch it on, the app sends it at most twice (and not at all if the scooter
already reports it as on), and switches it off again when you switch it off or
disconnect. That is the only message it sends besides read requests, which it
sends when you press a Read button. The one other thing it can write is the
battery's clock, and only when you press Set battery clock and confirm.

Reading the Parameters and the Event log: if the scooter does not answer, a
parameter read asks again twice and then stops, saying the battery or the ECU
stopped answering. An event-log read ends by itself a moment after the last
record. If the connection to the adapter stops unexpectedly, the app
disconnects and says so; press Connect to start again.
