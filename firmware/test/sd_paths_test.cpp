// Host test for firmware/main/sd_paths.h: the path rules, the query reader and
// the text the recorder writes. No ESP32 needed; scripts/test_firmware_filter.sh
// builds and runs it.

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

#include "../main/sd_paths.h"

static int g_failures = 0;
static int g_checks = 0;

#define EXPECT(cond)                                                       \
    do {                                                                   \
        ++g_checks;                                                        \
        if (!(cond)) {                                                     \
            ++g_failures;                                                  \
            std::printf("FAIL line %d: %s\n", __LINE__, #cond);            \
        }                                                                  \
    } while (0)

static bool path(const char *p) { return sdp::path_ok(p); }

int main()
{
    // Paths that are fine.
    EXPECT(path("/"));
    EXPECT(path("/a"));
    EXPECT(path("/rec-20261119-214501.log"));
    EXPECT(path("/folder/sub folder/file name.txt"));
    EXPECT(path("/a.b.c"));
    EXPECT(path("/UPPER_lower-1 2"));

    // Paths that are not.
    EXPECT(!path(nullptr));
    EXPECT(!path(""));
    EXPECT(!path("a"));              // no root
    EXPECT(!path("//"));
    EXPECT(!path("/a//b"));          // empty part
    EXPECT(!path("/a/"));            // trailing slash
    EXPECT(!path("/."));
    EXPECT(!path("/.."));
    EXPECT(!path("/a/../b"));
    EXPECT(!path("/a/./b"));
    EXPECT(!path("/a b ."));         // ends in a dot
    EXPECT(!path("/ a"));            // starts with a space
    EXPECT(!path("/a "));            // ends with a space
    EXPECT(!path("/a:b"));
    EXPECT(!path("/a\\b"));
    EXPECT(!path("/a*b"));
    EXPECT(!path("/a?b"));
    EXPECT(!path("/a\"b"));
    EXPECT(!path("/a<b"));
    EXPECT(!path("/a>b"));
    EXPECT(!path("/a|b"));
    EXPECT(!path("/a\tb"));          // control character
    EXPECT(!path("/a\x7f"));
    EXPECT(!path("/caf\xc3\xa9"));   // not plain ASCII
    // Lengths.
    EXPECT(path(("/" + std::string(100, 'a')).c_str()));
    EXPECT(!path(("/" + std::string(101, 'a')).c_str()));
    std::string deep;
    for (int i = 0; i < 40; ++i) deep += "/abcde";   // 240 characters
    EXPECT(!path(deep.c_str()));

    // Joining and splitting.
    char out[256];
    EXPECT(sdp::join("/", "a.txt", out, sizeof out) && std::strcmp(out, "/a.txt") == 0);
    EXPECT(sdp::join("/x", "a.txt", out, sizeof out) && std::strcmp(out, "/x/a.txt") == 0);
    EXPECT(!sdp::join("/x", "..", out, sizeof out));
    EXPECT(!sdp::join("/x", "a/b", out, sizeof out));
    EXPECT(!sdp::join("/x", "", out, sizeof out));
    EXPECT(!sdp::join("/x", "a.txt", out, 5));      // too small
    EXPECT(sdp::parent_of("/a/b.txt", out, sizeof out) && std::strcmp(out, "/a") == 0);
    EXPECT(sdp::parent_of("/a", out, sizeof out) && std::strcmp(out, "/") == 0);
    EXPECT(!sdp::parent_of("/", out, sizeof out));
    EXPECT(std::strcmp(sdp::leaf_of("/a/b.txt"), "b.txt") == 0);
    EXPECT(std::strcmp(sdp::leaf_of("/b"), "b") == 0);

    // The query reader.
    EXPECT(sdp::query_value("path=%2Fa%2Fb.txt", "path", out, sizeof out) && std::strcmp(out, "/a/b.txt") == 0);
    EXPECT(sdp::query_value("x=1&path=%2Fa&y=2", "path", out, sizeof out) && std::strcmp(out, "/a") == 0);
    EXPECT(sdp::query_value("path=%2Fa%20b", "path", out, sizeof out) && std::strcmp(out, "/a b") == 0);
    EXPECT(sdp::query_value("path=/a+b", "path", out, sizeof out) && std::strcmp(out, "/a+b") == 0);
    EXPECT(sdp::query_value("path=", "path", out, sizeof out) && std::strcmp(out, "") == 0);
    EXPECT(!sdp::query_value("xpath=%2Fa", "path", out, sizeof out));   // another key
    EXPECT(!sdp::query_value("pathx=%2Fa", "path", out, sizeof out));
    EXPECT(!sdp::query_value("a=1", "path", out, sizeof out));
    EXPECT(!sdp::query_value("path=%2", "path", out, sizeof out));      // cut escape
    EXPECT(!sdp::query_value("path=%", "path", out, sizeof out));
    EXPECT(!sdp::query_value("path=%zz", "path", out, sizeof out));
    EXPECT(!sdp::query_value("path=%00", "path", out, sizeof out));     // NUL
    EXPECT(!sdp::query_value("path=%0A", "path", out, sizeof out));     // control
    EXPECT(!sdp::query_value("path=abcdef", "path", out, 4));           // too long
    EXPECT(sdp::query_value("path=abc", "path", out, 4) && std::strcmp(out, "abc") == 0);
    EXPECT(!sdp::query_value(nullptr, "path", out, sizeof out));
    // An escaped dot-dot is decoded first, then refused by the path rules.
    EXPECT(sdp::query_value("path=%2F..%2Fx", "path", out, sizeof out) && !sdp::path_ok(out));

    // A frame line in the app's capture format.
    uint8_t data[8] = {0x00, 0x01, 0xAB, 0xFF, 0, 0, 0, 0};
    char line[96];
    size_t n = sdp::can_line(line, sizeof line, (17ull * 3600 + 45 * 60 + 30) * 1000 + 977, 0x280, 4, data);
    EXPECT(n > 0 && std::strcmp(line, "time:17:45:30.977 RX: STDID:00280 data:00 01 AB FF\n") == 0);
    n = sdp::can_line(line, sizeof line, 86400000ull + 5, 0x7FF, 0, data);
    EXPECT(n > 0 && std::strcmp(line, "time:00:00:00.005 RX: STDID:007FF data:\n") == 0);
    n = sdp::can_line(line, sizeof line, 0, 0x1AA, 8, data);
    EXPECT(n > 0 && std::strcmp(line, "time:00:00:00.000 RX: STDID:001AA data:00 01 AB FF 00 00 00 00\n") == 0);
    EXPECT(sdp::can_line(line, sizeof line, 0, 1, 9, data) == 0);       // dlc too big
    EXPECT(sdp::can_line(line, 10, 0, 1, 1, data) == 0);                // too small

    // A marker, as the app writes it.
    n = sdp::mark_line(line, sizeof line, (17ull * 3600 + 46 * 60 + 2) * 1000 + 310, "pressed reverse");
    EXPECT(n > 0 && std::strcmp(line, "# MARK 17:46:02.310 pressed reverse\n") == 0);
    n = sdp::mark_line(line, sizeof line, 0, "  padded  ");
    EXPECT(n > 0 && std::strcmp(line, "# MARK 00:00:00.000 padded\n") == 0);
    n = sdp::mark_line(line, sizeof line, 0, "a\tb\nc");
    EXPECT(n > 0 && std::strcmp(line, "# MARK 00:00:00.000 a b c\n") == 0);
    n = sdp::mark_line(line, sizeof line, 0, "");
    EXPECT(n > 0 && std::strcmp(line, "# MARK 00:00:00.000 \n") == 0);
    n = sdp::mark_line(line, 48, 0, "this text is far too long to fit in the little buffer it is given");
    EXPECT(n > 0 && n < 48 && line[n - 1] == '\n' && line[n] == 0);   // cut, never overrun
    EXPECT(sdp::mark_line(line, 20, 0, "x") == 0);

    // Calendar and names.
    sdp::Calendar c = sdp::calendar_from(0);
    EXPECT(c.year == 1970 && c.month == 1 && c.day == 1 && c.hour == 0);
    c = sdp::calendar_from(1795124701);   // 2026-11-19 21:45:01
    EXPECT(c.year == 2026 && c.month == 11 && c.day == 19 && c.hour == 21 && c.minute == 45 && c.second == 1);
    c = sdp::calendar_from(1709164800);   // 2024-02-29 00:00:00
    EXPECT(c.year == 2024 && c.month == 2 && c.day == 29);
    c = sdp::calendar_from(1709251199);   // 2024-02-29 23:59:59
    EXPECT(c.year == 2024 && c.month == 2 && c.day == 29 && c.hour == 23 && c.second == 59);
    c = sdp::calendar_from(1709251200);   // 2024-03-01
    EXPECT(c.year == 2024 && c.month == 3 && c.day == 1);
    char name[40];
    n = sdp::record_name(name, sizeof name, true, 1795124701, 0);
    EXPECT(n == 23 && std::strcmp(name, "rec-20261119-214501.log") == 0);
    n = sdp::record_name(name, sizeof name, false, 0, 7);
    EXPECT(n == 12 && std::strcmp(name, "rec-0007.log") == 0);
    n = sdp::record_name(name, sizeof name, false, 0, 12345);   // wraps to four digits
    EXPECT(std::strcmp(name, "rec-2345.log") == 0);
    EXPECT(sdp::path_ok((std::string("/") + name).c_str()));
    char date[24];
    n = sdp::date_text(date, sizeof date, 1795124701);
    EXPECT(n == 19 && std::strcmp(date, "2026-11-19 21:45:01") == 0);
    n = sdp::date_text(date, sizeof date, 315532800);   // 1980-01-01, a file with no clock
    EXPECT(n == 0 && date[0] == 0);

    std::printf("%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
