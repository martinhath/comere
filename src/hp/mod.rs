//! Hazard Pointer.  We implement Hazard Pointers for common concurrent data structures.
//! We keep the number of hazard pointers per thread fixed (`NUM_HP`).
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
pub mod queue;
pub mod list;


/// The number of hazard pointers for each thread.
const NUM_HP: usize = 2;

/// Data each thread needs to keep track of the hazard pointers.
#[derive(Debug)]
struct ThreadEntry {
    hazard_pointers: [usize; NUM_HP],
}

/// Register the given pointer as a hazzard pointer.
/// Return `true` if we succeed, `false` if not.
pub fn register_hp<T>(ptr: *const T) -> bool {
    let entry = &mut *get_entry();
    for i in 0..NUM_HP {
        if entry.hazard_pointers[i] == 0 {
            entry.hazard_pointers[i] = ptr as usize;
            return true;
        }
    }
    false
}

/// Deregister the given pointer as a hazzard pointer.
/// Return `true` if we succeed, `false` if not.
pub fn deregister_hp<T>(ptr: *const T) -> bool {
    let ptr = ptr as usize;
    let entry = &mut *get_entry();
    for i in 0..NUM_HP {
        if entry.hazard_pointers[i] == ptr {
            entry.hazard_pointers[i] = 0;
            return true;
        }
    }
    false
}

/// Checks weather the given pointer is a registered hazard pointer.
pub fn scan<T>(ptr: *const T) -> bool {
    let ptr = ptr as usize;
    for e in ENTRIES.iter() {
        for &p in e.hazard_pointers.iter() {
            if ptr == p {
                return true;
            }
        }
    }
    false
}

use std::cell::RefCell;
thread_local! {
    static ENTRY_PTR: RefCell<*const list::Node<ThreadEntry>> = {
        RefCell::new(0 as *const list::Node<ThreadEntry>)
    };
}

fn get_entry() -> &'static mut ThreadEntry {
    let p: *mut list::Node<ThreadEntry> = ENTRY_PTR.with(|ptr| if ptr.borrow().is_null() {
        let entry = ThreadEntry { hazard_pointers: [0; NUM_HP] };
        let p = ENTRIES.insert(entry).as_raw();
        *ptr.borrow_mut() = p;
        p
    } else {
        *ptr.borrow()
    }) as *mut list::Node<ThreadEntry>;
    // ????
    &mut (unsafe { &mut *p }).data as &'static mut ThreadEntry
}

lazy_static! {
    static ref ENTRIES: list::List<ThreadEntry> = {
        list::List::new()
    };
}

#[cfg(test)]
mod test {
    use super::*;
    use std::thread::spawn;

    #[test]
    fn setup() {
        get_entry();
        let a = spawn(|| { get_entry(); });
        let b = spawn(|| { get_entry(); });
        a.join();
        b.join();
        // Check that entries for all three threads are here
        assert_eq!(ENTRIES.iter().count(), 3);
        // Check that all HPs are zero.
        for entry in ENTRIES.iter() {
            assert_eq!(entry.hazard_pointers, [0; NUM_HP]);
        }
        assert!(NUM_HP >= 2);
        let ptr1 = 12 as *const u32;
        assert!(register_hp(ptr1));
        let ptr2 = 48 as *const u32;
        assert!(register_hp(ptr2));
        // confirm that one thread has set its pointers,
        // and the other has not.
        for entry in ENTRIES.iter() {
            let sum: usize = entry.hazard_pointers.iter().sum();
            assert!(sum == 0 || sum == 12 + 48);
        }
        // and back again
        assert!(deregister_hp(ptr1));
        assert!(deregister_hp(ptr2));
        for entry in ENTRIES.iter() {
            assert_eq!(entry.hazard_pointers, [0; NUM_HP]);
        }
    }
}
