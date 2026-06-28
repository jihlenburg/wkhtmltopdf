// wkhtmltox-rs — LGPL-3.0-or-later.
#ifdef __cplusplus
extern "C" {
#endif
int wkx_pdf_roundtrip(const char* in_path, const char* out_path);
int wkx_pdf_add_text_field(const char* in_path, const char* out_path,
                           const char* field_name, int page_index,
                           double x, double y, double w, double h);
int wkx_pdf_merge(const char** in_paths, int n, const char* out_path);
int wkx_pdf_set_outline(const char* in_path, const char* out_path,
                        const char** titles, const int* pages,
                        const int* levels, int n);
int wkx_pdf_stamp_footer(const char* in_path, const char* out_path,
                         const char* fmt, int start);
int wkx_pdf_page_count(const char* in_path);
/* Stamp header/footer cells on every page.
 *
 * cells has n_pages*6 entries.  For page p the six are indices
 * p*6+0..5 = [top-left, top-center, top-right, bottom-left, bottom-center,
 * bottom-right].  An empty string means "skip that cell".
 * font_size is the Helvetica point size used for all cells.
 *
 * Returns 0 on success, 1 on QPDF error, 2 if n_pages <= 0.
 */
int wkx_pdf_stamp_cells(const char* in_path, const char* out_path,
                        const char** cells, int n_pages, double font_size);
/* Add clickable /Link annotations to a copy of in_path.
 *
 * For i in 0..n: on page src_pages[i] append an annotation dict
 *   /Type/Annot /Subtype/Link /Rect[rects[i*4..i*4+4]] /Border[0 0 0]
 *   /Dest[dest_page_obj /XYZ null null null]
 * to that page's /Annots (creating the array if absent).
 * Page object handles are resolved via QPDFPageDocumentHelper::getAllPages().
 * Every page index is bounds-checked.
 *
 * Returns 0 on success, 1 on QPDF/std error, 2 if n < 0,
 * 3 if any page index is out of range.
 */
int wkx_pdf_add_links(const char* in_path, const char* out_path,
                      const int* src_pages, const double* rects,
                      const int* dest_pages, int n);
/* Overlay each overlay PDF's first page onto the corresponding base page as a
 * Form XObject, translated to (tx[i], ty[i]) in PDF points (bottom-left origin).
 *
 * For i in 0..n: load overlay_paths[i], convert its first page to a Form XObject,
 * copy it into base (via copyForeignObject), add it to the target page's
 * /Resources /XObject under a unique name /WkxOv{i}, and append the content
 * stream "q 1 0 0 1 tx ty cm /WkxOv{i} Do Q" to that page.
 *
 * Returns 0 on success, 1 on QPDF/std error, 2 if any page_index is out of
 * range, 3 on unknown exception.
 */
int wkx_pdf_overlay_pages(const char* base_path, const char* out_path,
                           const char** overlay_paths, const int* page_indices,
                           const double* tx, const double* ty, int n);
#ifdef __cplusplus
}
#endif
