// Insights, ported from matcha-reader's Stats screen as a dessen app: language
// tabs, the streak card with this week's days, six tiles, a month calendar and
// a book list; each book opens its own page (sessions, time, average, days,
// calendar with start and finish marks).
//
// Data: the firmware's reader.session events queue in this plugin's
// events.jsonl (device.json subscribes with a request that always fails, so
// the queue is kept). Each launch folds them into stats.db and clears the
// queue. The firmware drops a queue past 4 KB (about a dozen sessions), so the
// app should be opened now and then. A book's language and title come from the
// reader's own cache, /.crosspoint/epub_<std::hash(path)>/book.bin.
#include "dessen.h"
#include "icons.h"

#define MAX_DAYS 500
#define MAX_BOOKS 40
#define MAX_LANGS 12
#define BOOK_DAYS 24  // most recent reading days kept per book, for its calendar
#define PATH_MAX_LEN 96
#define TITLE_MAX 48

// matcha StatsWidgets metrics
#define CARD_MARGIN 20
#define CARD_PAD 16
#define CARD_RADIUS 12
#define CARD_GAP 10
#define TILE_HEIGHT 80
#define CHEVRON_TOUCH 44

typedef struct { unsigned short day; unsigned char lang; unsigned secs; } Day;
typedef struct {
  unsigned secs, end_bp;
  unsigned short sessions, first, finish, ndays;  // first/finish: day numbers, 0 = none
  unsigned char lang, nd;                         // nd: entries in bdays
  unsigned short bdays[BOOK_DAYS];
  char title[TITLE_MAX];
  char path[PATH_MAX_LEN];
} Book;

static Day days[MAX_DAYS];
static int nDays;
static Book books[MAX_BOOKS];
static int nBooks;
static char langs[MAX_LANGS][8];
static int nLangs;
static int tzOffset;

static char dir[128];
static int dirLen;
static char evbuf[4700];
static char scratch[22000];  // stats.db load/save and book.bin reads, one at a time

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
static int mkpath(char *out, const char *name) {
  int n = 0;
  for (int i = 0; i < dirLen; i++) out[n++] = dir[i];
  if (n == 0 || out[n - 1] != '/') out[n++] = '/';
  for (int i = 0; name[i]; i++) out[n++] = name[i];
  out[n] = 0;
  return n;
}
static void text(int x, int y, int w, int font, int align, int flags, const char *s) {
  dsn_text_box(x, y, w, dsn_line_height(font), s, slen(s), font, align, 1, flags);
}
static int textW(const char *s, int font, int flags) { return (int)(dsn_measure_text(s, slen(s), font, flags) >> 16); }
static void icon(int x, int y, const unsigned char *bmp, int size, int len) {
  dsn_bitmap(x, y, size, size, bmp, len, size, size, DSN_BITMAP_MASK, DSN_BITMAP_CENTER, DSN_BLACK, 0);
}
#define ICON(x, y, name) icon(x, y, name, name##_SZ, (int)sizeof name)

// "1h 20m", "45m", "2h"
static char *fmtDur(unsigned secs, char *p) {
  unsigned m = secs / 60, h = m / 60;
  m %= 60;
  if (h) { p = utoa(h, p); *p++ = 'h'; if (m) *p++ = ' '; }
  if (m || !h) { p = utoa(m, p); *p++ = 'm'; }
  *p = 0;
  return p;
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
static int daysInMonth(int y, int m) {
  static const unsigned char dm[12] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
  if (m == 2 && ((y % 4 == 0 && y % 100 != 0) || y % 400 == 0)) return 29;
  return dm[m - 1];
}
static int isoDow(unsigned day) { return (int)((day + 3) % 7); }  // Mon=0; 1970-01-01 was a Thursday

static const char *const DAY_LABELS[7] = {"Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"};
static const char *const MONTHS[12] = {"January", "February", "March", "April", "May", "June",
                                       "July", "August", "September", "October", "November", "December"};
// "5 Nov 2026"
static char *fmtDate(unsigned day, char *p) {
  int y, m, d;
  civilFromDays(day, &y, &m, &d);
  p = utoa((unsigned)d, p); *p++ = ' ';
  for (int i = 0; i < 3; i++) *p++ = MONTHS[m - 1][i];
  *p++ = ' ';
  return utoa((unsigned)y, p);
}

// ---------------------------------------------------------------- languages
static int langIndex(const char *code) {
  for (int i = 0; i < nLangs; i++) if (seq(langs[i], code)) return i;
  if (nLangs == MAX_LANGS) return 0;
  scpy(langs[nLangs], code, 8);
  return nLangs++;
}
// "ja-JP" -> "ja", "EN" -> "en"
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
static const char *langName(const char *code, char *buf) {
  static const char *const names[][2] = {
      {"en", "English"}, {"de", "Deutsch"}, {"fr", "Français"}, {"es", "Español"}, {"it", "Italiano"},
      {"pt", "Português"}, {"nl", "Nederlands"}, {"hu", "Magyar"}, {"pl", "Polski"}, {"cs", "Čeština"},
      {"sv", "Svenska"}, {"da", "Dansk"}, {"fi", "Suomi"}, {"no", "Norsk"}, {"nb", "Norsk"},
      {"tr", "Türkçe"}, {"ro", "Română"}, {"ru", "Русский"}, {"uk", "Українська"}, {"el", "Ελληνικά"},
      {"ja", "日本語"}, {"zh", "中文"}, {"ko", "한국어"},
  };
  if (!code[0]) return "Unknown";
  for (unsigned i = 0; i < sizeof names / sizeof names[0]; i++) if (seq(names[i][0], code)) return names[i][1];
  int i = 0;
  for (; code[i]; i++) buf[i] = (char)(code[i] >= 'a' && code[i] <= 'z' ? code[i] - 32 : code[i]);
  buf[i] = 0;
  return buf;
}

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

// Copy UTF-8 without cutting a character in half.
static void utf8cpy(char *d, const char *s, unsigned n, int cap) {
  unsigned k = n < (unsigned)cap - 1 ? n : (unsigned)cap - 1;
  while (k > 0 && k < n && ((unsigned char)s[k] & 0xC0) == 0x80) k--;
  for (unsigned i = 0; i < k; i++) d[i] = s[i];
  d[k] = 0;
}

// book.bin: u8 version, u32 lut, u16 spine, u16 toc, then u32-length strings title, author, language.
// Fills the book's language and title; leaves lang 255 when the cache is too big to read whole.
static void lookupBook(Book *b) {
  char p[48], *q = sput(p, "/.crosspoint/epub_");
  q = utoa(murmur2(b->path, (unsigned)slen(b->path)), q);
  sput(q, "/book.bin");
  int n = dsn_fs_read(p, slen(p), scratch, sizeof scratch);
  if (n == DSN_ERR_TOO_BIG) return;
  b->lang = (unsigned char)langIndex("");
  if (n < 9) return;
  unsigned off = 9;
  for (int field = 0; field < 3; field++) {
    if (off + 4 > (unsigned)n) return;
    const unsigned char *u = (const unsigned char *)scratch + off;
    unsigned len = u[0] | u[1] << 8 | u[2] << 16 | (unsigned)u[3] << 24;
    off += 4;
    if (len > (unsigned)n - off) return;
    if (field == 0 && len) utf8cpy(b->title, scratch + off, len, TITLE_MAX);
    if (field == 2) { char code[8]; normLang(scratch + off, (int)len, code); b->lang = (unsigned char)langIndex(code); }
    off += len;
  }
}
static int bookLang(Book *b) {
  if (b->lang == 255) lookupBook(b);
  return b->lang == 255 ? langIndex("") : b->lang;
}
// File name without folder and extension, for books without a cached title.
static const char *bookTitle(const Book *b, char *buf) {
  if (b->title[0]) return b->title;
  const char *n = b->path;
  for (const char *c = b->path; *c; c++) if (*c == '/' && c[1]) n = c + 1;
  int len = slen(n), dot = len;
  for (int i = len - 1; i > 0; i--) if (n[i] == '.') { dot = i; break; }
  utf8cpy(buf, n, (unsigned)dot, TITLE_MAX);
  return buf;
}

// ---------------------------------------------------------------- store
static Day *dayAt(unsigned day, int lang) {
  for (int i = 0; i < nDays; i++) if (days[i].day == day && days[i].lang == lang) return &days[i];
  if (nDays == MAX_DAYS) {
    int o = 0;
    for (int i = 1; i < nDays; i++) if (days[i].day < days[o].day) o = i;
    days[o] = days[--nDays];
  }
  days[nDays].day = (unsigned short)day; days[nDays].lang = (unsigned char)lang; days[nDays].secs = 0;
  return &days[nDays++];
}
static void clearBook(Book *b) {
  unsigned char *p = (unsigned char *)b;
  for (unsigned i = 0; i < sizeof *b; i++) p[i] = 0;
  b->lang = 255;
}
static unsigned lastDay(const Book *b) {
  unsigned d = 0;
  for (int i = 0; i < b->nd; i++) if (b->bdays[i] > d) d = b->bdays[i];
  return d;
}
static Book *bookAt(const char *path) {
  for (int i = 0; i < nBooks; i++) if (seq(books[i].path, path)) return &books[i];
  if (nBooks == MAX_BOOKS) {  // drop the one read longest ago
    int o = 0;
    for (int i = 1; i < nBooks; i++) if (lastDay(&books[i]) < lastDay(&books[o])) o = i;
    books[o] = books[--nBooks];
  }
  Book *b = &books[nBooks++];
  clearBook(b);
  scpy(b->path, path, PATH_MAX_LEN);
  return b;
}
static int bookReadOn(const Book *b, unsigned d) {
  for (int i = 0; i < b->nd; i++) if (b->bdays[i] == d) return 1;
  return 0;
}
static void bookAddDay(Book *b, unsigned d) {
  if (bookReadOn(b, d)) return;
  b->ndays++;
  if (!b->first || d < b->first) b->first = (unsigned short)d;
  if (b->nd < BOOK_DAYS) { b->bdays[b->nd++] = (unsigned short)d; return; }
  int o = 0;  // full: replace the oldest
  for (int i = 1; i < b->nd; i++) if (b->bdays[i] < b->bdays[o]) o = i;
  if (d > b->bdays[o]) b->bdays[o] = (unsigned short)d;
}

// stats.db, one record per line:
//   L code                     languages, in index order
//   D day lang secs            reading per local day and language
//   B secs end_bp lang sessions first finish ndays d1,d2,...<TAB>title<TAB>path
// lang 255 = not looked up yet. Older "B secs end_bp lang path" lines still load.
static void readField(const char **s, char *out, int cap, char stop) {
  int i = 0;
  while (**s && **s != '\n' && **s != stop) {
    if (i < cap - 1) out[i++] = **s;
    (*s)++;
  }
  out[i] = 0;
}
static void loadDb(void) {
  char p[160]; int pl = mkpath(p, "stats.db");
  int n = dsn_fs_read(p, pl, scratch, sizeof scratch - 1);
  if (n <= 0) return;
  scratch[n] = 0;
  const char *s = scratch;
  while (*s) {
    char k = *s++;
    if (k == 'L' && nLangs < MAX_LANGS) {
      if (*s == ' ') s++;
      readField(&s, langs[nLangs++], 8, '\n');
    } else if (k == 'D' && nDays < MAX_DAYS) {
      days[nDays].day = (unsigned short)parseu(&s); days[nDays].lang = (unsigned char)parseu(&s);
      days[nDays].secs = parseu(&s); nDays++;
    } else if (k == 'B' && nBooks < MAX_BOOKS) {
      Book *b = &books[nBooks++];
      clearBook(b);
      b->secs = parseu(&s); b->end_bp = parseu(&s); b->lang = (unsigned char)parseu(&s);
      const char *e = s;
      int tabbed = 0;
      while (*e && *e != '\n') if (*e++ == '\t') { tabbed = 1; break; }
      if (tabbed) {
        b->sessions = (unsigned short)parseu(&s); b->first = (unsigned short)parseu(&s);
        b->finish = (unsigned short)parseu(&s); b->ndays = (unsigned short)parseu(&s);
        while (*s == ' ') s++;
        while (*s >= '0' && *s <= '9') {
          unsigned d = parseu(&s);
          if (b->nd < BOOK_DAYS) b->bdays[b->nd++] = (unsigned short)d;
          if (*s == ',') s++;
        }
        if (*s == '\t') s++;
        readField(&s, b->title, TITLE_MAX, '\t');
        if (*s == '\t') s++;
      } else {  // old line: no per-book days, count it as one session
        b->sessions = 1;
        if (*s == ' ') s++;
      }
      readField(&s, b->path, PATH_MAX_LEN, '\n');
    }
    while (*s && *s != '\n') s++;
    if (*s) s++;
  }
}
static int saveDb(void) {
  char *p = scratch, *end = scratch + sizeof scratch - 256;
  for (int i = 0; i < nLangs; i++) { *p++ = 'L'; *p++ = ' '; p = sput(p, langs[i]); *p++ = '\n'; }
  for (int i = 0; i < nDays && p < end; i++) {
    p = sput(p, "D "); p = utoa(days[i].day, p); *p++ = ' '; p = utoa(days[i].lang, p);
    *p++ = ' '; p = utoa(days[i].secs, p); *p++ = '\n';
  }
  for (int i = 0; i < nBooks && p < end; i++) {
    const Book *b = &books[i];
    p = sput(p, "B "); p = utoa(b->secs, p); *p++ = ' '; p = utoa(b->end_bp, p); *p++ = ' ';
    p = utoa(b->lang, p); *p++ = ' '; p = utoa(b->sessions, p); *p++ = ' '; p = utoa(b->first, p); *p++ = ' ';
    p = utoa(b->finish, p); *p++ = ' '; p = utoa(b->ndays, p); *p++ = ' ';
    for (int j = 0; j < b->nd; j++) { if (j) *p++ = ','; p = utoa(b->bdays[j], p); }
    *p++ = '\t'; p = sput(p, b->title); *p++ = '\t'; p = sput(p, b->path); *p++ = '\n';
  }
  char path[160]; int pl = mkpath(path, "stats.db");
  return dsn_fs_write(path, pl, scratch, (int)(p - scratch));
}

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
      if (*s == '\\' && s[1]) {
        s++;
        if (*s == 'u') {  // \uXXXX from ArduinoJson for control bytes only; keep as '?'
          out[n++] = '?';
          for (int j = 0; j < 4 && s[1]; j++) s++;
          s++;
          continue;
        }
      }
      out[n++] = *s++;
    }
    out[n] = 0;
    return 1;
  }
  return 0;
}

static void addSession(Book *b, unsigned start, unsigned secs, unsigned endbp) {
  const int lang = bookLang(b);
  unsigned t = (unsigned)((int)start + tzOffset), day = t / 86400;
  b->secs += secs;
  b->sessions++;
  while (secs) {
    unsigned left = 86400 - t % 86400, take = secs < left ? secs : left;
    day = t / 86400;
    dayAt(day, lang)->secs += take;
    bookAddDay(b, day);
    t += take; secs -= take;
  }
  if (endbp >= 10000 && !b->finish) b->finish = (unsigned short)day;
  if (endbp > b->end_bp) b->end_bp = endbp;
}

static void ingest(void) {
  char p[160]; int pl = mkpath(p, "events.jsonl");
  int n = dsn_fs_read(p, pl, evbuf, sizeof evbuf - 1);
  if (n <= 0) return;
  evbuf[n] = 0;
  char *line = evbuf;
  while (*line) {
    char *e = line;
    while (*e && *e != '\n') e++;
    char save = *e; *e = 0;
    char ev[24], v[PATH_MAX_LEN];
    if (jstr(line, "e", ev, sizeof ev) && seq(ev, "reader.session")) {
      const char *q;
      unsigned start = 0, dur = 0, endbp = 0;
      if (jstr(line, "start_time", v, sizeof v)) { q = v; start = parseu(&q); }
      if (jstr(line, "duration_seconds", v, sizeof v)) { q = v; dur = parseu(&q); }
      if (jstr(line, "end_progress_bp", v, sizeof v)) { q = v; endbp = parseu(&q); }
      if (start && dur && jstr(line, "book", v, sizeof v)) addSession(bookAt(v), start, dur, endbp);
    }
    *e = save;
    line = *e ? e + 1 : e;
  }
  if (saveDb() == 0) dsn_fs_delete(p, pl);
}

// ---------------------------------------------------------------- numbers per tab
static unsigned today;  // local day number, 0 when the clock is unset
static int tabLang[MAX_LANGS + 1];  // -1 = All
static int nTabs, tab;

static int readOn(unsigned d, int lang) {
  for (int i = 0; i < nDays; i++)
    if (days[i].day == d && days[i].secs && (lang < 0 || days[i].lang == lang)) return 1;
  return 0;
}
static unsigned totalSecs(int lang) {
  unsigned s = 0;
  for (int i = 0; i < nDays; i++) if (lang < 0 || days[i].lang == lang) s += days[i].secs;
  return s;
}
static int daysRead(int lang) {
  int c = 0;
  for (int i = 0; i < nDays; i++) {
    if (!days[i].secs || (lang >= 0 && days[i].lang != lang)) continue;
    int dup = 0;
    if (lang < 0) for (int j = 0; j < i; j++) if (days[j].day == days[i].day && days[j].secs) { dup = 1; break; }
    c += !dup;
  }
  return c;
}
static int streakNow(int lang) {
  if (!today) return 0;
  unsigned d = readOn(today, lang) ? today : today - 1;
  int c = 0;
  while (readOn(d, lang)) { c++; d--; }
  return c;
}
static int longestStreak(int lang) {
  int best = 0;
  for (int i = 0; i < nDays; i++) {
    if (!days[i].secs || (lang >= 0 && days[i].lang != lang) || readOn(days[i].day - 1, lang)) continue;
    int run = 0;
    while (readOn(days[i].day + run, lang)) run++;
    if (run > best) best = run;
  }
  return best;
}
static int inLang(const Book *b, int lang) { return lang < 0 || b->lang == lang || (b->lang == 255 && !langs[lang][0]); }
static int booksFinished(int lang) {
  int c = 0;
  for (int i = 0; i < nBooks; i++) if (books[i].end_bp >= 10000 && inLang(&books[i], lang)) c++;
  return c;
}
static unsigned sessionsIn(int lang) {
  unsigned c = 0;
  for (int i = 0; i < nBooks; i++) if (inLang(&books[i], lang)) c += books[i].sessions;
  return c;
}
static unsigned bookSecsIn(int lang) {
  unsigned s = 0;
  for (int i = 0; i < nBooks; i++) if (inLang(&books[i], lang)) s += books[i].secs;
  return s;
}
static unsigned weekMinutes(int lang) {
  if (!today) return 0;
  unsigned monday = today - (unsigned)isoDow(today), s = 0;
  for (int i = 0; i < nDays; i++)
    if (days[i].day >= monday && days[i].day <= today && (lang < 0 || days[i].lang == lang)) s += days[i].secs;
  return s / 60;
}

static void buildTabs(void) {
  nTabs = 0;
  tabLang[nTabs++] = -1;
  // languages with reading time, most read first; Unknown last
  int order[MAX_LANGS], n = 0;
  for (int i = 0; i < nLangs; i++) if (totalSecs(i)) order[n++] = i;
  for (int i = 1; i < n; i++) {
    int t = order[i], j = i;
    unsigned ts = totalSecs(t);
    while (j > 0 && (!langs[order[j - 1]][0] || (langs[t][0] && totalSecs(order[j - 1]) < ts))) { order[j] = order[j - 1]; j--; }
    order[j] = t;
  }
  if (n > 1 || (n == 1 && langs[order[0]][0]))
    for (int i = 0; i < n; i++) tabLang[nTabs++] = order[i];
}

static void sortBooks(void) {  // most recently read first
  for (int i = 1; i < nBooks; i++) {
    Book t = books[i];
    unsigned td = lastDay(&t);
    int j = i;
    while (j > 0 && lastDay(&books[j - 1]) < td) { books[j] = books[j - 1]; j--; }
    books[j] = t;
  }
}

// ---------------------------------------------------------------- drawing
static int W, H;
static int scroll, maxScroll, pageH;
static int calYear, calMonth;
static int navY = -1000, navPrevX, navNextX;  // chevron touch targets (screen coords)
static int tabY, tabH, tabX[MAX_LANGS + 2];
static int page;        // 0 = Insights, 1 = a book
static int openBook;    // index into books on page 1
static int bookSel = -1;  // focused row in the book list (buttons), -1 = none
#define MAX_ROWS MAX_BOOKS
static int rowBook[MAX_ROWS], rowY[MAX_ROWS], rowH, nRows;  // rows as drawn (content coords)

static int drawStreakCard(int x, int y, int w, int lang) {
  const int iconSize = FlameIcon_SZ, sLH = dsn_line_height(DSN_FONT_SMALL), tLH = dsn_line_height(DSN_FONT_BODY);
  const int circle = CircleCheckIcon_SZ;
  const int h = CARD_PAD + iconSize + 4 + sLH + 16 + sLH + 8 + circle + CARD_PAD;
  dsn_stroke(x, y, w, h, DSN_BLACK, 2, CARD_RADIUS, DSN_CORNERS_ALL);

  char buf[64], *p = utoa((unsigned)streakNow(lang), buf);
  sput(p, " day streak");
  int tw = textW(buf, DSN_FONT_BODY, DSN_TEXT_BOLD);
  int rx = x + (w - (iconSize + 8 + tw)) / 2, ry = y + CARD_PAD;
  ICON(rx, ry, FlameIcon);
  text(rx + iconSize + 8, ry + (iconSize - tLH) / 2, tw + 4, DSN_FONT_BODY, DSN_ALIGN_LEFT, DSN_TEXT_BOLD, buf);

  unsigned wm = weekMinutes(lang);
  p = utoa(wm, buf);
  sput(p, wm == 1 ? " minute read this week" : " minutes read this week");
  int r2 = ry + iconSize + 4;
  text(x, r2, w, DSN_FONT_SMALL, DSN_ALIGN_CENTER, 0, buf);

  int sep = r2 + sLH + 8;
  dsn_line(x + CARD_PAD, sep, x + w - CARD_PAD, sep, 1, DSN_BLACK);
  int spacing = (w - 2 * CARD_PAD) / 7, labelsY = sep + 12, circlesY = labelsY + sLH + 6;
  int todayDow = today ? isoDow(today) : -1;
  unsigned monday = today ? today - (unsigned)todayDow : 0;
  for (int i = 0; i < 7; i++) {
    int cx = x + CARD_PAD + spacing / 2 + i * spacing;
    text(cx - spacing / 2, labelsY, spacing, DSN_FONT_SMALL, DSN_ALIGN_CENTER, i == todayDow ? DSN_TEXT_BOLD : 0,
         DAY_LABELS[i]);
    int done = today && readOn(monday + (unsigned)i, lang);
    if (done) ICON(cx - circle / 2, circlesY, CircleCheckIcon);
    else ICON(cx - circle / 2, circlesY, CircleEmptyIcon);
  }
  return h;
}

enum { IC_BOOK, IC_CAL, IC_CLOCK, IC_FLAME, IC_CHART };
typedef struct { char value[16]; const char *label; int icon; } Tile;

static int drawTileGrid(int x, int y, int w, const Tile *tiles, int count) {
  const int halfW = (w - CARD_GAP) / 2, topPad = 16, iconSm = 24;
  const int vLH = dsn_line_height(DSN_FONT_BODY), sLH = dsn_line_height(DSN_FONT_SMALL);
  int th = TILE_HEIGHT;
  if (topPad + vLH + 2 + sLH + 8 > th) th = topPad + vLH + 2 + sLH + 8;  // bigger board fonts
  for (int i = 0; i < count; i++) {
    int cx = x + (i % 2) * (halfW + CARD_GAP), cy = y + (i / 2) * (th + CARD_GAP);
    dsn_stroke(cx, cy, halfW, th, DSN_BLACK, 2, CARD_RADIUS, DSN_CORNERS_ALL);
    const int ix = cx + halfW - iconSm - topPad, bx = cx + halfW - 32 - topPad + 4;  // 24px / 32px icon spots
    switch (tiles[i].icon) {
      case IC_BOOK: ICON(ix, cy + topPad, BookOpenIcon24); break;
      case IC_CAL: ICON(ix, cy + topPad, CalendarIcon24); break;
      case IC_CLOCK: ICON(ix, cy + topPad, ClockIcon24); break;
      case IC_FLAME: ICON(bx, cy + topPad - 2, FlameIcon); break;
      case IC_CHART: ICON(bx, cy + topPad - 2, ChartBarIcon); break;
    }
    int vy = cy + topPad + (iconSm - vLH) / 2;
    text(cx + 10, vy, halfW - 20 - iconSm, DSN_FONT_BODY, DSN_ALIGN_LEFT, DSN_TEXT_BOLD, tiles[i].value);
    text(cx + 10, vy + vLH + 2, halfW - 20, DSN_FONT_SMALL, DSN_ALIGN_LEFT, 0, tiles[i].label);
  }
  return (count + 1) / 2 * (th + CARD_GAP);
}

static void avgText(unsigned secs, unsigned sessions, char *out) {
  if (!sessions) { sput(out, "0m"); return; }
  fmtDur(secs / sessions, out);
}

static int drawTiles(int x, int y, int w, int lang) {
  Tile t[6];
  utoa((unsigned)booksFinished(lang), t[0].value); t[0].label = "Books finished"; t[0].icon = IC_BOOK;
  utoa((unsigned)daysRead(lang), t[1].value); t[1].label = "Days read"; t[1].icon = IC_CAL;
  unsigned m = totalSecs(lang) / 60;
  if (m >= 60) sput(utoa(m / 60, t[2].value), "h"); else sput(utoa(m, t[2].value), "m");
  t[2].label = "Total reading time"; t[2].icon = IC_CLOCK;
  utoa((unsigned)longestStreak(lang), t[3].value); t[3].label = "Longest streak"; t[3].icon = IC_FLAME;
  unsigned ses = sessionsIn(lang);
  utoa(ses, t[4].value); t[4].label = "Sessions"; t[4].icon = IC_BOOK;
  avgText(bookSecsIn(lang), ses, t[5].value); t[5].label = "Average session"; t[5].icon = IC_CHART;
  return drawTileGrid(x, y, w, t, 6);
}

static void chevron(int cx, int cy, int dir) {  // dir -1 = left
  const int s = 6;
  for (int k = 0; k < 2; k++) {
    int tip = cx + (dir < 0 ? -s / 2 : s / 2) + k, back = cx + (dir < 0 ? s / 2 : -s / 2) + k;
    dsn_line(back, cy - s, tip, cy, 1, DSN_BLACK);
    dsn_line(tip, cy, back, cy + s, 1, DSN_BLACK);
  }
}

// One month: filled = read that day. With a book, also its start (dotted ring) and finish (ring).
static int drawCalendar(int x, int y, int w, int lang, const Book *bk) {
  const int sLH = dsn_line_height(DSN_FONT_SMALL), tLH = dsn_line_height(DSN_FONT_BODY);
  const int dim = daysInMonth(calYear, calMonth);
  const unsigned first = daysFromCivil(calYear, calMonth, 1);
  const int firstDow = isoDow(first);
  const int rows = (firstDow + dim + 6) / 7;
  const int cell = (w - 2 * CARD_PAD) / 7;
  int rowHt = cell - 2;
  if (rowHt < sLH + 4) rowHt = sLH + 4;
  const int h = 12 + tLH + sLH + 12 + sLH + rows * rowHt + 10;
  dsn_stroke(x, y, w, h, DSN_BLACK, 2, CARD_RADIUS, DSN_CORNERS_ALL);

  char buf[40], *p = sput(buf, MONTHS[calMonth - 1]);
  *p++ = ' '; utoa((unsigned)calYear, p);
  const int my = y + 12;
  text(x + CHEVRON_TOUCH, my, w - 2 * CHEVRON_TOUCH, DSN_FONT_BODY, DSN_ALIGN_CENTER, DSN_TEXT_BOLD, buf);
  const int chevY = my + (tLH + sLH) / 2;
  chevron(x + CARD_PAD + 3, chevY, -1);
  chevron(x + w - CARD_PAD - 3, chevY, 1);
  navY = chevY - CHEVRON_TOUCH / 2;
  navPrevX = x;
  navNextX = x + w - CHEVRON_TOUCH;

  int read = 0;
  for (int d = 1; d <= dim; d++) {
    unsigned day = first + (unsigned)d - 1;
    read += bk ? bookReadOn(bk, day) : readOn(day, lang);
  }
  p = utoa((unsigned)read, buf);
  sput(p, read == 1 ? " day read" : " days read");
  text(x, my + tLH + 2, w, DSN_FONT_SMALL, DSN_ALIGN_CENTER, 0, buf);

  const int hy = my + tLH + sLH + 12;
  for (int i = 0; i < 7; i++)
    text(x + CARD_PAD + i * cell, hy, cell, DSN_FONT_SMALL, DSN_ALIGN_CENTER, 0, DAY_LABELS[i]);
  const int gy = hy + sLH;
  int r = (cell - 6) / 2;
  if (r > rowHt / 2 - 3) r = rowHt / 2 - 3;
  for (int d = 1; d <= dim; d++) {
    const int pos = firstDow + d - 1;
    const int cx = x + CARD_PAD + (pos % 7) * cell + cell / 2;
    const int cy = gy + (pos / 7) * rowHt + rowHt / 2;
    const unsigned day = first + (unsigned)d - 1;
    char db[4];
    utoa((unsigned)d, db);
    const int isToday = day == today;
    if (bk ? bookReadOn(bk, day) : readOn(day, lang)) {
      dsn_fill_round(cx - r, cy - r, 2 * r, 2 * r, DSN_BLACK, (unsigned char)r, DSN_CORNERS_ALL);
      dsn_text_box(cx - r, cy - sLH / 2, 2 * r, sLH, db, slen(db), DSN_FONT_SMALL, DSN_ALIGN_CENTER, 1,
                   DSN_TEXT_BOLD | DSN_TEXT_INVERTED);
    } else {
      if (isToday) dsn_stroke(cx - r, cy - r, 2 * r, 2 * r, DSN_BLACK, 1, (unsigned char)r, DSN_CORNERS_ALL);
      dsn_text_box(cx - r, cy - sLH / 2, 2 * r, sLH, db, slen(db), DSN_FONT_SMALL, DSN_ALIGN_CENTER, 1,
                   isToday ? DSN_TEXT_BOLD : 0);
    }
    if (!bk) continue;
    const int rr = r + 3;
    if (bk->finish && day == bk->finish) {
      dsn_stroke(cx - rr, cy - rr, 2 * rr, 2 * rr, DSN_BLACK, 2, (unsigned char)rr, DSN_CORNERS_ALL);
    } else if (bk->first && day == bk->first) {  // dotted, so the start reads lighter than the finish
      static const signed char ring[18][2] = {{10, 0}, {9, 3}, {8, 6}, {5, 9}, {2, 10}, {-2, 10}, {-5, 9}, {-8, 6},
                                              {-9, 3}, {-10, 0}, {-9, -3}, {-8, -6}, {-5, -9}, {-2, -10}, {2, -10},
                                              {5, -9}, {8, -6}, {9, -3}};
      for (int k = 0; k < 18; k++)
        dsn_fill(cx + ring[k][0] * rr / 10 - 1, cy + ring[k][1] * rr / 10 - 1, 2, 2, DSN_BLACK);
    }
  }
  return h;
}

// Book rows under the calendar: title, then time and progress.
static int drawBookList(int x, int y, int w, int lang) {
  const int bLH = dsn_line_height(DSN_FONT_BODY), sLH = dsn_line_height(DSN_FONT_SMALL);
  int y0 = y;
  text(x + 4, y, w, DSN_FONT_BODY, DSN_ALIGN_LEFT, DSN_TEXT_BOLD, "Books");
  y += bLH + 8;
  rowH = 12 + bLH + 2 + sLH + 12;
  nRows = 0;
  for (int i = 0; i < nBooks; i++) {
    const Book *b = &books[i];
    if (!inLang(b, lang)) continue;
    const int sel = nRows == bookSel;
    rowBook[nRows] = i;
    rowY[nRows] = y + scroll;
    nRows++;
    if (sel) dsn_stroke(x, y + 2, w, rowH - 4, DSN_BLACK, 2, CARD_RADIUS, DSN_CORNERS_ALL);
    char tb[TITLE_MAX];
    text(x + CARD_PAD, y + 12, w - 2 * CARD_PAD - 20, DSN_FONT_BODY, DSN_ALIGN_LEFT, 0, bookTitle(b, tb));
    char line[48], *p = fmtDur(b->secs, line);
    p = sput(p, "  ·  ");
    if (b->end_bp >= 10000) sput(p, "Finished");
    else { p = utoa(b->end_bp / 100, p); sput(p, "%"); }
    text(x + CARD_PAD, y + 12 + bLH + 2, w - 2 * CARD_PAD - 20, DSN_FONT_SMALL, DSN_ALIGN_LEFT, 0, line);
    chevron(x + w - CARD_PAD - 4, y + rowH / 2, 1);
    y += rowH;
    if (i + 1 < nBooks) dsn_line(x + CARD_PAD, y, x + w - CARD_PAD, y, 1, DSN_LIGHT_GRAY);
  }
  if (!nRows) {
    text(x + 4, y, w, DSN_FONT_SMALL, DSN_ALIGN_LEFT, 0, "No reading sessions yet");
    y += sLH;
  }
  return y - y0;
}

// The Lyra header (Cover Grid's other screens use it), redrawn from the theme's
// metrics since apps cannot call the theme: a 20px status strip with the clock
// top-left and the battery top-right, the title left-aligned on the row below,
// and a 3px rule along the band's bottom. On touch boards a back arrow leads.
#define HDR_TOP 10
#define HDR_HEIGHT 84
#define HDR_STRIP 20
#define HDR_SIDE 18
#define HDR_BACK 48
static int hasTouch, safeTop;
static int bandTop(void) { return HDR_TOP > safeTop ? HDR_TOP - safeTop : 0; }

static void drawBattery(int right, int y) {
  int b = dsn_battery();
  if (b < 0) return;
  int pct = b & 0xff;
  char buf[8];
  sput(utoa((unsigned)pct, buf), "%");
  const int sLH = dsn_line_height(DSN_FONT_SMALL);
  int tw = textW(buf, DSN_FONT_SMALL, 0);
  const int gw = 16, gh = 12;
  int gx = right - gw - 2, gy = y + (HDR_STRIP - gh) / 2;
  dsn_stroke(gx, gy, gw, gh, DSN_BLACK, 1, 0, 0);
  dsn_fill(gx + gw, gy + 3, 2, gh - 6, DSN_BLACK);
  int fill = (gw - 4) * pct / 100;
  if (fill) dsn_fill(gx + 2, gy + 2, fill, gh - 4, DSN_BLACK);
  text(gx - 4 - tw - 2, y + (HDR_STRIP - sLH) / 2, tw + 4, DSN_FONT_SMALL, DSN_ALIGN_RIGHT, 0, buf);
}

static void drawThemeHeader(const char *title) {
  const int top = bandTop(), sLH = dsn_line_height(DSN_FONT_SMALL), tLH = dsn_line_height(DSN_FONT_TITLE);
  const int inset = HDR_SIDE + 4;
  dsn_clock_t c;
  if (dsn_clock(&c, sizeof c) == 0) {
    char buf[8], *p = buf;
    if (c.hour < 10) *p++ = '0';
    p = utoa(c.hour, p); *p++ = ':';
    if (c.minute < 10) *p++ = '0';
    utoa(c.minute, p);
    text(inset, top + (HDR_STRIP - sLH) / 2, 80, DSN_FONT_SMALL, DSN_ALIGN_LEFT, 0, buf);
  }
  drawBattery(W - inset, top);
  const int rowTop = top + HDR_STRIP, rowHt = HDR_HEIGHT - HDR_STRIP;
  int tx = HDR_SIDE;
  if (hasTouch) {  // back arrow in the leading 48px slot
    int cx = HDR_SIDE + HDR_BACK / 2 - 4, cy = rowTop + rowHt / 2 - 2;
    dsn_line(cx - 10, cy, cx + 10, cy, 3, DSN_BLACK);
    dsn_line(cx - 10, cy, cx - 2, cy - 8, 3, DSN_BLACK);
    dsn_line(cx - 10, cy, cx - 2, cy + 8, 3, DSN_BLACK);
    tx += HDR_BACK + 4;
  }
  text(tx, rowTop + (rowHt - tLH) / 2 - 2, W - tx - HDR_SIDE, DSN_FONT_TITLE, DSN_ALIGN_LEFT, DSN_TEXT_BOLD, title);
  dsn_fill(0, top + HDR_HEIGHT - 3, W, 3, DSN_BLACK);
}
static int tappedBack(int x, int y) {
  const int top = bandTop() + HDR_STRIP;
  return hasTouch && y >= top && y < bandTop() + HDR_HEIGHT && x < HDR_SIDE + HDR_BACK + 8;
}

// Header and language pills, drawn over the scrolled content.
static void drawHeader(const char *title, int withTabs) {
  const int sLH = dsn_line_height(DSN_FONT_SMALL);
  drawThemeHeader(title);
  int y = bandTop() + HDR_HEIGHT + 10;
  tabH = 0;
  if (!withTabs || nTabs <= 1) return;
  tabY = y;
  tabH = sLH + 16;
  char nb[8];
  int firstTab = 0;
  for (;;) {  // keep the selected pill on screen
    int x = CARD_MARGIN, fits = 0;
    for (int i = firstTab; i < nTabs; i++) {
      const char *l = tabLang[i] < 0 ? "All" : langName(langs[tabLang[i]], nb);
      x += textW(l, DSN_FONT_SMALL, DSN_TEXT_BOLD) + 28 + 6;
      if (i == tab) { fits = x <= W - CARD_MARGIN + 6; break; }
    }
    if (fits || firstTab >= tab) break;
    firstTab++;
  }
  int x = CARD_MARGIN;
  for (int i = 0; i <= nTabs; i++) tabX[i] = -1;
  for (int i = firstTab; i < nTabs; i++) {
    const char *l = tabLang[i] < 0 ? "All" : langName(langs[tabLang[i]], nb);
    int pw = textW(l, DSN_FONT_SMALL, DSN_TEXT_BOLD) + 28;
    if (x + pw > W - CARD_MARGIN) break;
    if (i == tab) dsn_fill_round(x, y, pw, tabH, DSN_BLACK, (unsigned char)(tabH / 2), DSN_CORNERS_ALL);
    else dsn_stroke(x, y, pw, tabH, DSN_LIGHT_GRAY, 1, (unsigned char)(tabH / 2), DSN_CORNERS_ALL);
    dsn_text_box(x, y + (tabH - sLH) / 2, pw, sLH, l, slen(l), DSN_FONT_SMALL, DSN_ALIGN_CENTER, 1,
                 i == tab ? (DSN_TEXT_BOLD | DSN_TEXT_INVERTED) : 0);
    tabX[i] = x;
    tabX[i + 1] = x + pw;
    x += pw + 6;
  }
}

static int headerBottom;
static void layoutHeader(void) {
  headerBottom = bandTop() + HDR_HEIGHT + 10;
  if (page == 0 && nTabs > 1) headerBottom += dsn_line_height(DSN_FONT_SMALL) + 16 + 5;
}

static void drawInsights(int *y) {
  const int lang = tabLang[tab];
  const int cardX = CARD_MARGIN, cardW = W - 2 * CARD_MARGIN;
  *y += drawStreakCard(cardX, *y, cardW, lang) + 16;
  *y += drawTiles(cardX, *y, cardW, lang) + 8;
  *y += drawCalendar(cardX, *y, cardW, lang, 0) + 24;
  *y += drawBookList(cardX, *y, cardW, lang);
}

static void drawBookPage(int *y) {
  const Book *b = &books[openBook];
  const int cardX = CARD_MARGIN, cardW = W - 2 * CARD_MARGIN, sLH = dsn_line_height(DSN_FONT_SMALL);
  char line[64], *p = line;
  *p = 0;
  if (b->first) { p = sput(p, "Since "); p = fmtDate(b->first, p); }
  if (b->finish) { if (p != line) p = sput(p, "  ·  "); p = sput(p, "Finished "); p = fmtDate(b->finish, p); }
  else if (b->end_bp) { if (p != line) p = sput(p, "  ·  "); p = utoa(b->end_bp / 100, p); sput(p, "% read"); }
  if (line[0]) { text(cardX + 4, *y, cardW, DSN_FONT_SMALL, DSN_ALIGN_LEFT, 0, line); *y += sLH + 12; }
  Tile t[4];
  utoa(b->sessions, t[0].value); t[0].label = "Sessions"; t[0].icon = IC_BOOK;
  fmtDur(b->secs, t[1].value); t[1].label = "Total reading time"; t[1].icon = IC_CLOCK;
  avgText(b->secs, b->sessions, t[2].value); t[2].label = "Average session"; t[2].icon = IC_CHART;
  utoa(b->ndays, t[3].value); t[3].label = "Days read"; t[3].icon = IC_CAL;
  *y += drawTileGrid(cardX, *y, cardW, t, 4) + 8;
  *y += drawCalendar(cardX, *y, cardW, -1, b);
}

static void draw(void) {
  dsn_clear();
  layoutHeader();
  int y = headerBottom + 8 - scroll;
  if (page == 0) drawInsights(&y); else drawBookPage(&y);
  if (!today) {
    y += 12;
    text(CARD_MARGIN, y, W - 2 * CARD_MARGIN, DSN_FONT_SMALL, DSN_ALIGN_CENTER, 0, "Set the clock to count days");
    y += dsn_line_height(DSN_FONT_SMALL);
  }
  const int contentEnd = y + 10 + scroll;
  pageH = H - headerBottom;
  maxScroll = contentEnd - H;
  if (maxScroll < 0) maxScroll = 0;
  dsn_fill(0, 0, W, headerBottom, DSN_WHITE);
  char tb[TITLE_MAX];
  drawHeader(page == 0 ? "Insights" : bookTitle(&books[openBook], tb), page == 0);
  dsn_refresh();
}

static void stepMonth(int d) {
  calMonth += d;
  if (calMonth < 1) { calMonth = 12; calYear--; }
  if (calMonth > 12) { calMonth = 1; calYear++; }
}
static void setScroll(int s) {
  if (s < 0) s = 0;
  if (s > maxScroll) s = maxScroll;
  scroll = s;
}
static void showMonthOf(unsigned day) {
  int y, m, d;
  civilFromDays(day, &y, &m, &d);
  calYear = y; calMonth = m;
}
static int savedYear, savedMonth, savedScroll;
static void openBookPage(int row) {
  openBook = rowBook[row];
  savedYear = calYear; savedMonth = calMonth; savedScroll = scroll;
  const Book *b = &books[openBook];
  unsigned last = b->finish ? b->finish : lastDay(b);
  if (last) showMonthOf(last);
  page = 1; scroll = 0;
}
static void closeBookPage(void) {
  page = 0;
  calYear = savedYear; calMonth = savedMonth; scroll = savedScroll;
}
// Buttons: past the bottom of the page, Down walks the book rows.
static void focusRow(int r) {
  bookSel = r;
  if (r < 0) return;
  const int top = rowY[r] - scroll, bottom = top + rowH;
  if (bottom > H) setScroll(scroll + bottom - H + 10);
  if (top < headerBottom) setScroll(rowY[r] - headerBottom - 8);
}

int main(void) {
  W = dsn_screen_width();
  H = dsn_screen_height();
  dirLen = dsn_fs_app_dir(dir, sizeof dir);
  if (dirLen < 0) dirLen = 0;
  dsn_device_info di;
  if ((int)dsn_device(&di, sizeof di) > 0) { hasTouch = di.has_touch; safeTop = di.inset_top; }
  unsigned now = dsn_time();
  dsn_clock_t c;
  calYear = 2026; calMonth = 1;
  if (now > 1600000000u && dsn_clock(&c, sizeof c) == 0) {
    unsigned local = daysFromCivil(c.year, c.month, c.day) * 86400u + c.hour * 3600u + c.minute * 60u + c.second;
    tzOffset = (int)(local - now);
    tzOffset = (tzOffset + (tzOffset >= 0 ? 450 : -450)) / 900 * 900;
    today = local / 86400u;
    calYear = c.year; calMonth = c.month;
  }
  loadDb();
  if (!nLangs) langIndex("");  // index 0 is always Unknown
  // books whose cache was too big last time, or saved before titles were kept, get another look
  int looked = 0;
  for (int i = 0; i < nBooks; i++)
    if (books[i].lang == 255 || !books[i].title[0]) { unsigned char l = books[i].lang; lookupBook(&books[i]); if (l != 255) books[i].lang = l; looked = 1; }
  ingest();
  if (looked) saveDb();
  sortBooks();
  buildTabs();
  draw();
  for (;;) {
    unsigned e = dsn_wait_event();
    const int t = DSN_EV_TYPE(e);
    const int old = scroll, oldTab = tab, oldM = calMonth, oldPage = page, oldSel = bookSel;
    switch (t) {
      case DSN_EV_BACK:
        if (page == 1) closeBookPage(); else return 0;
        break;
      case DSN_EV_CONFIRM:
        if (page == 0 && bookSel >= 0 && bookSel < nRows) openBookPage(bookSel);
        else if (page == 0 && nTabs > 1) tab = (tab + 1) % nTabs;
        break;
      case DSN_EV_SWIPE_LEFT: if (page == 0 && nTabs > 1) tab = (tab + 1) % nTabs; break;
      case DSN_EV_SWIPE_RIGHT:
        if (page == 1) closeBookPage();
        else if (nTabs > 1) tab = (tab + nTabs - 1) % nTabs;
        break;
      case DSN_EV_NEXT:
        if (page == 0 && bookSel >= 0) { if (bookSel + 1 < nRows) focusRow(bookSel + 1); }
        else if (page == 0 && scroll >= maxScroll && nRows) focusRow(0);
        else setScroll(scroll + 40);
        break;
      case DSN_EV_PREV:
        if (page == 0 && bookSel > 0) focusRow(bookSel - 1);
        else if (page == 0 && bookSel == 0) bookSel = -1;
        else setScroll(scroll - 40);
        break;
      case DSN_EV_SWIPE_UP: setScroll(scroll + pageH); break;
      case DSN_EV_SWIPE_DOWN: setScroll(scroll - pageH); break;
      case DSN_EV_TAP: {
        int x = DSN_EV_X(e), y = DSN_EV_Y(e);
        if (tappedBack(x, y)) {
          if (page == 1) closeBookPage(); else return 0;
        } else if (tabH && y >= tabY && y < tabY + tabH) {
          for (int i = 0; i < nTabs; i++) if (tabX[i] >= 0 && x >= tabX[i] && x < tabX[i + 1]) tab = i;
        } else if (y >= headerBottom && y >= navY && y < navY + CHEVRON_TOUCH &&
                   (x < navPrevX + CHEVRON_TOUCH || x >= navNextX)) {
          stepMonth(x < navPrevX + CHEVRON_TOUCH ? -1 : 1);
        } else if (page == 0 && y >= headerBottom) {
          for (int r = 0; r < nRows; r++)
            if (y + scroll >= rowY[r] && y + scroll < rowY[r] + rowH) { bookSel = -1; openBookPage(r); break; }
        }
        break;
      }
      default: continue;
    }
    if (tab != oldTab) { scroll = 0; bookSel = -1; }
    if (scroll != old || tab != oldTab || calMonth != oldM || page != oldPage || bookSel != oldSel) draw();
  }
}
