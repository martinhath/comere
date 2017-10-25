//! Hazard Pointer.  We implement Hazard Pointers for common concurrent data structures.
//! We keep the number of hazard pointers per thread fixed (`NUM_HP`).
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
pub mod queue;
pub mod list;

use std::cell::RefCell;


/// The number of hazard pointers for each thread.
const NUM_HP: usize = 2;

/// Data each thread needs to keep track of the hazard pointers.
struct ThreadEntry {
    hazard_pointers: [usize; NUM_HP],
}

/// Register the given pointer as a hazzard pointer.
/// Return `true` if we succeed, `false` if not.
pub fn regitser_hp<T>(ptr: *const T) -> bool {
    ENTRY.with(|te| {
        let mut te = te.borrow_mut();
        for i in 0..NUM_HP {
            if te.hazard_pointers[i] == 0 {
                te.hazard_pointers[i] = ptr as usize;
                return true;
            }
        }
        false
    })
}

/// Unregister the given pointer as a hazzard pointer.
/// Return `true` if we succeed, `false` if not.
pub fn unregitser_hp<T>(ptr: *const T) -> bool {
    ENTRY.with(|te| {
        let mut te = te.borrow_mut();
        let ptr = ptr as usize;
        for i in 0..NUM_HP {
            if te.hazard_pointers[i] == ptr {
                te.hazard_pointers[i] = 0;
                return true;
            }
        }
        false
    })
}

thread_local! {
    static ENTRY: RefCell<ThreadEntry> = {
        RefCell::new(ThreadEntry {
            hazard_pointers: [0; NUM_HP],
        })
    };
}

lazy_static! {
    static ref ENTRIES: list::List<ThreadEntry> = {
        list::List::new()
    };
}
