//! The dessen calls this app uses: Dessen's own generated wrappers, plus the constants from
//! dessen_abi.h and a few conveniences in the app's own types.
#![allow(dead_code)]

// The calls themselves: Dessen's generated Rust wrappers (sdk/dessen.rs, from abi/freeink.api).
include!("../../../dessen/sdk/dessen.rs");

pub const WHITE: u32 = 0;
pub const BLACK: u32 = 1;
pub const LIGHT_GRAY: u32 = 2;
pub const DARK_GRAY: u32 = 3;
pub const FONT_SMALL: u32 = 0;
pub const FONT_BODY: u32 = 1;
pub const FONT_TITLE: u32 = 2;
pub const ALIGN_LEFT: u32 = 0;
pub const ALIGN_RIGHT: u32 = 2;
pub const TEXT_BOLD: u32 = 1;
pub const CORNERS_ALL: u32 = 15;
pub const BITMAP_MASK: u32 = 0;
pub const BITMAP_BW: u32 = 1;

pub const EV_CONFIRM: u32 = 1;
pub const EV_BACK: u32 = 2;
pub const EV_PREV: u32 = 3;
pub const EV_NEXT: u32 = 4;
pub const EV_TAP: u32 = 5;
pub const EV_SWIPE_UP: u32 = 7;
pub const EV_SWIPE_DOWN: u32 = 8;
pub const EV_SWIPE_LEFT: u32 = 9;
pub const EV_SWIPE_RIGHT: u32 = 10;
pub fn ev_type(e: u32) -> u32 { e & 0xff }
pub fn ev_x(e: u32) -> i32 { ((e >> 8) & 0xfff) as i32 }
pub fn ev_y(e: u32) -> i32 { ((e >> 20) & 0xfff) as i32 }

pub const ERR_TOO_BIG: i32 = -3;

pub fn exit(code: i32) -> ! { dsn_exit(code); loop {} }
pub fn wait_event() -> u32 { dsn_wait_event() }
pub fn refresh() { dsn_refresh() }
pub fn screen() -> (i32, i32) { let v = dsn_screen(); ((v >> 16) as i32, (v & 0xffff) as i32) }
pub struct Device { pub width: u16, pub height: u16, pub inset_top: u16, pub has_touch: bool }
pub fn device() -> Device {
    let mut b = [0u8; 32];
    dsn_device(&mut b);
    // u16 width, height, panel w/h, inset t/r/b/l, min_touch, then u8 has_touch (dsn_device_info)
    let u = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    Device { width: u(0), height: u(2), inset_top: u(8), has_touch: b[18] != 0 }
}
pub fn clear() { dsn_clear() }
pub fn fill(x: i32, y: i32, w: i32, h: i32, color: u32) { dsn_fill(x, y, w, h, color as i32) }
pub fn fill_round(x: i32, y: i32, w: i32, h: i32, color: u32, r: u32) {
    dsn_fill_round(x, y, w, h, color as i32, r as i32, CORNERS_ALL as i32)
}
pub fn stroke(x: i32, y: i32, w: i32, h: i32, color: u32, width: u32, r: u32) {
    dsn_stroke(x, y, w, h, color as i32, width as i32, r as i32, CORNERS_ALL as i32)
}
pub fn line(x0: i32, y0: i32, x1: i32, y1: i32, width: u32, color: u32) {
    dsn_line(x0, y0, x1, y1, width as i32, color as i32)
}
pub fn text_box(x: i32, y: i32, w: i32, h: i32, s: &str, font: u32, align: u32, flags: u32) -> i32 {
    dsn_text_box(x, y, w, h, s.as_bytes(), font as i32, align as i32, 1, flags as i32)
}
pub fn measure_text(s: &str, font: u32, flags: u32) -> i32 {
    (dsn_measure_text(s.as_bytes(), font as i32, flags as i32) >> 16) as i32
}
pub fn line_height(font: u32) -> i32 { dsn_line_height(font as i32) }
pub fn bitmap(x: i32, y: i32, w: i32, h: i32, data: &[u8], format: u32, color: u32) -> i32 {
    // CENTER mode, no rotation; the bitmap is exactly w x h
    dsn_bitmap(x, y, w, h, data, w, h, format as i32, 0, color as i32, 0)
}
pub fn time() -> u32 { dsn_time() }
/// Local wall clock: (year, month, day, hour, minute, second), or None when unset.
pub fn clock() -> Option<(u16, u8, u8, u8, u8, u8)> {
    let mut b = [0u8; 8];
    if dsn_clock(&mut b) != 0 { return None; }
    Some((u16::from_le_bytes([b[0], b[1]]), b[2], b[3], b[4], b[5], b[6]))
}
pub fn fs_list(path: &str, out: &mut [u8]) -> i32 {
    dsn_fs_list(path.as_bytes(), out)
}
pub fn fs_read(path: &str, out: &mut [u8]) -> i32 {
    dsn_fs_read(path.as_bytes(), out)
}
pub fn fs_write(path: &str, data: &[u8]) -> i32 {
    dsn_fs_write(path.as_bytes(), data)
}
pub fn fs_delete(path: &str) -> i32 { dsn_fs_delete(path.as_bytes()) }
pub fn fs_app_dir(out: &mut [u8]) -> i32 { dsn_fs_app_dir(out) }
/// One HTTP request: `head` is "METHOD URL\nHeader: value\n...". Writes the status (big-endian
/// u16 + 2 bytes) then the body to `resp`; returns the body length or a DSN_ERR_*.
pub fn net_request(head: &str, body: &[u8], resp: &mut [u8]) -> i32 {
    dsn_net_request(head.as_bytes(), body, resp)
}
pub fn battery() -> i32 { dsn_battery() }
pub fn wifi_connect() -> i32 { dsn_wifi_connect() }
pub const HOME_LIBRARY: i32 = 1;
pub const HOME_TRANSFER: i32 = 4;
pub const HOME_SETTINGS: i32 = 5;
/// Opens a firmware screen and ends the app (returns only if it could not).
pub fn open_home(dest: i32) { dsn_open_home(dest); }
/// Opens another dessen app in this one's place (returns only if it could not).
pub fn open_app(path: &str) { dsn_open_app(path.as_bytes()); }
