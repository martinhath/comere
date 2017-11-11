//! Hazard Pointer.  We implement Hazard Pointers for common concurrent data structures.
//! We keep the number of hazard pointers per thread fixed (`NUM_HP`).
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
pub mod queue;
pub mod list;

use std::sync::atomic::AtomicUsize;


/// The number of hazard pointers for each thread.
const NUM_HP: usize = 3;

/// Data each thread needs to keep track of the hazard pointers.  We must use atomics here; if we
/// do not we will have race conditions when one threads scans, and another thread edits its entry.
///
/// We mark the entry with the thread id, for debugging. TODO: remove.
#[derive(Debug)]
struct ThreadEntry {
    hazard_pointers: [AtomicUsize; NUM_HP],
    id: usize,
}

impl ThreadEntry {
    fn new(id: usize) -> Self {
        unsafe {
            // We get uninitialized memory, and initialize it with ptr::write.
            let mut entry = Self {
                hazard_pointers: ::std::mem::uninitialized(),
                id,
            };
            use std::ptr::write;
            for i in 0..NUM_HP {
                write(&mut entry.hazard_pointers[i], AtomicUsize::new(0));
            }
            entry
        }
    }
}

use std::cell::RefCell;
thread_local! {
    /// A thread local pointer to the thread's entry in the global entry list. This pointer
    /// may be null, but will be set during the first call to `get_entry`.
    static ENTRY_PTR: RefCell<atomic::Ptr<'static, ThreadEntry>> = {
        RefCell::new(atomic::Ptr::null())
    };
    static LOCAL_ID: RefCell<usize> = {
        RefCell::new(0)
    }
}

/// Get a reference to the current threads entry in the global list. If this entry is not
/// created yet, create it, and add it to the list.
fn get_entry() -> &'static mut ThreadEntry {
    let p: atomic::Ptr<ThreadEntry> = ENTRY_PTR.with(|ptr| if ptr.borrow().is_null() {
        let p = ENTRIES.insert(ThreadEntry::new(get_thread_id()));
        *ptr.borrow_mut() = p;
        p
    } else {
        *ptr.borrow()
    });
    unsafe { &mut *(p.as_raw() as *mut ThreadEntry) }
}

lazy_static! {
    /// The global list of entries. Each thread will register into this list,
    /// and have a local pointer to its entry.
    static ref ENTRIES: list::List<ThreadEntry> = {
        list::List::new()
    };
}

// TODO: remove debug
pub fn register_thread(i: usize) {
    LOCAL_ID.with(|l| *l.borrow_mut() = i);
}

// TODO: remove debug
pub fn get_thread_id() -> usize {
    LOCAL_ID.with(|l| *l.borrow())
}
