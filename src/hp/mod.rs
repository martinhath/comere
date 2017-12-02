//! Hazard Pointer.  We implement Hazard Pointers for common concurrent data structures.
//! We keep the number of hazard pointers per thread fixed (`NUM_HP`).
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
pub mod queue;
pub mod list;

use std::sync::atomic::{AtomicUsize, Ordering};

use self::atomic::{Owned, HazardPtr};
use std::mem::{forget, drop, ManuallyDrop};

///
/// The number of hazard pointers for each thread.
const NUM_HP: usize = 5;

/// Data each thread needs to keep track of the hazard pointers.  We must use atomics here; if we
/// do not we will have race conditions when one threads scans, and another thread edits its entry.
#[derive(Debug)]
pub struct ThreadEntry {
    hazard_pointers: [AtomicUsize; NUM_HP],
    thread_id: usize,
}

impl ThreadEntry {
    fn new(id: usize) -> Self {
        unsafe {
            // We get uninitialized memory, and initialize it with ptr::write.
            let mut entry = Self {
                hazard_pointers: ::std::mem::uninitialized(),
                thread_id: id,
            };
            use std::ptr::write;
            for i in 0..NUM_HP {
                write(&mut entry.hazard_pointers[i], AtomicUsize::new(0));
            }
            entry
        }
    }
}

impl PartialEq for ThreadEntry {
    fn eq(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id
    }
}

#[derive(Debug)]
struct ThreadLocal {
    thread_marker: *const ThreadEntry,
    id: usize,
}

impl ThreadLocal {
    /// Returns a reference to the threads marker. Make the marker if it is not present.
    fn marker(&mut self) -> &'static mut ThreadEntry {
        let mut marker_ptr = self.thread_marker;
        if marker_ptr.is_null() {
            let te = ThreadEntry::new(self.id);
            marker_ptr = ENTRIES.insert(te).as_raw();
            self.thread_marker = marker_ptr;
        }
        unsafe {
            // TODO: transmute from *const to *mut !!! Bad idea!
            let ptr = ::std::mem::transmute::<*const ThreadEntry, *mut ThreadEntry>(marker_ptr);
            // This is safe, since we've just made sure it isn't null.
            &mut *ptr
        }
    }
}

pub fn marker() -> &'static mut ThreadEntry {
    let marker = THREAD_LOCAL.with(|tl| tl.borrow_mut().marker());
    marker
}

fn remove_thread_local() {
    let marker = marker();
    let ret = ENTRIES.remove(marker);
}

use std::sync::mpsc::{channel, Sender, Receiver};

pub struct JoinHandle<T> {
    thread: ::std::thread::JoinHandle<T>,
    sender: Sender<()>,
}

impl<T> JoinHandle<T> {
    pub fn join(self) -> ::std::thread::Result<T> {
        self.sender.send(());
        self.thread.join()
    }
}

pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = channel();
    JoinHandle {
        thread: ::std::thread::spawn(move || {
            let res = f();
            if let Ok(_) = rx.recv() {
                remove_thread_local();
            }
            res

        }),
        sender: tx,
    }
}

use std::cell::RefCell;
thread_local! {
    static THREAD_LOCAL: RefCell<ThreadLocal> = {
        let tl = ThreadLocal {
            thread_marker: ::std::ptr::null(),
            id: get_next_thread_id(),
        };
        RefCell::new(tl)
    }
}

lazy_static! {
    /// The global list of entries. Each thread will register into this list,
    /// and have a local pointer to its entry.
    static ref ENTRIES: list::List<ThreadEntry> = {
        list::List::new()
    };
    static ref THREAD_ID: AtomicUsize = {
        AtomicUsize::new(0)
    };
}

fn get_next_thread_id() -> usize {
    THREAD_ID.fetch_add(1, Ordering::SeqCst)
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

#[derive(Debug, Clone, Copy)]
pub enum HazardError {
    NoSpace,
    NotFound,
}
