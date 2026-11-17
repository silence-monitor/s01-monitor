cmake_minimum_required(VERSION 3.16)
# The firmware has the same version as the app: the one in the project's
# top-level Cargo.toml. ESP-IDF prints it at boot as "App version".
# Without the next line a new version in Cargo.toml would not reach the firmware
# until the build folder was cleared, because CMake would not look at it again.
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_LIST_DIR}/../Cargo.toml")
file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/../Cargo.toml" _cargo_version REGEX "^version = \"")
string(REGEX REPLACE "^version = \"([^\"]*)\".*$" "\\1" PROJECT_VER "${_cargo_version}")
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(s01-monitor-esp32)
