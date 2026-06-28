/*
 * image_consumer.c — C ABI integration test for libwkhtmltox (image side).
 *
 * Copyright 2026 wkhtmltopdf authors.
 * Licensed under the GNU Lesser General Public License, version 3 or later.
 * See <http://www.gnu.org/licenses/>.
 *
 * Exercises the full wkhtmltoimage_* C ABI:
 *   init → global settings (fmt=png, in=<tmp html>) → create converter →
 *   set callbacks → convert → get_output (assert \x89PNG) →
 *   string-lifetime check → destroy → deinit.
 *
 * Compile (from workspace root, after `cargo build -p wkhtmltox-capi`):
 *
 *   clang \
 *       -Icrates/wkhtmltox-capi/include \
 *       -Icrates/wkhtmltox-capi/ctest \
 *       crates/wkhtmltox-capi/ctest/image_consumer.c \
 *       -Ltarget/debug -lwkhtmltox \
 *       -Wl,-rpath,target/debug \
 *       -o target/debug/image_consumer
 *
 * Exit 0 on PASS, non-zero on FAIL.
 */

#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include "image.h"

/* -------------------------------------------------------------------------
 * Global state set by callbacks.
 * ------------------------------------------------------------------------- */

static int g_finished_ok = -1; /* -1 = not called */
static int g_error_fired = 0;

static void on_finished(wkhtmltoimage_converter *c, const int ok)
{
    (void)c;
    g_finished_ok = ok;
}

static void on_error(wkhtmltoimage_converter *c, const char *msg)
{
    (void)c;
    fprintf(stderr, "[error callback] %s\n", msg);
    g_error_fired = 1;
}

/* -------------------------------------------------------------------------
 * main
 * ------------------------------------------------------------------------- */

int main(void)
{
    int failures = 0;

    /* --- init ------------------------------------------------------------ */
    if (!wkhtmltoimage_init(0)) {
        fprintf(stderr, "FAIL: wkhtmltoimage_init returned 0\n");
        return 1;
    }

    /* --- Write a temporary HTML file to use as input -------------------- */
    /*
     * We use inline-data mode (the `data` parameter of create_converter),
     * so we don't need to write a file.  The inline HTML is passed directly.
     */
    const char *inline_html = "<html><body><h1>Hello wkhtmltoimage C ABI</h1></body></html>";

    /* --- global settings ------------------------------------------------- */
    wkhtmltoimage_global_settings *gs = wkhtmltoimage_create_global_settings();
    if (!gs) {
        fprintf(stderr, "FAIL: create_global_settings returned NULL\n");
        wkhtmltoimage_deinit();
        return 1;
    }

    /* Set output format to PNG. */
    if (!wkhtmltoimage_set_global_setting(gs, "fmt", "png")) {
        fprintf(stderr, "FAIL: set_global_setting(fmt) returned 0\n");
        failures++;
    }

    /* Verify the setting round-trips. */
    char fmt_buf[32];
    int got_fmt = wkhtmltoimage_get_global_setting(gs, "fmt", fmt_buf, sizeof(fmt_buf));
    if (!got_fmt || strcmp(fmt_buf, "png") != 0) {
        fprintf(stderr, "FAIL: get_global_setting(fmt) returned %d / \"%s\"\n",
                got_fmt, fmt_buf);
        failures++;
    } else {
        fprintf(stderr, "PASS: fmt setting round-trips as \"%s\"\n", fmt_buf);
    }

    /* --- converter ------------------------------------------------------- */
    /*
     * create_converter TRANSFERS ownership of gs to the converter.
     * The second argument is inline HTML data (non-NULL → use as source).
     */
    wkhtmltoimage_converter *c = wkhtmltoimage_create_converter(gs, inline_html);
    if (!c) {
        fprintf(stderr, "FAIL: create_converter returned NULL\n");
        wkhtmltoimage_deinit();
        return 1;
    }

    /* --- callbacks ------------------------------------------------------- */
    wkhtmltoimage_set_finished_callback(c, on_finished);
    wkhtmltoimage_set_error_callback(c, on_error);

    /* --- convert --------------------------------------------------------- */
    int ret = wkhtmltoimage_convert(c);
    if (ret != 1) {
        fprintf(stderr, "FAIL: convert returned %d (expected 1)\n", ret);
        failures++;
    }
    if (g_finished_ok != 1) {
        fprintf(stderr, "FAIL: finished callback fired=%d ok=%d (expected ok=1)\n",
                (g_finished_ok != -1), g_finished_ok);
        failures++;
    } else {
        fprintf(stderr, "PASS: finished callback fired with ok=1\n");
    }

    /* --- get_output: must start with \x89PNG ---------------------------- */
    const unsigned char *out = NULL;
    long n = wkhtmltoimage_get_output(c, &out);
    if (n < 4 || out == NULL) {
        fprintf(stderr, "FAIL: get_output n=%ld out=%p (expected >= 4 bytes)\n",
                n, (void *)out);
        failures++;
    } else if (out[0] != 0x89 || out[1] != 'P' || out[2] != 'N' || out[3] != 'G') {
        fprintf(stderr, "FAIL: output[0..4] = {%02x %02x %02x %02x} (expected PNG magic)\n",
                out[0], out[1], out[2], out[3]);
        failures++;
    } else {
        fprintf(stderr, "PASS: output starts with PNG magic (n=%ld bytes)\n", n);
    }

    /* --- string-lifetime check ------------------------------------------ */
    const char *ps = wkhtmltoimage_progress_string(c);
    const char *pd = wkhtmltoimage_phase_description(c, 0);

    (void)wkhtmltoimage_http_error_code(c);
    (void)wkhtmltoimage_current_phase(c);
    (void)wkhtmltoimage_phase_count(c);

    if (ps == NULL || pd == NULL) {
        fprintf(stderr, "FAIL: progress_string or phase_description returned NULL\n");
        failures++;
    } else {
        fprintf(stderr,
                "PASS: lifetime check: progress_string=\"%s\" phase_description[0]=\"%s\"\n",
                ps, pd);
    }

    /* --- cleanup --------------------------------------------------------- */
    /*
     * destroy_converter frees the converter AND its owned global settings (gs).
     * Do NOT call wkhtmltoimage_destroy_global_settings(gs) — that is a double-free.
     */
    wkhtmltoimage_destroy_converter(c);
    wkhtmltoimage_deinit();

    /* --- report ---------------------------------------------------------- */
    if (failures == 0) {
        printf("PASS\n");
        return 0;
    } else {
        printf("FAIL: %d failure(s)\n", failures);
        return failures;
    }
}
