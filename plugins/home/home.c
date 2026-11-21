// matcha's Home as a home-launcher plugin (components/CoverGridHomeUi.cpp): the book being read on
// top with its title, author and progress, the next six books in a 3x2 cover grid, and matcha's
// tab bar along the bottom with Home filled. Books come from the firmware's recent shelf
// (recently opened, then recently added); covers from its thumbnail cache.
// Tap a cover to open it; Up/Down and Confirm do the same with buttons.
#include "dessen.h"
#include "tabicons.h"

static int W, H, hasTouch, safeTop;

static int slen(const char *s) { int n = 0; while (s[n]) n++; return n; }
static void scpy(char *d, const char *s, int cap) { int i = 0; for (; s[i] && i < cap - 1; i++) d[i] = s[i]; d[i] = 0; }
static char *utoa(unsigned v, char *p) {
  char t[12]; int n = 0;
  do { t[n++] = (char)('0' + v % 10); v /= 10; } while (v);
  while (n) *p++ = t[--n];
  *p = 0;
  return p;
}
static void text(int x, int y, int w, int font, int align, int flags, const char *s, int lines) {
  dsn_text_box(x, y, w, dsn_line_height(font) * lines, s, slen(s), font, align, lines, flags);
}
static int textW(const char *s, int font, int flags) { return (int)(dsn_measure_text(s, slen(s), font, flags) >> 16); }

// ---------------------------------------------------------------- books
#define MAX_BOOKS 7  // the featured book plus a 3x2 grid
#define COLS 3
static char shelf[MAX_BOOKS * DSN_SHELF_RECORD_MAX + 1];
struct Book { const char *path, *title, *author; int percent; };
static struct Book books[MAX_BOOKS];
static int nBooks;

static void loadBooks(void) {
  int n = dsn_recent_shelf(0, MAX_BOOKS, shelf, sizeof shelf - 1);
  if (n < 0) n = dsn_recents(shelf, sizeof shelf - 1);
  nBooks = 0;
  if (n <= 0 || n >= (int)sizeof shelf) return;
  shelf[n] = 0;
  char *p = shelf;
  while (*p && nBooks < MAX_BOOKS) {
    char *f[4] = {p, 0, 0, 0};
    int k = 1;
    char *e = p;
    while (*e && *e != '\n') {
      if (*e == '\t' && k < 4) { *e = 0; f[k++] = e + 1; }
      e++;
    }
    const int more = *e == '\n';
    *e = 0;
    if (f[1]) {  // a blank line is an unreadable entry
      struct Book *b = &books[nBooks++];
      b->path = f[0];
      b->title = f[1][0] ? f[1] : f[0];
      b->author = f[2] ? f[2] : "";
      const int pct = dsn_book_progress(f[0], slen(f[0]));
      b->percent = pct >= 0 && pct <= 100 ? pct : -1;  // -1: never opened ("New")
    }
    p = more ? e + 1 : e;
  }
}

// Covers: {u16 w, u16 h} then MSB-first rows, set bits black. Loaded once, at one size for all.
#define COVER_MAX (4 + 26 * 300)
static unsigned char covers[MAX_BOOKS][COVER_MAX];
static int coverLen[MAX_BOOKS], coversLoaded;

// ---------------------------------------------------------------- layout
// The Cover Grid theme's metrics, as Insights redraws them (apps cannot call the theme).
#define HDR_TOP 10
#define STRIP 20
#define SIDE 18   // headerSidePadding
#define INSET 6   // COVER_CELL_INSET
#define TAB_H 72  // coverGridTabBarHeight
static int coverW, coverH, rowGap, heroY, gridY, colX[COLS];
static int bandTop(void) { return HDR_TOP > safeTop ? HDR_TOP - safeTop : 0; }

static void layout(void) {
  const int top = bandTop() + STRIP + (hasTouch ? 16 : 8);
  const int bottom = H - TAB_H - 8;
  rowGap = W / 100 > 4 ? W / 100 : 4;
  // Three equal cover rows (hero + two grid rows), every cover one size.
  const int rowH = (bottom - top - 2 * rowGap) / 3;
  const int maxW = (W - 2 * (SIDE - INSET)) / COLS - 2 * INSET;
  coverH = rowH - 12;
  if (coverH > maxW * 3 / 2) coverH = maxW * 3 / 2;
  if (coverH > 290) coverH = 290;
  coverW = coverH * 2 / 3;
  heroY = top + INSET;
  gridY = top + rowH + rowGap + INSET;
  // SpaceBetween: the outer covers sit on the side-padding line.
  const int left = SIDE, right = W - SIDE - coverW;
  for (int c = 0; c < COLS; c++) colX[c] = left + (right - left) * c / (COLS - 1);
}
static void coverRect(int i, int *x, int *y) {
  if (i == 0) { *x = colX[0]; *y = heroY; return; }
  const int g = i - 1;
  *x = colX[g % COLS];
  *y = gridY + (g / COLS) * (coverH + 12 + rowGap);
}

// ---------------------------------------------------------------- tab bar
// matcha's bar (components/HomeTabBar.cpp), Home filled. Library, Transfer and Settings open the
// firmware's screens; Insights and Flashcards open their plugins, which come back here on exit.
enum { T_HOME, T_LIBRARY, T_TRANSFER, T_INSIGHTS, T_FLASHCARDS, T_SETTINGS };
static int barTabs[6], nBar;
static char insightsApp[40], flashcardsApp[40];
static int appExists(const char *p) {
  char one[1];
  const int n = dsn_fs_read(p, slen(p), one, 1);
  return n == DSN_ERR_TOO_BIG || n == 1;
}
static void setupBar(void) {
  nBar = 0;
  barTabs[nBar++] = T_HOME; barTabs[nBar++] = T_LIBRARY; barTabs[nBar++] = T_TRANSFER;
  if (appExists("/plugins/insights/app.bin")) scpy(insightsApp, "/plugins/insights/app.bin", 40);
  else if (appExists("/plugins/stats/app.bin")) scpy(insightsApp, "/plugins/stats/app.bin", 40);
  if (insightsApp[0]) barTabs[nBar++] = T_INSIGHTS;
  if (appExists("/plugins/flashcards/app.bin")) { scpy(flashcardsApp, "/plugins/flashcards/app.bin", 40); barTabs[nBar++] = T_FLASHCARDS; }
  barTabs[nBar++] = T_SETTINGS;
}
static int barStep(void) { return (W - 2 * SIDE) / nBar; }
static void drawBar(int focus) {
  static const unsigned char *const OUTLINE[6] = {TabHomeIcon, TabLibraryTabIcon, TabTransferTabIcon, TabStatsIcon,
                                                  TabFlashcardsTabIcon, TabSettingsTabIcon};
  const int top = H - TAB_H, step = barStep();
  dsn_fill(0, top, W, TAB_H, DSN_WHITE);
  for (int i = 0; i < nBar; i++) {
    const int x = SIDE + i * step, t = barTabs[i];
    if (i == focus) dsn_stroke(x, top, step, TAB_H, DSN_BLACK, 4, 8, DSN_CORNERS_ALL);
    dsn_bitmap(x + (step - 32) / 2, top + (TAB_H - 32) / 2, 32, 32, t == T_HOME ? TabHomeFilledIcon : OUTLINE[t], 128,
               32, 32, DSN_BITMAP_MASK, DSN_BITMAP_CENTER, DSN_BLACK, 0);
  }
}
static void activateTab(int i) {
  if (i < 0 || i >= nBar) return;
  switch (barTabs[i]) {
    case T_LIBRARY: dsn_open_home(DSN_HOME_LIBRARY); break;
    case T_TRANSFER: dsn_open_home(DSN_HOME_TRANSFER); break;
    case T_SETTINGS: dsn_open_home(DSN_HOME_SETTINGS); break;
    case T_INSIGHTS: dsn_open_app(insightsApp, slen(insightsApp)); break;
    case T_FLASHCARDS: dsn_open_app(flashcardsApp, slen(flashcardsApp)); break;
    default: break;  // Home: already here
  }
}

// ---------------------------------------------------------------- drawing
static void drawBattery(void) {
  const int b = dsn_battery();
  if (b < 0) return;
  const int pct = b & 0xff, top = bandTop(), sLH = dsn_line_height(DSN_FONT_SMALL);
  char buf[8];
  char *p = utoa((unsigned)pct, buf);
  *p++ = '%'; *p = 0;
  const int gw = 16, gh = 12, right = W - SIDE - 4;
  const int gx = right - gw - 2, gy = top + (STRIP - gh) / 2;
  dsn_stroke(gx, gy, gw, gh, DSN_BLACK, 1, 0, 0);
  dsn_fill(gx + gw, gy + 3, 2, gh - 6, DSN_BLACK);
  const int fill = (gw - 4) * pct / 100;
  if (fill) dsn_fill(gx + 2, gy + 2, fill, gh - 4, DSN_BLACK);
  const int tw = textW(buf, DSN_FONT_SMALL, 0);
  const int ty = top + (STRIP - sLH) / 2;
  text(gx - 6 - tw, ty < 0 ? 0 : ty, tw + 4, DSN_FONT_SMALL, DSN_ALIGN_RIGHT, 0, buf, 1);
}

// A cover with matcha's outline and drop shadow; the title on white when there is no image.
static void drawCover(int i, int x, int y) {
  dsn_fill(x + 3, y + 3, coverW, coverH, DSN_BLACK);  // shadow
  dsn_fill(x, y, coverW, coverH, DSN_WHITE);
  if (coverLen[i] > 4) {
    const int cw = covers[i][0] | covers[i][1] << 8, ch = covers[i][2] | covers[i][3] << 8;
    dsn_bitmap(x, y, coverW, coverH, covers[i] + 4, coverLen[i] - 4, cw, ch, DSN_BITMAP_BW, DSN_BITMAP_STRETCH,
               DSN_BLACK, 0);
  } else {
    const int lh = dsn_line_height(DSN_FONT_BODY);
    text(x + 10, y + coverH / 2 - lh * 3 / 2, coverW - 20, DSN_FONT_BODY, DSN_ALIGN_CENTER, 0, books[i].title, 3);
  }
  dsn_stroke(x, y, coverW, coverH, DSN_BLACK, 1, 0, 0);
  if (i > 0 && books[i].percent < 0) {  // matcha's "New" tab in the top-right corner
    const int sLH = dsn_line_height(DSN_FONT_SMALL);
    const int bw = textW("New", DSN_FONT_SMALL, DSN_TEXT_BOLD) + 16, bh = sLH + 6;
    dsn_fill(x + coverW - bw, y, bw, bh, DSN_BLACK);
    text(x + coverW - bw, y + 3, bw, DSN_FONT_SMALL, DSN_ALIGN_CENTER, DSN_TEXT_BOLD | DSN_TEXT_INVERTED, "New", 1);
  }
}

static void drawHeroText(void) {
  const struct Book *b = &books[0];
  const int x = colX[0] + coverW + 24, w = W - SIDE - x;
  const int bLH = dsn_line_height(DSN_FONT_BODY), sLH = dsn_line_height(DSN_FONT_SMALL);
  const int blockH = 2 * bLH + 4 + bLH;
  int y = heroY + (coverH - blockH) / 2 - 10;
  // A text box centres its lines vertically, so size it to the lines the title takes.
  const int titleLines = textW(b->title, DSN_FONT_BODY, DSN_TEXT_BOLD) > w ? 2 : 1;
  text(x, y, w, DSN_FONT_BODY, DSN_ALIGN_LEFT, DSN_TEXT_BOLD, b->title, titleLines);
  y += titleLines * bLH + 4;
  if (b->author[0]) text(x, y, w, DSN_FONT_BODY, DSN_ALIGN_LEFT, DSN_TEXT_BOLD, b->author, 1);
  // Progress along the cover's foot: "NN%" then a thin bar, read part solid, rest dotted.
  const int pct = b->percent < 0 ? 0 : b->percent;
  char buf[8];
  char *p = utoa((unsigned)pct, buf);
  *p++ = '%'; *p = 0;
  const int py = heroY + coverH - bLH;
  text(x, py, 80, DSN_FONT_BODY, DSN_ALIGN_LEFT, DSN_TEXT_BOLD, buf, 1);
  const int bx = x + textW(buf, DSN_FONT_BODY, DSN_TEXT_BOLD) + 12, bw = W - SIDE - bx, by = py + bLH / 2 - 3;
  if (bw > 20) {
    dsn_fill(bx, by, bw, 6, DSN_LIGHT_GRAY);
    if (pct) dsn_fill(bx, by, bw * pct / 100, 6, DSN_BLACK);
  }
  (void)sLH;
}

static void drawEmpty(void) {
  const int tLH = dsn_line_height(DSN_FONT_TITLE), bLH = dsn_line_height(DSN_FONT_BODY);
  const int y = (H - TAB_H) / 2 - tLH;
  text(SIDE, y, W - 2 * SIDE, DSN_FONT_TITLE, DSN_ALIGN_CENTER, DSN_TEXT_BOLD, "No open book", 1);
  text(SIDE, y + tLH + 8, W - 2 * SIDE, DSN_FONT_BODY, DSN_ALIGN_CENTER, 0, "Start reading below", 1);
  (void)bLH;
}

static int sel = -1;  // a button board's cursor: 0..nBooks-1 covers, nBooks.. the tab bar
static void draw(void) {
  dsn_clear();
  drawBattery();
  if (!nBooks) drawEmpty();
  for (int i = 0; i < nBooks; i++) {
    int x, y;
    coverRect(i, &x, &y);
    drawCover(i, x, y);
    if (i == sel) {
      if (i == 0) dsn_stroke(x - 8, y - 8, W - SIDE - x + 16, coverH + 16, DSN_DARK_GRAY, 4, 8, DSN_CORNERS_ALL);
      else dsn_stroke(x - 6, y - 6, coverW + 12, coverH + 12, DSN_DARK_GRAY, 4, 8, DSN_CORNERS_ALL);
    }
  }
  if (nBooks) drawHeroText();
  drawBar(sel >= nBooks ? sel - nBooks : -1);
  dsn_refresh();
}

static void loadCovers(void) {
  for (int i = 0; i < nBooks; i++) {
    const int n = dsn_book_cover(books[i].path, slen(books[i].path), coverH, covers[i], COVER_MAX);
    coverLen[i] = n > 4 ? n : 0;
  }
  coversLoaded = 1;
}

static int hitCover(int x, int y) {
  for (int i = 0; i < nBooks; i++) {
    int cx, cy;
    coverRect(i, &cx, &cy);
    const int right = i == 0 ? W - SIDE : cx + coverW;  // the featured card is one target
    if (x >= cx - INSET && x < right + INSET && y >= cy - INSET && y < cy + coverH + INSET) return i;
  }
  return -1;
}

static void openBook(int i) { dsn_open_book(books[i].path, slen(books[i].path)); }

int main(void) {
  W = dsn_screen_width();
  H = dsn_screen_height();
  dsn_device_info di;
  if ((int)dsn_device(&di, sizeof di) > 0) { hasTouch = di.has_touch; safeTop = di.inset_top; }
  setupBar();
  layout();
  loadBooks();
  loadCovers();
  draw();
  for (;;) {
    const int e = dsn_wait_event();
    const int old = sel, total = nBooks + nBar;
    switch (DSN_EV_TYPE(e)) {
      case DSN_EV_TAP: {
        const int x = DSN_EV_X(e), y = DSN_EV_Y(e);
        if (y >= H - TAB_H) {
          if (x >= SIDE) activateTab((x - SIDE) / barStep());
        } else {
          const int i = hitCover(x, y);
          if (i >= 0) openBook(i);
        }
        continue;
      }
      case DSN_EV_NEXT: sel = sel < 0 ? 0 : (sel + 1) % total; break;
      case DSN_EV_PREV: sel = sel <= 0 ? total - 1 : sel - 1; break;
      case DSN_EV_CONFIRM:
        if (sel < 0 && nBooks) openBook(0);
        else if (sel >= 0 && sel < nBooks) openBook(sel);
        else if (sel >= nBooks) activateTab(sel - nBooks);
        continue;
      default: continue;
    }
    if (sel != old) draw();
  }
}
