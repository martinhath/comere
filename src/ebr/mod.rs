#![allow(unused_variables)]
#![allow(dead_code)]
//! Epoch Based Reclamation (EBR). This is the same approach that `crossbeam-epoch`
//! is based on. It is low very overhead compared to eg. Hazard Pointers.
//!
//! The scheme works as follows: there is a global number, called the `epoch`.
//! Each thread has a local epoch, which is the latest observed global epoch.
//! When threads want to delete stuff, the garbage is put in a list for the
//! current epoch.
//!
//! When threads want to perform memory operations, the `pin` the current epoch.
//! We keep track of the epochs of threads pinning. This way, if all threads
//! pinning have seen epoch `e`, we can safely destroy garbage from
//! epoch `e-2`.
//!
//! # Inner workings
//!
//! The system works as follows:
//! globally there is a list of `ThreadPinMarker`s, which contains
//! all threads that have ever `pin`ned somehting, as well as whether
//! the thread is currently pinning anything, and the last seen epoch
//! of that thread. Every once in a while, when a thread pins something,
//! it walks the list and checks which epoch all threads which are
//! pinned have seen.  If they have all seen epoch `n`, the garbage
//! from epoch `n-2` is free to be collected, so the thread can free
//! this garbage.
//!
//! Threads also have some local data, which includes a pointer to the
//! node in the global list. When `pin` is called, we use this pointer
//! to update the `ThreadPinMarker` struct in the list.

#[allow(unused_variables)]
#[allow(dead_code)]
pub mod atomic;
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod queue;
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod list;

use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::RefCell;

use self::atomic::Ptr;
use self::list::Node;

#[derive(Debug)]
struct ThreadPinMarker {
    epoch: AtomicUsize,
}

impl ThreadPinMarker {
    fn new() -> Self {
        Self { epoch: AtomicUsize::new(0) }
    }

    fn pin(&self, epoch: usize) {
        let current_epoch = {
            let e = self.epoch.load(Ordering::SeqCst);
            // Clear the set bit - we use this to assume that
            // the thread wasn't already pinned.
            e & !1
        };
        let epoch = epoch << 1;
        if self.epoch.compare_and_swap(
            current_epoch,
            epoch,
            Ordering::SeqCst,
        ) != current_epoch
        {
            panic!(
                "ThreadMarker::pin was called, but the thread is
                   already pinned!"
            );
        }
    }

    fn unpin(&self) {
        let pinned_epoch = self.epoch.load(Ordering::SeqCst);
        let unpinned_epoch = self.epoch.load(Ordering::SeqCst);
        if self.epoch.compare_and_swap(
            pinned_epoch,
            unpinned_epoch,
            Ordering::SeqCst,
        ) != pinned_epoch
        {
            panic!("ThreadMarker::unpin CAS failed!");
        }
    }

    fn epoch(&self, ord: Ordering) -> usize {
        self.epoch_and_pinned(ord).0
    }

    fn is_pinned(&self, ord: Ordering) -> bool {
        self.epoch_and_pinned(ord).1
    }

    fn epoch_and_pinned(&self, ord: Ordering) -> (usize, bool) {
        let e = self.epoch.load(ord);
        (e >> 1, e & 1 == 1)
    }
}

struct GlobalState {
    epoch: AtomicUsize,
    pins: list::List<ThreadPinMarker>,
}

impl GlobalState {
    fn can_increment_epoch(&self) -> bool {
        let global_epoch = self.epoch.load(Ordering::SeqCst);
        pin(|_pin| {
            self.pins.all(
                |n| {
                    let (epoch, pinned) = n.epoch_and_pinned(Ordering::SeqCst);
                    if pinned { epoch >= global_epoch } else { true }
                },
                _pin,
            )
        })
    }
}

pub fn print_epochs() {
    pin(|_pin| {
        GLOBAL.pins.all(
            |n| {
                println!("{:?}", n);
                true
            },
            _pin,
        )
    });
}

lazy_static! {
    static ref GLOBAL: GlobalState = {
        GlobalState {
            epoch: AtomicUsize::new(0),
            pins: list::List::new(),
        }
    };
}

struct LocalState {
    epoch: usize,
    thread_pin: *const Node<ThreadPinMarker>,
    pin_count: usize,
}

thread_local! {
    static LOCAL_EPOCH: RefCell<LocalState> = {
        RefCell::new(LocalState {
            epoch: 0,
            thread_pin: ::std::ptr::null(),
            pin_count: 0,
        })
    }
}

/// A marker value, used as a proof that Ptr functions are
/// only used when the current epoch is pinned (read).
#[derive(Clone, Copy)]
pub struct Pin<'scope> {
    _marker: PhantomData<&'scope ()>,
}

pub fn pin<'scope, F, R>(f: F) -> R
where
    F: Fn(Pin<'scope>) -> R,
{
    // Make the pin
    let p = Pin { _marker: PhantomData };

    // Get the marker, or make it if it is `null`.
    let marker = {
        let mut marker_ptr = LOCAL_EPOCH.with(|l| l.borrow().thread_pin);
        if marker_ptr.is_null() {
            marker_ptr = GLOBAL.pins.insert(ThreadPinMarker::new(), p).as_raw();
            LOCAL_EPOCH.with(|l| l.borrow_mut().thread_pin = marker_ptr);
        }
        unsafe { &(*marker_ptr).data }
    };

    // Read the global epoch.
    let global_epoch = GLOBAL.epoch.load(Ordering::SeqCst);

    // Every once in a while, try to update the global epoch.
    LOCAL_EPOCH.with(|e| {
        let pin_count = {
            let mut e = e.borrow_mut();
            e.pin_count += 1;
            e.pin_count
        };
        if pin_count % 1000 == 0 && GLOBAL.can_increment_epoch() {
            let a = GLOBAL.epoch.fetch_add(1, Ordering::SeqCst);
            println!("incrementing epoch to {}", a + 1);
        }
    });

    marker.pin(global_epoch);
    let ret = f(p);
    marker.unpin();
    ret
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn load_epoch() {
        let global = GLOBAL.epoch.load(Ordering::SeqCst);
        assert_eq!(global, 0);

        LOCAL_EPOCH.with(|e| {
            assert_eq!(e.borrow().epoch, 0);
        });
    }

    #[test]
    fn thread_epoch_writes() {
        let a = ::std::thread::spawn(|| LOCAL_EPOCH.with(|e| e.borrow_mut().epoch += 1));
        assert!(a.join().is_ok());
        LOCAL_EPOCH.with(|e| {
            assert_eq!(e.borrow().epoch, 0);
        });
    }
}
