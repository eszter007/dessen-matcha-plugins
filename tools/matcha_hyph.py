#!/usr/bin/env python3
"""Extract matcha-reader's serialized Liang hyphenation tries into plugin .bin files.

Usage: matcha_hyph.py <matcha checkout> <plugin dir>

Reads lib/Epub/Epub/hyphenation/generated/hyph-<lang>.trie.h and writes
<plugin dir>/hyph/<lang>.bin with this layout (little endian):

  0  4  magic b"HYP1"
  4  4  root offset (u32) into the trie data
  8  1  min prefix (codepoints left of a break)
  9  1  min suffix (codepoints right of a break)
 10  1  alphabet: 0 = Latin, 1 = Cyrillic
 11  1  reserved (0)
 12  .. raw trie bytes (the <lang>_trie_data array)

Min lengths and alphabets mirror LanguageRegistry.cpp.
"""
import os
import re
import struct
import sys

# lang -> (min prefix, min suffix, alphabet), from LanguageRegistry.cpp
LANGS = {
    "en": (3, 3, 0), "fr": (2, 2, 0), "de": (2, 2, 0), "ru": (2, 2, 1),
    "es": (2, 2, 0), "it": (2, 2, 0), "sv": (2, 2, 0), "uk": (2, 2, 1),
    "pl": (2, 2, 0), "fi": (2, 2, 0), "pt": (2, 2, 0),
}


def parse(text, lang):
    m = re.search(r"%s_trie_data\[\]\s*=\s*\{(.*?)\};" % lang, text, re.S)
    data = bytes(int(x, 16) for x in re.findall(r"0x([0-9A-Fa-f]{1,2})\b", m.group(1)))
    p = re.search(r"%s_patterns\s*=\s*\{\s*0x([0-9A-Fa-f]+)u?" % lang, text)
    return int(p.group(1), 16), data


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    gen = os.path.join(sys.argv[1], "lib/Epub/Epub/hyphenation/generated")
    out = os.path.join(sys.argv[2], "hyph")
    os.makedirs(out, exist_ok=True)
    total = 0
    for lang, (lmin, rmin, alpha) in sorted(LANGS.items()):
        src = os.path.join(gen, "hyph-%s.trie.h" % lang)
        if not os.path.exists(src):
            print("skip %s (no %s)" % (lang, src))
            continue
        with open(src, encoding="utf-8") as f:
            root, data = parse(f.read(), lang)
        blob = b"HYP1" + struct.pack("<IBBBB", root, lmin, rmin, alpha, 0) + data
        with open(os.path.join(out, lang + ".bin"), "wb") as f:
            f.write(blob)
        total += len(blob)
        print("%s: %d bytes (root 0x%x)" % (lang, len(blob), root))
    print("total %d bytes" % total)


if __name__ == "__main__":
    main()
