#!/usr/bin/env python3
"""Pre-turn matcha's end-of-book stamps for the Ticket plugin.

Turning the 220 px stamp and growing its halo is the costliest part of drawing a ticket in the
device's app interpreter, and it is the same for every book, so it is done here once. The
integer maths mirror turnDims(), turnCanvas() and haloOf() in plugins/ticket/ticket.c exactly.

Writes plugins/ticket/stamps/<wording>.bin: u32 width, u32 height, then the turned ink and its
2 px halo, each height rows of (width + 31) / 32 little-endian words, pixel x in bit 31 - x % 32.

usage: make_stamps.py <matcha-reader checkout> <plugin dir>
"""
import os
import re
import struct
import sys

root, plugin = sys.argv[1], sys.argv[2]
assets = open(os.path.join(plugin, 'assets.h')).read()
const = lambda n: int(re.search(r'#define %s (-?\d+)' % n, assets).group(1))
SIZE, S, T = const('STAMP_SIZE'), const('STAMP_SIN'), const('STAMP_TAN2')
src = open(os.path.join(root, 'src/components/icons/eobStamp.h'), encoding='utf-8').read()
WORDINGS = {'ja': 'EobStampJa', 'zh-hans': 'EobStampZhHans', 'zh-hant': 'EobStampZhHant', 'yue': 'EobStampYue'}


def rnd16(v):
    return (v + 32768) >> 16  # Python's >> floors like the C arithmetic shift


def dims(w, h):
    a = abs(S)
    dw = (w + ((a * h) >> 16) + 16 + 7) & ~7
    dh = (h + ((a * dw) >> 16) + 16 + 7) & ~7
    return dw, dh


def turn(px, w, h):
    """px: set of (x, y) ink pixels of a w x h canvas. Returns the turned set and dims."""
    dw, dh = dims(w, h)
    cx, cy, ox, oy = dw // 2, dh // 2, dw // 2 - w // 2, dh // 2 - h // 2
    b = set()
    for x, y in px:  # rows
        nx = x + ox + rnd16(-T * (y + oy - cy))
        if 0 <= nx < dw:
            b.add((nx, y + oy))
    a = set()
    for x, y in b:  # columns
        ny = y + rnd16(S * (x - cx))
        if 0 <= ny < dh:
            a.add((x, ny))
    out = set()
    for x, y in a:  # rows again
        nx = x + rnd16(-T * (y - cy))
        if 0 <= nx < dw:
            out.add((nx, y))
    return out, dw, dh


def halo(px, w, h):
    wide = {(x + dx, y) for x, y in px for dx in range(-2, 3) if 0 <= x + dx < w}
    return {(x, y + dy) for x, y in wide for dy in range(-2, 3) if 0 <= y + dy < h}


def words(px, w, h):
    n = (w + 31) // 32
    out = [0] * (n * h)
    for x, y in px:
        out[y * n + x // 32] |= 0x80000000 >> (x % 32)
    return struct.pack('<%dI' % len(out), *out)


os.makedirs(os.path.join(plugin, 'stamps'), exist_ok=True)
for name, array in WORDINGS.items():
    body = re.search(r'%s\[\] = \{(.*?)\};' % array, src, re.S).group(1)
    rle = [int(v, 16) for v in re.findall(r'0x[0-9A-Fa-f]+', body)]
    px, i = set(), 0
    for b in rle:
        run = (b & 0x7F) + 1
        if b & 0x80:
            px.update((k % SIZE, k // SIZE) for k in range(i, i + run))
        i += run
    ink, dw, dh = turn(px, SIZE, SIZE)
    blob = struct.pack('<II', dw, dh) + words(ink, dw, dh) + words(halo(ink, dw, dh), dw, dh)
    open(os.path.join(plugin, 'stamps', name + '.bin'), 'wb').write(blob)
    print(name, dw, dh, len(blob))
