// wkhtmltox-rs — LGPL-3.0-or-later.
#include "shim.h"
#include <qpdf/QPDF.hh>
#include <qpdf/QPDFWriter.hh>

extern "C" int wkx_pdf_roundtrip(const char* in_path, const char* out_path) {
    try {
        QPDF q;
        q.processFile(in_path);
        QPDFWriter w(q, out_path);
        w.write();
        return 0;
    } catch (std::exception& e) {
        return 1;
    }
}
