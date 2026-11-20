#!/usr/bin/env python3
"""Extract matcha-reader's built-in UI fonts into glyph files a dessen app can load.

Dessen apps draw text only upright through the firmware. To turn text with the rest of a
drawing, an app needs the glyph bitmaps itself. This reads matcha's compiled font headers
(lib/EpdFont/builtinFonts/*.h), inflates their DEFLATE groups and writes, per face, one file per
256-codepoint block:

  <out>/<face>/<block hex>.bin
    u16 count, then count entries of
      u16 low (cp & 0xff), u8 w, u8 h, u16 advance (12.4 fixed point), i16 left, i16 top, u32 offset
    then the glyphs' 1-bit bitmaps, rows packed MSB first, each glyph starting on a byte.

2-bit glyphs (the CJK subsets) keep any ink as black, as matcha's black-and-white pass does.
Also prints the faces' line metrics for the app.

A face can also be rasterized from an OpenType font, the way matcha's fontconvert.py does
(150 DPI, any ink kept), for the Chinese characters matcha's Japanese subsets lack:
  face=@font.otf:points:skipface   GB2312 and Big5 ideographs not already in skipface (needs Pillow)

usage: matcha_fonts.py <matcha-reader checkout> <out dir> face=header[:min-max] ...
"""
import os
import re
import struct
import sys
import zlib


def array(src, name):
    m = re.search(r'%s\[\d*\]\s*=\s*\{(.*?)\};' % re.escape(name), src, re.S)
    if not m:
        raise SystemExit('no array ' + name)
    return m.group(1)


def nums(body):
    return [int(x, 0) for x in re.findall(r'-?0x[0-9A-Fa-f]+|-?\d+', re.sub(r'//[^\n]*', '', body))]


def structs(body):
    return [nums(s) for s in re.findall(r'\{([^{}]*)\}', re.sub(r'//[^\n]*', '', body))]


def load(root, header):
    d = os.path.join(root, 'lib/EpdFont/builtinFonts')
    src = open(os.path.join(d, header + '.h'), encoding='utf-8').read()
    shared = open(os.path.join(d, 'SharedFontTables.h'), encoding='utf-8').read()
    init = re.search(r'static const EpdFontData %s = \{(.*?)\};' % header, src, re.S).group(1)
    fields = [f.strip() for f in re.sub(r'//[^\n]*', '', init).split(',')]
    advance_y, ascender, descender = int(fields[4]), int(fields[5]), int(fields[6])
    is2 = fields[7] == 'true'
    bitmap = bytes(nums(array(src, header + 'Bitmaps')))
    glyphs = structs(array(src, header + 'Glyphs'))
    groups = structs(array(src, header + 'Groups'))
    if fields[2] != 'nullptr':
        cps = []
        for first, last, off in structs(array(shared if fields[2] in shared else src, fields[2])):
            for cp in range(first, last + 1):
                cps.append((cp, off + cp - first))
    else:
        cps = list(enumerate(nums(array(src, header + 'Codepoints'))))
        cps = [(cp, i) for i, cp in cps]
    # Glyph index -> its 1-bit bitmap rows.
    out = {}
    for coff, csize, usize, count, first in groups:
        raw = zlib.decompress(bitmap[coff:coff + csize], -15)
        assert len(raw) == usize
        pos = 0
        for gi in range(first, first + count):
            w, h, adv, left, top, dlen, doff = glyphs[gi]
            rows = []
            for y in range(h):
                row = []
                for x in range(w):
                    if is2:  # row-aligned 2-bit
                        b = raw[pos + y * ((w + 3) // 4) + x // 4]
                        row.append((b >> (6 - 2 * (x % 4))) & 3 != 0)
                    else:  # packed 1-bit, continuous across rows
                        k = doff * 8 + y * w + x
                        row.append((raw[k >> 3] >> (7 - (k & 7))) & 1 == 1)
                rows.append(row)
            if is2:
                pos += ((w + 3) // 4) * h
            out[gi] = (w, h, adv, left, top, rows)
    return (advance_y, ascender, descender), cps, out


def write_face(outdir, cps, glyphs, lo, hi):
    blocks = {}
    for cp, gi in cps:
        if lo <= cp <= hi and gi in glyphs and cp >= 0x20:
            blocks.setdefault(cp >> 8, []).append((cp, glyphs[gi]))
    os.makedirs(outdir, exist_ok=True)
    total = 0
    for blk, items in sorted(blocks.items()):
        items.sort()
        head = struct.pack('<H', len(items))
        data = b''
        for cp, (w, h, adv, left, top, rows) in items:
            head += struct.pack('<HBBHhhI', cp & 0xff, w, h, adv, left, top, len(data))
            bits = [p for r in rows for p in r]
            bits += [False] * (-len(bits) % 8)
            data += bytes(sum(b << (7 - i) for i, b in enumerate(bits[k:k + 8])) for k in range(0, len(bits), 8))
        blob = head + data
        open(os.path.join(outdir, '%02x.bin' % blk), 'wb').write(blob)
        total += len(blob)
    return len(blocks), total


def existing(outdir):
    have = set()
    if not os.path.isdir(outdir):
        return have
    for fn in os.listdir(outdir):
        if not fn.endswith('.bin'):
            continue
        blk = int(fn[:-4], 16)
        b = open(os.path.join(outdir, fn), 'rb').read()
        for i in range(struct.unpack_from('<H', b)[0]):
            have.add(blk << 8 | struct.unpack_from('<H', b, 2 + i * 14)[0])
    return have


def rasterize(path, points, skip):
    from PIL import ImageFont
    font = ImageFont.truetype(path, round(points * 150 / 72))
    ascent, descent = font.getmetrics()
    cps, glyphs = [], {}
    for cp in range(0x4E00, 0xA000):
        ch = chr(cp)
        if cp in skip:
            continue
        try:
            ch.encode('gb2312')
        except UnicodeEncodeError:
            try:
                ch.encode('big5')
            except UnicodeEncodeError:
                continue
        mask, (ox, oy) = font.getmask2(ch, mode='L', anchor='ls')
        w, h = mask.size
        rows = [[mask.getpixel((x, y)) >= 64 for x in range(w)] for y in range(h)]
        glyphs[cp] = (w, h, round(font.getlength(ch) * 16), ox, -oy, rows)
        cps.append((cp, cp))
    return (round(ascent * 1.2), ascent, -descent), cps, glyphs


def main():
    root, out = sys.argv[1], sys.argv[2]
    for spec in sys.argv[3:]:
        face, header = spec.split('=')
        lo, hi = 0, 0xFFFF
        if header.startswith('@'):
            path, points, skipface = header[1:].split(':')
            metrics, cps, glyphs = rasterize(path, int(points), existing(os.path.join(out, skipface)))
            n, size = write_face(os.path.join(out, face), cps, glyphs, lo, hi)
            print('%-6s %-28s ascender %d  %d files, %d bytes' % (face, os.path.basename(path), metrics[1], n, size))
            continue
        if ':' in header:
            header, rng = header.split(':')
            lo, hi = (int(v, 16) for v in rng.split('-'))
        metrics, cps, glyphs = load(root, header)
        n, size = write_face(os.path.join(out, face), cps, glyphs, lo, hi)
        print('%-6s %-28s advanceY %d ascender %d descender %d  %d files, %d bytes' % ((face, header) + metrics + (n, size)))


main()
