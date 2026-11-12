#!/usr/bin/env python3
"""Generate Unicode 17 property lookup pages from the approved segmentation crate.

Run with the unicode-segmentation 1.13.3 source directory as the sole argument.
No download or new data source is used. Values combine that crate's disjoint
GCB/Extended_Pictographic/Consonant categories with two independent InCB bits.
The resulting binary is byte page indices followed by deduplicated
256-byte property pages. The Rust reader documents each category and state.
"""
from pathlib import Path
import re
import sys


def generate(source):
    """Extract the pinned tables and deterministically deduplicate property pages."""
    text = (source / "src/tables.rs").read_text()
    assert "UNICODE_VERSION: (u64, u64, u64) = (17, 0, 0)" in text
    names = ["Any", "CR", "Control", "Extend", "Extended_Pictographic",
             "InCB_Consonant", "L", "LF", "LV", "LVT", "Prepend",
             "Regional_Indicator", "SpacingMark", "T", "V", "ZWJ"]
    # Scalar facts are expanded only by this development generator. Runtime
    # lookup uses two bounded table loads, never a range search or allocation.
    values = bytearray(0x110000)
    table = text.split("const grapheme_cat_table:", 1)[1].split("];", 1)[0]
    pattern = r"\('\\u\{([0-9a-f]+)\}',\s*'\\u\{([0-9a-f]+)\}',\s*GC_(\w+)\)"
    ranges = re.findall(pattern, table)
    assert len(ranges) == 1618
    for low, high, name in ranges:
        begin, end = int(low, 16), int(high, 16) + 1
        values[begin:end] = bytes([names.index(name)]) * (end - begin)
    # GB9c's Extend is not the GCB Extend property. Keep it independently,
    # including ZWJ, and give linkers their own bit for the conjunct state.
    table = text.split("const InCB_Extend_table:", 1)[1].split("];", 1)[0]
    for low, high in re.findall(r"\('\\u\{([0-9a-f]+)\}',\s*'\\u\{([0-9a-f]+)\}'\)", table):
        for scalar in range(int(low, 16), int(high, 16) + 1):
            values[scalar] |= 16
    table = text.split("pub fn is_incb_linker", 1)[1].split("pub mod", 1)[0]
    for scalar in re.findall(r"\\u\{([0-9A-F]+)\}", table):
        values[int(scalar, 16)] |= 32
    pages, indices, unique = [], bytearray(), {}
    for offset in range(0, len(values), 256):
        page = bytes(values[offset:offset + 256])
        if len(set(page)) == 1:
            # A uniform property page stores its value directly in the index.
            indices.append(page[0])
            continue
        if page not in unique:
            unique[page] = len(pages)
            pages.append(page)
        indices.append(128 | unique[page])
    assert len(pages) < 128
    result = indices + b"".join(pages)
    Path(__file__).with_name("properties.bin").write_bytes(result)
    print(f"{len(pages)} unique pages; {len(result)} bytes")


if __name__ == "__main__":
    generate(Path(sys.argv[1]))
