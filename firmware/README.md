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
  | ATOM Lite + Unit CAN on the Grove port + Atomic TF Card Base (assumed fitted; the firmware does not use the card yet) | `sdkconfig.grovecan-tfbase.defaults` | GPIO 26 (Grove yellow) | GPIO 32 (Grove white) |

  The firmware cannot tell the two apart, so flash the image for the hardware.
  The boot log says which one it is ("Hardware: ..."). The Grove pins are the
  ones the Unit CAN page and an ATOM Lite example use; they have not been tried
  on a bus.
* Image size: about 2.1 MB in a 3 MB app partition (4 MB flash, no OTA updates)
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

* This has been compiled but not run on hardware. The first run is the test.
* If the upload cannot connect, M5 documents a download mode for ATOM boards
  (hold the side button while plugging in USB). Check their ATOM Lite page.
* The Atomic CAN Base has no built-in termination, which is right for a bus
  that is already terminated. Check its power requirements before connecting
  it to the scooter. The same goes for the Unit CAN: leave the loose 120 ohm
  resistor from its box out.
* Phones may warn that the network has no internet; stay connected.
