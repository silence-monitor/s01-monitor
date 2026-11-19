// The SD card on the Atomic TF Card Base: mounting it, the web endpoints for
// the SD card screen of the browser app, and the recorder that writes the CAN
// bus to the card. Only built for the hardware that has the card base (the
// Kconfig option S01_HAS_SD); for the other hardware these are empty stubs.
#pragma once

#include "driver/twai.h"
#include "esp_http_server.h"
#include "sdkconfig.h"

// Defined in main.cpp: true when the request is from a page this device served.
bool origin_is_ours(httpd_req_t *req);

#ifdef CONFIG_S01_HAS_SD

// Starts the card manager, the recorder and the transfer worker. Call after
// nvs_flash_init(), before the CAN task starts.
void sd_start();

// Adds the /time and /sd/... endpoints to the web server.
void sd_register(httpd_handle_t server);

// Offers one received frame to the recorder. Never waits; if the recorder is
// not recording or is behind, the frame is simply not kept.
void sd_log_frame(const twai_message_t &message);

#else

inline void sd_start() {}
inline void sd_register(httpd_handle_t) {}
inline void sd_log_frame(const twai_message_t &) {}

#endif
