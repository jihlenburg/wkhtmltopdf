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
    } catch (...) {
        return 2; // unknown exception must not unwind across extern "C"
    }
}

#include <qpdf/QPDFObjectHandle.hh>
#include <qpdf/QPDFAcroFormDocumentHelper.hh>
#include <qpdf/QPDFPageDocumentHelper.hh>
#include <qpdf/QPDFFormFieldObjectHelper.hh>
#include <qpdf/QPDFAnnotationObjectHelper.hh>
#include <string>

extern "C" int wkx_pdf_add_text_field(const char* in_path, const char* out_path,
                                      const char* field_name, int page_index,
                                      double x, double y, double w, double h) {
    try {
        QPDF q;
        q.processFile(in_path);

        QPDFPageDocumentHelper pdh(q);
        auto pages = pdh.getAllPages();
        if (page_index < 0 || (size_t)page_index >= pages.size()) return 2;
        QPDFObjectHandle page = pages[page_index].getObjectHandle();

        // Build the field/widget dictionary (a terminal text field that is also its own widget).
        QPDFObjectHandle field = q.makeIndirectObject(QPDFObjectHandle::newDictionary());
        field.replaceKey("/FT", QPDFObjectHandle::newName("/Tx"));
        field.replaceKey("/T", QPDFObjectHandle::newUnicodeString(std::string(field_name)));
        field.replaceKey("/Type", QPDFObjectHandle::newName("/Annot"));
        field.replaceKey("/Subtype", QPDFObjectHandle::newName("/Widget"));
        field.replaceKey("/F", QPDFObjectHandle::newInteger(4)); // Print
        field.replaceKey("/DA", QPDFObjectHandle::newString("/Helv 0 Tf 0 g"));
        field.replaceKey("/P", page);
        QPDFObjectHandle rect = QPDFObjectHandle::newArray();
        rect.appendItem(QPDFObjectHandle::newReal(x));
        rect.appendItem(QPDFObjectHandle::newReal(y));
        rect.appendItem(QPDFObjectHandle::newReal(x + w));
        rect.appendItem(QPDFObjectHandle::newReal(y + h));
        field.replaceKey("/Rect", rect);

        // Attach the widget to the page's /Annots.
        if (!page.hasKey("/Annots")) page.replaceKey("/Annots", QPDFObjectHandle::newArray());
        page.getKey("/Annots").appendItem(field);

        // Register with the document AcroForm (creates /AcroForm + /Fields if needed).
        QPDFAcroFormDocumentHelper afdh(q);
        afdh.addFormField(QPDFFormFieldObjectHelper(field));
        afdh.setNeedAppearances(true);

        QPDFWriter wr(q, out_path);
        wr.write();
        return 0;
    } catch (std::exception& e) {
        return 1;
    } catch (...) {
        return 3; // unknown exception must not unwind across extern "C"
    }
}
