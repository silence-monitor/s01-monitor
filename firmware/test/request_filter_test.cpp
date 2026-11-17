// Host test for firmware/main/request_filter.h. It runs on the computer, with
// no ESP32: scripts/test_firmware_filter.sh builds and runs it.

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <initializer_list>

#include "../main/request_filter.h"

static int g_failures = 0;
static int g_checks = 0;

static void expect(bool condition, const char *what, unsigned id, int dlc)
{
    ++g_checks;
    if (!condition) {
        ++g_failures;
        std::printf("FAIL: %s (id 0x%03X, dlc %d)\n", what, id, dlc);
    }
}

int main()
{
    uint8_t data[8] = {0};

    // The pack ID request, as the app sends it.
    expect(request_allowed(true, 0x183, 8, data), "pack id request allowed", 0x183, 8);
    // No other remote frame.
    for (uint32_t id = 0; id <= 0x7FF; ++id) {
        if (id != 0x183) {
            expect(!request_allowed(true, id, 8, data), "other remote frame refused", id, 8);
        }
    }

    // Battery reads: every index, one byte.
    for (int index = 0; index < 256; ++index) {
        data[0] = static_cast<uint8_t>(index);
        expect(request_allowed(false, 0x234, 1, data), "battery read allowed", 0x234, 1);
    }
    // A battery frame with more bytes is not a read.
    for (int dlc = 0; dlc <= 8; ++dlc) {
        if (dlc != 1) {
            std::memset(data, 0, sizeof data);
            expect(!request_allowed(false, 0x234, dlc, data), "battery frame of another length refused", 0x234, dlc);
        }
    }

    // ECU reads: every index followed by zeros.
    for (int index = 0; index < 256; ++index) {
        std::memset(data, 0, sizeof data);
        data[0] = static_cast<uint8_t>(index);
        expect(request_allowed(false, 0x31B, 8, data), "ECU read allowed", 0x31B, 8);
    }
    // The same frame with a value after the index is a write, and is refused.
    for (int position = 1; position < 8; ++position) {
        for (int value = 1; value < 256; value += 17) {
            std::memset(data, 0, sizeof data);
            data[0] = 0x05;
            data[position] = static_cast<uint8_t>(value);
            expect(!request_allowed(false, 0x31B, 8, data), "ECU write refused", 0x31B, 8);
        }
    }
    // An ECU frame of another length is refused.
    for (int dlc = 0; dlc <= 8; ++dlc) {
        if (dlc != 8) {
            std::memset(data, 0, sizeof data);
            expect(!request_allowed(false, 0x31B, dlc, data), "ECU frame of another length refused", 0x31B, dlc);
        }
    }

    // The event log: open (no data) and the window (four bytes).
    expect(request_allowed(false, 0x1ED, 0, data), "event open allowed", 0x1ED, 0);
    std::memset(data, 0xFF, sizeof data);
    expect(request_allowed(false, 0x1ED, 4, data), "event window allowed", 0x1ED, 4);
    for (int dlc : {1, 2, 3, 5, 6, 7, 8}) {
        expect(!request_allowed(false, 0x1ED, dlc, data), "event frame of another length refused", 0x1ED, dlc);
    }

    // The battery clock: the one write. 04, hour, minute, second, day, month, year.
    {
        // 22:28:03 on 09/11/26, the frame sent on the scooter on 9 Nov 2026.
        const uint8_t good[7] = {0x04, 0x22, 0x28, 0x03, 0x09, 0x11, 0x26};
        expect(request_allowed(false, 0x201, 7, good), "clock frame allowed", 0x201, 7);
        // The first and last moments, and the first and last days of a month.
        const uint8_t first[7] = {0x04, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00};
        const uint8_t last[7] = {0x04, 0x23, 0x59, 0x59, 0x31, 0x12, 0x99};
        expect(request_allowed(false, 0x201, 7, first), "clock frame, earliest, allowed", 0x201, 7);
        expect(request_allowed(false, 0x201, 7, last), "clock frame, latest, allowed", 0x201, 7);
        // Every value of every byte: allowed exactly when it is a real BCD value in range.
        const int low[7] = {4, 0, 0, 0, 1, 1, 0};
        const int high[7] = {4, 23, 59, 59, 31, 12, 99};
        for (int position = 0; position < 7; ++position) {
            for (int value = 0; value < 256; ++value) {
                uint8_t frame[7];
                std::memcpy(frame, good, sizeof frame);
                frame[position] = static_cast<uint8_t>(value);
                bool expected;
                if (position == 0) {
                    expected = value == 0x04;
                } else {
                    const int tens = value >> 4;
                    const int ones = value & 0x0F;
                    expected = tens <= 9 && ones <= 9 && tens * 10 + ones >= low[position] &&
                               tens * 10 + ones <= high[position];
                }
                expect(request_allowed(false, 0x201, 7, frame) == expected, "clock frame byte checked", 0x201, position);
            }
        }
        // Any other length, and a remote frame, are refused.
        for (int dlc = 0; dlc <= 8; ++dlc) {
            if (dlc != 7) {
                expect(!request_allowed(false, 0x201, dlc, good), "clock frame of another length refused", 0x201, dlc);
            }
        }
        expect(!request_allowed(true, 0x201, 7, good), "clock remote frame refused", 0x201, 7);
    }

    // Debug mode: 0x28A, eight bytes, byte 3 = 1 (on) or all zeros (off), and
    // nothing else. Any other value of any byte is god mode, an output control
    // or noise, and is refused.
    {
        const uint8_t on[8] = {0, 0, 0, 1, 0, 0, 0, 0};
        const uint8_t off[8] = {0, 0, 0, 0, 0, 0, 0, 0};
        expect(request_allowed(false, 0x28A, 8, on), "debug on allowed", 0x28A, 8);
        expect(request_allowed(false, 0x28A, 8, off), "debug off allowed", 0x28A, 8);
        for (int position = 0; position < 8; ++position) {
            for (int value = 0; value < 256; ++value) {
                for (const uint8_t *base : {on, off}) {
                    uint8_t frame[8];
                    std::memcpy(frame, base, sizeof frame);
                    frame[position] = static_cast<uint8_t>(value);
                    const bool is_on = std::memcmp(frame, on, 8) == 0;
                    const bool is_off = std::memcmp(frame, off, 8) == 0;
                    expect(request_allowed(false, 0x28A, 8, frame) == (is_on || is_off), "debug frame byte checked",
                           0x28A, position);
                }
            }
        }
        // God mode, and each output button bit, are refused on their own and with debug on.
        for (int position : {0, 1, 2}) {
            for (int bit = 0; bit < 8; ++bit) {
                for (const uint8_t *base : {on, off}) {
                    uint8_t frame[8];
                    std::memcpy(frame, base, sizeof frame);
                    frame[position] |= static_cast<uint8_t>(1 << bit);
                    expect(!request_allowed(false, 0x28A, 8, frame), "god mode or output bit refused", 0x28A, position);
                }
            }
        }
        // Any other length, and a remote frame, are refused.
        for (int dlc = 0; dlc <= 8; ++dlc) {
            if (dlc != 8) {
                expect(!request_allowed(false, 0x28A, dlc, on), "debug frame of another length refused", 0x28A, dlc);
            }
        }
        expect(!request_allowed(true, 0x28A, 8, on), "debug remote frame refused", 0x28A, 8);
    }

    // Nothing else may be sent, whatever its length or contents. This includes
    // every other frame the scooter itself sends.
    std::memset(data, 0, sizeof data);
    for (uint32_t id = 0; id <= 0x7FF; ++id) {
        if (id == 0x234 || id == 0x31B || id == 0x1ED || id == 0x201 || id == 0x28A) {
            continue;
        }
        for (int dlc = 0; dlc <= 8; ++dlc) {
            expect(!request_allowed(false, id, dlc, data), "other data frame refused", id, dlc);
        }
    }
    // Identifiers beyond the 11-bit range are refused too.
    expect(!request_allowed(false, 0x800, 1, data), "extended id refused", 0x800, 1);
    expect(!request_allowed(false, 0x1FFFFFFF, 8, data), "extended id refused", 0x1FFFFFFF, 8);

    std::printf("%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
