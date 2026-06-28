// wkhtmltox-rs — LGPL-3.0-or-later.
#include "shim.h"
#include <string>
#include <sstream>
#include <vector>
#include <qpdf/QPDF.hh>
#include <qpdf/QPDFWriter.hh>
#include <qpdf/QPDFObjectHandle.hh>
#include <qpdf/QPDFAcroFormDocumentHelper.hh>
#include <qpdf/QPDFPageDocumentHelper.hh>
#include <qpdf/QPDFPageObjectHelper.hh>
#include <qpdf/QPDFFormFieldObjectHelper.hh>
#include <qpdf/QPDFAnnotationObjectHelper.hh>

namespace {

// ── Inheritance-aware resource helper ────────────────────────────────────────
//
// Walk the page-tree upward to find the nearest /Resources dict (which may
// be on the page itself or inherited from a parent /Pages node).  A
// shallow copy is placed on the page dict so subsequent modifications
// (adding /WKXF) stay local and do not clobber a shared parent resource
// dict.  This addresses the M2a deferred finding.
//
// After ensuring a local /Resources, the function also ensures a local
// /Font sub-dict (again, shallow-copying if the existing one is indirect)
// and returns it ready for mutation.

static QPDFObjectHandle ensure_page_font_dict(QPDFObjectHandle page) {
    // Step 1: ensure the page has its own /Resources.
    if (!page.hasKey("/Resources")) {
        QPDFObjectHandle cur = page;
        bool found = false;
        while (cur.hasKey("/Parent")) {
            cur = cur.getKey("/Parent");
            if (cur.hasKey("/Resources")) {
                // Shallow-copy so we don't modify the parent's shared dict.
                page.replaceKey("/Resources",
                                cur.getKey("/Resources").shallowCopy());
                found = true;
                break;
            }
        }
        if (!found)
            page.replaceKey("/Resources", QPDFObjectHandle::newDictionary());
    }
    QPDFObjectHandle res = page.getKey("/Resources");

    // Step 2: ensure /Font is a local writable dict.
    if (!res.hasKey("/Font")) {
        res.replaceKey("/Font", QPDFObjectHandle::newDictionary());
    } else {
        QPDFObjectHandle fd = res.getKey("/Font");
        if (fd.isIndirect()) {
            // Shallow-copy so adding /WKXF doesn't pollute a shared parent
            // font dict.
            res.replaceKey("/Font", fd.shallowCopy());
        }
    }
    return res.getKey("/Font");
}

// ── Add /WKXF (Helvetica) to the page if not already present ─────────────────
static void add_wkxf_font(QPDFObjectHandle page) {
    QPDFObjectHandle font_dict = ensure_page_font_dict(page);
    if (!font_dict.hasKey("/WKXF")) {
        QPDFObjectHandle font = QPDFObjectHandle::newDictionary();
        font.replaceKey("/Type",     QPDFObjectHandle::newName("/Font"));
        font.replaceKey("/Subtype",  QPDFObjectHandle::newName("/Type1"));
        font.replaceKey("/BaseFont", QPDFObjectHandle::newName("/Helvetica"));
        font_dict.replaceKey("/WKXF", font);
    }
}

} // namespace

extern "C" int wkx_pdf_roundtrip(const char* in_path, const char* out_path) {
    try {
        QPDF q;
        q.processFile(in_path);
        QPDFWriter w(q, out_path);
        w.write();
        return 0;
    } catch (const std::exception&) {
        return 1;
    } catch (...) {
        return 2; // unknown exception must not unwind across extern "C"
    }
}

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
    } catch (const std::exception&) {
        return 1;
    } catch (...) {
        return 3; // unknown exception must not unwind across extern "C"
    }
}

extern "C" int wkx_pdf_merge(const char** in_paths, int n, const char* out_path) {
    try {
        if (n <= 0) return 2;
        QPDF out;
        out.emptyPDF();
        for (int i = 0; i < n; ++i) {
            QPDF in;
            in.processFile(in_paths[i]);
            for (auto& page : QPDFPageDocumentHelper(in).getAllPages())
                QPDFPageDocumentHelper(out).addPage(page, false);
        }
        QPDFWriter w(out, out_path);
        w.write();
        return 0;
    } catch (const std::exception&) {
        return 1;
    } catch (...) {
        return 2; // unknown exception must not unwind across extern "C"
    }
}

// Build a nested /Outlines tree from parallel arrays of titles, 0-based page indices, and
// nesting levels (1 = top level, 2 = child, etc.).  Returns 0 on success, 1 on QPDF error,
// 2 if n <= 0.
extern "C" int wkx_pdf_set_outline(const char* in_path, const char* out_path,
                                   const char** titles, const int* pages,
                                   const int* levels, int n) {
    try {
        if (n <= 0) return 2;

        QPDF q;
        q.processFile(in_path);

        // Collect page object handles indexed by 0-based page number.
        std::vector<QPDFObjectHandle> page_objs;
        for (auto& ph : QPDFPageDocumentHelper(q).getAllPages())
            page_objs.push_back(ph.getObjectHandle());

        // Create the root /Outlines indirect object.
        auto outlines = q.makeIndirectObject(QPDFObjectHandle::newDictionary());
        outlines.replaceKey("/Type", QPDFObjectHandle::newName("/Outlines"));

        // Stack of open ancestors; each entry carries the handle and level.
        struct Ancestor {
            int level;
            QPDFObjectHandle handle;
            int child_count = 0; // direct children
            // last direct child (for /Last and /Next linkage)
            QPDFObjectHandle last_child; // invalid until first child added
            bool has_children = false;
        };

        std::vector<Ancestor> stack; // stack[0] is the outlines root (level 0)
        {
            Ancestor root;
            root.level = 0;
            root.handle = outlines;
            stack.push_back(root);
        }

        // total descendant counts for nodes — computed with a post-pass.
        // We store items in order with their handle and level so we can do the /Count pass.
        struct ItemInfo {
            QPDFObjectHandle handle;
            int level;
        };
        std::vector<ItemInfo> all_items;
        all_items.reserve(n);

        for (int i = 0; i < n; ++i) {
            int level = levels[i];  // >= 1
            int page_idx = pages[i];
            const char* title = titles[i];

            // Pop stack entries whose level >= this item's level so that the top of the
            // stack is the nearest ancestor with level < current level.
            while (stack.size() > 1 && stack.back().level >= level)
                stack.pop_back();

            Ancestor& parent = stack.back();

            // Build this item as an indirect dict.
            auto item = q.makeIndirectObject(QPDFObjectHandle::newDictionary());
            item.replaceKey("/Title",
                QPDFObjectHandle::newUnicodeString(std::string(title)));

            // /Dest = [ pageObj /XYZ null null null ]
            QPDFObjectHandle dest = QPDFObjectHandle::newArray();
            if (page_idx >= 0 && (size_t)page_idx < page_objs.size())
                dest.appendItem(page_objs[page_idx]);
            else
                dest.appendItem(QPDFObjectHandle::newNull());
            dest.appendItem(QPDFObjectHandle::newName("/XYZ"));
            dest.appendItem(QPDFObjectHandle::newNull());
            dest.appendItem(QPDFObjectHandle::newNull());
            dest.appendItem(QPDFObjectHandle::newNull());
            item.replaceKey("/Dest", dest);

            // /Parent
            item.replaceKey("/Parent", parent.handle);

            // Sibling linkage: /Prev / /Next.
            if (parent.has_children) {
                // Link previous last child → this item (/Next) and this → prev (/Prev).
                parent.last_child.replaceKey("/Next", item);
                item.replaceKey("/Prev", parent.last_child);
            } else {
                // First child of parent.
                parent.handle.replaceKey("/First", item);
            }
            parent.handle.replaceKey("/Last", item);
            parent.last_child = item;
            parent.has_children = true;
            parent.child_count++;

            all_items.push_back({item, level});

            // Push this item onto the stack as a potential parent.
            {
                Ancestor a;
                a.level = level;
                a.handle = item;
                stack.push_back(a);
            }
        }

        // Compute /Count for root and each internal node.
        // /Count on root = total number of all outline items (all levels), open by convention.
        // /Count on a node = total number of its descendants (positive = open).
        // We do this with a simple backward pass: accumulate counts up the level tree.
        // For each item, count = (number of items that are descendants) = items that have
        // level > item level and appear consecutively after it until we hit level <= item level.
        int total = n;
        outlines.replaceKey("/Count", QPDFObjectHandle::newInteger(total));

        for (int i = 0; i < n; ++i) {
            // Count direct+indirect descendants.
            int desc = 0;
            int my_level = all_items[i].level;
            for (int j = i + 1; j < n; ++j) {
                if (all_items[j].level > my_level) ++desc;
                else break;
            }
            if (desc > 0) {
                // Positive /Count means the node is open.
                all_items[i].handle.replaceKey("/Count",
                    QPDFObjectHandle::newInteger(desc));
            }
        }

        // Attach outlines to the document catalog.
        q.getRoot().replaceKey("/Outlines", outlines);

        QPDFWriter w(q, out_path);
        w.write();
        return 0;
    } catch (const std::exception&) {
        return 1;
    } catch (...) {
        return 2; // unknown exception must not unwind across extern "C"
    }
}

// Stamp a centered page-number footer on each page.
// fmt: format string with [page] and [topage] tokens.
// start: number assigned to the first page (e.g. 1).
extern "C" int wkx_pdf_stamp_footer(const char* in_path, const char* out_path,
                                    const char* fmt, int start) {
    try {
        QPDF q;
        q.processFile(in_path);

        QPDFPageDocumentHelper pdh(q);
        auto pages = pdh.getAllPages();
        int page_count = (int)pages.size();

        if (page_count == 0) {
            QPDFWriter w(q, out_path);
            w.write();
            return 0;
        }

        int topage = start + page_count - 1;

        for (int i = 0; i < page_count; ++i) {
            QPDFObjectHandle page = pages[i].getObjectHandle();

            // Substitute [page] and [topage] tokens.
            std::string text(fmt);
            std::string page_str = std::to_string(start + i);
            std::string topage_str = std::to_string(topage);
            size_t pos;
            while ((pos = text.find("[page]")) != std::string::npos)
                text.replace(pos, 6, page_str);
            while ((pos = text.find("[topage]")) != std::string::npos)
                text.replace(pos, 8, topage_str);

            // Escape PDF string special characters: '(', ')', '\'.
            std::string escaped;
            escaped.reserve(text.size());
            for (char c : text) {
                if (c == '(' || c == ')' || c == '\\')
                    escaped += '\\';
                escaped += c;
            }

            // Determine page width from /MediaBox (default to 595 if absent).
            double page_width = 595.0;
            if (page.hasKey("/MediaBox")) {
                QPDFObjectHandle mb = page.getKey("/MediaBox");
                if (mb.isArray() && mb.getArrayNItems() >= 4) {
                    double llx = mb.getArrayItem(0).getNumericValue();
                    double urx = mb.getArrayItem(2).getNumericValue();
                    page_width = urx - llx;
                }
            }

            // Approximate centred x position.
            double x = (page_width / 2.0) - (text.size() * 2.5);
            if (x < 10.0) x = 10.0;

            // Build the content stream.
            std::ostringstream ss;
            ss << "q BT /WKXF 9 Tf " << (int)x << " 18 Td ("
               << escaped << ") Tj ET Q\n";
            std::string content = ss.str();

            // Add /WKXF Helvetica to page resources (inheritance-aware).
            add_wkxf_font(page);

            // Append the footer content stream after existing page contents.
            QPDFObjectHandle stream = QPDFObjectHandle::newStream(&q, content);
            QPDFPageObjectHelper(page).addPageContents(stream, false);
        }

        QPDFWriter w(q, out_path);
        w.write();
        return 0;
    } catch (const std::exception&) {
        return 1;
    } catch (...) {
        return 2;
    }
}

// Return the number of pages in `in_path`, or -1 on error.
extern "C" int wkx_pdf_page_count(const char* in_path) {
    try {
        QPDF q;
        q.processFile(in_path);
        return (int)QPDFPageDocumentHelper(q).getAllPages().size();
    } catch (const std::exception&) {
        return -1;
    } catch (...) {
        return -1; // unknown exception must not unwind across extern "C"
    }
}

// Stamp header/footer cells on each page.
//
// `cells` must have exactly `n_pages * 6` entries (caller-enforced).
// For page p (0-based) the six cell strings are at indices p*6+0..5:
//   0=top-left  1=top-center  2=top-right
//   3=bot-left  4=bot-center  5=bot-right
// An empty string means "skip that cell".
// font_size: Helvetica point size applied to all non-empty cells.
//
// Returns 0 on success, 1 on QPDF/std error, 2 if n_pages <= 0.
extern "C" int wkx_pdf_stamp_cells(const char* in_path, const char* out_path,
                                    const char** cells, int n_pages,
                                    double font_size) {
    try {
        if (n_pages <= 0) return 2;

        QPDF q;
        q.processFile(in_path);

        QPDFPageDocumentHelper pdh(q);
        auto pages = pdh.getAllPages();

        // Clamp to actual page count so callers can pass a conservative n_pages.
        if ((int)pages.size() < n_pages)
            n_pages = (int)pages.size();

        for (int pi = 0; pi < n_pages; ++pi) {
            QPDFObjectHandle page = pages[pi].getObjectHandle();

            // ── Determine page dimensions (MediaBox, possibly inherited) ─────
            double pw = 595.0, ph = 842.0; // A4 defaults
            {
                QPDFObjectHandle cur = page;
                while (true) {
                    if (cur.hasKey("/MediaBox")) {
                        auto mb = cur.getKey("/MediaBox");
                        if (mb.isArray() && mb.getArrayNItems() >= 4) {
                            double llx = mb.getArrayItem(0).getNumericValue();
                            double lly = mb.getArrayItem(1).getNumericValue();
                            double urx = mb.getArrayItem(2).getNumericValue();
                            double ury = mb.getArrayItem(3).getNumericValue();
                            pw = urx - llx;
                            ph = ury - lly;
                        }
                        break;
                    }
                    if (!cur.hasKey("/Parent")) break;
                    cur = cur.getKey("/Parent");
                }
            }

            const double margin = 36.0;
            const double y_top = ph - 28.0;
            const double y_bottom = 20.0;

            std::string stream_content;

            for (int ci = 0; ci < 6; ++ci) {
                const char* text_c = cells[pi * 6 + ci];
                if (!text_c || text_c[0] == '\0') continue;

                std::string text(text_c);
                double approx_width = (double)text.size() * font_size * 0.5;

                double y = (ci < 3) ? y_top : y_bottom;
                int col = ci % 3; // 0=left 1=center 2=right

                double x;
                if (col == 0) {
                    x = margin;
                } else if (col == 1) {
                    x = (pw - approx_width) / 2.0;
                } else {
                    x = pw - margin - approx_width;
                }
                if (x < 10.0) x = 10.0;

                // Escape PDF string specials: '(' ')' '\'
                std::string escaped;
                escaped.reserve(text.size());
                for (char c : text) {
                    if (c == '(' || c == ')' || c == '\\') escaped += '\\';
                    escaped += c;
                }

                std::ostringstream ss;
                ss << "q BT /WKXF " << font_size << " Tf "
                   << x << " " << y << " Td ("
                   << escaped << ") Tj ET Q\n";
                stream_content += ss.str();
            }

            if (stream_content.empty()) continue;

            // Add /WKXF Helvetica (inheritance-aware, no clobber).
            add_wkxf_font(page);

            // Append the cell content stream after existing page contents.
            QPDFObjectHandle stream =
                QPDFObjectHandle::newStream(&q, stream_content);
            QPDFPageObjectHelper(page).addPageContents(stream, false);
        }

        QPDFWriter w(q, out_path);
        w.write();
        return 0;
    } catch (const std::exception&) {
        return 1;
    } catch (...) {
        return 2; // unknown exception must not unwind across extern "C"
    }
}
