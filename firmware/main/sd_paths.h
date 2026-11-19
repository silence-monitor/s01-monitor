// The parts of the SD card feature that need no hardware: which paths and
// names are accepted, how a query string is read, and how a recorded frame and
// a time are written as text. They are in a header of their own so a host test
// can check them (firmware/test/sd_paths_test.cpp), with no ESP32.
//
// The rules are strict on purpose. A path is only ever one that starts at the
// card's root, with plain printable ASCII names, and never "." or ".." or
// anything a FAT file system treats specially.
#pragma once

#include <cstddef>
#include <cstdint>
#include <cstring>

namespace sdp {

constexpr size_t NAME_MAX_LEN = 100;   // one file or folder name
constexpr size_t PATH_MAX_LEN = 200;   // a whole path, without the mount point

// One name: 1 to NAME_MAX_LEN printable ASCII characters, none of the ones FAT
// does not allow, not "." or "..", and no space or dot at either end.
inline bool name_ok(const char *name, size_t length)
{
    if (length == 0 || length > NAME_MAX_LEN) {
        return false;
    }
    if (name[0] == ' ' || name[length - 1] == ' ' || name[length - 1] == '.') {
        return false;
    }
    for (size_t i = 0; i < length; ++i) {
        unsigned char c = static_cast<unsigned char>(name[i]);
        if (c < 0x20 || c > 0x7E) {
            return false;
        }
        switch (c) {
        case '\\': case '/': case ':': case '*': case '?': case '"': case '<': case '>': case '|':
            return false;
        default:
            break;
        }
    }
    return true;
}

// "/" or a path like "/a/b.txt": starts with '/', no empty parts, every part a
// good name, no trailing slash except for the root itself.
inline bool path_ok(const char *path)
{
    if (path == nullptr || path[0] != '/') {
        return false;
    }
    size_t length = strlen(path);
    if (length > PATH_MAX_LEN) {
        return false;
    }
    if (length == 1) {
        return true;
    }
    size_t start = 1;
    for (size_t i = 1; i <= length; ++i) {
        if (i == length || path[i] == '/') {
            if (!name_ok(path + start, i - start)) {
                return false;
            }
            start = i + 1;
        }
    }
    return true;
}

inline bool is_root(const char *path)
{
    return path != nullptr && path[0] == '/' && path[1] == 0;
}

// Writes the folder that holds `path` into `out` ("/a/b.txt" gives "/a", "/a"
// gives "/"). False if the path is the root or `out` is too small.
inline bool parent_of(const char *path, char *out, size_t capacity)
{
    size_t length = strlen(path);
    if (length <= 1 || length + 1 > capacity) {
        return false;
    }
    size_t cut = length;
    while (cut > 1 && path[cut - 1] != '/') {
        --cut;
    }
    size_t keep = cut > 1 ? cut - 1 : 1;
    memcpy(out, path, keep);
    out[keep] = 0;
    return true;
}

// The last part of a path ("/a/b.txt" gives "b.txt").
inline const char *leaf_of(const char *path)
{
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

// "folder" and "name" joined ("/" and "a" give "/a", "/x" and "a" give "/x/a").
// False if the result does not fit or is not a good path.
inline bool join(const char *folder, const char *name, char *out, size_t capacity)
{
    size_t folder_length = strlen(folder);
    size_t name_length = strlen(name);
    bool root = is_root(folder);
    size_t total = (root ? 0 : folder_length) + 1 + name_length;
    if (total + 1 > capacity || !name_ok(name, name_length)) {
        return false;
    }
    size_t n = 0;
    if (!root) {
        memcpy(out, folder, folder_length);
        n = folder_length;
    }
    out[n++] = '/';
    memcpy(out + n, name, name_length);
    out[n + name_length] = 0;
    return path_ok(out);
}

inline int hex_digit(char c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

// Reads the value of `key` from a URL query string ("path=%2Fa&x=1") into
// `out`, with %XX escapes decoded. False if the key is missing, the value is
// too long, or an escape is wrong or decodes to a control character. A '+'
// stays a plus (the page sends spaces as %20).
inline bool query_value(const char *query, const char *key, char *out, size_t capacity)
{
    if (query == nullptr || capacity == 0) {
        return false;
    }
    size_t key_length = strlen(key);
    const char *at = query;
    while (*at) {
        const char *end = strchr(at, '&');
        size_t part = end ? static_cast<size_t>(end - at) : strlen(at);
        if (part > key_length && memcmp(at, key, key_length) == 0 && at[key_length] == '=') {
            const char *value = at + key_length + 1;
            size_t n = 0;
            size_t value_length = part - key_length - 1;
            for (size_t i = 0; i < value_length; ++i) {
                char c = value[i];
                if (c == '%') {
                    if (i + 2 >= value_length) {
                        return false;
                    }
                    int hi = hex_digit(value[i + 1]);
                    int lo = hex_digit(value[i + 2]);
                    if (hi < 0 || lo < 0) {
                        return false;
                    }
                    c = static_cast<char>(hi * 16 + lo);
                    i += 2;
                }
                if (static_cast<unsigned char>(c) < 0x20) {
                    return false;
                }
                if (n + 1 >= capacity) {
                    return false;
                }
                out[n++] = c;
            }
            out[n] = 0;
            return true;
        }
        if (!end) {
            break;
        }
        at = end + 1;
    }
    return false;
}

// ---- Text ----------------------------------------------------------------

// "HH:MM:SS.mmm" for a time in milliseconds; only the time of day is kept.
inline size_t clock_text(char *out, size_t capacity, uint64_t milliseconds)
{
    if (capacity < 13) {
        return 0;
    }
    uint32_t day = static_cast<uint32_t>(milliseconds % 86400000ull);
    uint32_t h = day / 3600000u;
    uint32_t m = (day / 60000u) % 60u;
    uint32_t s = (day / 1000u) % 60u;
    uint32_t ms = day % 1000u;
    out[0] = static_cast<char>('0' + h / 10);
    out[1] = static_cast<char>('0' + h % 10);
    out[2] = ':';
    out[3] = static_cast<char>('0' + m / 10);
    out[4] = static_cast<char>('0' + m % 10);
    out[5] = ':';
    out[6] = static_cast<char>('0' + s / 10);
    out[7] = static_cast<char>('0' + s % 10);
    out[8] = '.';
    out[9] = static_cast<char>('0' + ms / 100);
    out[10] = static_cast<char>('0' + (ms / 10) % 10);
    out[11] = static_cast<char>('0' + ms % 10);
    out[12] = 0;
    return 12;
}

// One frame as a line of the app's own capture format, the one it records and
// replays: "time:17:45:30.977 RX: STDID:00280 data:00 00 00 00\n". Returns the
// length, or 0 if it does not fit.
inline size_t can_line(char *out, size_t capacity, uint64_t milliseconds, uint32_t id, uint8_t dlc, const uint8_t *data)
{
    static const char digits[] = "0123456789ABCDEF";
    if (dlc > 8 || capacity < 64) {
        return 0;
    }
    size_t n = 0;
    memcpy(out, "time:", 5);
    n += 5;
    n += clock_text(out + n, capacity - n, milliseconds);
    memcpy(out + n, " RX: STDID:", 11);
    n += 11;
    for (int shift = 16; shift >= 0; shift -= 4) {
        out[n++] = digits[(id >> shift) & 0xF];
    }
    memcpy(out + n, " data:", 6);
    n += 6;
    for (uint8_t i = 0; i < dlc; ++i) {
        if (i > 0) {
            out[n++] = ' ';
        }
        out[n++] = digits[data[i] >> 4];
        out[n++] = digits[data[i] & 0xF];
    }
    out[n++] = '\n';
    out[n] = 0;
    return n;
}

// Turns seconds since 1970 (taken as a calendar time with no zone) into a
// year, month, day, hour, minute and second.
struct Calendar {
    int year, month, day, hour, minute, second;
};

inline Calendar calendar_from(int64_t seconds)
{
    int64_t days = seconds / 86400;
    int64_t rest = seconds % 86400;
    if (rest < 0) {
        rest += 86400;
        --days;
    }
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    days += 719468;
    int64_t era = (days >= 0 ? days : days - 146096) / 146097;
    int64_t doe = days - era * 146097;
    int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int64_t y = yoe + era * 400;
    int64_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    int64_t mp = (5 * doy + 2) / 153;
    int64_t d = doy - (153 * mp + 2) / 5 + 1;
    int64_t m = mp < 10 ? mp + 3 : mp - 9;
    Calendar c;
    c.year = static_cast<int>(y + (m <= 2));
    c.month = static_cast<int>(m);
    c.day = static_cast<int>(d);
    c.hour = static_cast<int>(rest / 3600);
    c.minute = static_cast<int>((rest / 60) % 60);
    c.second = static_cast<int>(rest % 60);
    return c;
}

inline void put2(char *out, int value)
{
    out[0] = static_cast<char>('0' + (value / 10) % 10);
    out[1] = static_cast<char>('0' + value % 10);
}

// "rec-20261119-214501.log" for a time that was set, "rec-0007.log" for a
// sequence number when no time is known.
inline size_t record_name(char *out, size_t capacity, bool clock_known, int64_t local_seconds, unsigned sequence)
{
    if (capacity < 32) {
        return 0;
    }
    if (clock_known) {
        Calendar c = calendar_from(local_seconds);
        memcpy(out, "rec-", 4);
        out[4] = static_cast<char>('0' + (c.year / 1000) % 10);
        out[5] = static_cast<char>('0' + (c.year / 100) % 10);
        put2(out + 6, c.year);
        put2(out + 8, c.month);
        put2(out + 10, c.day);
        out[12] = '-';
        put2(out + 13, c.hour);
        put2(out + 15, c.minute);
        put2(out + 17, c.second);
        memcpy(out + 19, ".log", 5);
        return 23;
    }
    memcpy(out, "rec-", 4);
    out[4] = static_cast<char>('0' + (sequence / 1000) % 10);
    out[5] = static_cast<char>('0' + (sequence / 100) % 10);
    out[6] = static_cast<char>('0' + (sequence / 10) % 10);
    out[7] = static_cast<char>('0' + sequence % 10);
    memcpy(out + 8, ".log", 5);
    return 12;
}

// "2026-11-19 21:45:01" for a calendar time; "" (empty) for a date before 2020,
// which is what a file written with no clock set has.
inline size_t date_text(char *out, size_t capacity, int64_t local_seconds)
{
    if (capacity < 20) {
        return 0;
    }
    Calendar c = calendar_from(local_seconds);
    if (c.year < 2020) {
        out[0] = 0;
        return 0;
    }
    out[0] = static_cast<char>('0' + (c.year / 1000) % 10);
    out[1] = static_cast<char>('0' + (c.year / 100) % 10);
    put2(out + 2, c.year);
    out[4] = '-';
    put2(out + 5, c.month);
    out[7] = '-';
    put2(out + 8, c.day);
    out[10] = ' ';
    put2(out + 11, c.hour);
    out[13] = ':';
    put2(out + 14, c.minute);
    out[16] = ':';
    put2(out + 17, c.second);
    out[19] = 0;
    return 19;
}

}  // namespace sdp
