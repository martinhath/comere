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
/// Note that we use `usize`; `0` means that the pointer is not set.
/// We could replace this with eg. `Option<*const T>`, or something like that.
// TODO: Look into PhantomData, and see if we need it here.
// An entry should more be able to point to whatever, so maybe not, but we need
// to check what to do about destructors.
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
    /// A thread local pointer to the thread's entry in the global entry list. This pointer
    /// may be null, but will be set during the first call to `get_entry`.
    static ENTRY_PTR: RefCell<*const list::Node<ThreadEntry>> = {
        RefCell::new(0 as *const list::Node<ThreadEntry>)
    };
}

/// Get a reference to the current threads entry in the global list. If this entry is not
/// created yet, create it, and add it to the list.
fn get_entry() -> &'static mut ThreadEntry {
    // TODO: fix this implemnetation! This seems very sketchy!
    let p: *mut list::Node<ThreadEntry> = ENTRY_PTR.with(|ptr| if ptr.borrow().is_null() {
        let entry = ThreadEntry { hazard_pointers: [0; NUM_HP] };
        let p = ENTRIES.insert(entry).as_raw();
        *ptr.borrow_mut() = p;
        p
    } else {
        *ptr.borrow()
    }) as *mut list::Node<ThreadEntry>;
    &mut (unsafe { &mut *p }).data as &'static mut ThreadEntry
}

lazy_static! {
    /// The global list of entries. Each thread will register into this list,
    /// and have a local pointer to its entry.
    static ref ENTRIES: list::List<ThreadEntry> = {
        list::List::new()
    };
}

#[cfg(test)]
mod test {
    use super::*;
    use std::thread::spawn;

    #[test]
    /// Confirm that `get_entry` makes an entry on the initial call,
    /// and that it does not create multiple entries. Also check that registering
    /// and deregistering HPs works.
    fn setup() {
        get_entry();
        let a = spawn(|| for _ in 0..10 {
            get_entry();
        });
        let b = spawn(|| for _ in 0..10 {
            get_entry();
        });
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
