/*
 * consumer.c — C ABI integration test for libwkhtmltox.
 *
 * Copyright 2026 wkhtmltopdf authors.
 * Licensed under the GNU Lesser General Public License, version 3 or later.
 * See <http://www.gnu.org/licenses/>.
 *
 * Exercises the full wkhtmltopdf_* C ABI:
 *   init → global/object settings → create converter → set callbacks →
 *   add_object (inline HTML) → convert → get_output → string-lifetime check →
 *   destroy → deinit.
 *
 * Compile (from workspace root, after `cargo build -p wkhtmltox-capi`):
 *
 *   clang \
 *       -Icrates/wkhtmltox-capi/include \
 *       -Icrates/wkhtmltox-capi/ctest \
 *       crates/wkhtmltox-capi/ctest/consumer.c \
 *       -Ltarget/debug -lwkhtmltox \
 *       -Wl,-rpath,target/debug \
 *       -o target/debug/consumer
 *
 * Run:
 *   ./target/debug/consumer
 *
 * Exit 0 on PASS, non-zero on FAIL.
 *
 * The <wkhtmltox/dllbegin.inc> and <wkhtmltox/dllend.inc> shims live in
 * ctest/wkhtmltox/ and are found via -Ictest.  These are verbatim copies of
 * src/lib/dllbegin.inc and dllend.inc so the consumer sees identical macros
 * to a normal system install.
 */

#include <stdio.h>
#include <string.h>

#include "pdf.h"

/* -------------------------------------------------------------------------
 * Global state set by callbacks.
 * ------------------------------------------------------------------------- */

static int g_finished_ok  = -1; /* -1 = not called */
static int g_error_fired  = 0;

/* Finished callback: records whether conversion succeeded. */
static void on_finished(wkhtmltopdf_converter *c, const int ok)
{
    (void)c;
    g_finished_ok = ok;
}

/* Error callback: prints the message and marks that an error was fired. */
static void on_error(wkhtmltopdf_converter *c, const char *msg)
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
    if (!wkhtmltopdf_init(0)) {
        fprintf(stderr, "FAIL: wkhtmltopdf_init returned 0\n");
        return 1;
    }

    /* --- global settings ------------------------------------------------- */
    wkhtmltopdf_global_settings *gs = wkhtmltopdf_create_global_settings();
    if (!gs) {
        fprintf(stderr, "FAIL: create_global_settings returned NULL\n");
        wkhtmltopdf_deinit();
        return 1;
    }
    if (!wkhtmltopdf_set_global_setting(gs, "size.pageSize", "A4")) {
        fprintf(stderr, "FAIL: set_global_setting(size.pageSize) returned 0\n");
        failures++;
    }

    /* --- object settings ------------------------------------------------- */
    wkhtmltopdf_object_settings *os = wkhtmltopdf_create_object_settings();
    if (!os) {
        fprintf(stderr, "FAIL: create_object_settings returned NULL\n");
        wkhtmltopdf_destroy_global_settings(gs);
        wkhtmltopdf_deinit();
        return 1;
    }

    /* --- converter ------------------------------------------------------- */
    /*
     * create_converter TRANSFERS ownership of gs to the converter.
     * Do NOT call wkhtmltopdf_destroy_global_settings(gs) after this — the
     * converter (or destroy_converter) owns and frees it.  This matches the
     * upstream wkhtmltopdf C ABI (pdf.h) semantics.
     */
    wkhtmltopdf_converter *c = wkhtmltopdf_create_converter(gs);
    if (!c) {
        fprintf(stderr, "FAIL: create_converter returned NULL\n");
        /*
         * gs ownership was transferred to (the failed) create_converter call;
         * do NOT destroy gs here — it is already freed.
         * os was not yet passed to add_object, so it is still our responsibility.
         */
        wkhtmltopdf_destroy_object_settings(os);
        wkhtmltopdf_deinit();
        return 1;
    }

    /* --- callbacks ------------------------------------------------------- */
    wkhtmltopdf_set_finished_callback(c, on_finished);
    wkhtmltopdf_set_error_callback(c, on_error);

    /* --- add inline HTML object ------------------------------------------ */
    /*
     * add_object TRANSFERS ownership of os to the converter.
     * Do NOT call wkhtmltopdf_destroy_object_settings(os) after this.
     */
    wkhtmltopdf_add_object(c, os, "<h1>Hello C ABI</h1><p>body</p>");

    /* --- convert --------------------------------------------------------- */
    int ret = wkhtmltopdf_convert(c);
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

    /* --- get_output: must start with %%PDF ------------------------------ */
    const unsigned char *out = NULL;
    long n = wkhtmltopdf_get_output(c, &out);
    if (n < 4 || out == NULL) {
        fprintf(stderr, "FAIL: get_output n=%ld out=%p (expected >= 4 bytes)\n",
                n, (void *)out);
        failures++;
    } else if (memcmp(out, "%PDF", 4) != 0) {
        fprintf(stderr, "FAIL: output[0..4] = {%02x %02x %02x %02x} (expected %%PDF)\n",
                out[0], out[1], out[2], out[3]);
        failures++;
    } else {
        fprintf(stderr, "PASS: output starts with %%PDF (n=%ld bytes)\n", n);
    }

    /* --- string-lifetime check ------------------------------------------ */
    /*
     * Capture two const char* pointers returned by the converter's string
     * cache.  Then call another ABI function to prove the converter's
     * internal state changes don't free or relocate the cached strings.
     * Reading the saved pointers after that call must still yield valid,
     * NUL-terminated strings.
     */
    const char *ps = wkhtmltopdf_progress_string(c);
    const char *pd = wkhtmltopdf_phase_description(c, 0);

    /* Trigger an unrelated accessor — this must not invalidate ps/pd. */
    (void)wkhtmltopdf_http_error_code(c);
    (void)wkhtmltopdf_current_phase(c);
    (void)wkhtmltopdf_phase_count(c);

    if (ps == NULL || pd == NULL) {
        fprintf(stderr, "FAIL: progress_string or phase_description returned NULL\n");
        failures++;
    } else {
        /* Reading ps and pd after the accessor calls above — must not crash. */
        fprintf(stderr,
                "PASS: lifetime check: progress_string=\"%s\" phase_description[0]=\"%s\"\n",
                ps, pd);
    }

    /* --- cleanup --------------------------------------------------------- */
    /*
     * destroy_converter frees the converter AND its owned GlobalSettings (gs)
     * and all owned PdfObjectSettings (os).  Do NOT separately call
     * destroy_object_settings(os) or destroy_global_settings(gs) — that would
     * be a double-free, exactly as with the real wkhtmltopdf library.
     */
    wkhtmltopdf_destroy_converter(c);
    wkhtmltopdf_deinit();

    /* --- report ---------------------------------------------------------- */
    if (failures == 0) {
        printf("PASS\n");
        return 0;
    } else {
        printf("FAIL: %d failure(s)\n", failures);
        return failures;
    }
}
