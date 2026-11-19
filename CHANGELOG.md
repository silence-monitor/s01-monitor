# Changelog

What changed in each version of the Silence S01 monitor, in plain words. The
newest version is first. The version number lives in one place, `version` under
`[workspace.package]` in the top-level `Cargo.toml`. The app, the browser page
and the ESP32 firmware all show that same number.

Every change that alters what the app, the firmware or the scripts do gets a
new version and an entry here, in the same commit. A new feature raises the
middle number (0.3.0). A fix raises the last one (0.2.2). `cargo test` fails
if the newest entry here does not match the version in `Cargo.toml`.

## 0.12.1 (19 Nov 2026)

**Changed**
- The ESP32 package for the Unit CAN on the Grove port is now called
  `s01-monitor-<version>-esp-grovecan-tfbase`, because that version assumes an
  Atomic TF Card Base is fitted under the ATOM Lite. Its settings file is
  `firmware/sdkconfig.grovecan-tfbase.defaults`, its README says so, and its
  boot log reads "Hardware: M5 ATOM Lite + Unit CAN (Grove) + TF Card Base".
  Nothing else changed: the CAN pins are the same (GPIO 26 and 32), and the
  firmware still does not use the card.

## 0.12.0 (19 Nov 2026)

**Added**
- **The ESP32 firmware now supports a second piece of hardware: an M5 ATOM Lite
  with a Unit CAN on its Grove port.** An Atomic TF Card Base may be fitted
  under the ATOM Lite; the firmware does not use the card yet. The Unit CAN
  uses GPIO 26 for transmit (the Grove cable's yellow wire) and GPIO 32 for
  receive (white). These pins come from the M5 documentation and an ATOM Lite
  example and have **not been tried on a bus yet**. The first version, an ATOM
  Lite with the Atomic CAN Base (GPIO 22 and 19), is unchanged. The firmware
  cannot tell the two apart, so each has its own image. The hardware is chosen
  when the firmware is built (`firmware/main/Kconfig.projbuild` and the two
  `sdkconfig.*.defaults` files), and the boot log says which one it is
  ("Hardware: ...").

**Changed**
- `scripts/release.sh esp` now makes two ESP32 packages, each a folder and a
  zip: `s01-monitor-<version>-esp-canbase` (ATOM Lite + Atomic CAN Base) and
  `s01-monitor-<version>-esp-grovecan` (ATOM Lite + Unit CAN on the Grove
  port). Each README says which hardware it is for. The script checks that an
  image names its own hardware and not the other one. These builds use their own
  folders under `firmware/build-release/`, so they no longer touch the build
  folder used by a person's own `idf.py`.

## 0.11.3 (17 Nov 2026)

**Changed**
- `scripts/release.sh esp` now also makes a zip of the ESP32 folder,
  `s01-monitor-<version>-esp.zip`, like the Windows and Mac packages. It checks
  what is in the zip and that the three images in it are the ones that were
  built, and prints the zip's SHA-256.

## 0.11.2 (17 Nov 2026)

**Fixed**
- The browser page, and so the ESP32 image that carries it, no longer contains
  the name of the folder it was built in or the home folder of whoever built
  it. `scripts/build_web.sh` now replaces both in the paths the page remembers,
  and `scripts/release.sh esp` looks inside the compressed page and stops if
  either is still there.

## 0.11.1 (17 Nov 2026)

**Fixed**
- The Windows program no longer contains the name of the folder it was built in.
  The build now replaces the project folder as well as the home folder in the
  paths a program remembers, and `scripts/release.sh` stops if either is still
  in the finished program.

## 0.11.0 (17 Nov 2026)

**Added (experimental)**
- **Replay recordings made by other software.** A user asked for it. **Replay a
  capture...** now also reads these text formats, recognised from the first
  lines of the file, with nothing to choose. This is **experimental support**: it
  has only been tried on small files written from each format's documentation
  (not on real files from those tools), the formats have variants, and the app
  says so on the replay bar ("Experimental: read as ...").
  - **candump log**, from `candump -L` (SocketCAN, python-can):
    `(1731234567.123456) can0 280#31000000`
  - **candump, classic layout**, from plain `candump` with time stamps
    (`candump -ta`): `(1731234567.123456)  can0  280   [4]  31 00 00 00`. A
    line may carry `RX` or `TX` before the identifier. A log with no time
    stamps is refused with advice, since it cannot be replayed at its own pace.
  - **SavvyCAN CSV**: a header `Time Stamp,ID,Extended,Dir,Bus,LEN,D1,...,D8`,
    then one row per frame. The time is whole microseconds, or seconds if it
    has a decimal point. Columns are found by name, so their order does not
    matter.
  - **Vector ASC**, from CANalyzer, CANoe and many loggers:
    `   0.012345 1  280  Rx   d 4 31 00 00 00`. The `base hex` or `base dec`
    header line is honoured.
- Only ordinary 11-bit data frames are used. These are left out, counted and
  named on the replay bar: 29-bit identifiers, remote, error and CAN FD frames,
  frames the logger sent itself (`Tx`), frames on a channel other than the
  busiest (which is the one used, and named), and lines that look like frames
  but cannot be read.
- The file box now offers `.asc` and `.csv` as well as `.log` and `.txt`.
- **Not supported yet:** PEAK PCAN-View `.trc`, Vector `.blf`, ASAM `.mf4`, CAN
  FD, and saving the app's recordings in these formats.
- The app's own recordings and the CANable dump logs are read exactly as
  before, and are not marked experimental.

## 0.10.0 (16 Nov 2026)

**Changed**
- **A recording made in a browser is now saved in the browser's own storage as
  it goes**, in pieces of a few seconds, instead of piling up in the page's
  memory. A long recording no longer grows the page's memory, and one hour no
  longer risks a phone killing the tab. Pressing **Stop recording** builds the
  file from the stored pieces and downloads it as before. A very large
  recording (over about 64 MB) downloads as several files, named `-part1`,
  `-part2` and so on, each carrying a note of where it continues from.
- If the browser will not store the recording (private mode, no room), the
  Frames screen says so in amber and the rest of the recording is kept in memory,
  so it is still saved. A browser with no storage at all keeps it all in memory
  as before.

**Added**
- **Recovery.** If the page closes or the browser kills it while recording, the
  next time the page opens it shows, on every screen, "A recording was left in
  this browser when the page closed" with **Download it** and **Delete it**. The
  last few seconds before the page closed may be missing.
- The desktop app is unchanged: it writes to the file as frames arrive.
- The ESP32 needs reflashing for the page it serves to do this.

## 0.9.0 (16 Nov 2026)

**Changed**
- **Debug mode is now a manual switch, off at every start.** Since 0.8.0 the app
  switched the scooter's ECU debug mode on by itself when the scooter woke up. A
  user asked for the opposite, so that a scooter can be recorded unmodified, for
  example on long rides: the app now only listens until someone switches debug
  mode on. A **Debug mode** switch sits on the Frames screen, under Record, and
  on the Flags screen. Switched on, it does what 0.8.2 did: it looks first for
  the ECU's own report, sends debug on only if needed, resends once, and says on
  the screen what happened; it sends debug on again at each wake-up while the
  switch stays on. Switched off, it sends debug off if the app had sent debug on.
  The app also switches it off when you disconnect, and the switch is off again
  at the next connection. Nothing is remembered between runs.

**Added**
- A hint beside the switch: with it off the app only listens, and the ECU's
  inputs, outputs and status have only been seen with debug mode on, and not on
  every scooter.
- On the Flags screen, a group whose frames have not arrived says "No frames
  yet" (for the ECU groups, with the debug mode hint), instead of showing flags
  that look clear.

## 0.8.2 (13 Nov 2026)

**Changed**
- **Debug mode is now checked, not just sent.** When the bus wakes, the app
  first looks for a short moment to see whether the ECU already reports debug
  mode (`0x28A` byte 3 bit 0). If it does, the app sends nothing, and it does
  not switch debug mode off at the end, since it did not switch it on. If it
  does not, the app sends debug on, waits a second for the ECU to report it, and
  sends once more if it still does not. It never sends more than twice, and then
  says on the Frames screen that the scooter did not report it as on (some
  scooters may not). Debug off is still sent on disconnect, only if the app sent
  debug on, and never to a bus that has gone quiet. The Frames screen now says
  which of these happened.
- On a scooter that never reports debug mode (such as the one used for the first tests), the
  app now sends debug on twice per wake-up instead of once, and the first send
  comes about 0.15 s after the bus wakes instead of at the first frame.

## 0.8.1 (13 Nov 2026)

**Fixed**
- **White L and White R were the wrong way round** in the ECU Outputs flags. A
  recording showed it: with the left indicator on,
  the lamp bit labelled White R goes out, and with the
  right indicator on, the one labelled White L goes out. The app now names them
  as seen (`0x294` byte 3: bit 3 is White R, bit 4 is White L).

## 0.8.0 (12 Nov 2026)

**Added**
- **Debug mode.** The app now switches the
  scooter's ECU debug mode on when the bus wakes (the first frame after more
  than a second of silence, and again after each sleep), and off when you
  disconnect or close the app. It is a single `0x28A` frame (byte 3 = 1 for on, all zeros for off) and the app sends nothing
  else in that message: never the god-mode bit or the output buttons. Debug off
  is not sent to a bus that has gone quiet. The Frames screen says "Debug mode
  is on" while it is on. The reason for doing it: the ECU's flag frames (`0x28A`, `0x293`, `0x294`) have never been seen without
  it. One earlier test did not make them appear, so it may change nothing.
- The ESP32's send filter now lets through exactly those two `0x28A` frames and
  still refuses every other frame. A browser using the ESP needs the unit
  reflashed before it can do this.

**Changed**
- The user guide and the READMEs now say that the app sends debug mode, and that
  it can write the battery clock. They said it wrote nothing.
- The detail text for the **FS1**, **DM1**, **DM2** and **DM3** flags is reworded:
  FS1 is a switch input on the motor controller that came on with both forward
  and reverse, and DM1 to DM3 are drive-mode bits, which is a guess as to their
  purpose (DM3 was on whenever reverse was selected).

## 0.7.2 (11 Nov 2026)

**Changed**
- The **INH** flag (Flags screen, and the Live and Readings detail panels) now
  has a meaning instead of "No written definition". It is described as an error condition, caused by faults such as overtemperature or a hard
  shock. While it is set, the high-current output (K_plus) must not be switched
  on and charging is blocked. The letters probably stand for "inhibit", which is a guess and is not shown in the app.

## 0.7.1 (10 Nov 2026)

**Changed**
- The user guide's pictures of the Live screen (computer and phone) are retaken,
  so they show the new median summary above the cell bars.
- The guide has a new picture of the replay bar, in the Record and replay
  section. All its pictures use invented demo data only.

## 0.7.0 (10 Nov 2026)

**Added**
- **Replay controls on every screen.** While a capture is replaying, a bar under
  the header on every screen (Live, Readings, Flags, Frames, Explore, Parameters,
  Event log, and on a phone too) has **Play / Pause**, **<** and **>** to step back
  and forward, a **step size** (10 ms, 100 ms, 1 s, 10 s or 1 minute), the speed,
  **Restart**, **Stop replay**, and a **slider** you can drag to any moment. The
  arrow keys step and Space plays or pauses. Stepping pauses, so you can stop on
  a short moment, such as a burst of full power, and read the cells and every
  other screen at exactly that moment. A user asked for this.
- Going backwards works: the app rebuilds what it knows from the start of the
  capture up to the chosen moment, so the readings, charts, signals and markers
  all match that moment. Signals you made and markers you dropped yourself stay.
- Two more slow speeds, 0.1x and 0.25x.

**Changed**
- The replay buttons that used to be on the Frames screen (Pause, Restart, speed,
  Stop) moved into the bar. Frames still has **Replay a capture...**.
- Jumping is switched off while a recording or a values log is being written,
  because it would write lines twice or miss some. Play, pause and speed still
  work, and the bar says so.

## 0.6.0 (10 Nov 2026)

**Changed**
- The **Cells** card now measures from the **median** of the cells instead of
  the average, so one cell that is far off no longer drags the baseline and
  makes its healthy neighbours look wrong. With an even number of cells the
  median is the average of the two middle ones. A user asked for this.
- The card's summary now names the highest and the lowest cell against the
  median, each with its cell number, for example
  `+50 mV (cell 3)   -20 mV (cell 2)   median 3986 mV`. It used to show the
  total spread and the average. The spread (highest minus lowest) is still
  there: hover over the summary on a computer, or click any bar, and the detail
  panel gives that cell's distance from the median and the pack's spread.
- The bars are drawn around the median (80 mV either side), and a bar turns
  amber when it is more than 30 mV from the median, not from the average. The
  30 mV rule is unchanged, so a different set of cells can turn amber.
- On a phone the summary has its own line under the title and wraps.

## 0.5.0 (10 Nov 2026)

**Added**
- A **Battery clock** card at the top of the Parameters screen. It shows the
  battery's date and time next to this device's, and how far apart they are
  (amber when it is more than a minute). A **Set battery clock...** button sets
  the battery's clock to this device's own date and time, a computer's or a
  phone's, in its own time zone, so daylight saving needs no special handling.
  It asks first, showing the time it will set, takes the time again at the
  moment you confirm, and afterwards watches the battery's clock for three
  seconds and says whether it now matches. A user asked for this: their
  battery clock runs about 5 minutes fast a month, and ours lost its date
  altogether (January 2001) after a restart.
- **This is the first and only thing the app writes to the scooter.** It is one
  `0x201` message, the "Set current time" frame. It is never done by itself: only when you press the button and confirm.
  The button is off with no live connection, while a read runs, and for ten
  seconds after a set. The battery records each set in its event log.
- The app now reads the battery's date from its clock frame, not only the time.

**Changed**
- ESP32 firmware: its send filter now passes exactly one more frame, the clock
  set (`0x201`, seven bytes, `04` then a real hour, minute, second, day, month
  and year in BCD), and still refuses everything else. **The ESP32 needs
  reflashing for the browser path to set the clock.** Until then it refuses the
  frame and the card says the clock still does not match. The host test of the
  filter now makes 22,893 checks.
- The local time comes from the device itself, with no new library: the C
  library's `localtime_r` on a Mac or Linux computer, `GetLocalTime` on
  Windows, and the page's own date in a browser.
- The user guide describes the card and the confirmation, says clearly that the
  clock is the one thing the app writes, and has a new Parameters picture.

## 0.4.4 (10 Nov 2026)

**Fixed**
- The **Display message** reading showed only eight characters of the text the
  dash shows, so "SIDESTAND" appeared as "SIDESTAN" and "Kplus OFF" as
  "Kplus OF". The ninth character is carried in the high bit of each of the
  eight bytes (a user pointed to a report of it, and two recordings confirm it). The reading now shows all nine, as the dash does.

## 0.4.3 (9 Nov 2026)

**Added**
- The Event log screen has a **No.** column: the record's number in the
  battery's log. The newest record has the highest number, so the order on the
  screen never depends on the dates.
- **Save Logs** and **Export CSV** now end each record with its record number and
  the raw bytes of its three frames. The first four columns (time, code, data,
  text) are where they were. A saved log can now be checked against the bus, for
  example to see exactly what a battery sends for a date that looks wrong.
  (The CSV's Origin column is now last, after the two new ones.)

**Changed**
- The user guide mentions both, and asks for the saved file when something
  about the log looks wrong.

## 0.4.2 (9 Nov 2026)

**Changed**
- `scripts/event_log_test.py` (the tool that steps through an event-log read on
  the real bus): the last page of a paged read now asks for only the records
  that remain. Asking for more made the battery wrap round and send eight extra
  records numbered 4095 down to 4088.
- The same script has a new `setclock` test that sets the battery clock to the
  computer's date and time. It is the one thing the script can write: it sends a
  single `0x201` clock frame, only in that test, only in that exact form, and
  only when both `--send` and `--write-clock` are given. The app itself never
  sends it.
- Notes: what the clock and the event log showed on 9 Nov 2026 (three full log
  dumps, the clock history, the display setting the time, and the clock being
  set). A November date set over CAN is stored correctly on our battery, so the
  month problem a user reported belongs to their battery or its firmware.

## 0.4.1 (9 Nov 2026)

**Fixed**
- Reading the event log could make the motor controller report a fault and shut
  down until the scooter was switched off and on again. A user reported it, and
  we reproduced it on our own scooter: the app asked the battery for the whole
  log in one request, and the battery answered with about 900 frames a second.
  The app now reads the log **ten records at a time**, with a one-second pause
  between each ten (about 20 seconds for 160 records), and stops at the oldest
  record. Reading the whole log this way never upset the controller in our
  test. If the controller reports a new fault during the read anyway, the read
  stops at once, keeps what it has, and says so.
- A lost frame during an event-log read could mix up every record after it. The
  read now checks that the records come in order and stops if one does not.

**Changed**
- The Event log screen says how the log is read, and the user guide says the
  same and no longer calls the read harmless. The Windows and Mac packages need
  rebuilding to carry this, and the ESP32's page needs reflashing.
- `scripts/event_log_test.py`, a new tool, steps through an event-log read on
  the real bus one test at a time and watches the controller's fault bytes.
- Notes: `PROTOCOL.md` now says what a four-byte `0x1ED` request means (a start
  index and a count) and that the log's dates run year, month, day. The reasons
  and the log files are in `CAN_NOTES.md`.

## 0.4.0 (6 Nov 2026)

**Added**
- A **user guide** (`User-Guide.pdf`, 18 pages) that explains every screen step
  by step: getting started, the window, Live, Readings, Flags, Frames, Explore,
  Parameters, the Event log, saved views, the phone layout, a worked example of
  finding what an unknown frame means, troubleshooting and a short glossary. The
  Windows and Mac packages carry it, and their READMEs point to it. Its
  pictures are made from invented demo data, never from a real scooter.
- The guide is written in `docs/user-guide.html` and turned into a PDF by
  `scripts/build_guide.sh` (it uses Google Chrome in headless mode; nothing to
  install). `scripts/make_demo_capture.py`, `scripts/guide_rig.py` and
  `scripts/guide_images.py` make the demo data and the pictures again when a
  screen changes.

**Changed**
- `scripts/release.sh` builds the guide for the Windows and Mac packages and
  checks it is in the zip along with everything else that should be.

## 0.3.1 (6 Nov 2026)

**Changed**
- The release script now builds all three packages and is called
  `scripts/release.sh` (it replaces `scripts/build_windows.sh`). Say what to
  build: `windows`, `mac`, `esp` or `all`. Every package gets the same checks:
  tests, version and changelog agree, a clean tree, no user name or path in
  the programs, only the expected files in the package, and no overwriting of a
  package of the same version.
  - `mac` makes `Silence S01.app` (Apple silicon, with an icon and an ad-hoc
    signature, not Apple-approved so macOS asks on first open) in a zip with a
    README and the changelog.
  - `esp` builds the browser page and the firmware, checks the image fits and
    carries the version, and copies the three `.bin` files, the flash settings,
    checksums, a README and the changelog into `dist-esp/`. It never flashes.
- `scripts/make_icon.py` draws the app icon.

## 0.3.0 (6 Nov 2026)

**Added**
- A new **Explore** screen (in the left menu, and under More on a phone) with
  two tools for working out what an unknown frame means.
  - **Signals:** read any bytes of any frame as a number (pick the identifier,
    the first byte, how many bytes, signed or not, which end is biggest, a
    scale and an offset) and watch it draw live. The bytes being read are
    highlighted in the frame. A guess can be tried before it is named; "Save
    signal" keeps it (up to 8), and "Delete" removes it after asking. Saved
    signals are kept with the saved views: in a file on a computer, in the
    device's flash for the browser page.
  - **Compare two moments:** every marker now remembers what each identifier
    was saying when it was dropped. Pick two markers and the screen lists the
    bytes that differ, with both values and which bits changed. Bytes that
    change on most frames (counters, clocks) are left out unless you untick the
    box. "Save as CSV" exports the result. Markers can be dropped from this
    screen too.
- `scripts/build_windows.sh` (replaced in 0.3.1 by `scripts/release.sh`) builds the Windows zip and checks the project's
  rules as it goes (see the notes): tests, matching version and changelog, no
  user name or path in the exe, the zip holding only the exe, README and
  CHANGELOG, and no overwriting of an existing version.
- `scripts/test_firmware_filter.sh` tests the firmware's send filter on the
  computer, and a test in the app checks that every request the app can send
  gets through it.

**Changed**
- Firmware (takes effect the next time the ESP32 is flashed): it now sends on
  the CAN bus only the read requests the app uses (the pack ID, the battery
  and ECU parameter reads and the event-log reads) and refuses everything
  else a WebSocket client sends, including writes to the ECU. It also refuses a
  WebSocket connection from a page that was not served by the device itself.
- The saved-views file may now also hold lines for saved signals. An older
  version of the app skips them, but would drop them if it changed the views.

## 0.2.3 (6 Nov 2026)

**Fixed**
- The app now notices when its live feed stops on its own, for example after
  an internal error. It disconnects and says "The connection stopped
  unexpectedly". Before, the window stayed on "Connected" over numbers that no
  longer moved. (On a computer only: in a browser a crash stops the whole page.)
- A damaged line from the adapter that names an impossible CAN identifier
  (above 0x7FF) is now ignored, so it can no longer add a phantom identifier to
  the Frames list.

## 0.2.2 (5 Nov 2026)

**Fixed**
- On the Live screen the "Set flags" card now stretches the full width, so its
  right edge lines up with the Cells and Stream cards instead of stopping
  short.

## 0.2.1 (5 Nov 2026)

**Fixed**
- Reading the event log never finished. The app stayed on "Reading the event
  log", and "Read from Battery" and "Read from ECU" then did nothing until you
  reconnected. An event-log read now ends once the scooter stops sending
  records, and a Read button pressed while another read is running says so.
- A parameter read that lost a reply waited for ever. It now asks again twice
  and then stops, showing what it has and saying that the battery or the ECU
  stopped answering.
- A noisy serial line could crash the app, and in a browser the whole page.
  Lines like that are now ignored.
- The ESP32 could keep serving an old page to a browser that already had a
  copy, because the page's cache tag came from when the firmware source was
  compiled. The tag is now made from the page files themselves, so a new page
  is picked up after a flash.
- A browser that had stopped reading the live feed (a phone with a locked
  screen, say) could hold up the feed to every other browser for up to 10
  seconds. The wait is now 1 second, and the stalled browser is dropped.
- The history CSV could show a nonsense elapsed time if the computer's clock was
  set back during the session.
- `web_bridge.py --serial` lost data when the serial line was noisy.

**Changed**
- Removed code nothing used (a no-op formatting choice in `readings.rs`, and
  `flag_text` outside the tests). Nothing on screen changes.

## 0.2.0 (5 Nov 2026)

First numbered version, and the first one kept in git.

**Added**
- Saved views. On the Live and Readings screens, choose "All", turn items off,
  name the view and press "Save view". It is still there next time. "Delete
  view" removes it after a confirmation. On a computer the views are a file
  (`~/Library/Application Support/Silence S01/views.txt` on a Mac,
  `%APPDATA%\Silence S01\views.txt` on Windows). In a browser they are kept on
  the ESP32, so every phone and browser sees the same ones.
- The left menu collapses to a small arrow and opens again.
- A phone layout for narrow windows (under 600 points wide): a tab bar along
  the bottom, a slim header with a "..." menu, view chips, a shorter chart,
  the cell bars in two rows, Parameters one side at a time, and Frames as
  cards. At 600 to 900 points the menu starts collapsed. Wider than that is
  the layout as before.
- The window can now be made much smaller (down to 360 x 520).
- The browser page can be added to an iPhone's Home Screen and then opens
  without Safari's address bar. It keeps clear of the camera notch.
- The version number is shown in the app (bottom of the left menu, or in the
  "..." menu on a phone) and in the firmware's boot log.
- A Windows build, made on the Mac.

**Changed**
- The ESP32 now has a larger storage area for settings, so the next firmware
  flash also writes a new partition table.
- `web_bridge.py` also serves the saved views, so the browser page can be tried
  without the ESP32.

**Fixed**
- The "No stream" hint on the empty Live screen ran off the edge of a narrow
  window.

**Not checked yet**
- Readings, Flags, the Event log results table and the Parameters changes list
  on a phone-sized screen.
- The firmware has not been connected to the scooter's CAN bus.

## 0.1.0 (up to 2 Nov 2026, before git)

The app as it was before versioning began, written up from the project notes.

- A desktop dashboard (Rust, egui) with Live, Readings, Flags, Frames,
  Parameters and Event log screens.
- Live data from a CANable adapter, a built-in simulation, or a replay of a
  recorded capture. Recording, markers, value logging and CSV export.
- Parameter and event-log reads from the battery and the ECU (read requests
  only).
- A browser version (WebAssembly) of the same app.
- Firmware for an M5 ATOM Lite with an Atomic CAN Base: it makes a WiFi
  network, serves the browser app and streams the CAN bus to it.
- A Windows test build, made on 1 Nov 2026.
