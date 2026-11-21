#![no_std]
extern crate alloc;

mod cardfaces;
mod cardhtml;
mod dsn;
mod fsrs;
mod gfx;
mod handover;
mod html;
mod hyph;
mod i18n;
mod icons;
mod mined;
mod png;
mod rpc;
mod rtl;
mod spine;
mod store;
mod sync;
mod tabbar;
mod tabicons;
mod ui;

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use linked_list_allocator::Heap;

// The app runs on one thread and RV32IM has no atomics for a lock, so the heap is used directly.
struct Alloc(UnsafeCell<Heap>);
unsafe impl Sync for Alloc {}
unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        (*self.0.get()).allocate_first_fit(l).map(|p| p.as_ptr()).unwrap_or(core::ptr::null_mut())
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if let Some(p) = core::ptr::NonNull::new(p) {
            (*self.0.get()).deallocate(p, l)
        }
    }
}
#[global_allocator]
static HEAP: Alloc = Alloc(UnsafeCell::new(Heap::empty()));
const HEAP_SIZE: usize = 3456 * 1024; // of the 4 MB a PSRAM board gives an app
static mut ARENA: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

/// A panic ends the app; where and why goes to panic.txt in the plugin folder, for bug reports.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;
    struct Buf([u8; 512], usize);
    impl Write for Buf {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for &b in s.as_bytes() {
                if self.1 < self.0.len() {
                    self.0[self.1] = b;
                    self.1 += 1;
                }
            }
            Ok(())
        }
    }
    let mut b = Buf([0; 512], 0);
    let _ = write!(b, "{}", info);
    let mut dir = [0u8; 160];
    let n = dsn::fs_app_dir(&mut dir).max(0) as usize;
    let mut path = [0u8; 180];
    let d = &dir[..n.min(160)];
    path[..d.len()].copy_from_slice(d);
    path[d.len()..d.len() + 10].copy_from_slice(b"/panic.txt");
    if let Ok(p) = core::str::from_utf8(&path[..d.len() + 10]) {
        dsn::fs_write(p, &b.0[..b.1]);
    }
    dsn::exit(101)
}

#[no_mangle]
pub extern "C" fn main() -> i32 {
    unsafe { (*HEAP.0.get()).init(core::ptr::addr_of_mut!(ARENA) as *mut u8, HEAP_SIZE) };
    store::init();
    ui::run();
    0
}
