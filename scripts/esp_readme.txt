This folder is a finished firmware image for the M5 ATOM Lite (ESP32). The
monitor web page is inside it. It makes the WiFi network
"S01-Monitor" and serves the page at http://192.168.4.1/. It listens on the
CAN bus at 250 kbit/s and sends only what the app needs: its read requests, the
message that sets the battery clock (a person presses a button to send it), and
the two messages that switch the scooter's debug mode on and off (only when a person switches debug mode on in the app,
and off again when they switch it off or the app disconnects). Every other message is refused by the unit itself.

@BOARD@

Connecting to it
----------------
WiFi network:   S01-Monitor
WiFi password:  silence01
Page:           http://192.168.4.1/

Join the network from a phone or computer, then open the page in a browser. The
network has no internet, so a phone may ask whether to stay connected; say yes.

What changed in this version, and in every version before it, is in
CHANGELOG.md in this folder (the newest version is first).

Files
-----
bootloader.bin         written at 0x1000
partition-table.bin    written at 0x8000
s01-monitor-esp32.bin  the firmware and the web page, written at 0x20000
flash_args             the three offsets and the flash settings, for esptool
SHA256SUMS             check the three .bin files with: shasum -a 256 -c SHA256SUMS

To put it on the device
-----------------------
Nothing here flashes by itself. When you want this build on the ESP32:

1. Close anything that has the serial port open (an idf.py monitor, a serial
   terminal), then find the port, for example: ls /dev/cu.usbserial-*
2. In a terminal opened in THIS folder, with the ESP-IDF tools loaded
   (. ~/esp-idf/export.sh):

   esptool.py --chip esp32 -b 115200 --port PORT --before default_reset --after hard_reset write_flash @flash_args

   Replace PORT with the port from step 1. It takes about 3 minutes at 115200.

The saved views live in a separate part of the flash (the nvs partition at
0x9000) that this does not touch, so they survive.


Without the ESP-IDF tools: ESP Launchpad (any computer, including Windows)
-------------------------------------------------------------------------
Espressif's ESP Launchpad flashes from a web page, with nothing to install:
https://espressif.github.io/esp-launchpad/

1. Use Google Chrome or Microsoft Edge (it needs "Web Serial"; Firefox and
   Safari do not have it). Close anything else that has the unit's serial
   port open.
2. Plug the unit in with a USB cable. On Windows it may need the USB-serial
   driver for the unit before a COM port appears (see Device Manager).
3. On the page, choose the "DIY" mode (your own firmware files, from this
   computer), press Connect and pick the unit's port.
4. Add the three files from this folder, each at its address:
       bootloader.bin         at 0x1000
       partition-table.bin    at 0x8000
       s01-monitor-esp32.bin  at 0x20000
5. Press Flash and wait; it takes a few minutes. Press the unit's reset
   button or unplug and replug it afterwards.

The same files, the same addresses and the same saved views as above. This
route has not been tried with these files yet, so if a step on the page looks
different, go by the addresses.

This image has not been put on the scooter's CAN bus by this script. The
version is printed in the boot log ("App version").
