# S01 monitor firmware (M5 ATOM Lite with an Atomic CAN Base or a Unit CAN)

The ESP32 makes its own WiFi network, serves the browser app, and streams the
scooter's CAN bus to it. Decoding happens in the browser.

* WiFi network: `S01-Monitor`, password `silence01` (change both at the top of `main/main.cpp`)
* Page: http://192.168.4.1/
* CAN: 250 kbit/s, normal mode (it acknowledges frames and can send the app's
  read requests and the battery clock message, like the CANable does; every
  other frame is refused). The pins depend on the hardware it is built for:

  | Hardware | `sdkconfig` file | TX | RX |
  | --- | --- | --- | --- |
  | ATOM Lite + Atomic CAN Base (the default) | `sdkconfig.canbase.defaults` | GPIO 22 | GPIO 19 |
  | ATOM Lite + Unit CAN on the Grove port + Atomic TF Card Base (assumed fitted) | `sdkconfig.grovecan-tfbase.defaults` | GPIO 26 (Grove yellow) | GPIO 32 (Grove white) |

  The firmware cannot tell the two apart, so flash the image for the hardware.
  The boot log says which one it is ("Hardware: ..."). The Grove pins are the
  ones the Unit CAN page and an ATOM Lite example use. They were tried on one
  scooter's bus on 23 Nov 2026 and receiving and sending both worked.
* SD card (the `grovecan-tfbase` build only, `CONFIG_S01_HAS_SD`): the card is
  mounted over SPI with MOSI GPIO 19, MISO GPIO 33 and CLK GPIO 23 and no chip
  select (the base ties it to ground), at 10 MHz, FAT, 16 GB or smaller. The
  browser page's SD card screen talks to these endpoints (all plain text;
  everything that changes something is refused unless the request comes from
  the unit's own page):

  | Endpoint | What |
  | --- | --- |
  | `GET /info` | `version`, `hardware`, `sd 0/1` (the page shows the SD screen only if `sd 1`) |
  | `POST /time?local=SECONDS` | sets the unit's clock from the page's local time |
  | `GET /sd/status` | state (`ready`, `nocard`, `nofs`, `toobig`, `ejected`, `formatting`), card, space, the running job, the recording, free heap |
  | `GET /sd/list?path=` | a folder, tab-separated, at most 300 items |
  | `GET /sd/file?path=` | download (runs in the worker task) |
  | `PUT /sd/file?path=` | upload, written as `.part` and renamed when whole |
  | `POST /sd/delete?path=`, `/sd/mkdir?path=` | delete a file or a folder with its contents, make a folder |
  | `POST /sd/format`, `/sd/eject`, `/sd/mount` | format (refused while recording), let go of the card, look again |
  | `POST /sd/mark?text=` | puts a `# MARK` line into the recording being written (409 if none) |
  | `POST /sd/record?on=1/0`, `/sd/auto?on=1/0` | start or stop the recording, start it by itself when there is a card (kept in `nvs`) |

  How it holds together: a manager task owns mounting and notices a lost card
  (no card-detect pin, so it asks the card for its status every 2 s); anything
  using the card is "inside" (a counter) and the manager unmounts only when
  nobody is, after setting an abort flag the long jobs check; downloads,
  uploads, deletes, format and eject run one at a time in a worker task, with
  the request detached from the web server's single task
  (`httpd_req_async_handler_begin`), so they never hold up the live feed; the
  recorder has its own task fed by a 24 KB message buffer from the CAN task
  (which never waits), and writes the app's own capture format (`time:` lines)
  in 4 KB pieces, flushed every second, into `/recordings`. Paths and the
  recorder's text are in `main/sd_paths.h`, tested on the computer by
  `test/sd_paths_test.cpp`. **Tried on a real card and a real bus (23 Nov 2026):** the card was formatted and mounted from the page, and the unit recorded the live bus to it without losing a frame, even at about 585 frames a second with the ECU's debug frames on. Uploads, downloads and deletes have not yet been tried on the real card.
* Image size: about 2.1 MB in a 3 MB app partition (2.3 MB with the SD card code) (4 MB flash, no OTA updates)
* Saved views: the custom views people create on the Live and Readings screens
  are kept on the device (`GET`/`PUT /views`, one small text list of at most
  6000 bytes in a 64 KB `nvs` partition), so they are the same in every
  browser. They survive a reboot and a reflash of the app, but not an erase of
  the whole flash. Changing `partitions.csv` (done when this was added) means
  the next flash must write the partition table too, which `idf.py flash` does.

## Build and flash (once)

1. Install ESP-IDF 5.3 (https://docs.espressif.com/projects/esp-idf/en/v5.3.2/esp32/get-started/):

       git clone -b v5.3.2 --recursive --shallow-submodules https://github.com/espressif/esp-idf.git ~/esp-idf
       ~/esp-idf/install.sh esp32
       . ~/esp-idf/export.sh        # in every new terminal before using idf.py

2. The web app must be built first. `web/dist` already holds a built copy. To
   rebuild after changing the Rust app, run `scripts/build_web.sh` from the
   project folder.

3. Plug the ATOM Lite into the Mac with USB, then (this is the Atomic CAN Base
   version, the default):

       cd firmware
       idf.py set-target esp32
       idf.py -p /dev/cu.usbserial-XXXX -b 115200 flash monitor

   For the Unit CAN on the Grove port (with the TF Card Base), use its own build folder and settings so
   the two builds never mix:

       idf.py -B build-grovecan-tfbase -D SDKCONFIG=build-grovecan-tfbase/sdkconfig \
         -D "SDKCONFIG_DEFAULTS=sdkconfig.defaults;sdkconfig.grovecan-tfbase.defaults" \
         build
       idf.py -B build-grovecan-tfbase -D SDKCONFIG=build-grovecan-tfbase/sdkconfig \
         -p /dev/cu.usbserial-XXXX -b 115200 flash monitor

   `scripts/release.sh esp` (from the project folder) builds both versions and
   makes one package for each, without flashing.

   (`ls /dev/cu.*` shows the port name.) `Ctrl+]` leaves the monitor.

4. Join the `S01-Monitor` WiFi on a phone or laptop, open http://192.168.4.1/,
   press Connect.

## Notes

* The Unit CAN + TF base build has run on an ATOM Lite and on one scooter's bus (23 Nov 2026). The Atomic CAN Base build has been compiled and flashed but has not yet been on a bus.
* If the upload cannot connect, M5 documents a download mode for ATOM boards
  (hold the side button while plugging in USB). Check their ATOM Lite page.
* The Atomic CAN Base has no built-in termination, which is right for a bus
  that is already terminated. Check its power requirements before connecting
  it to the scooter. The same goes for the Unit CAN: leave the loose 120 ohm
  resistor from its box out.
* Phones may warn that the network has no internet; stay connected.
