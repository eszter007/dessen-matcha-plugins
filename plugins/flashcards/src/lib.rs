#![no_std]
extern crate alloc;

mod dsn;
mod fsrs;
mod gfx;
mod html;
mod icons;
mod png;
mod rpc;
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
const HEAP_SIZE: usize = 3584 * 1024; // of the 4 MB a PSRAM board gives an app
static mut ARENA: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { dsn::exit(101) }

#[no_mangle]
pub extern "C" fn main() -> i32 {
    unsafe { (*HEAP.0.get()).init(core::ptr::addr_of_mut!(ARENA) as *mut u8, HEAP_SIZE) };
    store::init();
    ui::run();
    0
}
