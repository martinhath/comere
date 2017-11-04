//! Hazard Pointer.  We implement Hazard Pointers for common concurrent data structures.
//! We keep the number of hazard pointers per thread fixed (`NUM_HP`).
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
pub mod queue;
pub mod list;

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;


/// The number of hazard pointers for each thread.
const NUM_HP: usize = 3;

/// Data each thread needs to keep track of the hazard pointers.
/// We must use atomics here; if we do not we will have race conditions when one threads
/// scans, and another thread edits its entry.
#[derive(Debug)]
struct ThreadEntry {
    hazard_pointers: [AtomicUsize; NUM_HP],
    id: usize,
}

impl ThreadEntry {
    fn new(id: usize) -> Self {
        let mut entry = Self {
            hazard_pointers: unsafe { ::std::mem::uninitialized() },
            id,
        };
        for i in 0..NUM_HP {
            entry.hazard_pointers[i] = AtomicUsize::new(0);
        }
        entry
    }
}

pub struct HazardHandle {
    ptr: *const (),
}

impl HazardHandle {
    fn new<T>(ptr: *const T) -> Self {
        Self { ptr: ptr as *const ()}
    }
}

impl Drop for HazardHandle {
    fn drop(&mut self) {
        deregister_hp(self.ptr);
    }
}

/// Register the given pointer as a hazzard pointer.
/// Return `true` if we succeed, `false` if not.
pub fn register_hp<T>(ptr: *const T) -> Option<HazardHandle> {
    // println!("registering {:p}", ptr);
    let entry = &mut *get_entry();
    entry.id = get_thread_id();
    for i in 0..NUM_HP {
        let hp = entry.hazard_pointers[i].load(SeqCst);
        if hp == 0 {
            entry.hazard_pointers[i].store(ptr as usize, SeqCst);
            return Some(HazardHandle::new(ptr));
        } else {
            // println!("  hp was {:x}", hp);
        }
    }
    None
}

/// Deregister the given pointer as a hazzard pointer.
/// Return `true` if we succeed, `false` if not.
fn deregister_hp<T>(ptr: *const T) -> bool {
    // println!("deregistering {:p}", ptr);
    let ptr = ptr as usize;
    let entry = &mut *get_entry();
    for i in 0..NUM_HP {
        let hp = entry.hazard_pointers[i].load(SeqCst);
        if hp == ptr {
            entry.hazard_pointers[i].store(0, SeqCst);
            return true;
        } else {
            // println!("  hp was {:x}", hp);
        }
    }
    false
}

/// Checks weather the given pointer is a registered hazard pointer.
pub fn scan<T>(ptr: *const T) -> bool {
    let ptr = ptr as usize;
    for e in ENTRIES.iter() {
        for p in e.hazard_pointers.iter() {
            if ptr == p.load(SeqCst) {
                println!("[{}] thread {} has hp {:x}", get_thread_id(), e.id, ptr);
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
        a.join().unwrap();
        b.join().unwrap();
        // Check that entries for all three threads are here
        assert_eq!(ENTRIES.iter().count(), 3);
        // Check that all HPs are zero.
        for entry in ENTRIES.iter() {
            for hp in entry.hazard_pointers.iter() {
                assert_eq!(hp.load(SeqCst), 0);
            }
        }
        assert!(NUM_HP >= 2);
        let ptr1 = 12 as *const u32;
        let handle1 = register_hp(ptr1);
        assert!(handle1.is_some());
        let ptr2 = 48 as *const u32;
        let handle2 = register_hp(ptr2);
        assert!(handle2.is_some());
        // confirm that one thread has set its pointers,
        // and the other has not.
        for entry in ENTRIES.iter() {
            let sum: usize = entry.hazard_pointers.iter().map(|a| a.load(SeqCst)).sum();
            assert!(sum == 0 || sum == 12 + 48);
        }
        // and back again
        ::std::mem::drop(handle1);
        ::std::mem::drop(handle2);
        for entry in ENTRIES.iter() {
            for hp in entry.hazard_pointers.iter() {
                assert_eq!(hp.load(SeqCst), 0);
            }
        }
    }
}
