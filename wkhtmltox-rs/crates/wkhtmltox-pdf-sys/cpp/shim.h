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
#ifdef __cplusplus
}
#endif
