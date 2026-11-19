# The browser app is built by ../../scripts/build_web.sh into ../../web/dist
# and baked into the firmware image, so the device serves it with no storage
# partition or file system.
get_filename_component(WEB "${CMAKE_CURRENT_LIST_DIR}/../../web/dist" ABSOLUTE)
foreach(file index.html s01-monitor.js s01-monitor_bg.wasm.gz)
    if(NOT EXISTS "${WEB}/${file}")
        message(FATAL_ERROR "Missing ${WEB}/${file}. Run scripts/build_web.sh first.")
    endif()
endforeach()

set(S01_SOURCES "main.cpp")
# The list of required components is worked out before the settings are known, so
# the SD card ones are always listed. Without CONFIG_S01_HAS_SD nothing uses them
# and the linker drops them.
set(S01_REQUIRES driver esp_event esp_http_server esp_netif esp_wifi lwip nvs_flash esp_app_format
                 fatfs sdmmc esp_driver_sdspi esp_driver_spi vfs esp_timer)
if(CONFIG_S01_HAS_SD)
    list(APPEND S01_SOURCES "sd_card.cpp")
endif()

idf_component_register(
    SRCS ${S01_SOURCES}
    INCLUDE_DIRS "."
    REQUIRES ${S01_REQUIRES}
    EMBED_FILES "${WEB}/index.html" "${WEB}/s01-monitor.js" "${WEB}/s01-monitor_bg.wasm.gz"
)
