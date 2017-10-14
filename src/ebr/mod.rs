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
use std::sync::atomic::{AtomicUsize, AtomicPtr, Ordering};
use std::cell::RefCell;
use std::default::Default;
use std::mem::ManuallyDrop;

use self::atomic::{Ptr, Owned};
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

/// We cannot afford to allocate a new node in any data structure
/// per thing we want to garbage collect, since we'd get an infinite
/// loop of generated garbage, as the garbage list itself also needs
/// to be garbage collected. We use this `Bag` to group together some
/// number of elements, and use them as one unit.
#[derive(Debug)]
struct Bag {
    data: [Option<Garbage>; BAG_SIZE],
    index: usize,
}

const BAG_SIZE: usize = 32;

struct Garbage(Box<FnOnce()>);

unsafe impl Send for Garbage {}
unsafe impl Sync for Garbage {}

impl Garbage {
    fn new<T>(t: Owned<T>) -> Self
    where
        T: 'static,
    {
        let data = unsafe { ::std::mem::transmute::<Owned<T>, usize>(t) };
        let t = unsafe { ::std::mem::transmute::<usize, Owned<T>>(data) };
        let g = Garbage(Box::new(move || { ::std::mem::drop(t); }));
        g
    }
}

impl ::std::fmt::Debug for Garbage {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
        use std::fmt::Pointer;
        fmt.write_str("Garbage { fn: ")?;
        self.0.fmt(fmt)?;
        fmt.write_str(" }")
    }
}

impl Bag {
    fn new() -> Self {
        Self {
            data: Default::default(),
            index: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.index == BAG_SIZE
    }

    fn try_insert(&mut self, t: Garbage) -> Result<(), Garbage> {
        if self.is_full() {
            Err(t)
        } else {
            self.data[self.index] = Some(t);
            self.index += 1;
            Ok(())
        }
    }
}

struct GlobalState {
    epoch: AtomicUsize,
    pins: list::List<ThreadPinMarker>,
    garbage: queue::Queue<(usize, ManuallyDrop<Bag>)>,
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

    fn add_garbage_bag<'scope>(&self, bag: Bag, epoch: usize, _pin: Pin<'scope>) {
        self.garbage.push((epoch, ManuallyDrop::new(bag)), _pin);
    }

    /// Increments the current epoch and puts all garbage in the safe-to-free
    /// garbare queue.
    fn increment_epoch<'scope>(&self, pin: Pin<'scope>) {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let ret = self.epoch.compare_and_swap(
            epoch,
            epoch + 1,
            Ordering::SeqCst,
        );
        if ret == epoch {
            let current_epoch = epoch + 1;
            while let Some((e, mut bag)) =
                self.garbage.pop_if(
                    |&(e, _)| current_epoch.saturating_sub(e) >= 2,
                    pin,
                )
            {
                // Since we've popped the bag from the queue,
                // this thread is the only thread accessing the bag.
                // This isn't true in general, since `pop_if` accesses
                // the bag, and can read whatever it wants.
                for i in 0..bag.index {
                    let garbage = bag.data[i].take();
                    if garbage.is_none() {
                        break;
                    }
                    let garbage: Garbage = garbage.unwrap();
                    ::std::mem::drop(garbage);
                    // ::std::mem::forget(garbage);
                    // TODO: call the FnOnce here
                }
                // The node we just popped may be causing problems?
                // Maybe this bag is double freed or something?
            }
        }
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
            garbage: queue::Queue::new(),
        }
    };
}

struct LocalState {
    epoch: usize,
    thread_pin: *const Node<ThreadPinMarker>,
    pin_count: usize,
    garbage_bag: Bag,
}

impl LocalState {
    /// Adds the garbage in the local bag if there is room.  If not, we push it to the global
    /// queue, and make a new local bag.
    ///
    /// Note that we assume that only one thread is calling this on some data.
    /// This is maybe enforced by `Owned`?
    fn add_garbage<'scope, T>(&mut self, o: Owned<T>, pin: Pin<'scope>)
    where
        T: 'static,
    {
        let g = Garbage::new(o);
        match self.garbage_bag.try_insert(g) {
            Ok(()) => {}
            Err(o) => {
                let mut new_bag = Bag::new();
                let res = new_bag.try_insert(o);
                assert!(res.is_ok());
                ::std::mem::swap(&mut self.garbage_bag, &mut new_bag);
                GLOBAL.add_garbage_bag(new_bag, self.epoch, pin);
            }
        };
    }
}

thread_local! {
    static LOCAL_EPOCH: RefCell<LocalState> = {
        RefCell::new(LocalState {
            epoch: 0,
            thread_pin: ::std::ptr::null(),
            pin_count: 0,
            garbage_bag: Bag::new(),
        })
    }
}

/// A marker value, used as a proof that Ptr functions are
/// only used when the current epoch is pinned (read).
#[derive(Clone, Copy)]
pub struct Pin<'scope> {
    _marker: PhantomData<&'scope ()>,
}

impl<'scope> Pin<'scope> {
    pub(crate) fn fake() -> Self {
        Pin { _marker: PhantomData }
    }

    pub fn add_garbage<T>(&self, o: Owned<T>)
    where
        T: 'static,
    {
        LOCAL_EPOCH.with(|l| l.borrow_mut().add_garbage(o, *self));
    }
}

pub fn pin<'scope, F, R>(f: F) -> R
where
    F: FnOnce(Pin<'scope>) -> R,
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
        // TODO: reset this number to something higher
        if pin_count % 1000 == 0 && GLOBAL.can_increment_epoch() {
            GLOBAL.increment_epoch(p);
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
