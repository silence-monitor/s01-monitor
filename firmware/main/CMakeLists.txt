# The browser app is built by ../../scripts/build_web.sh into ../../web/dist
# and baked into the firmware image, so the device serves it with no storage
# partition or file system.
get_filename_component(WEB "${CMAKE_CURRENT_LIST_DIR}/../../web/dist" ABSOLUTE)
foreach(file index.html s01-monitor.js s01-monitor_bg.wasm.gz)
    if(NOT EXISTS "${WEB}/${file}")
        message(FATAL_ERROR "Missing ${WEB}/${file}. Run scripts/build_web.sh first.")
    endif()
endforeach()

idf_component_register(
    SRCS "main.cpp"
    INCLUDE_DIRS "."
    REQUIRES driver esp_event esp_http_server esp_netif esp_wifi lwip nvs_flash
    EMBED_FILES "${WEB}/index.html" "${WEB}/s01-monitor.js" "${WEB}/s01-monitor_bg.wasm.gz"
)
