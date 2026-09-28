#!/usr/bin/env python3
"""Build a machine-readable index of ECMA-376 Part 1 reference sections.

The index maps every element and simple-type section of Part 1 (clauses 17-23)
to its section number, human-readable title, first descriptive paragraph,
schema type name and, where the tables can be recovered, attribute and
enumeration-value titles. The code generator uses it to emit documentation
comments that point back to the specification.

Usage:
    python3 tools/spec_index.py dump-pages <part1.pdf> <pages.jsonl>
    python3 tools/spec_index.py build-index <part1.pdf> <pages.jsonl> <out.json>

Requires `pypdf` (only needed when regenerating the index).
"""
import json
import re
import sys

# Section-number prefix -> namespace prefix used by the code generator.
SECTION_NS = [
    ("17.", "w"), ("18.", "x"), ("19.", "p"),
    ("20.1.", "a"), ("20.2.", "pic"), ("20.3.", "lc"), ("20.4.", "wp"), ("20.5.", "xdr"),
    ("21.1.", "a"), ("21.2.", "c"), ("21.3.", "cdr"), ("21.4.", "dgm"),
    ("22.1.", "m"), ("22.2.", "ep"), ("22.3.", "op"), ("22.4.", "vt"), ("22.5.", "ds"),
    ("22.6.", "b"), ("22.7.", "ac"), ("22.8.", "r"), ("22.9.", "s"), ("23.", "sl"),
]

FIRST_PAGE = 176   # 0-based page index where clause 17 starts
LAST_PAGE = 3818   # Annex A starts here

HEADING_RE = re.compile(r"^(\d+(?:\.\d+)+) ([A-Za-z_][\w.-]*) \((.+)\)\s*$")


def section_ns(section):
    for prefix, ns in SECTION_NS:
        if (section + ".").startswith(prefix):
            return ns
    return None


def dump_pages(pdf, out):
    import pypdf
    reader = pypdf.PdfReader(pdf)
    with open(out, "w", encoding="utf-8") as f:
        for i in range(FIRST_PAGE, LAST_PAGE):
            text = reader.pages[i].extract_text() or ""
            f.write(json.dumps({"page": i, "text": text}) + "\n")
            if i % 100 == 0:
                print("page", i, file=sys.stderr, flush=True)


def load_outline(pdf):
    import pypdf
    reader = pypdf.PdfReader(pdf)
    out = []

    def walk(items):
        for it in items:
            if isinstance(it, list):
                walk(it)
            else:
                try:
                    page = reader.get_destination_page_number(it)
                except Exception:
                    page = None
                out.append((it.title.strip(), page))

    walk(reader.outline)
    return out


def clean_lines(text):
    """Drop running headers/footers and page numbers."""
    lines = []
    for line in text.splitlines():
        s = line.strip()
        if not s:
            lines.append("")
            continue
        if s.startswith("ECMA-376 Part 1") or re.match(r"^\d+\. \w.* Reference Material$", s):
            continue
        if re.match(r"^\d{1,4}$", s):
            continue
        lines.append(s)
    return lines


def join_paragraph(lines):
    text = " ".join(l for l in lines if l)
    text = re.sub(r"\s+", " ", text).strip()
    return text


def first_paragraph(body_lines):
    """First descriptive paragraph: stops at examples, tables or notes."""
    para = []
    for line in body_lines:
        if not line:
            if para:
                break
            continue
        if line.startswith(("[Example", "[Note", "Attributes Description", "Enumeration Value",
                            "The following", "This element's content model", "This element’s content model",
                            "Parent Elements", "Child Elements", "This simple type's contents",
                            "This simple type’s contents", "Referenced By")):
            if para:
                break
            if line.startswith(("[Example", "[Note", "Attributes Description", "Enumeration Value")):
                break
        para.append(line)
        if len(" ".join(para)) > 600:
            break
    text = join_paragraph(para)
    # Keep it to at most three sentences.
    sentences = re.split(r"(?<=[.])\s+(?=[A-Z])", text)
    return " ".join(sentences[:3]).strip()


TYPE_RE = re.compile(r"content model \((C[T]_\w+|ST_\w+)\)")
# Elements whose content model is described by reference to a shared definition.
COMMON_DEFS = [
    (re.compile(r"common boolean property definition"), "CT_OnOff"),
    (re.compile(r"common border properties definition"), "CT_Border"),
    (re.compile(r"common table measurement"), "CT_TblWidth"),
    (re.compile(r"common shading properties definition"), "CT_Shd"),
    (re.compile(r"common string property definition"), "CT_String"),
]


def parse_table(body_text, header):
    """Parse `name (Title) description` rows following a table header."""
    idx = body_text.find(header)
    if idx < 0:
        return []
    rest = body_text[idx + len(header):]
    stop = re.search(r"\[Note: The W3C XML Schema definition", rest)
    if stop:
        rest = rest[:stop.start()]
    rows = []
    # A row starts with an identifier followed by a parenthesised title.
    for m in re.finditer(r"(?:(?<=\s)|^)([A-Za-z0-9_:.+-]+) \(([^()]{2,120})\)\s+(.*?)(?=\s[A-Za-z0-9_:.+-]+ \([^()]{2,120}\)\s+[A-Z]|$)", rest, re.S):
        name, title, desc = m.group(1), m.group(2), m.group(3)
        title = re.sub(r"\s+", " ", title).strip()
        desc = re.sub(r"\s+", " ", desc).strip()
        desc = re.split(r"(?<=[.])\s+(?=[A-Z\[])", desc)[0] if desc else ""
        if desc.startswith("[Example"):
            desc = ""
        rows.append({"name": name, "title": title, "description": desc[:400]})
    return rows


def build_index(pdf, pages_path, out):
    pages = {}
    with open(pages_path, encoding="utf-8") as f:
        for line in f:
            rec = json.loads(line)
            pages[rec["page"]] = rec["text"]
    outline = load_outline(pdf)
    entries = []
    for i, (title, page) in enumerate(outline):
        m = HEADING_RE.match(title)
        if not m or page is None:
            continue
        section, name, friendly = m.group(1), m.group(2), m.group(3)
        ns = section_ns(section)
        if ns is None:
            continue
        # End page: next outline entry's page.
        end_page = page
        for t2, p2 in outline[i + 1:]:
            if p2 is not None:
                end_page = p2
                break
        text_lines = []
        for p in range(page, min(end_page, LAST_PAGE - 1) + 1):
            text_lines.extend(clean_lines(pages.get(p, "")))
        # Locate our heading and the next heading.
        start = None
        for j, line in enumerate(text_lines):
            if line.startswith(section + " " + name):
                start = j + 1
                break
        if start is None:
            continue
        end = len(text_lines)
        for j in range(start, len(text_lines)):
            hm = HEADING_RE.match(text_lines[j])
            if hm and hm.group(1) != section:
                end = j
                break
        body = text_lines[start:end]
        body_text = join_paragraph(body)
        rec = {
            "section": section,
            "ns": ns,
            "name": name,
            "title": friendly,
            "page": page + 1,
            "description": first_paragraph(body),
        }
        tm = TYPE_RE.search(body_text)
        if tm:
            rec["type"] = tm.group(1)
        else:
            for regex, type_name in COMMON_DEFS:
                if regex.search(body_text):
                    rec["type"] = type_name
                    break
        if name.startswith("ST_"):
            rec["kind"] = "simpleType"
            rec["values"] = parse_table(body_text, "Enumeration Value Description")
        else:
            rec["kind"] = "element"
            rec["attributes"] = parse_table(body_text, "Attributes Description")
        entries.append(rec)
    with open(out, "w", encoding="utf-8") as f:
        json.dump({"source": "ECMA-376 Part 1, 5th edition (2016)", "entries": entries}, f,
                  indent=1, ensure_ascii=False)
    print("entries:", len(entries), file=sys.stderr)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "dump-pages":
        dump_pages(sys.argv[2], sys.argv[3])
    elif cmd == "build-index":
        build_index(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit("unknown command")
