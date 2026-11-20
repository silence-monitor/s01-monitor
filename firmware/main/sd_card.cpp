// The SD card on the Atomic TF Card Base. See sd_card.h.
//
// How the pieces fit together:
//
//  * One manager task owns the card's life: it mounts it when there is one,
//    notices when it has gone, and unmounts it when a person ejects it.
//  * Everything else that uses the card first "enters" (a counter) and "leaves"
//    when done. The manager never unmounts while anyone is inside; it sets an
//    abort flag, the long operations notice it at their next chunk and leave.
//  * The web server's one task never does slow card work. Downloads, uploads,
//    deletes, formatting and ejecting are handed to a worker task (the request
//    is detached with httpd_req_async_handler_begin), one at a time. A second
//    one while the worker is busy is answered "busy".
//  * The recorder has a task of its own, so a long download cannot make it lose
//    frames. The CAN task only copies each frame into a message buffer.
//  * The CAN bus and the web server never wait for the card.

#include "sd_card.h"

#ifdef CONFIG_S01_HAS_SD

#include <atomic>
#include <cerrno>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dirent.h>
#include <initializer_list>
#include <sys/stat.h>
#include <sys/time.h>
#include <unistd.h>

#include "driver/sdspi_host.h"
#include "driver/spi_common.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "esp_vfs_fat.h"
#include "ff.h"
#include "diskio_sdmmc.h"
#include "freertos/FreeRTOS.h"
#include "freertos/message_buffer.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "nvs.h"
#include "sd_paths.h"
#include "sdmmc_cmd.h"

static const char *TAG = "s01-sd";

// ---- Settings ---------------------------------------------------------------

static constexpr const char *MOUNT = "/sdcard";
static constexpr size_t FULL_MAX = 256;  // the mount point and the longest path, with room to spare
static constexpr spi_host_device_t SD_HOST = SPI2_HOST;
static constexpr int SD_FREQ_KHZ = 10000;
// The base lists cards up to 16 GB. Real "16 GB" cards hold a little under
// 16e9 bytes; anything above this is a bigger card.
static constexpr uint64_t MAX_CARD_BYTES = 17000000000ull;
static constexpr size_t CHUNK = 4096;                 // transfer and write size
static constexpr size_t LOG_BUFFER_BYTES = 24 * 1024; // frames waiting for the recorder
static constexpr uint64_t FILE_LIMIT = 64ull * 1024 * 1024;  // a recording continues in a new file
static constexpr const char *RECORD_DIR = "/recordings";
static constexpr size_t LIST_LIMIT = 300;
static constexpr const char *NVS_NAMESPACE = "s01";
static constexpr const char *NVS_AUTO_KEY = "sdauto";
static constexpr const char *NVS_SEQ_KEY = "recseq";

// ---- State ------------------------------------------------------------------

enum class State : int {
    NoCard,        // nothing answered (no card, or no base)
    NoFilesystem,  // a card answered but has no file system we can read
    TooBig,        // a card over 16 GB
    Ejected,       // a person ejected it; it stays unmounted until Mount
    Ready,
    Formatting,
    Unmounting,
};

static const char *state_name(State s)
{
    switch (s) {
    case State::NoCard: return "nocard";
    case State::NoFilesystem: return "nofs";
    case State::TooBig: return "toobig";
    case State::Ejected: return "ejected";
    case State::Ready: return "ready";
    case State::Formatting: return "formatting";
    case State::Unmounting: return "unmounting";
    }
    return "nocard";
}

static std::atomic<int> g_state{static_cast<int>(State::NoCard)};
static std::atomic<int> g_users{0};         // tasks inside, using the card
static std::atomic<bool> g_abort{false};    // asks everyone inside to leave
static std::atomic<bool> g_user_eject{false};
static std::atomic<bool> g_mount_now{false};
// Set when a person stopped the recording, or it failed: "start by itself" waits
// until a card is (re)mounted or a person starts one.
static std::atomic<bool> g_hold{false};
static sdmmc_card_t *g_card = nullptr;
static bool g_bus_up = false;

static State state() { return static_cast<State>(g_state.load()); }
static void set_state(State s) { g_state.store(static_cast<int>(s)); }

// What the status page shows. Written by the manager, read by the web server.
static SemaphoreHandle_t g_info_lock = nullptr;
static char g_message[120] = "";
static char g_card_name[16] = "";
static char g_card_type[8] = "";
static uint64_t g_card_bytes = 0;
static uint64_t g_total_bytes = 0;
static uint64_t g_free_bytes = 0;
static bool g_info_valid = false;

static void set_message(const char *format, ...) __attribute__((format(printf, 1, 2)));
static void set_message(const char *format, ...)
{
    xSemaphoreTake(g_info_lock, portMAX_DELAY);
    va_list args;
    va_start(args, format);
    vsnprintf(g_message, sizeof g_message, format, args);
    va_end(args);
    xSemaphoreGive(g_info_lock);
}

// A task that uses the card is "inside" from enter() to leave().
static bool enter()
{
    g_users.fetch_add(1);
    if (state() != State::Ready) {
        g_users.fetch_sub(1);
        return false;
    }
    return true;
}

static void leave() { g_users.fetch_sub(1); }

static void wait_until_empty()
{
    int waited = 0;
    while (g_users.load() > 0 && waited < 10000) {
        vTaskDelay(pdMS_TO_TICKS(20));
        waited += 20;
    }
    if (g_users.load() > 0) {
        ESP_LOGE(TAG, "%d tasks were still using the card after 10 s", g_users.load());
    }
}

// ---- Clock ------------------------------------------------------------------
//
// The unit has no clock of its own. The browser app sends its local time (as
// seconds, taken as if it were UTC) and the unit counts from there until it is
// powered off. File dates and the recorder's names and times use it.

static std::atomic<bool> g_clock_valid{false};
static std::atomic<int64_t> g_wall_offset_ms{0};  // local time in ms minus uptime in ms
static std::atomic<unsigned> g_clock_epoch{0};    // changes when the clock jumps

static int64_t uptime_ms() { return esp_timer_get_time() / 1000; }

static uint64_t wall_ms(uint64_t uptime)
{
    return g_clock_valid.load() ? static_cast<uint64_t>(static_cast<int64_t>(uptime) + g_wall_offset_ms.load()) : uptime;
}

// ---- Mounting ---------------------------------------------------------------

static void quiet_the_card_drivers()
{
    // With no card these would log an error every few seconds.
    for (const char *tag : {"sdmmc_common", "sdmmc_init", "sdmmc_cmd", "sdspi_transaction", "vfs_fat_sdmmc", "sdspi_host"}) {
        esp_log_level_set(tag, ESP_LOG_NONE);
    }
}

static void read_space()
{
    uint64_t total = 0;
    uint64_t free_bytes = 0;
    bool ok = false;
    if (g_card != nullptr) {
        BYTE pdrv = ff_diskio_get_pdrv_card(g_card);
        if (pdrv != 0xFF) {
            char drive[3] = {static_cast<char>('0' + pdrv), ':', 0};
            FATFS *fs = nullptr;
            DWORD free_clusters = 0;
            if (f_getfree(drive, &free_clusters, &fs) == FR_OK && fs != nullptr) {
                uint64_t cluster_bytes = static_cast<uint64_t>(fs->csize) * fs->ssize;
                total = static_cast<uint64_t>(fs->n_fatent - 2) * cluster_bytes;
                free_bytes = static_cast<uint64_t>(free_clusters) * cluster_bytes;
                ok = true;
            }
        }
    }
    xSemaphoreTake(g_info_lock, portMAX_DELAY);
    g_total_bytes = total;
    g_free_bytes = free_bytes;
    g_info_valid = ok;
    xSemaphoreGive(g_info_lock);
}

// For a card that answered but has no file system we can read: asks the card
// itself how big it is, so a card over 16 GB is refused before anything is
// formatted, and the page can say what the card is. Returns 0 if it cannot tell.
static uint64_t probe_card_bytes(const sdspi_device_config_t &slot)
{
    esp_err_t err = sdspi_host_init();
    if (err != ESP_OK && err != ESP_ERR_INVALID_STATE) {
        return 0;
    }
    uint64_t bytes = 0;
    sdspi_dev_handle_t handle;
    if (sdspi_host_init_device(&slot, &handle) == ESP_OK) {
        sdmmc_host_t host = SDSPI_HOST_DEFAULT();
        host.slot = handle;
        host.max_freq_khz = SD_FREQ_KHZ;
        sdmmc_card_t *card = static_cast<sdmmc_card_t *>(calloc(1, sizeof(sdmmc_card_t)));
        if (card != nullptr && sdmmc_card_init(&host, card) == ESP_OK) {
            bytes = static_cast<uint64_t>(card->csd.capacity) * card->csd.sector_size;
        }
        free(card);
        sdspi_host_remove_device(handle);
    }
    sdspi_host_deinit();
    return bytes;
}

// Tries to start the card. `format` means: make a new file system if there is
// none (only ever asked for by a person). Returns the state it ended in.
static State mount_card(bool format)
{
    spi_bus_config_t bus = {};
    bus.mosi_io_num = CONFIG_S01_SD_MOSI_GPIO;
    bus.miso_io_num = CONFIG_S01_SD_MISO_GPIO;
    bus.sclk_io_num = CONFIG_S01_SD_CLK_GPIO;
    bus.quadwp_io_num = -1;
    bus.quadhd_io_num = -1;
    bus.max_transfer_sz = 4096;
    esp_err_t err = spi_bus_initialize(SD_HOST, &bus, SPI_DMA_CH_AUTO);
    if (err != ESP_OK && err != ESP_ERR_INVALID_STATE) {
        set_message("Could not start the SPI bus (%s)", esp_err_to_name(err));
        return State::NoCard;
    }
    g_bus_up = true;

    sdmmc_host_t host = SDSPI_HOST_DEFAULT();
    host.slot = SD_HOST;
    host.max_freq_khz = SD_FREQ_KHZ;
    sdspi_device_config_t slot = SDSPI_DEVICE_CONFIG_DEFAULT();
    slot.host_id = SD_HOST;
    slot.gpio_cs = SDSPI_SLOT_NO_CS;  // the base ties the card's chip select to ground
    esp_vfs_fat_mount_config_t config = {};
    config.format_if_mount_failed = format;
    config.max_files = 6;
    config.allocation_unit_size = 16 * 1024;
    sdmmc_card_t *card = nullptr;
    err = esp_vfs_fat_sdspi_mount(MOUNT, &host, &slot, &config, &card);
    if (err != ESP_OK) {
        uint64_t probed = err == ESP_FAIL ? probe_card_bytes(slot) : 0;
        spi_bus_free(SD_HOST);
        g_bus_up = false;
        if (err == ESP_FAIL) {
            if (probed > MAX_CARD_BYTES) {
                set_message("This card holds %.1f GB; only cards up to 16 GB are supported", static_cast<double>(probed) / 1e9);
                return State::TooBig;
            }
            if (probed > 0) {
                set_message("This %.1f GB card has no file system the unit can read (new, or made for another system). It can be formatted; everything on it would be erased.",
                            static_cast<double>(probed) / 1e9);
            } else {
                set_message("The card has no file system the unit can read. It can be formatted; everything on it would be erased.");
            }
            ESP_LOGI(TAG, "a card answered but has no readable file system%s", probed > 0 ? " (size read)" : "");
            return State::NoFilesystem;
        }
        set_message("No card found");
        return State::NoCard;
    }

    uint64_t bytes = static_cast<uint64_t>(card->csd.capacity) * card->csd.sector_size;
    if (bytes > MAX_CARD_BYTES) {
        esp_vfs_fat_sdcard_unmount(MOUNT, card);
        spi_bus_free(SD_HOST);
        g_bus_up = false;
        set_message("This card holds %.1f GB; only cards up to 16 GB are supported",
                    static_cast<double>(bytes) / 1e9);
        return State::TooBig;
    }
    g_card = card;
    g_hold.store(false);
    xSemaphoreTake(g_info_lock, portMAX_DELAY);
    snprintf(g_card_name, sizeof g_card_name, "%s", card->cid.name);
    snprintf(g_card_type, sizeof g_card_type, "%s", (card->ocr & SD_OCR_SDHC_CAP) ? "SDHC" : "SD");
    g_card_bytes = bytes;
    g_info_valid = false;
    g_message[0] = 0;
    xSemaphoreGive(g_info_lock);
    set_state(State::Ready);
    ESP_LOGI(TAG, "card ready: %s, %.1f GB", card->cid.name, static_cast<double>(bytes) / 1e9);
    return State::Ready;
}

// Waits for everyone to leave, then unmounts. The caller sets the state after.
static void unmount_card()
{
    set_state(State::Unmounting);
    g_abort.store(true);
    wait_until_empty();
    if (g_card != nullptr) {
        esp_vfs_fat_sdcard_unmount(MOUNT, g_card);
        g_card = nullptr;
    }
    if (g_bus_up) {
        spi_bus_free(SD_HOST);
        g_bus_up = false;
    }
    g_abort.store(false);
    xSemaphoreTake(g_info_lock, portMAX_DELAY);
    g_info_valid = false;
    g_card_name[0] = 0;
    g_card_type[0] = 0;
    g_card_bytes = 0;
    xSemaphoreGive(g_info_lock);
}

// ---- The recorder's controls (used by the status page too) -------------------

static std::atomic<bool> g_want_record{false};  // a person asked for a recording
static std::atomic<bool> g_auto_record{false};  // start one whenever a card is ready
static std::atomic<bool> g_rec_active{false};
static std::atomic<uint32_t> g_rec_frames{0};
static std::atomic<uint64_t> g_rec_bytes{0};
static std::atomic<uint32_t> g_dropped{0};      // frames the recorder could not keep
static char g_rec_path[sdp::PATH_MAX_LEN + 1] = "";   // the file being written, or the last one
static SemaphoreHandle_t g_rec_lock = nullptr;

static bool rec_path_is(const char *path)
{
    xSemaphoreTake(g_rec_lock, portMAX_DELAY);
    bool same = g_rec_active.load() && strcmp(path, g_rec_path) == 0;
    xSemaphoreGive(g_rec_lock);
    return same;
}

// True if the active recording's file is `path` or is inside the folder `path`.
static bool rec_path_inside(const char *path)
{
    xSemaphoreTake(g_rec_lock, portMAX_DELAY);
    bool inside = false;
    if (g_rec_active.load()) {
        size_t n = strlen(path);
        inside = strncmp(g_rec_path, path, n) == 0 && (g_rec_path[n] == 0 || g_rec_path[n] == '/' || sdp::is_root(path));
    }
    xSemaphoreGive(g_rec_lock);
    return inside;
}

// ---- Transfers: the worker task ------------------------------------------------

enum class Job : int { None = 0, Download, Upload, Delete, Format, Eject };

static const char *job_name(Job j)
{
    switch (j) {
    case Job::Download: return "download";
    case Job::Upload: return "upload";
    case Job::Delete: return "delete";
    case Job::Format: return "format";
    case Job::Eject: return "eject";
    case Job::None: break;
    }
    return "none";
}

struct JobRequest {
    Job kind;
    httpd_req_t *req;
    char path[sdp::PATH_MAX_LEN + 1];
};

static std::atomic<int> g_busy{static_cast<int>(Job::None)};
static std::atomic<uint64_t> g_done_bytes{0};
static std::atomic<uint64_t> g_total_job_bytes{0};
static QueueHandle_t g_jobs = nullptr;
static uint8_t g_buffer[CHUNK];   // the worker's transfer buffer: one job at a time

static void reply(httpd_req_t *req, const char *status, const char *text)
{
    httpd_resp_set_status(req, status);
    httpd_resp_set_type(req, "text/plain; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    httpd_resp_send(req, text, HTTPD_RESP_USE_STRLEN);
}

static void reply_ok(httpd_req_t *req)
{
    httpd_resp_set_status(req, "204 No Content");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    httpd_resp_send(req, nullptr, 0);
}

// A request for something that changes the card: only from our own page.
static bool allowed(httpd_req_t *req)
{
    if (!origin_is_ours(req)) {
        reply(req, "403 Forbidden", "not from this device's own page");
        return false;
    }
    return true;
}

static bool path_param(httpd_req_t *req, char *out, size_t capacity)
{
    char query[320];
    if (httpd_req_get_url_query_str(req, query, sizeof query) != ESP_OK ||
        !sdp::query_value(query, "path", out, capacity) || !sdp::path_ok(out)) {
        reply(req, "400 Bad Request", "bad path");
        return false;
    }
    return true;
}

static void full_path(const char *path, char *out, size_t capacity)
{
    if (sdp::is_root(path)) {
        snprintf(out, capacity, "%s", MOUNT);
    } else {
        snprintf(out, capacity, "%s%s", MOUNT, path);
    }
}

static void reply_not_ready(httpd_req_t *req)
{
    reply(req, "503 Service Unavailable", "the card is not ready");
}

// Starts a job on the worker, or says why not. The request is handed over.
static esp_err_t start_job(httpd_req_t *req, Job kind, const char *path)
{
    int expected = static_cast<int>(Job::None);
    if (!g_busy.compare_exchange_strong(expected, static_cast<int>(kind))) {
        reply(req, "409 Conflict", "busy with another job");
        return ESP_OK;
    }
    httpd_req_t *copy = nullptr;
    if (httpd_req_async_handler_begin(req, &copy) != ESP_OK) {
        g_busy.store(static_cast<int>(Job::None));
        reply(req, "500 Internal Server Error", "could not start the job");
        return ESP_OK;
    }
    JobRequest job = {};
    job.kind = kind;
    job.req = copy;
    snprintf(job.path, sizeof job.path, "%s", path);
    g_done_bytes.store(0);
    g_total_job_bytes.store(0);
    if (xQueueSend(g_jobs, &job, 0) != pdTRUE) {
        g_busy.store(static_cast<int>(Job::None));
        reply(copy, "500 Internal Server Error", "the job queue is full");
        httpd_req_async_handler_complete(copy);
    }
    return ESP_OK;
}

static void drop_connection(httpd_req_t *req)
{
    httpd_sess_trigger_close(req->handle, httpd_req_to_sockfd(req));
}

static void do_download(httpd_req_t *req, const char *path)
{
    if (!enter()) {
        reply_not_ready(req);
        return;
    }
    char full[FULL_MAX];
    full_path(path, full, sizeof full);
    struct stat info;
    if (stat(full, &info) != 0 || S_ISDIR(info.st_mode)) {
        leave();
        reply(req, "404 Not Found", "no such file");
        return;
    }
    if (rec_path_is(path)) {
        leave();
        reply(req, "409 Conflict", "this file is being recorded; stop the recording first");
        return;
    }
    FILE *file = fopen(full, "rb");
    if (file == nullptr) {
        leave();
        reply(req, "500 Internal Server Error", "could not open the file");
        return;
    }
    g_total_job_bytes.store(static_cast<uint64_t>(info.st_size));
    char disposition[sdp::PATH_MAX_LEN + 48];
    snprintf(disposition, sizeof disposition, "attachment; filename=\"%s\"", sdp::leaf_of(path));
    httpd_resp_set_type(req, "application/octet-stream");
    httpd_resp_set_hdr(req, "Content-Disposition", disposition);
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    bool complete = true;
    for (;;) {
        size_t n = fread(g_buffer, 1, sizeof g_buffer, file);
        if (n == 0) {
            complete = !ferror(file);
            break;
        }
        if (g_abort.load() || httpd_resp_send_chunk(req, reinterpret_cast<const char *>(g_buffer), n) != ESP_OK) {
            complete = false;
            break;
        }
        g_done_bytes.fetch_add(n);
        vTaskDelay(1);  // give the live feed and the recorder their turn
    }
    fclose(file);
    leave();
    if (complete) {
        httpd_resp_send_chunk(req, nullptr, 0);
    } else {
        drop_connection(req);  // a cut-off file must look cut off, not finished
    }
}

static void do_upload(httpd_req_t *req, const char *path)
{
    size_t total = req->content_len;
    if (!enter()) {
        reply_not_ready(req);
        drop_connection(req);
        return;
    }
    char full[FULL_MAX];
    full_path(path, full, sizeof full);
    char folder[sdp::PATH_MAX_LEN + 1];
    char folder_full[FULL_MAX];
    struct stat info;
    if (!sdp::parent_of(path, folder, sizeof folder)) {
        leave();
        reply(req, "400 Bad Request", "bad path");
        drop_connection(req);
        return;
    }
    full_path(folder, folder_full, sizeof folder_full);
    if (stat(folder_full, &info) != 0 || !S_ISDIR(info.st_mode)) {
        leave();
        reply(req, "404 Not Found", "that folder does not exist");
        drop_connection(req);
        return;
    }
    if ((stat(full, &info) == 0 && S_ISDIR(info.st_mode)) || rec_path_is(path)) {
        leave();
        reply(req, "409 Conflict", "that name is a folder or a recording in progress");
        drop_connection(req);
        return;
    }
    read_space();
    xSemaphoreTake(g_info_lock, portMAX_DELAY);
    uint64_t free_bytes = g_free_bytes;
    bool known = g_info_valid;
    xSemaphoreGive(g_info_lock);
    if (known && total + CHUNK > free_bytes) {
        leave();
        reply(req, "507 Insufficient Storage", "not enough free space on the card");
        drop_connection(req);
        return;
    }
    char part[sizeof full + 8];
    snprintf(part, sizeof part, "%s.part", full);
    FILE *file = fopen(part, "wb");
    if (file == nullptr) {
        leave();
        reply(req, "500 Internal Server Error", "could not create the file");
        drop_connection(req);
        return;
    }
    g_total_job_bytes.store(total);
    size_t got = 0;
    int stalls = 0;
    bool ok = true;
    const char *problem = "the upload was cut off";
    while (got < total) {
        if (g_abort.load()) {
            ok = false;
            problem = "the card was removed or ejected";
            break;
        }
        size_t want = total - got < sizeof g_buffer ? total - got : sizeof g_buffer;
        int n = httpd_req_recv(req, reinterpret_cast<char *>(g_buffer), want);
        if (n == HTTPD_SOCK_ERR_TIMEOUT) {
            if (++stalls < 6) {
                continue;
            }
            ok = false;
            break;
        }
        if (n <= 0) {
            ok = false;
            break;
        }
        stalls = 0;
        if (fwrite(g_buffer, 1, static_cast<size_t>(n), file) != static_cast<size_t>(n)) {
            ok = false;
            problem = errno == ENOSPC ? "the card is full" : "writing to the card failed";
            break;
        }
        got += static_cast<size_t>(n);
        g_done_bytes.store(got);
        vTaskDelay(1);
    }
    if (ok) {
        fflush(file);
        fsync(fileno(file));
    }
    fclose(file);
    if (ok) {
        unlink(full);
        if (rename(part, full) != 0) {
            ok = false;
            problem = "could not finish the file";
        }
    }
    if (!ok) {
        unlink(part);
    }
    leave();
    read_space();
    if (ok) {
        ESP_LOGI(TAG, "uploaded %s, %u bytes", path, static_cast<unsigned>(total));
        reply_ok(req);
    } else {
        ESP_LOGW(TAG, "upload of %s failed: %s", path, problem);
        reply(req, "500 Internal Server Error", problem);
        drop_connection(req);
    }
}

// Removes a file, or a folder with everything in it. False if something in it
// could not be removed or the card went away.
static bool remove_tree(const char *full, int depth)
{
    struct stat info;
    if (stat(full, &info) != 0) {
        return false;
    }
    if (!S_ISDIR(info.st_mode)) {
        return unlink(full) == 0;
    }
    if (depth > 8) {
        return false;
    }
    DIR *dir = opendir(full);
    if (dir == nullptr) {
        return false;
    }
    bool ok = true;
    while (ok) {
        if (g_abort.load()) {
            ok = false;
            break;
        }
        struct dirent *entry = readdir(dir);
        if (entry == nullptr) {
            break;
        }
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        char child[FULL_MAX];
        if (snprintf(child, sizeof child, "%s/%s", full, entry->d_name) >= static_cast<int>(sizeof child)) {
            ok = false;
            break;
        }
        ok = remove_tree(child, depth + 1);
    }
    closedir(dir);
    return ok && rmdir(full) == 0;
}

static void do_delete(httpd_req_t *req, const char *path)
{
    if (!enter()) {
        reply_not_ready(req);
        return;
    }
    if (sdp::is_root(path)) {
        leave();
        reply(req, "400 Bad Request", "the whole card cannot be deleted this way; use Format");
        return;
    }
    if (rec_path_inside(path)) {
        leave();
        reply(req, "409 Conflict", "a recording is being written there; stop it first");
        return;
    }
    char full[FULL_MAX];
    full_path(path, full, sizeof full);
    struct stat info;
    if (stat(full, &info) != 0) {
        leave();
        reply(req, "404 Not Found", "no such file or folder");
        return;
    }
    bool ok = remove_tree(full, 0);
    leave();
    read_space();
    if (ok) {
        ESP_LOGI(TAG, "deleted %s", path);
        reply_ok(req);
    } else {
        reply(req, "500 Internal Server Error", "could not delete everything");
    }
}

// Formatting takes a while (tens of seconds on a big card), so the request that
// asked for it is answered at once (see format_post) and the page watches the
// status. The result is left in the status message.
static void do_format()
{
    State before = state();
    if (before != State::Ready && before != State::NoFilesystem) {
        set_message("The card is not ready to be formatted");
        return;
    }
    ESP_LOGW(TAG, "formatting the card");
    set_message("Formatting the card...");
    esp_err_t err = ESP_OK;
    if (before == State::Ready) {
        set_state(State::Formatting);
        g_abort.store(true);
        wait_until_empty();
        g_abort.store(false);
        esp_vfs_fat_mount_config_t config = {};
        config.max_files = 6;
        config.allocation_unit_size = 16 * 1024;
        err = esp_vfs_fat_sdcard_format_cfg(MOUNT, g_card, &config);
        unmount_card();
    } else {
        set_state(State::Formatting);
    }
    State after = mount_card(before == State::NoFilesystem);
    if (after == State::Ready) {
        read_space();
    }
    set_state(after);
    if (err == ESP_OK && after == State::Ready) {
        set_message("Formatted.");
        ESP_LOGI(TAG, "formatted");
    } else {
        set_message("Formatting failed (%s)", esp_err_to_name(err == ESP_OK ? ESP_FAIL : err));
        ESP_LOGW(TAG, "formatting failed");
    }
}

static void do_eject(httpd_req_t *req)
{
    g_want_record.store(false);
    g_hold.store(true);
    g_user_eject.store(true);
    unmount_card();
    set_state(State::Ejected);
    set_message("Ejected. It is safe to take the card out.");
    ESP_LOGI(TAG, "card ejected");
    reply_ok(req);
}

static void worker_task(void *)
{
    for (;;) {
        JobRequest job;
        if (xQueueReceive(g_jobs, &job, portMAX_DELAY) != pdTRUE) {
            continue;
        }
        switch (job.kind) {
        case Job::Download: do_download(job.req, job.path); break;
        case Job::Upload: do_upload(job.req, job.path); break;
        case Job::Delete: do_delete(job.req, job.path); break;
        case Job::Format: do_format(); break;
        case Job::Eject: do_eject(job.req); break;
        case Job::None: break;
        }
        if (job.req != nullptr) {
            httpd_req_async_handler_complete(job.req);
        }
        g_busy.store(static_cast<int>(Job::None));
    }
}

// ---- The manager task ----------------------------------------------------------

static void manager_task(void *)
{
    int64_t last_try = -100000;
    int64_t last_check = 0;
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(250));
        State s = state();
        int64_t now = uptime_ms();
        if (g_busy.load() != static_cast<int>(Job::None) && g_busy.load() != static_cast<int>(Job::Download) &&
            g_busy.load() != static_cast<int>(Job::Upload)) {
            continue;  // a format, eject or delete is changing things
        }
        if (s == State::NoCard || s == State::NoFilesystem || s == State::TooBig) {
            int64_t wait = s == State::NoCard ? 3000 : 10000;
            if (!g_user_eject.load() && (g_mount_now.exchange(false) || now - last_try >= wait)) {
                last_try = now;
                State result = mount_card(false);
                set_state(result);
                if (result == State::Ready) {
                    read_space();
                } else if (result != s) {
                    ESP_LOGI(TAG, "card: %s", state_name(result));
                }
            }
        } else if (s == State::Ejected) {
            if (g_mount_now.exchange(false)) {
                g_user_eject.store(false);
                set_state(State::NoCard);
                last_try = -100000;
            }
        } else if (s == State::Ready) {
            if (!g_info_valid && enter()) {
                read_space();
                leave();
            }
            if (now - last_check >= 2000) {
                last_check = now;
                if (g_card != nullptr && sdmmc_get_status(g_card) != ESP_OK) {
                    ESP_LOGW(TAG, "the card stopped answering");
                    unmount_card();
                    set_state(State::NoCard);
                    set_message("The card was removed or stopped answering");
                }
            }
        }
    }
}

// ---- The recorder ---------------------------------------------------------------

struct LogRecord {
    uint64_t uptime;
    uint32_t id;
    uint8_t dlc;
    uint8_t data[8];
};

struct MarkRecord {
    uint64_t uptime;
    char text[64];
};

static MessageBufferHandle_t g_log = nullptr;
static QueueHandle_t g_marks = nullptr;  // markers waiting to go into the recording
static std::atomic<bool> g_log_open{false};  // the recorder wants frames

void sd_log_frame(const twai_message_t &message)
{
    if (!g_log_open.load(std::memory_order_relaxed)) {
        return;
    }
    LogRecord record = {};
    record.uptime = static_cast<uint64_t>(uptime_ms());
    record.id = message.identifier;
    record.dlc = message.data_length_code > 8 ? 8 : message.data_length_code;
    memcpy(record.data, message.data, record.dlc);
    if (xMessageBufferSend(g_log, &record, sizeof record, 0) == 0) {
        g_dropped.fetch_add(1);
    }
}

static unsigned next_sequence()
{
    unsigned sequence = 1;
    nvs_handle_t handle;
    if (nvs_open(NVS_NAMESPACE, NVS_READWRITE, &handle) == ESP_OK) {
        uint32_t stored = 0;
        if (nvs_get_u32(handle, NVS_SEQ_KEY, &stored) == ESP_OK) {
            sequence = stored + 1;
        }
        nvs_set_u32(handle, NVS_SEQ_KEY, sequence);
        nvs_commit(handle);
        nvs_close(handle);
    }
    return sequence;
}

// Opens a new recording file and writes its header. Returns null on failure.
static FILE *open_recording(char *path_out, size_t capacity)
{
    char full[FULL_MAX];
    snprintf(full, sizeof full, "%s%s", MOUNT, RECORD_DIR);
    mkdir(full, 0777);  // fine if it is already there
    char name[40];
    bool known = g_clock_valid.load();
    int64_t local_seconds = (uptime_ms() + g_wall_offset_ms.load()) / 1000;
    sdp::record_name(name, sizeof name, known, local_seconds, known ? 0 : next_sequence());
    snprintf(path_out, capacity, "%s/%s", RECORD_DIR, name);
    snprintf(full, sizeof full, "%s%s", MOUNT, path_out);
    FILE *file = fopen(full, "ab");
    if (file == nullptr) {
        return nullptr;
    }
    char started[24] = "at power-up";
    if (known) {
        sdp::date_text(started, sizeof started, local_seconds);
    }
    char header[200];
    int n = snprintf(header, sizeof header,
                     "# recording from the unit's SD card, 250 kbit/s, started %s\n"
                     "# times are the clock time of the device that set the unit's clock, or time since the unit started\n",
                     started);
    fwrite(header, 1, static_cast<size_t>(n), file);
    return file;
}

static void recorder_run()
{
    static char chunk[CHUNK + 256];
    size_t pending = 0;
    FILE *file = nullptr;
    uint64_t file_bytes = 0;
    int64_t last_flush = uptime_ms();
    unsigned clock_epoch = g_clock_epoch.load();
    uint32_t dropped_seen = g_dropped.load();
    bool failed = false;

    auto flush = [&]() {
        if (file != nullptr && pending > 0) {
            if (fwrite(chunk, 1, pending, file) != pending) {
                failed = true;
                set_message(errno == ENOSPC ? "Recording stopped: the card is full" : "Recording stopped: writing to the card failed");
            }
            file_bytes += pending;
            g_rec_bytes.fetch_add(pending);
            pending = 0;
        }
        if (file != nullptr) {
            fflush(file);
            fsync(fileno(file));
        }
        last_flush = uptime_ms();
    };
    auto close_file = [&]() {
        flush();
        if (file != nullptr) {
            fclose(file);
            file = nullptr;
        }
        xSemaphoreTake(g_rec_lock, portMAX_DELAY);
        g_rec_active.store(false);
        xSemaphoreGive(g_rec_lock);
    };

    g_log_open.store(true);
    while (!failed && g_want_record.load() && !g_abort.load() && state() == State::Ready) {
        LogRecord record;
        size_t got = xMessageBufferReceive(g_log, &record, sizeof record, pdMS_TO_TICKS(200));
        if (got == sizeof record) {
            if (file != nullptr && (clock_epoch != g_clock_epoch.load() || file_bytes + pending > FILE_LIMIT)) {
                close_file();  // the times would jump, or the file is big: start a new one
            }
            if (file == nullptr) {
                clock_epoch = g_clock_epoch.load();
                char path[sdp::PATH_MAX_LEN + 1];
                file = open_recording(path, sizeof path);
                if (file == nullptr) {
                    set_message("Recording stopped: could not create a file on the card");
                    failed = true;
                    break;
                }
                xSemaphoreTake(g_rec_lock, portMAX_DELAY);
                snprintf(g_rec_path, sizeof g_rec_path, "%s", path);
                g_rec_active.store(true);
                xSemaphoreGive(g_rec_lock);
                file_bytes = 0;
                ESP_LOGI(TAG, "recording to %s", path);
            }
            uint32_t dropped_now = g_dropped.load();
            if (dropped_now != dropped_seen && pending + 80 < sizeof chunk) {
                pending += static_cast<size_t>(snprintf(chunk + pending, sizeof chunk - pending,
                                                        "# %lu frames were lost here (the recorder fell behind)\n",
                                                        static_cast<unsigned long>(dropped_now - dropped_seen)));
                dropped_seen = dropped_now;
            }
            pending += sdp::can_line(chunk + pending, sizeof chunk - pending, wall_ms(record.uptime), record.id, record.dlc, record.data);
            g_rec_frames.fetch_add(1);
        }
        // Markers a person dropped in the page go into the file in the order they come.
        MarkRecord mark;
        while (file != nullptr && pending + 160 < sizeof chunk && xQueueReceive(g_marks, &mark, 0) == pdTRUE) {
            pending += sdp::mark_line(chunk + pending, sizeof chunk - pending, wall_ms(mark.uptime), mark.text);
        }
        if (pending >= CHUNK || (pending > 0 && uptime_ms() - last_flush >= 1000)) {
            flush();
        }
    }
    g_log_open.store(false);
    close_file();
    // Anything still waiting belongs to a recording that has ended.
    LogRecord discard;
    while (xMessageBufferReceive(g_log, &discard, sizeof discard, 0) > 0) {
    }
    if (failed) {
        g_want_record.store(false);
        g_hold.store(true);
    }
}

static void recorder_task(void *)
{
    for (;;) {
        if (g_want_record.load() && state() == State::Ready && enter()) {
            recorder_run();
            leave();
        } else if (g_auto_record.load() && !g_hold.load() && !g_want_record.load() && state() == State::Ready && !g_user_eject.load()) {
            g_want_record.store(true);  // "start by itself" is on: record as soon as a card is ready
        }
        vTaskDelay(pdMS_TO_TICKS(200));
    }
}

// ---- Web endpoints ---------------------------------------------------------------

static esp_err_t time_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char query[64];
    char value[24];
    if (httpd_req_get_url_query_str(req, query, sizeof query) != ESP_OK || !sdp::query_value(query, "local", value, sizeof value)) {
        reply(req, "400 Bad Request", "no time");
        return ESP_OK;
    }
    char *end = nullptr;
    long long seconds = strtoll(value, &end, 10);
    if (end == value || *end != 0 || seconds < 1700000000LL || seconds > 4102444800LL) {
        reply(req, "400 Bad Request", "not a usable time");
        return ESP_OK;
    }
    int64_t offset = seconds * 1000 - uptime_ms();
    bool was_valid = g_clock_valid.load();
    int64_t before = g_wall_offset_ms.load();
    g_wall_offset_ms.store(offset);
    g_clock_valid.store(true);
    if (!was_valid || (offset > before ? offset - before : before - offset) > 2000) {
        g_clock_epoch.fetch_add(1);
    }
    struct timeval now = {static_cast<time_t>(seconds), 0};
    settimeofday(&now, nullptr);  // file dates on the card come from this
    reply_ok(req);
    return ESP_OK;
}

static esp_err_t status_get(httpd_req_t *req)
{
    char text[900];
    State s = state();
    xSemaphoreTake(g_info_lock, portMAX_DELAY);
    int n = snprintf(text, sizeof text,
                     "state %s\n"
                     "message %s\n"
                     "card_name %s\n"
                     "card_type %s\n"
                     "card_bytes %llu\n"
                     "total_bytes %llu\n"
                     "free_bytes %llu\n"
                     "space_known %d\n",
                     state_name(s), g_message, g_card_name, g_card_type,
                     static_cast<unsigned long long>(g_card_bytes), static_cast<unsigned long long>(g_total_bytes),
                     static_cast<unsigned long long>(g_free_bytes), g_info_valid ? 1 : 0);
    xSemaphoreGive(g_info_lock);
    xSemaphoreTake(g_rec_lock, portMAX_DELAY);
    n += snprintf(text + n, sizeof text - n,
                  "busy %s\n"
                  "busy_done %llu\n"
                  "busy_total %llu\n"
                  "recording %d\n"
                  "record_wanted %d\n"
                  "record_file %s\n"
                  "record_frames %lu\n"
                  "record_bytes %llu\n"
                  "record_lost %lu\n"
                  "auto %d\n"
                  "clock %d\n"
                  "heap_free %u\n",
                  job_name(static_cast<Job>(g_busy.load())),
                  static_cast<unsigned long long>(g_done_bytes.load()),
                  static_cast<unsigned long long>(g_total_job_bytes.load()),
                  g_rec_active.load() ? 1 : 0, g_want_record.load() ? 1 : 0, g_rec_path,
                  static_cast<unsigned long>(g_rec_frames.load()), static_cast<unsigned long long>(g_rec_bytes.load()),
                  static_cast<unsigned long>(g_dropped.load()), g_auto_record.load() ? 1 : 0,
                  g_clock_valid.load() ? 1 : 0, static_cast<unsigned>(esp_get_free_heap_size()));
    xSemaphoreGive(g_rec_lock);
    httpd_resp_set_type(req, "text/plain; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    return httpd_resp_send(req, text, n);
}

static esp_err_t list_get(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char path[sdp::PATH_MAX_LEN + 1];
    if (!path_param(req, path, sizeof path)) {
        return ESP_OK;
    }
    if (!enter()) {
        reply_not_ready(req);
        return ESP_OK;
    }
    char full[FULL_MAX];
    full_path(path, full, sizeof full);
    DIR *dir = opendir(full);
    if (dir == nullptr) {
        leave();
        reply(req, "404 Not Found", "no such folder");
        return ESP_OK;
    }
    httpd_resp_set_type(req, "text/plain; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    char line[sdp::NAME_MAX_LEN + 80];
    int n = snprintf(line, sizeof line, "path %s\n", path);
    httpd_resp_send_chunk(req, line, n);
    size_t count = 0;
    bool more = false;
    struct dirent *entry;
    while ((entry = readdir(dir)) != nullptr) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        if (count >= LIST_LIMIT) {
            more = true;
            break;
        }
        char child[FULL_MAX];
        if (snprintf(child, sizeof child, "%s/%s", full, entry->d_name) >= static_cast<int>(sizeof child)) {
            continue;
        }
        struct stat info;
        if (stat(child, &info) != 0 || !sdp::name_ok(entry->d_name, strlen(entry->d_name))) {
            continue;  // a name this firmware would not accept is not offered
        }
        char date[24];
        sdp::date_text(date, sizeof date, static_cast<int64_t>(info.st_mtime));
        n = snprintf(line, sizeof line, "%c\t%s\t%llu\t%s\n", S_ISDIR(info.st_mode) ? 'd' : 'f', entry->d_name,
                     S_ISDIR(info.st_mode) ? 0ull : static_cast<unsigned long long>(info.st_size), date);
        if (httpd_resp_send_chunk(req, line, n) != ESP_OK) {
            closedir(dir);
            leave();
            return ESP_FAIL;
        }
        ++count;
    }
    closedir(dir);
    leave();
    if (more) {
        httpd_resp_send_chunk(req, "more 1\n", HTTPD_RESP_USE_STRLEN);
    }
    return httpd_resp_send_chunk(req, nullptr, 0);
}

static esp_err_t file_get(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char path[sdp::PATH_MAX_LEN + 1];
    if (!path_param(req, path, sizeof path)) {
        return ESP_OK;
    }
    return start_job(req, Job::Download, path);
}

static esp_err_t file_put(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char path[sdp::PATH_MAX_LEN + 1];
    if (!path_param(req, path, sizeof path)) {
        return ESP_FAIL;  // the body is unread, so close the connection
    }
    if (sdp::is_root(path)) {
        reply(req, "400 Bad Request", "that is the card's root, not a file");
        return ESP_FAIL;
    }
    return start_job(req, Job::Upload, path);
}

static esp_err_t delete_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char path[sdp::PATH_MAX_LEN + 1];
    if (!path_param(req, path, sizeof path)) {
        return ESP_OK;
    }
    return start_job(req, Job::Delete, path);
}

static esp_err_t mkdir_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char path[sdp::PATH_MAX_LEN + 1];
    if (!path_param(req, path, sizeof path)) {
        return ESP_OK;
    }
    if (sdp::is_root(path)) {
        reply(req, "400 Bad Request", "the root exists already");
        return ESP_OK;
    }
    if (!enter()) {
        reply_not_ready(req);
        return ESP_OK;
    }
    char full[FULL_MAX];
    full_path(path, full, sizeof full);
    char folder[sdp::PATH_MAX_LEN + 1];
    char folder_full[FULL_MAX];
    struct stat info;
    bool parent_ok = sdp::parent_of(path, folder, sizeof folder);
    if (parent_ok) {
        full_path(folder, folder_full, sizeof folder_full);
        parent_ok = stat(folder_full, &info) == 0 && S_ISDIR(info.st_mode);
    }
    int result = parent_ok ? mkdir(full, 0777) : -1;
    int error = errno;
    leave();
    if (!parent_ok) {
        reply(req, "404 Not Found", "the folder above does not exist");
    } else if (result == 0) {
        reply_ok(req);
    } else if (error == EEXIST) {
        reply(req, "409 Conflict", "that name is already used");
    } else {
        reply(req, "500 Internal Server Error", "could not create the folder");
    }
    return ESP_OK;
}

static esp_err_t format_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    State s = state();
    if (g_want_record.load() || g_rec_active.load()) {
        reply(req, "409 Conflict", "stop the recording first");
        return ESP_OK;
    }
    if (s != State::Ready && s != State::NoFilesystem) {
        reply_not_ready(req);
        return ESP_OK;
    }
    // Answered at once; the job carries on without a request and the page
    // follows it in the status.
    int expected = static_cast<int>(Job::None);
    if (!g_busy.compare_exchange_strong(expected, static_cast<int>(Job::Format))) {
        reply(req, "409 Conflict", "busy with another job");
        return ESP_OK;
    }
    JobRequest job = {};
    job.kind = Job::Format;
    job.req = nullptr;
    snprintf(job.path, sizeof job.path, "/");
    if (xQueueSend(g_jobs, &job, 0) != pdTRUE) {
        g_busy.store(static_cast<int>(Job::None));
        reply(req, "500 Internal Server Error", "the job queue is full");
        return ESP_OK;
    }
    reply_ok(req);
    return ESP_OK;
}

static esp_err_t eject_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    if (state() != State::Ready) {
        reply(req, "409 Conflict", "there is no card ready to eject");
        return ESP_OK;
    }
    return start_job(req, Job::Eject, "/");
}

static esp_err_t mount_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    g_user_eject.store(false);
    g_mount_now.store(true);
    reply_ok(req);
    return ESP_OK;
}

static esp_err_t record_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char query[32];
    char value[4];
    if (httpd_req_get_url_query_str(req, query, sizeof query) != ESP_OK || !sdp::query_value(query, "on", value, sizeof value)) {
        reply(req, "400 Bad Request", "on=1 or on=0");
        return ESP_OK;
    }
    if (value[0] == '1') {
        if (state() != State::Ready) {
            reply(req, "409 Conflict", "there is no card ready to record to");
            return ESP_OK;
        }
        if (g_busy.load() == static_cast<int>(Job::Format)) {
            reply(req, "409 Conflict", "the card is being formatted");
            return ESP_OK;
        }
        g_hold.store(false);
        g_want_record.store(true);
    } else {
        g_want_record.store(false);
        g_hold.store(true);  // so "start by itself" does not start it again at once
    }
    reply_ok(req);
    return ESP_OK;
}

static esp_err_t mark_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char query[300];
    MarkRecord mark = {};
    if (httpd_req_get_url_query_str(req, query, sizeof query) != ESP_OK || !sdp::query_value(query, "text", mark.text, sizeof mark.text)) {
        reply(req, "400 Bad Request", "no text, or too long (at most 63 characters)");
        return ESP_OK;
    }
    if (!g_rec_active.load()) {
        reply(req, "409 Conflict", "nothing is being recorded");
        return ESP_OK;
    }
    mark.uptime = static_cast<uint64_t>(uptime_ms());
    if (xQueueSend(g_marks, &mark, 0) != pdTRUE) {
        reply(req, "503 Service Unavailable", "too many markers at once");
        return ESP_OK;
    }
    reply_ok(req);
    return ESP_OK;
}

static esp_err_t auto_post(httpd_req_t *req)
{
    if (!allowed(req)) {
        return ESP_OK;
    }
    char query[32];
    char value[4];
    if (httpd_req_get_url_query_str(req, query, sizeof query) != ESP_OK || !sdp::query_value(query, "on", value, sizeof value)) {
        reply(req, "400 Bad Request", "on=1 or on=0");
        return ESP_OK;
    }
    bool on = value[0] == '1';
    g_auto_record.store(on);
    nvs_handle_t handle;
    if (nvs_open(NVS_NAMESPACE, NVS_READWRITE, &handle) == ESP_OK) {
        nvs_set_u8(handle, NVS_AUTO_KEY, on ? 1 : 0);
        nvs_commit(handle);
        nvs_close(handle);
    }
    reply_ok(req);
    return ESP_OK;
}

static void add_route(httpd_handle_t server, const char *uri, httpd_method_t method, esp_err_t (*handler)(httpd_req_t *))
{
    static httpd_uri_t routes[16];
    static int used = 0;
    httpd_uri_t &route = routes[used++];
    route = {};
    route.uri = uri;
    route.method = method;
    route.handler = handler;
    ESP_ERROR_CHECK(httpd_register_uri_handler(server, &route));
}

void sd_register(httpd_handle_t server)
{
    add_route(server, "/time", HTTP_POST, time_post);
    add_route(server, "/sd/status", HTTP_GET, status_get);
    add_route(server, "/sd/list", HTTP_GET, list_get);
    add_route(server, "/sd/file", HTTP_GET, file_get);
    add_route(server, "/sd/file", HTTP_PUT, file_put);
    add_route(server, "/sd/delete", HTTP_POST, delete_post);
    add_route(server, "/sd/mkdir", HTTP_POST, mkdir_post);
    add_route(server, "/sd/format", HTTP_POST, format_post);
    add_route(server, "/sd/eject", HTTP_POST, eject_post);
    add_route(server, "/sd/mount", HTTP_POST, mount_post);
    add_route(server, "/sd/record", HTTP_POST, record_post);
    add_route(server, "/sd/auto", HTTP_POST, auto_post);
    add_route(server, "/sd/mark", HTTP_POST, mark_post);
}

void sd_start()
{
    g_info_lock = xSemaphoreCreateMutex();
    g_rec_lock = xSemaphoreCreateMutex();
    g_jobs = xQueueCreate(1, sizeof(JobRequest));
    g_log = xMessageBufferCreate(LOG_BUFFER_BYTES);
    g_marks = xQueueCreate(8, sizeof(MarkRecord));
    quiet_the_card_drivers();
    nvs_handle_t handle;
    uint8_t automatic = 0;
    if (nvs_open(NVS_NAMESPACE, NVS_READONLY, &handle) == ESP_OK) {
        nvs_get_u8(handle, NVS_AUTO_KEY, &automatic);
        nvs_close(handle);
    }
    g_auto_record.store(automatic != 0);
    set_message("Looking for a card...");
    ESP_LOGI(TAG, "SD card: MOSI GPIO%d, MISO GPIO%d, CLK GPIO%d, no chip select; start recording by itself: %s",
             CONFIG_S01_SD_MOSI_GPIO, CONFIG_S01_SD_MISO_GPIO, CONFIG_S01_SD_CLK_GPIO, automatic ? "on" : "off");
    // The card tasks run on core 0 beside the WiFi stack, below the web server in priority.
    xTaskCreatePinnedToCore(manager_task, "sd-mgr", 7168, nullptr, 4, nullptr, 0);
    xTaskCreatePinnedToCore(worker_task, "sd-work", 8192, nullptr, 3, nullptr, 0);
    xTaskCreatePinnedToCore(recorder_task, "sd-rec", 6144, nullptr, 3, nullptr, 0);
}

#endif  // CONFIG_S01_HAS_SD
