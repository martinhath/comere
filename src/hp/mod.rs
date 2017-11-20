//! Hazard Pointer.  We implement Hazard Pointers for common concurrent data structures.
//! We keep the number of hazard pointers per thread fixed (`NUM_HP`).
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
pub mod queue;
pub mod list;

use std::sync::atomic::AtomicUsize;

use self::atomic::{Owned, HazardPtr};
use std::mem::{forget, drop, ManuallyDrop};


/// The number of hazard pointers for each thread.
const NUM_HP: usize = 3;

/// Data each thread needs to keep track of the hazard pointers.  We must use atomics here; if we
/// do not we will have race conditions when one threads scans, and another thread edits its entry.
#[derive(Debug)]
struct ThreadEntry {
    hazard_pointers: [AtomicUsize; NUM_HP],
}

impl ThreadEntry {
    fn new() -> Self {
        unsafe {
            // We get uninitialized memory, and initialize it with ptr::write.
            let mut entry = Self { hazard_pointers: ::std::mem::uninitialized() };
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
}

/// Get a reference to the current threads entry in the global list. If this entry is not
/// created yet, create it, and add it to the list.
fn get_entry() -> &'static mut ThreadEntry {
    let p: atomic::Ptr<ThreadEntry> = ENTRY_PTR.with(|ptr| if ptr.borrow().is_null() {
        let p = ENTRIES.insert(ThreadEntry::new());
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

struct Garbage(Box<FnOnce()>, usize);

unsafe impl Send for Garbage {}
unsafe impl Sync for Garbage {}

impl Garbage {
    /// Make a new `Garbage` object given the data `t`.
    fn new<T>(t: Owned<T>) -> Self
    where
        T: 'static,
    {
        let d = t.data;
        Garbage(Box::new(move || { ::std::mem::forget(t); }), d)
    }

    fn address(&self) -> usize {
        self.1
    }
}


#[cfg(not(feature = "hp-wait"))]
lazy_static! {
    // This queue is `usize`, because we do not know what type the HP is pointing to.
    static ref HAZARD_QUEUE: queue::Queue<Garbage> = {
        queue::Queue::new()
    };
}

fn defer_hp<T>(hp: atomic::HazardPtr<T>)
where
    T: 'static,
{
    unsafe {
        HAZARD_QUEUE.push(Garbage::new(hp.into_owned()));
    }
}

fn free_from_queue() {
    const N: usize = 3;
    for _ in 0..N {
        if let Some(garbage) = HAZARD_QUEUE.pop_hp_fn(|h| {
            h.spin();
            unsafe {
                h.into_owned();
            }
        })
        {
            if HazardPtr::<()>::scan_addr(garbage.address()) {
                // used
                HAZARD_QUEUE.push(garbage);
            } else {
                drop(garbage);
            }
        } else {
            return;
        }
    }
}
