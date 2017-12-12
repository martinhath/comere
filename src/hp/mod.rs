//! Hazard Pointer.  We implement Hazard Pointers for common concurrent data structures.
//! We keep the number of hazard pointers per thread fixed (`NUM_HP`).
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
pub mod queue;
pub mod list;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::mem::drop;

use self::atomic::{Owned, HazardPtr};

use bench::Spawner;

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

use std::cell::UnsafeCell;

#[derive(Debug)]
struct ThreadLocal {
    thread_marker: UnsafeCell<*mut ThreadEntry>,
    id: usize,
}

impl ThreadLocal {
    /// Returns a reference to the threads marker. Make the marker if it is not present.
    unsafe fn marker(&self) -> &'static mut ThreadEntry {
        let marker_ptr = self.thread_marker.get();
        if (*marker_ptr).is_null() {
            let te = ThreadEntry::new(self.id);
            use self::list::Node;
            let owned = Owned::new(Node::new(te));
            *marker_ptr = (*owned).data_ptr().as_raw() as *mut _;
            ENTRIES.insert_owned(owned);
        }
        &mut **marker_ptr
    }
}

pub fn marker() -> &'static mut ThreadEntry {
    unsafe {
        let marker = THREAD_LOCAL.with(|tl| tl.borrow().marker());
        marker
    }
}

fn remove_thread_local() {
    let marker = marker();
    let ret = ENTRIES.remove_with_node(marker);
    if let Some(owned) = ret {
        while HazardPtr::<()>::scan_addr(owned.data as usize) {}
    } else {
        panic!("Failed to remove own thread loacal thing!");
    }
}

pub struct JoinHandle<T> {
    thread: ::std::thread::JoinHandle<T>,
    sender: Sender<()>,
}

impl<T> JoinHandle<T> {
    pub fn join(self) -> ::std::thread::Result<T> {
        assert!(self.sender.send(()).is_ok());
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

impl<T> Spawner for JoinHandle<T> {
    type Return = T;
    type Result = ::std::thread::Result<T>;

    fn spawn<F>(f: F) -> Self
    where
        F: FnOnce() -> Self::Return,
        F: Send + 'static,
        Self::Return: Send + 'static,
    {
        spawn(f)
    }

    fn join(self) -> Self::Result {
        self.join()
    }
}

use std::cell::RefCell;
thread_local! {
    static THREAD_LOCAL: RefCell<ThreadLocal> = {
        let tl = ThreadLocal {
            thread_marker: UnsafeCell::new(::std::ptr::null_mut()),
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

#[cfg(not(feature = "hp-wait"))]
fn defer_hp<T>(hp: atomic::HazardPtr<T>)
where
    T: 'static,
{
    unsafe {
        HAZARD_QUEUE.push(Garbage::new(hp.into_owned()));
    }
}

#[cfg(not(feature = "hp-wait"))]
fn free_from_queue() {
    const N: usize = 32;
    thread_local! {
        static COUNTER: RefCell<usize> = { RefCell::new(0) }
    }
    let c = COUNTER.with(|c| {
        let c = &mut *c.borrow_mut();
        *c += 1;
        *c
    });
    if c % N == 0 {
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
}

#[derive(Debug, Clone, Copy)]
pub enum HazardError {
    NoSpace,
    NotFound,
}
