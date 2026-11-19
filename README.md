# S01 Monitor

A live dashboard for the CAN bus of a Silence S01 (SEAT MÓ) electric scooter,
and the notes on what is on that bus.

> **Unofficial.** This project is not made, approved or supported by Silence,
> SEAT or any of their suppliers. It is the result of listening to a bus.

> **Use at your own risk.** The monitor mostly listens. It can send a small
> number of messages (read requests, setting the battery clock when you press
> a button and confirm, and switching the scooter's debug mode on and off when
> you flip a switch). Anything you connect to a vehicle's bus can affect it.
> Read the [user guide](docs/user-guide.html) before you connect anything, and
> do not use this while riding.

## What it is

- **A desktop app** (Windows and macOS) written in Rust with `egui`. It talks
  to the scooter through a CANable USB adapter running slcan firmware.
- **The same app in a browser**, compiled to WebAssembly and served by a small
  ESP32 unit (an M5 ATOM Lite with an Atomic CAN Base, or with a Unit CAN on
  its Grove port) that sits on the scooter and makes its own WiFi network. Open the page on a phone.
- **Live, Readings, Flags, Frames, Explore, Parameters and Event log screens**:
  pack voltage and current, state of charge, the cells, speed and range, the
  on/off signals, every frame on the bus, tools for working out unknown frames,
  the battery and ECU parameters, and the battery's event log.
- **Recording and replay**, with step and scrub controls. Replaying recordings
  made by some other tools (candump, SavvyCAN CSV, Vector ASC) is experimental.
- **[PROTOCOL.md](PROTOCOL.md)** is the byte layout of the frames, and
  **[CAN_NOTES.md](CAN_NOTES.md)** says how well each entry is known.

## Build and run

```sh
cargo run --release -p s01-monitor        # the desktop app
cargo test -p s01-monitor                 # the tests
scripts/build_web.sh                      # the browser app, into web/dist
scripts/test_firmware_filter.sh           # a host test of the ESP32's send filter
```

The browser build needs `rustup target add wasm32-unknown-unknown` and
`cargo install wasm-bindgen-cli` at the version in `Cargo.lock`. The firmware
(`firmware/`) is built with ESP-IDF 5.3; see [firmware/README.md](firmware/README.md).
`scripts/release.sh` builds the Windows and Mac packages and the ESP32 image.

## Safety notes

- The ESP32's firmware has a send filter: it refuses to transmit anything except
  the read requests the app uses, the battery clock frame, and the two debug mode
  frames. Its test is `scripts/test_firmware_filter.sh`.
- Do not terminate the bus a second time. The scooter's bus is already
  terminated.
- Recordings of a real vehicle contain identifying data (such as the VIN and
  the pack ID). Do not publish them without checking them first.

## Changes

See [CHANGELOG.md](CHANGELOG.md). The project is licensed under the MIT licence
(see [LICENSE](LICENSE)).
