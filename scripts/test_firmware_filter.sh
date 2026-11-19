#!/bin/bash
# Builds and runs the host test for the firmware's request filter
# (firmware/main/request_filter.h) and for the SD card paths and text
# (firmware/main/sd_paths.h). No ESP32 and no ESP-IDF needed: it only
# needs a C++ compiler.
set -euo pipefail
cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
for test in request_filter sd_paths; do
  "${CXX:-c++}" -std=c++17 -Wall -Wextra -Werror "firmware/test/${test}_test.cpp" -o "$work/${test}_test"
  "$work/${test}_test"
done
