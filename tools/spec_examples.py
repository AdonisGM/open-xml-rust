#!/usr/bin/env python3
"""Extract XML examples from ECMA-376 Part 1 element sections.

For every element section of the spec index, the "[Example: … end example]"
blocks are scanned for XML fragments whose root element is the element the
section describes. Such fragments are well-formed (after namespace
declarations are added) and their content type is known from the index,
which makes them test inputs for the generated types.

Usage:
    python3 tools/spec_examples.py <pages.jsonl> <spec-index.json> <out.json>
"""
import json
import re
import sys
import xml.dom.minidom
from xml.parsers.expat import ExpatError

NAMESPACES = {
    "w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "x": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "pic": "http://schemas.openxmlformats.org/drawingml/2006/picture",
    "c": "http://schemas.openxmlformats.org/drawingml/2006/chart",
    "cdr": "http://schemas.openxmlformats.org/drawingml/2006/chartDrawing",
    "dgm": "http://schemas.openxmlformats.org/drawingml/2006/diagram",
    "lc": "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas",
    "wp": "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    "xdr": "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "m": "http://schemas.openxmlformats.org/officeDocument/2006/math",
    "s": "http://schemas.openxmlformats.org/officeDocument/2006/sharedTypes",
    "vt": "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes",
    "ds": "http://schemas.openxmlformats.org/officeDocument/2006/customXml",
    "b": "http://schemas.openxmlformats.org/officeDocument/2006/bibliography",
    "sl": "http://schemas.openxmlformats.org/schemaLibrary/2006/main",
    "v": "urn:schemas-microsoft-com:vml",
    "o": "urn:schemas-microsoft-com:office:office",
    "mc": "http://schemas.openxmlformats.org/markup-compatibility/2006",
}
# Namespaces whose examples are conventionally written without a prefix.
DEFAULT_NS = {
    "x": NAMESPACES["x"],
    "ep": "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
    "op": "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties",
    "ac": "http://schemas.openxmlformats.org/officeDocument/2006/characteristics",
}


def clean(line):
    s = line.rstrip()
    if s.startswith("ECMA-376 Part 1") or re.match(r"^\s*\d{1,4}\s*$", s):
        return None
    if re.match(r"^\d+\. .* Reference Material\s*$", s.strip()):
        return None
    return s


def fragments(block):
    """Yields maximal runs of lines that look like XML markup."""
    run = []
    for line in block:
        stripped = line.strip()
        looks_xml = stripped.startswith("<") or (run and not stripped.endswith(".") and "<" in stripped)
        if looks_xml:
            run.append(stripped)
        elif run:
            yield " ".join(run)
            run = []
    if run:
        yield " ".join(run)


def wrap(fragment, ns_prefix):
    decls = " ".join(f'xmlns:{p}="{u}"' for p, u in NAMESPACES.items())
    default = DEFAULT_NS.get(ns_prefix)
    if default:
        decls += f' xmlns="{default}"'
    return f"<root {decls}>{fragment}</root>"


def main(pages_path, index_path, out_path):
    pages = {}
    with open(pages_path, encoding="utf-8") as f:
        for line in f:
            rec = json.loads(line)
            pages[rec["page"]] = rec["text"]
    index = json.load(open(index_path, encoding="utf-8"))["entries"]
    elements = [e for e in index if e["kind"] == "element" and e.get("type")]
    out = []
    for i, e in enumerate(elements):
        start = e["page"] - 1
        lines = []
        for p in range(start, start + 4):
            for l in pages.get(p, "").splitlines():
                c = clean(l)
                if c is not None:
                    lines.append(c)
        # Restrict to the section text.
        head = f'{e["section"]} {e["name"]} ('
        begin = next((k for k, l in enumerate(lines) if l.startswith(head)), None)
        if begin is None:
            continue
        end = next((k for k in range(begin + 1, len(lines))
                    if re.match(r"^\d+(\.\d+)+ \S+ \(", lines[k])), len(lines))
        body = lines[begin + 1:end]
        text = "\n".join(body)
        for m in re.finditer(r"\[Example:(.*?)end example\]", text, re.S):
            block = m.group(1).splitlines()
            for frag in fragments(block):
                frag = frag.replace("…", "").replace("...", "")
                # The fragment must be exactly one element named like the section.
                prefix = "" if e["ns"] in DEFAULT_NS and e["ns"] != "x" or e["ns"] == "x" else e["ns"] + ":"
                opener = re.match(r"<([\w:]+)", frag)
                if not opener or opener.group(1) not in (f'{e["ns"]}:{e["name"]}', e["name"]):
                    continue
                try:
                    doc = xml.dom.minidom.parseString(wrap(frag, e["ns"]))
                except ExpatError:
                    continue
                roots = [n for n in doc.documentElement.childNodes if n.nodeType == n.ELEMENT_NODE]
                if len(roots) != 1:
                    continue
                out.append({
                    "section": e["section"],
                    "ns": e["ns"],
                    "element": e["name"],
                    "type": e["type"],
                    "xml": frag,
                })
    json.dump({
        "source": "ECMA-376 Part 1, 5th edition (2016)",
        "note": "Wrap each fragment in an element declaring `namespaces` (and the "
                "default namespace from `default_namespaces` for its `ns`).",
        "namespaces": NAMESPACES,
        "default_namespaces": DEFAULT_NS,
        "examples": out,
    }, open(out_path, "w"), indent=1, ensure_ascii=False)
    print("examples:", len(out), file=sys.stderr)


if __name__ == "__main__":
    main(*sys.argv[1:4])
