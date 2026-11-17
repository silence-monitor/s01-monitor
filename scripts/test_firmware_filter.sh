#!/bin/bash
# Builds and runs the host test for the firmware's request filter
# (firmware/main/request_filter.h). No ESP32 and no ESP-IDF needed: it only
# needs a C++ compiler.
set -euo pipefail
cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
"${CXX:-c++}" -std=c++17 -Wall -Wextra -Werror firmware/test/request_filter_test.cpp -o "$work/request_filter_test"
"$work/request_filter_test"
