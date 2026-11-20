//! The dessen calls this app uses (numbers and packing from dessen/sdk/dessen_calls.h).
#![allow(dead_code)]
use core::arch::asm;

#[inline(always)]
fn call(n: usize, a: [usize; 6]) -> isize {
    let r: isize;
    unsafe {
        asm!("ecall", inlateout("a0") a[0] => r, in("a1") a[1], in("a2") a[2], in("a3") a[3],
             in("a4") a[4], in("a5") a[5], in("a7") n, options(nostack));
    }
    r
}
fn xy(x: i32, y: i32) -> usize { ((x as u32 & 0xffff) | (y as u32 & 0xffff) << 16) as usize }

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

pub fn exit(code: i32) -> ! { call(0, [code as usize, 0, 0, 0, 0, 0]); loop {} }
pub fn wait_event() -> u32 { call(1, [0; 6]) as u32 }
pub fn refresh() { call(2, [0; 6]); }
pub fn screen() -> (i32, i32) { let v = call(3, [0; 6]) as u32; ((v >> 16) as i32, (v & 0xffff) as i32) }
pub struct Device { pub width: u16, pub height: u16, pub inset_top: u16, pub has_touch: bool }
pub fn device() -> Device {
    let mut b = [0u8; 32];
    call(5, [b.as_mut_ptr() as usize, b.len(), 0, 0, 0, 0]);
    // u16 width, height, panel w/h, inset t/r/b/l, min_touch, then u8 has_touch (dsn_device_info)
    let u = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    Device { width: u(0), height: u(2), inset_top: u(8), has_touch: b[18] != 0 }
}
pub fn clear() { call(16, [0; 6]); }
pub fn fill(x: i32, y: i32, w: i32, h: i32, color: u32) { call(17, [xy(x, y), xy(w, h), color as usize, 0, 0, 0]); }
pub fn fill_round(x: i32, y: i32, w: i32, h: i32, color: u32, r: u32) {
    call(18, [xy(x, y), xy(w, h), (color | r << 8 | CORNERS_ALL << 16) as usize, 0, 0, 0]);
}
pub fn stroke(x: i32, y: i32, w: i32, h: i32, color: u32, width: u32, r: u32) {
    call(19, [xy(x, y), xy(w, h), (color | width << 8 | r << 16 | CORNERS_ALL << 24) as usize, 0, 0, 0]);
}
pub fn line(x0: i32, y0: i32, x1: i32, y1: i32, width: u32, color: u32) {
    call(20, [xy(x0, y0), xy(x1, y1), (width | color << 8) as usize, 0, 0, 0]);
}
pub fn text_box(x: i32, y: i32, w: i32, h: i32, s: &str, font: u32, align: u32, flags: u32) -> i32 {
    call(22, [xy(x, y), xy(w, h), s.as_ptr() as usize, s.len(), (font | align << 8 | 1 << 16 | flags << 24) as usize, 0]) as i32
}
pub fn measure_text(s: &str, font: u32, flags: u32) -> i32 {
    (call(23, [s.as_ptr() as usize, s.len(), (font | flags << 8) as usize, 0, 0, 0]) as u32 >> 16) as i32
}
pub fn line_height(font: u32) -> i32 { call(24, [font as usize, 0, 0, 0, 0, 0]) as i32 }
pub fn bitmap(x: i32, y: i32, w: i32, h: i32, data: &[u8], format: u32, color: u32) -> i32 {
    // CENTER mode, no rotation; the bitmap is exactly w x h
    call(25, [xy(x, y), xy(w, h), data.as_ptr() as usize, data.len(), xy(w, h),
              (format | color << 16) as usize]) as i32
}
pub fn time() -> u32 { call(32, [0; 6]) as u32 }
/// Local wall clock: (year, month, day, hour, minute, second), or None when unset.
pub fn clock() -> Option<(u16, u8, u8, u8, u8, u8)> {
    let mut b = [0u8; 8];
    if call(35, [b.as_mut_ptr() as usize, 8, 0, 0, 0, 0]) != 0 { return None; }
    Some((u16::from_le_bytes([b[0], b[1]]), b[2], b[3], b[4], b[5], b[6]))
}
pub fn fs_list(path: &str, out: &mut [u8]) -> i32 {
    call(48, [path.as_ptr() as usize, path.len(), out.as_mut_ptr() as usize, out.len(), 0, 0]) as i32
}
pub fn fs_read(path: &str, out: &mut [u8]) -> i32 {
    call(49, [path.as_ptr() as usize, path.len(), out.as_mut_ptr() as usize, out.len(), 0, 0]) as i32
}
pub fn fs_write(path: &str, data: &[u8]) -> i32 {
    call(50, [path.as_ptr() as usize, path.len(), data.as_ptr() as usize, data.len(), 0, 0]) as i32
}
pub fn fs_delete(path: &str) -> i32 { call(51, [path.as_ptr() as usize, path.len(), 0, 0, 0, 0]) as i32 }
pub fn fs_app_dir(out: &mut [u8]) -> i32 { call(52, [out.as_mut_ptr() as usize, out.len(), 0, 0, 0, 0]) as i32 }
/// One HTTP request: `head` is "METHOD URL\nHeader: value\n...". Writes the status (big-endian
/// u16 + 2 bytes) then the body to `resp`; returns the body length or a DSN_ERR_*.
pub fn net_request(head: &str, body: &[u8], resp: &mut [u8]) -> i32 {
    call(56, [head.as_ptr() as usize, head.len(), body.as_ptr() as usize, body.len(),
              resp.as_mut_ptr() as usize, resp.len()]) as i32
}
pub fn battery() -> i32 { call(82, [0; 6]) as i32 }
pub fn wifi_connect() -> i32 { call(103, [0; 6]) as i32 }
