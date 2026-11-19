// S01 monitor firmware for an M5 ATOM Lite, with either the Atomic CAN Base or a
// Unit CAN on the Grove port (chosen when it is built, see Kconfig.projbuild).
//
// It does four things:
//   1. starts a WiFi network of its own,
//   2. serves the browser app (baked into this image) at http://192.168.4.1/,
//   3. reads the scooter's CAN bus and streams it to the browser over a
//      WebSocket at /ws, as the same slcan text lines a CANable prints, and
//      sends onto the bus whatever request lines the browser sends back,
//   4. keeps the custom views people save in the browser app (at /views), so
//      they are the same in every browser.
//
// The decoding all happens in the browser.

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <initializer_list>

#include "driver/twai.h"
#include "esp_app_desc.h"
#include "esp_event.h"
#include "esp_http_server.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "esp_wifi.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/stream_buffer.h"
#include "freertos/task.h"
#include "lwip/sockets.h"
#include "nvs.h"
#include "nvs_flash.h"
#include "request_filter.h"
#include "sd_card.h"

static const char *TAG = "s01";

// ---- Settings ---------------------------------------------------------------

// The CAN pins and the hardware's name come from the board chosen at build time
// (idf.py menuconfig, "S01 Monitor"): the Atomic CAN Base uses GPIO 22 and 19,
// a Unit CAN on the Grove port uses GPIO 26 and 32.
static constexpr gpio_num_t CAN_TX = static_cast<gpio_num_t>(CONFIG_S01_CAN_TX_GPIO);
static constexpr gpio_num_t CAN_RX = static_cast<gpio_num_t>(CONFIG_S01_CAN_RX_GPIO);
#ifdef CONFIG_S01_BOARD_GROVE_CAN
static constexpr const char *BOARD_NAME = "M5 ATOM Lite + Unit CAN (Grove) + TF Card Base";
#else
static constexpr const char *BOARD_NAME = "M5 ATOM Lite + Atomic CAN Base";
#endif
static constexpr const char *AP_SSID = "S01-Monitor";
static constexpr const char *AP_PASSWORD = "silence01";  // 8 characters or more
static constexpr int AP_CHANNEL = 6;
static constexpr int MAX_CLIENTS = 4;

// ---- The browser app, baked into the image ----------------------------------

extern const uint8_t index_html_start[] asm("_binary_index_html_start");
extern const uint8_t index_html_end[] asm("_binary_index_html_end");
extern const uint8_t app_js_start[] asm("_binary_s01_monitor_js_start");
extern const uint8_t app_js_end[] asm("_binary_s01_monitor_js_end");
extern const uint8_t app_wasm_start[] asm("_binary_s01_monitor_bg_wasm_gz_start");
extern const uint8_t app_wasm_end[] asm("_binary_s01_monitor_bg_wasm_gz_end");

struct Asset {
    const uint8_t *start;
    const uint8_t *end;
    const char *type;
    bool gzip;
};

static const Asset ASSET_HTML = {index_html_start, index_html_end, "text/html", false};
static const Asset ASSET_JS = {app_js_start, app_js_end, "text/javascript", false};
static const Asset ASSET_WASM = {app_wasm_start, app_wasm_end, "application/wasm", true};

// Changes whenever the page does, so a browser reloads it after a reflash and
// not before. It is a hash of the three baked-in files themselves. (It used to
// come from the time this source file was compiled, which does not change when
// only the page is rebuilt, so browsers that held an older page were told it
// was still current.)
static char g_etag[24];

static uint32_t fnv1a(uint32_t hash, const uint8_t *data, size_t length)
{
    for (size_t i = 0; i < length; ++i) {
        hash = (hash ^ data[i]) * 16777619u;
    }
    return hash;
}

static void make_etag()
{
    uint32_t hash = 2166136261u;
    for (const Asset *asset : {&ASSET_HTML, &ASSET_JS, &ASSET_WASM}) {
        hash = fnv1a(hash, asset->start, static_cast<size_t>(asset->end - asset->start));
    }
    snprintf(g_etag, sizeof g_etag, "\"%08lx\"", static_cast<unsigned long>(hash));
    ESP_LOGI(TAG, "page tag %s", g_etag);
}

static esp_err_t serve_asset(httpd_req_t *req)
{
    const Asset *asset = static_cast<const Asset *>(req->user_ctx);
    char seen[40];
    httpd_resp_set_hdr(req, "ETag", g_etag);
    httpd_resp_set_hdr(req, "Cache-Control", "no-cache");
    if (httpd_req_get_hdr_value_str(req, "If-None-Match", seen, sizeof seen) == ESP_OK &&
        strcmp(seen, g_etag) == 0) {
        httpd_resp_set_status(req, "304 Not Modified");
        return httpd_resp_send(req, nullptr, 0);
    }
    httpd_resp_set_type(req, asset->type);
    if (asset->gzip) {
        httpd_resp_set_hdr(req, "Content-Encoding", "gzip");
    }
    return httpd_resp_send(req, reinterpret_cast<const char *>(asset->start), asset->end - asset->start);
}

// ---- What this device is -----------------------------------------------------
//
// The browser app asks once, so it can show what the hardware has (the SD card
// screen appears only when "sd 1" is reported).

static esp_err_t info_get(httpd_req_t *req)
{
    char text[200];
#ifdef CONFIG_S01_HAS_SD
    const int has_sd = 1;
#else
    const int has_sd = 0;
#endif
    int n = snprintf(text, sizeof text, "version %s\nhardware %s\nsd %d\n", esp_app_get_description()->version,
                     BOARD_NAME, has_sd);
    httpd_resp_set_type(req, "text/plain; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    return httpd_resp_send(req, text, n);
}

// ---- Saved views ------------------------------------------------------------
//
// The custom views a person saves in the browser app are kept here, so every
// browser that opens the page sees the same ones. The app sends and receives
// the whole list as one small text file at /views. The device does not read
// it; it only checks the first line, to keep stray data out.

static constexpr size_t VIEWS_MAX = 6000;  // the app keeps its list under this
static constexpr const char *VIEWS_HEADER = "S01 views 1";
static constexpr const char *NVS_NAMESPACE = "s01";
static constexpr const char *NVS_VIEWS_KEY = "views";

// One request is handled at a time, so one buffer serves both directions.
static char g_views[VIEWS_MAX + 1];

static esp_err_t views_get(httpd_req_t *req)
{
    httpd_resp_set_type(req, "text/plain; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    size_t size = 0;
    nvs_handle_t handle;
    // Nothing saved yet is an empty answer, not an error.
    if (nvs_open(NVS_NAMESPACE, NVS_READONLY, &handle) != ESP_OK) {
        return httpd_resp_send(req, "", 0);
    }
    esp_err_t err = nvs_get_blob(handle, NVS_VIEWS_KEY, nullptr, &size);
    if (err == ESP_OK && size > 0 && size <= VIEWS_MAX) {
        err = nvs_get_blob(handle, NVS_VIEWS_KEY, g_views, &size);
    } else {
        size = 0;
    }
    nvs_close(handle);
    if (err != ESP_OK) {
        size = 0;
    }
    return httpd_resp_send(req, g_views, size);
}

static esp_err_t views_put(httpd_req_t *req)
{
    size_t total = req->content_len;
    if (total > VIEWS_MAX) {
        httpd_resp_set_status(req, "413 Payload Too Large");
        httpd_resp_send(req, "too big", HTTPD_RESP_USE_STRLEN);
        return ESP_FAIL;  // the body is unread, so close the connection
    }
    size_t got = 0;
    int stalls = 0;
    while (got < total) {
        int n = httpd_req_recv(req, g_views + got, total - got);
        if (n == HTTPD_SOCK_ERR_TIMEOUT && ++stalls < 5) {
            continue;
        }
        if (n <= 0) {
            return ESP_FAIL;
        }
        got += n;
    }
    // An empty body clears the list; anything else must be a list.
    size_t header = strlen(VIEWS_HEADER);
    if (total != 0 && (total < header || memcmp(g_views, VIEWS_HEADER, header) != 0)) {
        httpd_resp_set_status(req, "400 Bad Request");
        return httpd_resp_send(req, "not a views list", HTTPD_RESP_USE_STRLEN);
    }
    nvs_handle_t handle;
    esp_err_t err = nvs_open(NVS_NAMESPACE, NVS_READWRITE, &handle);
    if (err == ESP_OK) {
        err = total == 0 ? nvs_erase_key(handle, NVS_VIEWS_KEY) : nvs_set_blob(handle, NVS_VIEWS_KEY, g_views, total);
        if (err == ESP_ERR_NVS_NOT_FOUND) {
            err = ESP_OK;  // clearing what was never saved
        }
        if (err == ESP_OK) {
            err = nvs_commit(handle);
        }
        nvs_close(handle);
    }
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "saving the views failed: %s", esp_err_to_name(err));
        httpd_resp_set_status(req, "507 Insufficient Storage");
        return httpd_resp_send(req, "could not store", HTTPD_RESP_USE_STRLEN);
    }
    ESP_LOGI(TAG, "saved views: %u bytes", static_cast<unsigned>(total));
    httpd_resp_set_status(req, "204 No Content");
    return httpd_resp_send(req, nullptr, 0);
}

// ---- Browsers connected over the WebSocket ----------------------------------

static httpd_handle_t g_server = nullptr;
static SemaphoreHandle_t g_clients_lock = nullptr;
static int g_clients[MAX_CLIENTS];

static void clients_init()
{
    g_clients_lock = xSemaphoreCreateMutex();
    for (int &fd : g_clients) {
        fd = -1;
    }
}

// The web server would wait 10 seconds for a browser that has stopped reading,
// such as a phone that locked its screen, and the feed to every other browser
// waits with it. A second is plenty for a healthy connection, and a browser
// that cannot take a send in that time is dropped (see broadcast).
static void limit_send_wait(int fd)
{
    struct timeval wait;
    wait.tv_sec = 1;
    wait.tv_usec = 0;
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &wait, sizeof wait);
}

static void client_add(int fd)
{
    limit_send_wait(fd);
    xSemaphoreTake(g_clients_lock, portMAX_DELAY);
    bool known = false;
    for (int saved : g_clients) {
        known = known || saved == fd;
    }
    for (int &slot : g_clients) {
        if (!known && slot == -1) {
            slot = fd;
            known = true;
        }
    }
    xSemaphoreGive(g_clients_lock);
    if (!known) {
        ESP_LOGW(TAG, "too many browsers; dropping %d", fd);
        httpd_sess_trigger_close(g_server, fd);
    }
}

// Sends to every connected browser. A browser that has gone, or can't keep up,
// is dropped.
static bool broadcast(const uint8_t *data, size_t len)
{
    bool any = false;
    xSemaphoreTake(g_clients_lock, portMAX_DELAY);
    for (int &fd : g_clients) {
        if (fd < 0) {
            continue;
        }
        if (httpd_ws_get_fd_info(g_server, fd) != HTTPD_WS_CLIENT_WEBSOCKET) {
            fd = -1;
            continue;
        }
        httpd_ws_frame_t frame = {};
        frame.type = HTTPD_WS_TYPE_TEXT;
        frame.final = true;
        frame.payload = const_cast<uint8_t *>(data);
        frame.len = len;
        if (httpd_ws_send_frame_async(g_server, fd, &frame) == ESP_OK) {
            any = true;
        } else {
            httpd_sess_trigger_close(g_server, fd);
            fd = -1;
        }
    }
    xSemaphoreGive(g_clients_lock);
    return any;
}

// ---- Browser to bus ---------------------------------------------------------

static int hex_value(char c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

// Frames a browser asked for that were not on the list in request_filter.h.
static uint32_t g_refused = 0;

static void note_refused(uint32_t id)
{
    ++g_refused;
    if (g_refused == 1 || g_refused % 100 == 0) {
        ESP_LOGW(TAG, "refused a frame a browser asked to send (id 0x%03lX); %lu refused so far",
                 static_cast<unsigned long>(id), static_cast<unsigned long>(g_refused));
    }
}

// One slcan line from the browser: "t<id3><dlc><data>" sends a frame, and
// "r<id3><dlc>" sends a remote request. Only the app's own read requests go
// out (see request_filter.h); anything else is refused and counted.
static void send_line(const char *line, size_t len)
{
    if (len < 5 || (line[0] != 't' && line[0] != 'r')) {
        return;
    }
    int id = 0;
    for (int i = 1; i <= 3; ++i) {
        int v = hex_value(line[i]);
        if (v < 0) return;
        id = id * 16 + v;
    }
    int dlc = hex_value(line[4]);
    if (dlc < 0 || dlc > 8) return;
    twai_message_t message = {};
    message.identifier = id;
    message.data_length_code = dlc;
    if (line[0] == 'r') {
        message.rtr = 1;
    } else {
        if (len < 5 + static_cast<size_t>(dlc) * 2) return;
        for (int i = 0; i < dlc; ++i) {
            int hi = hex_value(line[5 + i * 2]);
            int lo = hex_value(line[6 + i * 2]);
            if (hi < 0 || lo < 0) return;
            message.data[i] = static_cast<uint8_t>(hi * 16 + lo);
        }
    }
    if (!request_allowed(message.rtr != 0, message.identifier, message.data_length_code, message.data)) {
        note_refused(message.identifier);
        return;
    }
    twai_transmit(&message, 0);  // never wait: a full queue just drops the request
}

// A web page from any other site can open a WebSocket to this device while a
// phone is on its WiFi. Browsers always say where the page came from in the
// Origin header, so only a page served by this device (same host) may connect.
// A tool that sends no Origin header is not a web page and is let through.
bool origin_is_ours(httpd_req_t *req)
{
    char origin[96];
    esp_err_t found = httpd_req_get_hdr_value_str(req, "Origin", origin, sizeof origin);
    if (found == ESP_ERR_NOT_FOUND) {
        return true;
    }
    if (found != ESP_OK) {
        return false;  // too long to be ours
    }
    char host[64];
    if (httpd_req_get_hdr_value_str(req, "Host", host, sizeof host) != ESP_OK) {
        return false;
    }
    const char *after_scheme = strstr(origin, "://");
    after_scheme = after_scheme ? after_scheme + 3 : origin;
    return strcmp(after_scheme, host) == 0;
}

static esp_err_t ws_handler(httpd_req_t *req)
{
    if (req->method == HTTP_GET) {  // the opening handshake
        int fd = httpd_req_to_sockfd(req);
        if (!origin_is_ours(req)) {
            ESP_LOGW(TAG, "closed a WebSocket from another site");
            httpd_sess_trigger_close(g_server, fd);
            return ESP_OK;
        }
        client_add(fd);
        return ESP_OK;
    }
    uint8_t buffer[256];
    httpd_ws_frame_t frame = {};
    frame.payload = buffer;
    esp_err_t err = httpd_ws_recv_frame(req, &frame, 0);
    if (err != ESP_OK) {
        return err;
    }
    if (frame.len == 0) {
        return ESP_OK;
    }
    if (frame.len >= sizeof buffer) {
        return ESP_FAIL;  // nothing the app sends is this long; close
    }
    err = httpd_ws_recv_frame(req, &frame, frame.len);
    if (err != ESP_OK) {
        return err;
    }
    if (frame.type == HTTPD_WS_TYPE_TEXT || frame.type == HTTPD_WS_TYPE_BINARY) {
        buffer[frame.len] = 0;
        size_t start = 0;
        for (size_t i = 0; i <= frame.len; ++i) {
            if (i == frame.len || buffer[i] == '\r' || buffer[i] == '\n') {
                send_line(reinterpret_cast<const char *>(buffer) + start, i - start);
                start = i + 1;
            }
        }
    }
    return ESP_OK;
}

// ---- Bus to browser ---------------------------------------------------------

static StreamBufferHandle_t g_feed = nullptr;

static void can_init()
{
    twai_general_config_t general = TWAI_GENERAL_CONFIG_DEFAULT(CAN_TX, CAN_RX, TWAI_MODE_NORMAL);
    general.rx_queue_len = 128;
    general.tx_queue_len = 8;
    general.alerts_enabled = TWAI_ALERT_BUS_OFF | TWAI_ALERT_BUS_RECOVERED;
#ifdef CONFIG_TWAI_ISR_IN_IRAM
    general.intr_flags = ESP_INTR_FLAG_IRAM;
#endif
    twai_timing_config_t timing = TWAI_TIMING_CONFIG_250KBITS();
    twai_filter_config_t filter = TWAI_FILTER_CONFIG_ACCEPT_ALL();
    ESP_ERROR_CHECK(twai_driver_install(&general, &timing, &filter));
    ESP_ERROR_CHECK(twai_start());
    ESP_LOGI(TAG, "Hardware: %s", BOARD_NAME);
    ESP_LOGI(TAG, "CAN started at 250 kbit/s (TX GPIO%d, RX GPIO%d)", CAN_TX, CAN_RX);
}

// Every frame on the bus becomes one text line, "t<id3><dlc><data hex>\r".
static void can_task(void *)
{
    static const char digits[] = "0123456789ABCDEF";
    for (;;) {
        uint32_t alerts = 0;
        if (twai_read_alerts(&alerts, 0) == ESP_OK) {
            if (alerts & TWAI_ALERT_BUS_OFF) {
                twai_initiate_recovery();
            }
            if (alerts & TWAI_ALERT_BUS_RECOVERED) {
                twai_start();
            }
        }
        twai_message_t message;
        if (twai_receive(&message, pdMS_TO_TICKS(100)) != ESP_OK) {
            continue;
        }
        if (message.extd || message.rtr) {
            continue;
        }
        sd_log_frame(message);  // for the recorder, if there is a card and it is recording
        char line[24];
        size_t n = 0;
        line[n++] = 't';
        line[n++] = digits[(message.identifier >> 8) & 0xF];
        line[n++] = digits[(message.identifier >> 4) & 0xF];
        line[n++] = digits[message.identifier & 0xF];
        line[n++] = digits[message.data_length_code & 0xF];
        for (int i = 0; i < message.data_length_code && i < 8; ++i) {
            line[n++] = digits[message.data[i] >> 4];
            line[n++] = digits[message.data[i] & 0xF];
        }
        line[n++] = '\r';
        // Never wait: if the browser side is behind, the newest lines are lost.
        xStreamBufferSend(g_feed, line, n, 0);
    }
}

// Collects what arrived in the last 20 ms into one WebSocket message.
static void feed_task(void *)
{
    static uint8_t batch[1400];
    for (;;) {
        size_t n = xStreamBufferReceive(g_feed, batch, sizeof batch, pdMS_TO_TICKS(200));
        if (n == 0) {
            continue;
        }
        vTaskDelay(pdMS_TO_TICKS(20));
        n += xStreamBufferReceive(g_feed, batch + n, sizeof batch - n, 0);
        broadcast(batch, n);
    }
}

// ---- WiFi and web server ----------------------------------------------------

static void wifi_start()
{
    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    esp_netif_create_default_wifi_ap();  // 192.168.4.1, with a DHCP server
    wifi_init_config_t init = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&init));
    ESP_ERROR_CHECK(esp_wifi_set_storage(WIFI_STORAGE_RAM));
    wifi_config_t config = {};
    strncpy(reinterpret_cast<char *>(config.ap.ssid), AP_SSID, sizeof config.ap.ssid - 1);
    strncpy(reinterpret_cast<char *>(config.ap.password), AP_PASSWORD, sizeof config.ap.password - 1);
    config.ap.ssid_len = strlen(AP_SSID);
    config.ap.channel = AP_CHANNEL;
    config.ap.max_connection = MAX_CLIENTS;
    config.ap.authmode = WIFI_AUTH_WPA2_PSK;
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_AP));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_AP, &config));
    ESP_ERROR_CHECK(esp_wifi_start());
    ESP_ERROR_CHECK(esp_wifi_set_ps(WIFI_PS_NONE));  // steady latency for the live feed
    ESP_LOGI(TAG, "WiFi network \"%s\" up; open http://192.168.4.1/", AP_SSID);
}

static void web_start()
{
    httpd_config_t config = HTTPD_DEFAULT_CONFIG();
    config.stack_size = 8192;
    config.max_uri_handlers = 26;  // four pages, /ws, /views twice, /info and the SD card ones
    config.lru_purge_enable = true;
    config.send_wait_timeout = 10;
    config.recv_wait_timeout = 10;
    ESP_ERROR_CHECK(httpd_start(&g_server, &config));

    static httpd_uri_t routes[4];
    const struct {
        const char *uri;
        const Asset *asset;
    } pages[4] = {
        {"/", &ASSET_HTML},
        {"/index.html", &ASSET_HTML},
        {"/s01-monitor.js", &ASSET_JS},
        {"/s01-monitor_bg.wasm", &ASSET_WASM},
    };
    for (int i = 0; i < 4; ++i) {
        routes[i] = {};
        routes[i].uri = pages[i].uri;
        routes[i].method = HTTP_GET;
        routes[i].handler = serve_asset;
        routes[i].user_ctx = const_cast<Asset *>(pages[i].asset);
        ESP_ERROR_CHECK(httpd_register_uri_handler(g_server, &routes[i]));
    }
    static httpd_uri_t views_read = {};
    views_read.uri = "/views";
    views_read.method = HTTP_GET;
    views_read.handler = views_get;
    ESP_ERROR_CHECK(httpd_register_uri_handler(g_server, &views_read));
    static httpd_uri_t views_write = {};
    views_write.uri = "/views";
    views_write.method = HTTP_PUT;
    views_write.handler = views_put;
    ESP_ERROR_CHECK(httpd_register_uri_handler(g_server, &views_write));
    static httpd_uri_t info = {};
    info.uri = "/info";
    info.method = HTTP_GET;
    info.handler = info_get;
    ESP_ERROR_CHECK(httpd_register_uri_handler(g_server, &info));
    sd_register(g_server);
    static httpd_uri_t ws = {};
    ws.uri = "/ws";
    ws.method = HTTP_GET;
    ws.handler = ws_handler;
    ws.is_websocket = true;
    ESP_ERROR_CHECK(httpd_register_uri_handler(g_server, &ws));
}

extern "C" void app_main(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        err = nvs_flash_init();
    }
    ESP_ERROR_CHECK(err);

    make_etag();
    clients_init();
    sd_start();
    g_feed = xStreamBufferCreate(8192, 1);
    wifi_start();
    web_start();
    can_init();
    // The CAN and feed tasks run on core 1; the WiFi stack lives on core 0.
    xTaskCreatePinnedToCore(can_task, "can", 4096, nullptr, 10, nullptr, 1);
    xTaskCreatePinnedToCore(feed_task, "feed", 4096, nullptr, 6, nullptr, 1);
}
