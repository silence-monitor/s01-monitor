// Which frames a browser may make this device send onto the scooter's bus.
//
// The browser app sends read requests, one write that a person starts with a
// button and a confirmation (setting the battery clock), and the two debug mode
// frames (debug on, debug off), which the app only sends when a person switches debug mode on. These are all of them:
//   remote frame 0x183            the pack ID request            ("r1838")
//   data frame   0x234, 1 byte    a battery parameter read       (the index)
//   data frame   0x31B, 8 bytes   an ECU parameter read         (the index, then 7 zeros)
//   data frame   0x1ED, 0 bytes   open the event log
//   data frame   0x1ED, 4 bytes   the event log's window of records
//   data frame   0x201, 7 bytes   set the battery clock: 04, then hour, minute,
//                                 second, day, month, year as BCD, each in range
//   data frame   0x28A, 8 bytes   debug mode: 00 00 00 01 00 00 00 00 (on) or
//                                 eight zeros (off). Nothing else on 0x28A: the
//                                 other bits of that frame are god mode (byte 0)
//                                 and the output buttons (bytes 1 and 2).
//
// Everything else is refused here, in the device. Without this, anyone on the
// WiFi network, or a web page that reached the device, could send any frame at
// all to the scooter. A write to the ECU is the same 0x31B frame with a value
// in the bytes after the index, so those bytes must be zero.
//
// Keep this in step with the requests built in s01-monitor/src/stored.rs and
// slcan.rs. The Rust tests check that every request the app can make passes
// this rule, and firmware/test/request_filter_test.cpp checks the rule itself
// (run scripts/test_firmware_filter.sh).
#pragma once

#include <cstdint>

// A BCD byte (two decimal digits) between low and high, inclusive.
inline bool bcd_in_range(uint8_t byte, int low, int high)
{
    const int tens = byte >> 4;
    const int ones = byte & 0x0F;
    if (tens > 9 || ones > 9) {
        return false;
    }
    const int value = tens * 10 + ones;
    return value >= low && value <= high;
}

// The battery clock frame, the "Set current time" frame. Anything that is not a real date and time is refused.
inline bool clock_frame_allowed(int dlc, const uint8_t *data)
{
    return dlc == 7 && data[0] == 0x04 && bcd_in_range(data[1], 0, 23) && bcd_in_range(data[2], 0, 59) &&
           bcd_in_range(data[3], 0, 59) && bcd_in_range(data[4], 1, 31) && bcd_in_range(data[5], 1, 12) &&
           bcd_in_range(data[6], 0, 99);
}

// Debug mode on or off. Any other
// bit set is god mode or an output control, and is refused.
inline bool debug_frame_allowed(int dlc, const uint8_t *data)
{
    if (dlc != 8) {
        return false;
    }
    for (int i = 0; i < 8; ++i) {
        const uint8_t on = (i == 3) ? 0x01 : 0x00;
        if (data[i] != 0x00 && data[i] != on) {
            return false;
        }
    }
    // Only byte 3 may be set, so this is the on frame or the off frame.
    return true;
}

inline bool request_allowed(bool remote, uint32_t id, int dlc, const uint8_t *data)
{
    if (remote) {
        return id == 0x183;
    }
    switch (id) {
    case 0x234:
        return dlc == 1;
    case 0x31B:
        if (dlc != 8) {
            return false;
        }
        for (int i = 1; i < 8; ++i) {
            if (data[i] != 0) {
                return false;
            }
        }
        return true;
    case 0x1ED:
        return dlc == 0 || dlc == 4;
    case 0x201:
        return clock_frame_allowed(dlc, data);
    case 0x28A:
        return debug_frame_allowed(dlc, data);
    default:
        return false;
    }
}
