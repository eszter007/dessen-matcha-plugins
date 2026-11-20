// Ticket, ported from matcha-reader's end-of-book screen as a dessen app: the finished book as a
// ticket torn in two -- the cover and title on one half with a 読了 stamp, the reading stats on the
// stub -- and under it a Home button beside "Continue with" the next book in the folder.
//
// device.json launches the app on reader.exit; it shows the ticket only when the book was read to
// the end and exits straight away otherwise. Opened from the plugin list, it shows the last
// finished book. The numbers come from the Insights plugin's stats.db plus the sessions still
// queued for it, so both screens agree.
//
// Like matcha, each half, the stub and the stamp are composed first -- with matcha's own fonts,
// extracted into fonts/ -- then turned as a whole and drawn as bitmaps.
#include "dessen.h"
#include "assets.h"

#define PATH_MAX_LEN 256
#define TITLE_MAX 128

// Ticket geometry, as in matcha's EndOfBookOptions.cpp.
#define CORNER 18
#define TEAR 6
#define TEAR_PERIOD 24
#define BORDER 3
#define BUTTON_H 80
#define MIN_TOP_W 280
#define COVER_HEIGHT 300
#define TEAR_GAP 16

static int W, H, hasTouch;
static char dir[128];
static int dirLen;
static char buf[32000];  // queues, stats.db, book.bin, listings, the stamp: one at a time
static unsigned char cover[40000];  // {u16 w, u16 h} + 1-bit rows from dsn_book_cover
static unsigned char nextThumb[2400];

// ---------------------------------------------------------------- helpers
static int slen(const char *s) { int n = 0; while (s[n]) n++; return n; }
static int seq(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return *a == *b; }
static void scpy(char *d, const char *s, int cap) { int i = 0; for (; s[i] && i < cap - 1; i++) d[i] = s[i]; d[i] = 0; }
static char *utoa(unsigned v, char *p) {
  char t[12]; int n = 0;
  do { t[n++] = (char)('0' + v % 10); v /= 10; } while (v);
  while (n) *p++ = t[--n];
  *p = 0;
  return p;
}
static char *sput(char *p, const char *s) { while (*s) *p++ = *s++; *p = 0; return p; }
static unsigned parseu(const char **s) {
  unsigned v = 0;
  while (**s == ' ' || **s == ',') (*s)++;
  while (**s >= '0' && **s <= '9') v = v * 10 + (unsigned)(*(*s)++ - '0');
  return v;
}
static int imin(int a, int b) { return a < b ? a : b; }
static int imax(int a, int b) { return a > b ? a : b; }
static int iabs(int a) { return a < 0 ? -a : a; }

static int jstr(const char *line, const char *key, char *out, int cap) {
  int kl = slen(key);
  for (const char *s = line; *s; s++) {
    if (*s != '"') continue;
    int i = 0;
    while (i < kl && s[1 + i] == key[i]) i++;
    if (i < kl || s[1 + kl] != '"') continue;
    s += kl + 2;
    while (*s == ':' || *s == ' ') s++;
    if (*s != '"') return 0;
    s++;
    int n = 0;
    while (*s && *s != '"' && n < cap - 1) {
      if (*s == '\\' && s[1]) s++;
      out[n++] = *s++;
    }
    out[n] = 0;
    return 1;
  }
  return 0;
}

// ---------------------------------------------------------------- dates
static unsigned daysFromCivil(int y, int m, int d) {
  y -= m <= 2;
  const int era = y / 400, yoe = y - era * 400;
  const int doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
  return (unsigned)(era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468);
}
static void civilFromDays(unsigned z, int *y, int *m, int *d) {
  int zz = (int)z + 719468;
  const int era = zz / 146097, doe = zz - era * 146097;
  const int yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  const int doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  const int mp = (5 * doy + 2) / 153;
  *d = doy - (153 * mp + 2) / 5 + 1;
  *m = mp + (mp < 10 ? 3 : -9);
  *y = yoe + era * 400 + (*m <= 2);
}
static char *two(unsigned v, char *p) { if (v < 10) *p++ = '0'; return utoa(v, p); }
// The stamp's date, "MM/DD/YYYY" like matcha's English UI.
static void fmtStampDate(unsigned day, char *p) {
  int y, m, d;
  civilFromDays(day, &y, &m, &d);
  p = two((unsigned)m, p); *p++ = '/'; p = two((unsigned)d, p); *p++ = '/'; utoa((unsigned)y, p);
}
static unsigned today;
static int tzOffset;

// ---------------------------------------------------------------- languages
static void normLang(const char *in, int n, char *out) {
  int k = 0;
  for (int i = 0; i < n && k < 7; i++) {
    char c = in[i];
    if (c == '-' || c == '_' || c == ' ') break;
    if (c >= 'A' && c <= 'Z') c = (char)(c - 'A' + 'a');
    out[k++] = c;
  }
  out[k] = 0;
}
static const char *langName(const char *code) {
  static const char *const names[][2] = {
      {"en", "English"}, {"de", "Deutsch"}, {"fr", "Français"}, {"es", "Español"}, {"it", "Italiano"},
      {"pt", "Português"}, {"nl", "Nederlands"}, {"hu", "Magyar"}, {"pl", "Polski"}, {"cs", "Čeština"},
      {"sv", "Svenska"}, {"da", "Dansk"}, {"fi", "Suomi"}, {"no", "Norsk"}, {"nb", "Norsk"},
      {"tr", "Türkçe"}, {"ro", "Română"}, {"ru", "Русский"}, {"uk", "Українська"}, {"el", "Ελληνικά"},
      {"ja", "日本語"}, {"zh", "中文"}, {"yue", "粵語"}, {"ko", "한국어"},
  };
  for (unsigned i = 0; i < sizeof names / sizeof names[0]; i++) if (seq(names[i][0], code)) return names[i][1];
  return 0;
}
static int hasCI(const char *s, const char *sub) {  // case-insensitive substring
  for (; *s; s++) {
    int i = 0;
    for (; sub[i]; i++) {
      char a = s[i], b = sub[i];
      if (a >= 'A' && a <= 'Z') a = (char)(a + 32);
      if (!a || a != b) break;
    }
    if (!sub[i]) return 1;
  }
  return 0;
}
// 読了 for Japanese and every non-Chinese book, 读完 / 讀完 for Chinese, 睇完 for Cantonese.
static const char *stampFor(const char *tag) {
  char code[8];
  normLang(tag, slen(tag), code);
  if (seq(code, "yue")) return "yue";
  if (seq(code, "zh"))
    return hasCI(tag, "hant") || hasCI(tag, "-tw") || hasCI(tag, "-hk") || hasCI(tag, "-mo") || hasCI(tag, "_tw") ||
                   hasCI(tag, "_hk")
               ? "zh-hant"
               : "zh-hans";
  return "ja";
}

// ---------------------------------------------------------------- the book
static char bookPath[PATH_MAX_LEN], title[TITLE_MAX], langTag[24], langCode[8];

// libstdc++'s std::hash<std::string> on a 32-bit target: MurmurHash2, seed 0xc70f6907.
static unsigned murmur2(const char *s, unsigned len) {
  const unsigned m = 0x5bd1e995u;
  unsigned h = 0xc70f6907u ^ len;
  const unsigned char *b = (const unsigned char *)s;
  while (len >= 4) {
    unsigned k = b[0] | b[1] << 8 | b[2] << 16 | (unsigned)b[3] << 24;
    k *= m; k ^= k >> 24; k *= m;
    h *= m; h ^= k;
    b += 4; len -= 4;
  }
  switch (len) {
    case 3: h ^= (unsigned)b[2] << 16; /* fallthrough */
    case 2: h ^= (unsigned)b[1] << 8; /* fallthrough */
    case 1: h ^= b[0]; h *= m;
  }
  h ^= h >> 13; h *= m; h ^= h >> 15;
  return h;
}
static void utf8cpy(char *d, const char *s, unsigned n, int cap) {
  unsigned k = n < (unsigned)cap - 1 ? n : (unsigned)cap - 1;
  while (k > 0 && k < n && ((unsigned char)s[k] & 0xC0) == 0x80) k--;
  for (unsigned i = 0; i < k; i++) d[i] = s[i];
  d[k] = 0;
}
static const char *baseName(const char *path) {
  const char *n = path;
  for (const char *c = path; *c; c++) if (*c == '/' && c[1]) n = c + 1;
  return n;
}
static void stripExt(const char *name, char *out, int cap) {
  int len = slen(name), dot = len;
  for (int i = len - 1; i > 0; i--) if (name[i] == '.') { dot = i; break; }
  utf8cpy(out, name, (unsigned)dot, cap);
}
// Title and language from the reader's cache: book.bin is u8 version, u32 lut, u16 spine, u16 toc,
// then u32-length strings title, author, language.
static void lookupBook(void) {
  char p[48], *q = sput(p, "/.crosspoint/epub_");
  q = utoa(murmur2(bookPath, (unsigned)slen(bookPath)), q);
  sput(q, "/book.bin");
  int n = dsn_fs_read(p, slen(p), buf, sizeof buf);
  unsigned off = 9;
  for (int field = 0; n >= 9 && field < 3; field++) {
    if (off + 4 > (unsigned)n) break;
    const unsigned char *u = (const unsigned char *)buf + off;
    unsigned len = u[0] | u[1] << 8 | u[2] << 16 | (unsigned)u[3] << 24;
    off += 4;
    if (len > (unsigned)n - off) break;
    if (field == 0 && len) utf8cpy(title, buf + off, len, TITLE_MAX);
    if (field == 2) utf8cpy(langTag, buf + off, len, sizeof langTag);
    off += len;
  }
  if (!title[0]) stripExt(baseName(bookPath), title, TITLE_MAX);
  normLang(langTag, slen(langTag), langCode);
}

// ---------------------------------------------------------------- stats (from Insights)
// stats.db lines: "L code" per language index, "B secs end_bp lang sessions first finish ndays
// d1,d2,...<TAB>title<TAB>path" per book (older "B secs end_bp lang path" lines have no days).
static const char *const STATS_DIRS[] = {"/plugins/stats/", "/plugins/insights/"};
static int statsDir = -1;
static unsigned bookSecs, bookFirst, bookFinish, bookDays, booksFinished, langFinished;
static unsigned short seenDays[64];
static int nSeen;
static char dbLangs[16][8];
static int nDbLangs;

static void addDay(unsigned d) {
  for (int i = 0; i < nSeen; i++) if (seenDays[i] == d) return;
  if (nSeen < 64) seenDays[nSeen++] = (unsigned short)d;
  bookDays++;
}
static int statsPath(char *out, const char *name) {
  char *p = sput(out, STATS_DIRS[statsDir]);
  p = sput(p, name);
  return (int)(p - out);
}
static void readField(const char **s, char *out, int cap, char stop) {
  int i = 0;
  while (**s && **s != '\n' && **s != stop) { if (i < cap - 1) out[i++] = **s; (*s)++; }
  out[i] = 0;
}
// Last finished book in stats.db, for when the app is opened by hand.
static char lastFinished[PATH_MAX_LEN];
static void loadStats(int wantLast) {
  char p[64];
  int n = 0;
  statsDir = -1;
  for (int i = 0; i < 2 && statsDir < 0; i++) {  // the first Insights folder with a db or a queue
    statsDir = i;
    n = dsn_fs_read(p, statsPath(p, "stats.db"), buf, sizeof buf - 1);
    if (n <= 0 && dsn_fs_read(p, statsPath(p, "app-events.jsonl"), buf, 0) == DSN_ERR_IO) statsDir = -1;
  }
  if (n < 0) n = 0;
  buf[n] = 0;
  unsigned lastDay = 0;
  int finishedHere = 0;
  for (const char *s = buf; *s;) {
    char k = *s++;
    if (k == 'L' && nDbLangs < 16) {
      if (*s == ' ') s++;
      readField(&s, dbLangs[nDbLangs++], 8, '\n');
    } else if (k == 'B') {
      unsigned secs = parseu(&s), endbp = parseu(&s), lang = parseu(&s);
      unsigned first = 0, finish = 0, nd = 0;
      int tabbed = 0;
      for (const char *e = s; *e && *e != '\n'; e++) if (*e == '\t') { tabbed = 1; break; }
      unsigned short days[24];
      int ndl = 0;
      if (tabbed) {
        parseu(&s); first = parseu(&s); finish = parseu(&s); nd = parseu(&s);
        while (*s == ' ') s++;
        while (*s >= '0' && *s <= '9') { unsigned d = parseu(&s); if (ndl < 24) days[ndl++] = (unsigned short)d; if (*s == ',') s++; }
        if (*s == '\t') s++;
        while (*s && *s != '\t' && *s != '\n') s++;
        if (*s == '\t') s++;
      } else if (*s == ' ') s++;
      char path[PATH_MAX_LEN];
      readField(&s, path, PATH_MAX_LEN, '\n');
      const int done = finish || endbp >= 10000;
      if (done) {
        booksFinished++;
        if (lang < (unsigned)nDbLangs && langCode[0] && seq(dbLangs[lang], langCode)) langFinished++;
        if (wantLast && finish >= lastDay) { lastDay = finish; scpy(lastFinished, path, PATH_MAX_LEN); }
      }
      if (!wantLast && seq(path, bookPath)) {
        finishedHere = done;
        bookSecs = secs; bookFirst = first; bookFinish = finish; bookDays = nd;
        for (int i = 0; i < ndl; i++) if (nSeen < 64) seenDays[nSeen++] = days[i];
      }
    }
    while (*s && *s != '\n') s++;
    if (*s) s++;
  }
  if (wantLast || statsDir < 0) return;
  // Sessions Insights has not folded in yet: the one that just ended is still in its queue.
  n = dsn_fs_read(p, statsPath(p, "app-events.jsonl"), buf, sizeof buf - 1);
  if (n <= 0) return;
  buf[n] = 0;
  for (char *line = buf; *line;) {
    char *e = line;
    while (*e && *e != '\n') e++;
    char save = *e; *e = 0;
    char v[PATH_MAX_LEN];
    if (jstr(line, "e", v, sizeof v) && seq(v, "reader.session") && jstr(line, "book", v, sizeof v) && seq(v, bookPath)) {
      const char *q;
      unsigned start = 0, dur = 0, endbp = 0;
      if (jstr(line, "start_time", v, sizeof v)) { q = v; start = parseu(&q); }
      if (jstr(line, "duration_seconds", v, sizeof v)) { q = v; dur = parseu(&q); }
      if (jstr(line, "end_progress_bp", v, sizeof v)) { q = v; endbp = parseu(&q); }
      if (start && dur) {
        unsigned day = (unsigned)((int)start + tzOffset) / 86400u;
        bookSecs += dur;
        addDay(day);
        if (!bookFirst || day < bookFirst) bookFirst = day;
        if (endbp >= 10000 && !bookFinish) bookFinish = (unsigned)((int)(start + dur) + tzOffset) / 86400u;
      }
    }
    *e = save;
    line = *e ? e + 1 : e;
  }
  if (!finishedHere) { booksFinished++; if (langCode[0]) langFinished++; }
}

// ---------------------------------------------------------------- next book in the folder
static char nextPath[PATH_MAX_LEN], nextName[PATH_MAX_LEN];

static int lower(int c) { return c >= 'A' && c <= 'Z' ? c + 32 : c; }
// Natural order, case-insensitive: digit runs compare as numbers ("2" < "10").
static int naturalLess(const char *a, const char *b) {
  while (*a && *b) {
    if (*a >= '0' && *a <= '9' && *b >= '0' && *b <= '9') {
      while (*a == '0') a++;
      while (*b == '0') b++;
      int la = 0, lb = 0;
      while (a[la] >= '0' && a[la] <= '9') la++;
      while (b[lb] >= '0' && b[lb] <= '9') lb++;
      if (la != lb) return la < lb;
      for (int i = 0; i < la; i++) if (a[i] != b[i]) return a[i] < b[i];
      a += la; b += lb;
      continue;
    }
    int ca = lower((unsigned char)*a), cb = lower((unsigned char)*b);
    if (ca != cb) return ca < cb;
    a++, b++;
  }
  return !*a && *b;
}
static int endsWithCI(const char *s, const char *ext) {
  int n = slen(s), e = slen(ext);
  if (n < e) return 0;
  for (int i = 0; i < e; i++) if (lower((unsigned char)s[n - e + i]) != ext[i]) return 0;
  return 1;
}
static int isBook(const char *name) {
  return endsWithCI(name, ".epub") || endsWithCI(name, ".xtc") || endsWithCI(name, ".xtch") || endsWithCI(name, ".txt") ||
         endsWithCI(name, ".md");
}
static void findNext(void) {
  const char *name = baseName(bookPath);
  char folder[PATH_MAX_LEN];
  int fl = (int)(name - bookPath);
  if (fl <= 0) return;
  utf8cpy(folder, bookPath, (unsigned)fl, PATH_MAX_LEN);
  int n = dsn_fs_list(folder, fl, buf, sizeof buf - 1);
  if (n <= 0) return;
  buf[n] = 0;
  for (char *e = buf; *e;) {
    char *end = e;
    while (*end && *end != '\n') end++;
    char save = *end; *end = 0;
    if (e[0] != '.' && end > e && end[-1] != '/' && isBook(e) && naturalLess(name, e) &&
        (!nextName[0] || naturalLess(e, nextName)))
      scpy(nextName, e, PATH_MAX_LEN);
    *end = save;
    e = *end ? end + 1 : end;
  }
  if (nextName[0]) { scpy(nextPath, folder, PATH_MAX_LEN); scpy(nextPath + fl, nextName, PATH_MAX_LEN - fl); }
}

// ---------------------------------------------------------------- canvases
// 1-bit images in app memory, set bit = ink, 32 pixels to a word (pixel x is bit 31 - x % 32).
// The device runs apps in an interpreter at a few million instructions a second, so everything
// here works on whole words, never pixel by pixel over a whole piece.
typedef struct { unsigned *b; int w, h, stride; } Canvas;  // stride in words
static unsigned pool[200000];  // canvases, freed after each piece
static int poolUsed;           // in words
static void wclear(unsigned *p, int n) { while (n-- > 0) *p++ = 0; }
static int canvasNew(Canvas *c, int w, int h) {
  c->w = w; c->h = h; c->stride = (w + 31) / 32;
  const int n = c->stride * h;
  if (poolUsed + n > (int)(sizeof pool / 4)) { c->b = 0; return 0; }
  c->b = pool + poolUsed;
  poolUsed += n;
  wclear(c->b, n);
  return 1;
}
static unsigned *row(const Canvas *c, int y) { return c->b + y * c->stride; }
static void cvSet(Canvas *c, int x, int y) {
  if (x >= 0 && y >= 0 && x < c->w && y < c->h) c->b[y * c->stride + (x >> 5)] |= 0x80000000u >> (x & 31);
}
static void cvClr(Canvas *c, int x, int y) {
  if (x >= 0 && y >= 0 && x < c->w && y < c->h) c->b[y * c->stride + (x >> 5)] &= ~(0x80000000u >> (x & 31));
}
static void cvLine(Canvas *c, int x0, int y0, int x1, int y1) {  // straight lines only
  for (int y = imin(y0, y1); y <= imax(y0, y1); y++)
    for (int x = imin(x0, x1); x <= imax(x0, x1); x++) cvSet(c, x, y);
}
// Draws a canvas's ink in `color`, cropped to the screen (the firmware refuses a bitmap that
// leaves it). dsn_bitmap takes bytes, MSB first, and its mask draws clear bits, so each row is
// written out byte by byte, inverted.
static unsigned char *scratchMask;
static int scratchCap;
static void blit(const Canvas *c, int x, int y, int color) {
  const int x0 = imax(x, 0), y0 = imax(y, 0), x1 = imin(x + c->w, W), y1 = imin(y + c->h, H);
  const int w = x1 - x0, h = y1 - y0, bs = (w + 7) / 8, n = bs * h;
  if (w <= 0 || h <= 0 || n > scratchCap) return;
  unsigned char *o = scratchMask;
  const int off = (x0 - x) & 31, k0 = (x0 - x) >> 5;
  for (int r = y0 - y; r < y1 - y; r++) {
    const unsigned *s = row(c, r) + k0;
    const int left = c->stride - k0;
    for (int i = 0, wi = 0; i < bs; wi++) {  // a word at a time, out as up to 4 bytes
      unsigned v = s[wi] << off;
      if (off && wi + 1 < left) v |= s[wi + 1] >> (32 - off);
      v = ~v;
      o[i++] = (unsigned char)(v >> 24);
      if (i < bs) o[i++] = (unsigned char)(v >> 16);
      if (i < bs) o[i++] = (unsigned char)(v >> 8);
      if (i < bs) o[i++] = (unsigned char)v;
    }
    o += bs;
  }
  dsn_bitmap(x0, y0, w, h, scratchMask, n, w, h, DSN_BITMAP_MASK, DSN_BITMAP_CENTER, color, 0);
}

// d |= s moved k pixels to the right (left when k < 0); bits moved past either end drop out.
static void orShifted(unsigned *d, const unsigned *s, int n, int k) {
  const int q = k >> 5, r = k & 31;  // floor division, so r is 0..31 for negative k too
  int i0 = q > 0 ? q : 0, i1 = n + q < n ? n + q : n;
  for (int i = i0; i < i1 && i < n; i++) d[i] |= s[i - q] >> r;
  if (r) {
    i0 = q + 1 > 0 ? q + 1 : 0;
    for (int i = i0; i < n && i - q - 1 < n; i++) d[i] |= s[i - q - 1] << (32 - r);
  }
}
static void andShifted(unsigned *d, const unsigned *s, int n, int k) {
  const int q = k >> 5, r = k & 31;
  for (int i = 0; i < n; i++) {
    const int j = i - q;
    unsigned v = 0;
    if (j >= 0 && j < n) v = s[j] >> r;
    if (r && j - 1 >= 0 && j - 1 < n) v |= s[j - 1] << (32 - r);
    d[i] &= v;
  }
}

// ---------------------------------------------------------------- matcha's fonts
// fonts/<face>/<block>.bin, extracted from matcha's built-in fonts by tools/matcha_fonts.py:
// u16 count, count x {u16 low, u8 w, u8 h, u16 advance (12.4), i16 left, i16 top, u32 offset},
// then 1-bit bitmaps. The UI faces fall back to the Japanese subsets, as in matcha, then to
// Chinese ideographs (GB2312 and Big5) the Japanese subsets lack.
enum { F_S, F_B10, F_B12, F_J8, F_J12, F_C8, F_C12, FACES };
static const char *const FACE_DIR[FACES] = {"s", "b10", "b12", "j8", "j12", "c8", "c12"};
static const signed char FACE_ASC[FACES] = {16, 20, 24, 20, 29, 20, 29};
// Text styles: matcha's SMALL, UI_10 bold and UI_12 bold.
typedef struct { int face, fallback, fallback2, bold, lineH; } Style;
static const Style SMALL = {F_S, F_J8, F_C8, 0, 19}, UI10B = {F_B10, F_J8, F_C8, 1, 24},
                   UI12B = {F_B12, F_J12, F_C12, 1, 29};

static unsigned char fontPool[500000];  // loaded glyph blocks, kept for the whole run
static int fontUsed;
typedef struct { unsigned char face, blk; const unsigned char *data; } Block;
static Block blocks[96];
static int nBlocks;
static const unsigned char *blockFor(int face, int blk) {
  for (int i = 0; i < nBlocks; i++) if (blocks[i].face == face && blocks[i].blk == blk) return blocks[i].data;
  if (nBlocks == 96) return 0;
  char p[200], *q = p;
  for (int i = 0; i < dirLen; i++) *q++ = dir[i];
  if (q == p || q[-1] != '/') *q++ = '/';
  q = sput(q, "fonts/"); q = sput(q, FACE_DIR[face]); *q++ = '/';
  const char *hex = "0123456789abcdef";
  *q++ = hex[blk >> 4]; *q++ = hex[blk & 15];
  q = sput(q, ".bin");
  unsigned char *dst = fontPool + fontUsed;
  const int room = (int)sizeof fontPool - fontUsed - 8;  // glyph rows are read 4 bytes at a time
  int n = room > 0 ? dsn_fs_read(p, (int)(q - p), dst, room) : -1;
  Block *b = &blocks[nBlocks++];
  b->face = (unsigned char)face; b->blk = (unsigned char)blk;
  b->data = n > 2 ? dst : 0;
  if (n > 2) fontUsed += (n + 8 + 3) & ~3;
  return b->data;
}
typedef struct { int w, h, adv, left, top; const unsigned char *bits; } Glyph;
static int glyphIn(int face, unsigned cp, Glyph *g) {
  if (cp > 0xFFFF) return 0;
  const unsigned char *b = blockFor(face, (int)(cp >> 8));
  if (!b) return 0;
  int lo = 0, hi = (b[0] | b[1] << 8) - 1;
  const int n = hi + 1;
  const unsigned want = cp & 0xff;
  while (lo <= hi) {  // entries are sorted by codepoint
    const int mid = (lo + hi) >> 1;
    const unsigned char *e = b + 2 + mid * 14;
    const unsigned have = e[0] | e[1] << 8;
    if (have < want) { lo = mid + 1; continue; }
    if (have > want) { hi = mid - 1; continue; }
    g->w = e[2]; g->h = e[3];
    g->adv = e[4] | e[5] << 8;
    g->left = (short)(e[6] | e[7] << 8);
    g->top = (short)(e[8] | e[9] << 8);
    g->bits = b + 2 + n * 14 + (e[10] | e[11] << 8 | e[12] << 16 | (unsigned)e[13] << 24);
    return 1;
  }
  return 0;
}
// The glyph for cp in a style; sets *fromFallback when the Japanese face supplied it.
static int glyphFor(Style st, unsigned cp, Glyph *g, int *fromFallback) {
  *fromFallback = 0;
  if (glyphIn(st.face, cp, g)) return 1;
  *fromFallback = 1;
  return glyphIn(st.fallback, cp, g) || glyphIn(st.fallback2, cp, g);
}
static unsigned utf8next(const char **s) {
  const unsigned char *p = (const unsigned char *)*s;
  unsigned c = *p++;
  int extra = c >= 0xF0 ? 3 : c >= 0xE0 ? 2 : c >= 0xC0 ? 1 : 0;
  if (extra) c &= 0x3F >> extra;
  while (extra-- && (*p & 0xC0) == 0x80) c = c << 6 | (*p++ & 0x3F);
  *s = (const char *)p;
  return c;
}
static int textWidth(Style st, const char *s) {
  int acc = 0, extra = 0;
  while (*s) {
    Glyph g; int fb;
    if (glyphFor(st, utf8next(&s), &g, &fb)) { acc += g.adv; extra = fb && st.bold; }
  }
  return ((acc + 8) >> 4) + extra;
}
// ORs `n` (<= 24) bits starting at bit `k` of a glyph's bitmap into canvas row y at x.
static void orBits(Canvas *c, int x, int y, const unsigned char *bits, int k, int n) {
  if (y < 0 || y >= c->h || n <= 0) return;
  const unsigned char *p = bits + (k >> 3);
  unsigned v = ((unsigned)p[0] << 24 | (unsigned)p[1] << 16 | (unsigned)p[2] << 8 | p[3]) << (k & 7);
  v &= ~0u << (32 - n);
  if (x < 0) { if (x <= -32) return; v <<= -x; x = 0; }
  unsigned *r = row(c, y);
  const int i = x >> 5, o = x & 31;
  if (i < c->stride) r[i] |= v >> o;
  if (o && i + 1 < c->stride) r[i + 1] |= v << (32 - o);
}
// Draws one line with its top at y; bold styles thicken the borrowed Japanese glyphs by a pixel.
static void drawText(Canvas *c, int x, int y, Style st, const char *s) {
  int acc = 0;
  while (*s) {
    Glyph g; int fb;
    if (!glyphFor(st, utf8next(&s), &g, &fb)) continue;
    const int gx = x + ((acc + 8) >> 4) + g.left, gy = y + FACE_ASC[st.face] - g.top;
    for (int yy = 0; yy < g.h; yy++)
      for (int part = 0; part < g.w; part += 24) {
        const int n = imin(24, g.w - part), k = yy * g.w + part;
        orBits(c, gx + part, gy + yy, g.bits, k, n);
        if (fb && st.bold) orBits(c, gx + part + 1, gy + yy, g.bits, k, n);
      }
    acc += g.adv;
  }
}
static void drawTextAligned(Canvas *c, int x, int y, int w, Style st, const char *s, int align) {
  const int tw = textWidth(st, s);
  drawText(c, align == DSN_ALIGN_CENTER ? x + (w - tw) / 2 : align == DSN_ALIGN_RIGHT ? x + w - tw : x, y, st, s);
}
// Cuts s to fit maxW, ending in "…" when it had to cut.
static void truncated(Style st, const char *s, int maxW, char *out, int cap) {
  scpy(out, s, cap);
  if (textWidth(st, out) <= maxW) return;
  int n = slen(out);
  while (n > 0) {
    do n--; while (n > 0 && ((unsigned char)out[n] & 0xC0) == 0x80);
    while (n > 0 && out[n - 1] == ' ') n--;
    if (n + 4 > cap) continue;
    out[n] = 0;
    char *e = out + n;
    sput(e, "…");
    if (textWidth(st, out) <= maxW) return;
    *e = 0;
  }
  sput(out, "…");
}
static int isWide(unsigned cp) { return cp >= 0x2E80; }
// Up to two lines: breaks at spaces, or between characters where either side is CJK. The second
// line ends in "…" when the rest does not fit.
static int wrap2(Style st, const char *s, int maxW, char *l1, char *l2, int cap) {
  scpy(l1, s, cap);
  l2[0] = 0;
  if (textWidth(st, s) <= maxW) return 1;
  int best = 0;
  const char *p = s;
  unsigned prev = 0;
  while (*p) {
    const char *at = p;
    unsigned cp = utf8next(&p);
    if (at > s && (cp == ' ' || isWide(cp) || isWide(prev))) {
      const int n = (int)(at - s);
      if (n < cap) {
        utf8cpy(l1, s, (unsigned)n, cap);
        if (textWidth(st, l1) <= maxW) best = n; else break;
      }
    }
    prev = cp;
  }
  if (!best) {  // one long word: cut it where it fills the line
    for (p = s; *p;) {
      const char *at = p;
      utf8next(&p);
      utf8cpy(l1, s, (unsigned)(p - s), cap);
      if (textWidth(st, l1) > maxW) { best = (int)(at - s); break; }
    }
  }
  utf8cpy(l1, s, (unsigned)best, cap);
  const char *rest = s + best;
  while (*rest == ' ') rest++;
  truncated(st, rest, maxW, l2, cap);
  return 2;
}

// ---------------------------------------------------------------- shapes
enum { SHAPE_TOP, SHAPE_STUB };

static int tearAt(int x) {
  const int t = ((x % TEAR_PERIOD) + TEAR_PERIOD) % TEAR_PERIOD;
  return iabs(t - TEAR_PERIOD / 2) * TEAR * 2 / TEAR_PERIOD;
}
static int outsideCorner(int x, int y, int cx, int cy) {
  const int dx = x - cx, dy = y - cy;
  return dx * dx + dy * dy > CORNER * CORNER;
}
// The top half has round top corners and a torn bottom edge, the stub a torn top edge and round
// bottom corners.
static int inShape(int shape, int w, int h, int x, int y) {
  if (x < 0 || y < 0 || x >= w || y >= h) return 0;
  if (x >= CORNER && x < w - CORNER && y >= TEAR && y < h - TEAR) return 1;  // the bulk: no edge near
  if (shape == SHAPE_TOP) {
    if (y > h - 1 - TEAR + tearAt(x)) return 0;
    if (y < CORNER && x < CORNER && outsideCorner(x, y, CORNER, CORNER)) return 0;
    if (y < CORNER && x >= w - CORNER && outsideCorner(x, y, w - 1 - CORNER, CORNER)) return 0;
    return 1;
  }
  if (y < tearAt(x)) return 0;
  if (y >= h - CORNER && x < CORNER && outsideCorner(x, y, CORNER, h - 1 - CORNER)) return 0;
  if (y >= h - CORNER && x >= w - CORNER && outsideCorner(x, y, w - 1 - CORNER, h - 1 - CORNER)) return 0;
  return 1;
}
// One row of a piece's paper: every pixel inShape(), tested one by one only near its corners
// and along the tear.
static void paperRow(Canvas *p, int shape, int y) {
  const int w = p->w, h = p->h, n = p->stride;
  unsigned *r = row(p, y);
  for (int i = 0; i < n; i++) r[i] = ~0u;
  if (w & 31) r[n - 1] = ~0u << (32 - (w & 31));
  const int tearRow = shape == SHAPE_TOP ? y >= h - 1 - TEAR : y < TEAR;
  const int cornerRow = shape == SHAPE_TOP ? y < CORNER : y >= h - CORNER;
  if (tearRow) {
    for (int x = 0, m = 0; x < w; x++, m = m + 1 == TEAR_PERIOD ? 0 : m + 1) {  // m = x % period
      const int tear = iabs(m - TEAR_PERIOD / 2) * TEAR * 2 / TEAR_PERIOD;  // tearAt(x)
      const int out = shape == SHAPE_TOP ? y > h - 1 - TEAR + tear : y < tear;
      if (out || ((x < CORNER || x >= w - CORNER) && !inShape(shape, w, h, x, y))) cvClr(p, x, y);
    }
  } else if (cornerRow) {
    for (int x = 0; x < CORNER; x++) {
      if (!inShape(shape, w, h, x, y)) cvClr(p, x, y);
      if (!inShape(shape, w, h, w - 1 - x, y)) cvClr(p, w - 1 - x, y);
    }
  }
}
// Adds the piece's outline to ink: paper whose neighbourhood (3 px across, 2 px diagonally)
// leaves the paper. Rows away from the corners and the tear are solid, so only their ends ink.
static void outline(Canvas *ink, int shape) {
  Canvas paper;
  const int mark = poolUsed;
  if (!canvasNew(&paper, ink->w, ink->h)) return;
  const int w = paper.w, h = paper.h, n = paper.stride, M = CORNER + BORDER + TEAR;
  for (int y = 0; y < h; y++)  // only the bands the outline is computed in, and their neighbours
    if (y < M + 3 || y >= h - M - 3) paperRow(&paper, shape, y);
  static const signed char OFF[8][2] = {{-3, 0}, {3, 0}, {0, -3}, {0, 3}, {-2, -2}, {2, -2}, {-2, 2}, {2, 2}};
  unsigned e[32];
  if (n > 32) { poolUsed = mark; return; }
  for (int y = 0; y < h; y++) {
    if (y >= M && y < h - M) {
      for (int x = 0; x < BORDER; x++) cvSet(ink, x, y), cvSet(ink, w - 1 - x, y);
      continue;
    }
    for (int i = 0; i < n; i++) e[i] = row(&paper, y)[i];
    for (int k = 0; k < 8; k++) {
      const int sy = y + OFF[k][1];
      if (sy < 0 || sy >= h) { wclear(e, n); break; }
      andShifted(e, row(&paper, sy), n, -OFF[k][0]);
    }
    unsigned *d = row(ink, y);
    const unsigned *p = row(&paper, y);
    for (int i = 0; i < n; i++) d[i] |= p[i] & ~e[i];
  }
  poolUsed = mark;
}

// ---------------------------------------------------------------- turning
typedef struct { int c, s, t; } Turn;  // Q16 cos, sin and tan(angle / 2), clockwise on screen
static const Turn TOP_TURN = {TOP_COS, TOP_SIN, TOP_TAN2}, STUB_TURN = {STUB_COS, STUB_SIN, STUB_TAN2},
                  STAMP_TURN = {STAMP_COS, STAMP_SIN, STAMP_TAN2};

// Turns local (x, y) of a w x h piece about its centre, which sits at (cx, cy).
static void turnPoint(int x, int y, int w, int h, Turn t, int cx, int cy, int *ox, int *oy) {
  const int px = 2 * x - w, py = 2 * y - h;  // doubled, to keep the half pixel
  *ox = cx + (t.c * px - t.s * py + 65536) / 131072;
  *oy = cy + (t.s * px + t.c * py + 65536) / 131072;
}
static int rnd16(int v) { return (v + 32768) >> 16; }

// The canvas a w x h piece needs once turned: a margin for what the tilt pushes out.
// tools/make_stamps.py mirrors this and turnCanvas() for the pre-turned stamps.
static void turnDims(int w, int h, Turn t, int *dw, int *dh) {
  const int as = iabs(t.s);
  *dw = (w + ((as * h) >> 16) + 16 + 7) & ~7;
  *dh = (h + ((as * *dw) >> 16) + 16 + 7) & ~7;
}

// Turns src about its centre into a new dw x dh canvas by three shears -- rows, columns, rows:
// x -= tan(a/2) y, then y += sin(a) x, then x -= tan(a/2) y again. Only rows that can hold ink
// are visited.
static int turnCanvas(const Canvas *src, Turn t, Canvas *out) {
  int dw, dh;
  turnDims(src->w, src->h, t, &dw, &dh);
  Canvas a, b;
  if (!canvasNew(&a, dw, dh) || !canvasNew(&b, dw, dh)) return 0;
  const int n = a.stride, cx = dw / 2, cy = dh / 2, ox = cx - src->w / 2, oy = cy - src->h / 2;
  if (n > 32) return 0;
  for (int y = 0; y < src->h; y++) {  // centred in b, sheared on the way
    unsigned tmp[32];
    wclear(tmp, n);
    for (int i = 0; i < src->stride && i < n; i++) tmp[i] = row(src, y)[i];
    orShifted(row(&b, y + oy), tmp, n, ox + rnd16(-t.t * (y + oy - cy)));
  }
  int kmin = 0, kmax = 0;
  for (int bx = 0; bx < n; bx++) {  // columns: the 32 of a word, in runs that move alike
    int bit = 0;
    while (bit < 32) {
      const int k = rnd16(t.s * (bx * 32 + bit - cx));
      int end = bit + 1;
      while (end < 32 && rnd16(t.s * (bx * 32 + end - cx)) == k) end++;
      const unsigned mask = end - bit == 32 ? ~0u : ((1u << (end - bit)) - 1) << (32 - end);
      kmin = imin(kmin, k); kmax = imax(kmax, k);
      const int y0 = imax(oy, -k), y1 = imin(oy + src->h, dh - k);
      for (int y = y0; y < y1; y++) a.b[(y + k) * n + bx] |= b.b[y * n + bx] & mask;
      bit = end;
    }
  }
  wclear(row(&b, oy), n * src->h);
  for (int y = imax(0, oy + kmin); y < imin(dh, oy + src->h + kmax); y++)
    orShifted(row(&b, y), row(&a, y), n, rnd16(-t.t * (y - cy)));
  *out = b;
  return 1;
}

// Draws a composed piece turned about its centre (cx, cy). The screen under the ticket is
// blank, so only the ink is drawn.
static void drawTurned(const Canvas *ink, Turn t, int cx, int cy) {
  const int mark = poolUsed;
  Canvas k;
  if (turnCanvas(ink, t, &k)) blit(&k, cx - k.w / 2, cy - k.h / 2, DSN_BLACK);
  poolUsed = mark;
}
// A 2 px halo around a canvas's ink, as a new canvas.
static int haloOf(const Canvas *k, Canvas *halo) {
  Canvas wide;
  const int n = k->stride, h = k->h;
  if (!canvasNew(&wide, k->w, h) || !canvasNew(halo, k->w, h)) return 0;
  for (int y = 0; y < h; y++) {
    unsigned any = 0;
    for (int i = 0; i < n; i++) any |= row(k, y)[i];
    if (any)  // most rows of a turned strip are empty
      for (int dx = -2; dx <= 2; dx++) orShifted(row(&wide, y), row(k, y), n, dx);
  }
  for (int y = 0; y < h; y++)
    for (int dy = -2; dy <= 2; dy++)
      if (y + dy >= 0 && y + dy < h) {
        unsigned *o = row(halo, y);
        const unsigned *s = row(&wide, y + dy);
        for (int i = 0; i < n; i++) o[i] |= s[i];
      }
  return 1;
}

// ---------------------------------------------------------------- the ticket
// "1st", "2nd", "3rd", "11th", "22nd"
static char *ordinal(unsigned n, char *p) {
  p = utoa(n, p);
  const unsigned lastTwo = n % 100;
  const char *suffix = "th";
  if (lastTwo < 11 || lastTwo > 13) {
    if (n % 10 == 1) suffix = "st";
    if (n % 10 == 2) suffix = "nd";
    if (n % 10 == 3) suffix = "rd";
  }
  return sput(p, suffix);
}
// "4h 32m", or "45m" under an hour.
static void fmtMinutes(unsigned minutes, char *p) {
  if (minutes >= 60) { p = utoa(minutes / 60, p); p = sput(p, "h "); }
  p = utoa(minutes % 60, p);
  sput(p, "m");
}

static int stubHeight(void) {
  return TEAR + 10 + (booksFinished ? SMALL.lineH + 8 : 0) + UI12B.lineH + SMALL.lineH + 14 + CORNER / 2;
}

// The stub: the read in numbers. The milestone across the top, then time and days.
static void composeStub(Canvas *c) {
  const int w = c->w, pad = 18;
  char line[200], shown[200], *p;
  if (booksFinished) {  // "Your 14th book · 6th in 日本語"
    p = sput(line, "Your ");
    p = ordinal(booksFinished, p);
    p = sput(p, " book");
    const char *ln = langName(langCode);
    if (langFinished && ln) { p = sput(p, " · "); p = ordinal(langFinished, p); p = sput(p, " in "); sput(p, ln); }
    truncated(SMALL, line, w - 2 * pad, shown, sizeof shown);
    drawText(c, pad, TEAR + 10, SMALL, shown);
  }
  char v0[16], v1[16];
  fmtMinutes(bookSecs / 60, v0);
  utoa(bookDays ? bookDays : 1, v1);  // a book finished the day it was started still took a day
  const char *values[2] = {v0, v1}, *labels[2] = {"read", "days"};
  const int valueY = TEAR + 10 + (booksFinished ? SMALL.lineH + 8 : 0), colW = (w - 2 * pad) / 2;
  for (int i = 0; i < 2; i++) {
    const int x = pad + colW * i;
    drawTextAligned(c, x, valueY, colW, UI12B, values[i], DSN_ALIGN_CENTER);
    truncated(SMALL, labels[i], colW - 6, shown, sizeof shown);
    drawTextAligned(c, x, valueY + UI12B.lineH, colW, SMALL, shown, DSN_ALIGN_CENTER);
  }
}

// The stamp, pre-turned by tools/make_stamps.py: stamps/<wording>.bin holds u32 width, height,
// then the turned ink and its 2 px halo, row by row in canvas words.
static int loadStamp(Canvas *ink, Canvas *halo) {
  char p[200], *q = p;
  for (int i = 0; i < dirLen; i++) *q++ = dir[i];
  if (q == p || q[-1] != '/') *q++ = '/';
  q = sput(q, "stamps/"); q = sput(q, stampFor(langTag)); q = sput(q, ".bin");
  const int n = dsn_fs_read(p, (int)(q - p), buf, sizeof buf);
  if (n < 8) return 0;
  const unsigned char *u = (const unsigned char *)buf;
  const int w = u[0] | u[1] << 8, h = u[4] | u[5] << 8;
  if (!canvasNew(ink, w, h) || !canvasNew(halo, w, h) || n < 8 + 8 * ink->stride * h) return 0;
  const int words = ink->stride * h;
  for (int i = 0; i < 2 * words; i++) {
    const unsigned char *b = u + 8 + 4 * i;
    const unsigned v = b[0] | b[1] << 8 | b[2] << 16 | (unsigned)b[3] << 24;
    if (i < words) ink->b[i] = v; else halo->b[i - words] = v;
  }
  return 1;
}

// The stamp's dates under 読了, inside the inner ring: the start day in the small face, joined by
// a short tick to the finish day in bold. A book read within one day shows the finish only. They
// are turned on their own and placed where they sit on the turned stamp.
static void drawStamp(int cx, int cy) {
  const int mark = poolUsed;
  Canvas ink, halo, dates, dk, dh;
  if (!loadStamp(&ink, &halo)) { poolUsed = mark; return; }
  char started[16] = "", finished[16] = "";
  const unsigned fin = bookFinish ? bookFinish : today;
  if (fin) fmtStampDate(fin, finished);
  if (bookFirst && bookFirst < fin) fmtStampDate(bookFirst, started);
  const int top = STAMP_SIZE * (started[0] ? 56 : 60) / 100;
  const int h = (started[0] ? SMALL.lineH + 7 : 0) + UI10B.lineH;
  int haveDates = 0;
  if (finished[0] && canvasNew(&dates, STAMP_SIZE, h)) {
    int y = 0;
    if (started[0]) {
      drawTextAligned(&dates, 0, 0, STAMP_SIZE, SMALL, started, DSN_ALIGN_CENTER);
      cvLine(&dates, STAMP_SIZE / 2, SMALL.lineH + 1, STAMP_SIZE / 2, SMALL.lineH + 6);
      y = SMALL.lineH + 7;
    }
    drawTextAligned(&dates, 0, y, STAMP_SIZE, UI10B, finished, DSN_ALIGN_CENTER);
    haveDates = turnCanvas(&dates, STAMP_TURN, &dk) && haloOf(&dk, &dh);
  }
  int dx = cx, dy = cy;
  turnPoint(STAMP_SIZE / 2, top + h / 2, STAMP_SIZE, STAMP_SIZE, STAMP_TURN, cx, cy, &dx, &dy);
  // Halos first, so neither the stamp's nor the dates' blanks the other's ink.
  blit(&halo, cx - halo.w / 2, cy - halo.h / 2, DSN_WHITE);
  if (haveDates) blit(&dh, dx - dh.w / 2, dy - dh.h / 2, DSN_WHITE);
  blit(&ink, cx - ink.w / 2, cy - ink.h / 2, DSN_BLACK);
  if (haveDates) blit(&dk, dx - dk.w / 2, dy - dk.h / 2, DSN_BLACK);
  poolUsed = mark;
}

static int buttonsY, buttonsX, buttonsW;
static void drawTicket(int bottom) {
  const int lineH = UI12B.lineH;
  const int stubH = stubHeight(), stubW = imin(380, W - 40);
  char l1[TITLE_MAX + 8], l2[TITLE_MAX + 8];
  // Wrapped to the narrowest the top half gets, so the lines fit whatever the cover's width.
  const int tl = wrap2(UI12B, title, MIN_TOP_W - 24, l1, l2, sizeof l1), titleH = lineH * tl;
  const int topChrome = 12 + 8 + titleH + 10 + TEAR;
  const int maxCoverH = bottom - 8 - topChrome - TEAR_GAP - stubH - 28;

  int coverW = 0, coverH = 0;
  int want = imin(maxCoverH, COVER_HEIGHT);
  for (int tries = 0; tries < 3 && want >= 40; tries++) {
    const int n = dsn_book_cover(bookPath, slen(bookPath), want, cover, sizeof cover);
    if (n == DSN_ERR_TOO_BIG) { want = want * 3 / 4; continue; }
    if (n <= 4) break;
    coverW = cover[0] | cover[1] << 8;
    coverH = cover[2] | cover[3] << 8;
    if (coverW > W - 90 && tries < 2) { want = want * (W - 90) / coverW; continue; }
    break;
  }
  const int hasCover = coverW > 0;
  if (!hasCover) { coverH = imin(maxCoverH, 200); coverW = coverH * 2 / 3; }
  if (coverW > W - 90) coverW = W - 90;

  const int topW = imax(coverW + 24, MIN_TOP_W);
  const int topH = 12 + coverH + 8 + titleH + 10 + TEAR;
  const int groupH = topH + TEAR_GAP + stubH;
  const int topY = imax(8, (bottom - groupH) / 2);
  const int topX = (W - topW) / 2 - 12;
  const int stubX = (W - stubW) / 2 + 14, stubY = topY + topH + TEAR_GAP;
  const int coverX = (topW - coverW) / 2, coverY = 12;
  const int topCx = topX + topW / 2, topCy = topY + topH / 2;

  // Top half: the cover and the title.
  Canvas top;
  const int mark = poolUsed;
  if (canvasNew(&top, topW, topH)) {
    outline(&top, SHAPE_TOP);
    if (hasCover) {  // the cover's byte rows, as words, shifted into place
      const int bw = cover[0] | cover[1] << 8, bs = (bw + 7) / 8, n = top.stride;
      unsigned tmp[32];
      for (int y = 0; y < coverH && n <= 32; y++) {
        wclear(tmp, n);
        const unsigned char *src = cover + 4 + y * bs;
        for (int i = 0; i < bs && (i >> 2) < n; i++) tmp[i >> 2] |= (unsigned)src[i] << (24 - 8 * (i & 3));
        if (coverW < n * 32) {
          const int wi = coverW >> 5;
          if (coverW & 31) tmp[wi] &= ~0u << (32 - (coverW & 31)); else if (wi < n) tmp[wi] = 0;
          for (int i = wi + 1; i < n; i++) tmp[i] = 0;
        }
        orShifted(row(&top, coverY + y), tmp, n, coverX);
      }
    }
    for (int x = 0; x < coverW; x++) cvSet(&top, coverX + x, coverY), cvSet(&top, coverX + x, coverY + coverH - 1);
    for (int y = 0; y < coverH; y++) cvSet(&top, coverX, coverY + y), cvSet(&top, coverX + coverW - 1, coverY + y);
    int ty = coverY + coverH + 8;
    drawTextAligned(&top, 0, ty, topW, UI12B, l1, DSN_ALIGN_CENTER);
    if (tl > 1) drawTextAligned(&top, 0, ty + lineH, topW, UI12B, l2, DSN_ALIGN_CENTER);
    drawTurned(&top, TOP_TURN, topCx, topCy);
  }
  poolUsed = mark;

  Canvas stub;
  if (canvasNew(&stub, stubW, stubH)) {
    outline(&stub, SHAPE_STUB);
    composeStub(&stub);
    drawTurned(&stub, STUB_TURN, stubX + stubW / 2, stubY + stubH / 2);
  }
  poolUsed = mark;

  // Pressed on the cover's lower right corner, wherever the tilt carried it, hanging off the
  // cover's right edge but never off the screen.
  int sx, sy;
  turnPoint(coverX + coverW - 40, coverY + coverH - STAMP_SIZE / 2 + 30, topW, topH, TOP_TURN, topCx, topCy, &sx, &sy);
  drawStamp(imin(sx + 92, W - STAMP_SIZE / 2 - 4), sy);
}

// ---------------------------------------------------------------- buttons
static int selected;  // 0 = Continue (when there is a next book), the last index = Home
static int itemCount(void) { return nextName[0] ? 2 : 1; }
static int isHome(int i) { return i == itemCount() - 1; }
static int homeX, homeW, contX, contW;

static void iconAt(int x, int y, const unsigned char *bmp, int len, int color) {
  dsn_bitmap(x, y, 32, 32, bmp, len, 32, 32, DSN_BITMAP_MASK, DSN_BITMAP_CENTER, color, 0);
}
// One line of matcha text, upright, drawn in `color`.
static void label(int x, int y, Style st, const char *s, int color) {
  Canvas c;
  const int mark = poolUsed, w = textWidth(st, s) + 2;
  if (canvasNew(&c, w, st.lineH + 8)) { drawText(&c, 0, 0, st, s); blit(&c, x, y, color); }
  poolUsed = mark;
}
static void drawButtons(void) {
  const int x = buttonsX, y = buttonsY, w = buttonsW, h = BUTTON_H, R = 10;
  dsn_fill(x, y, w, h, DSN_WHITE);
  const int homeSel = isHome(selected);
  homeX = x;
  homeW = nextName[0] ? h : w;  // a square beside Continue
  if (homeSel) dsn_fill_round(x, y, homeW, h, DSN_BLACK, R, DSN_CORNERS_ALL);
  else dsn_stroke(x, y, homeW, h, DSN_BLACK, 2, R, DSN_CORNERS_ALL);
  int iconX = x + (homeW - 32) / 2;
  if (!nextName[0]) {  // Home alone: the house and its label, centred together
    const Style st = {F_B12, F_J12, F_C12, 0, 29};
    const char *text = "Go to Home";
    const int lw = textWidth(st, text);
    iconX = x + (homeW - 32 - 12 - lw) / 2;
    label(iconX + 44, y + (h - st.lineH) / 2, st, text, homeSel ? DSN_WHITE : DSN_BLACK);
  }
  iconAt(iconX, y + (h - 32) / 2, HomeIcon, (int)sizeof HomeIcon, homeSel ? DSN_WHITE : DSN_BLACK);
  if (!nextName[0]) return;

  const int cx = x + homeW + 12, cw = w - homeW - 12, sel = !homeSel, ink = sel ? DSN_WHITE : DSN_BLACK;
  contX = cx; contW = cw;
  if (sel) dsn_fill_round(cx, y, cw, h, DSN_BLACK, R, DSN_CORNERS_ALL);
  else dsn_stroke(cx, y, cw, h, DSN_BLACK, 2, R, DSN_CORNERS_ALL);
  int textX = cx + 16;
  const int thumbH = h - 14;
  static int n = 0;  // fetched once: a selection change redraws only the buttons
  if (!n) n = dsn_book_cover(nextPath, slen(nextPath), thumbH, nextThumb, sizeof nextThumb);
  if (n > 4) {
    const int tw = nextThumb[0] | nextThumb[1] << 8, th = nextThumb[2] | nextThumb[3] << 8;
    dsn_fill(cx + 10, y + 7, tw, th, DSN_WHITE);  // on the black button the cover's ink would vanish
    dsn_bitmap(cx + 10, y + 7, tw, th, nextThumb + 4, n - 4, tw, th, DSN_BITMAP_BW, DSN_BITMAP_CENTER, DSN_BLACK, 0);
    textX = cx + 10 + tw + 14;
  }
  const Style big = UI12B;
  const int top = y + (h - SMALL.lineH - big.lineH) / 2;
  label(textX, top, SMALL, "Continue with:", ink);
  char name[PATH_MAX_LEN], shown[PATH_MAX_LEN];
  stripExt(nextName, name, sizeof name);
  truncated(big, name, cx + cw - 16 - 32 - 10 - textX, shown, sizeof shown);
  label(textX, top + SMALL.lineH, big, shown, ink);
  iconAt(cx + cw - 10 - 32, y + (h - 32) / 2, ChevronIcon, (int)sizeof ChevronIcon, ink);
}

static void drawAll(void) {
  dsn_clear();
  buttonsX = 16;
  buttonsY = H - BUTTON_H - 10;
  buttonsW = W - 32;
  drawTicket(buttonsY - 12);
  drawButtons();
  dsn_refresh();
}

// ---------------------------------------------------------------- input
// Back returns to the last page; Home exits to the home screen.
static void activate(int i) {
  if (isHome(i)) dsn_exit(0);
  dsn_open_book(nextPath, slen(nextPath));
  dsn_exit(0);
}
static void backToBook(void) {
  dsn_open_book(bookPath, slen(bookPath));
  dsn_exit(0);
}

// The newest reader.exit this app was launched for; 0 when there is none or it was not the end.
static int takeExitEvent(int *sawExit) {
  char p[160];
  int pl = 0;
  for (int i = 0; i < dirLen; i++) p[pl++] = dir[i];
  if (!pl || p[pl - 1] != '/') p[pl++] = '/';
  const char *name = "app-events.jsonl";
  for (int i = 0; name[i]; i++) p[pl++] = name[i];
  p[pl] = 0;
  int n = dsn_fs_read(p, pl, buf, sizeof buf - 1);
  if (n == DSN_ERR_TOO_BIG) { dsn_fs_delete(p, pl); return 0; }
  if (n <= 0) return 0;
  buf[n] = 0;
  int end = 0;
  for (char *line = buf; *line;) {
    char *e = line;
    while (*e && *e != '\n') e++;
    char save = *e; *e = 0;
    char v[PATH_MAX_LEN];
    if (jstr(line, "e", v, sizeof v) && seq(v, "reader.exit") && jstr(line, "book", v, sizeof v)) {
      scpy(bookPath, v, PATH_MAX_LEN);
      *sawExit = 1;
      end = jstr(line, "end_percent", v, sizeof v) && seq(v, "100");
    }
    *e = save;
    line = *e ? e + 1 : e;
  }
  dsn_fs_delete(p, pl);
  return end;
}

int main(void) {
  W = dsn_screen_width();
  H = dsn_screen_height();
  dirLen = dsn_fs_app_dir(dir, sizeof dir);
  if (dirLen < 0) dirLen = 0;
  dsn_device_info di;
  if ((int)dsn_device(&di, sizeof di) > 0) hasTouch = di.has_touch;
  static unsigned char maskBuf[90000];
  scratchMask = maskBuf;
  scratchCap = (int)sizeof maskBuf;

  int sawExit = 0;
  const int finishedNow = takeExitEvent(&sawExit);
  if (sawExit && !finishedNow) return 0;  // left a book before its end: nothing to show

  unsigned now = dsn_time();
  dsn_clock_t c;
  if (now > 1600000000u && dsn_clock(&c, sizeof c) == 0) {
    unsigned local = daysFromCivil(c.year, c.month, c.day) * 86400u + c.hour * 3600u + c.minute * 60u + c.second;
    tzOffset = (int)(local - now);
    tzOffset = (tzOffset + (tzOffset >= 0 ? 450 : -450)) / 900 * 900;
    today = local / 86400u;
  }

  if (!finishedNow) {  // opened by hand: the last finished book
    loadStats(1);
    if (!lastFinished[0]) {
      dsn_clear();
      const char *m = "Finish a book to get its ticket.";
      dsn_text_box(20, H / 2 - 20, W - 40, 40, m, slen(m), DSN_FONT_BODY, DSN_ALIGN_CENTER, 2, 0);
      dsn_refresh();
      for (;;) {
        const int t = DSN_EV_TYPE(dsn_wait_event());
        if (t == DSN_EV_BACK || t == DSN_EV_CONFIRM || t == DSN_EV_TAP) return 0;
      }
    }
    scpy(bookPath, lastFinished, PATH_MAX_LEN);
    booksFinished = 0; nDbLangs = 0;
  }
  lookupBook();
  loadStats(0);
  findNext();
  selected = itemCount() - 1;  // focus starts on Home; Continue is one press away
  drawAll();

  for (;;) {
    const unsigned e = dsn_wait_event();
    const int t = DSN_EV_TYPE(e), old = selected;
    switch (t) {
      case DSN_EV_CONFIRM: activate(selected); break;
      case DSN_EV_BACK: case DSN_EV_SWIPE_RIGHT: backToBook(); break;
      case DSN_EV_PREV: selected = (selected + itemCount() - 1) % itemCount(); break;
      case DSN_EV_NEXT: selected = (selected + 1) % itemCount(); break;
      case DSN_EV_TAP: {
        const int x = DSN_EV_X(e), y = DSN_EV_Y(e);
        if (y >= buttonsY && y < buttonsY + BUTTON_H) {
          if (x >= homeX && x < homeX + homeW) activate(itemCount() - 1);
          if (nextName[0] && x >= contX && x < contX + contW) activate(0);
        }
        break;
      }
      default: break;
    }
    if (selected != old) { drawButtons(); dsn_refresh(); }
  }
}
