#![allow(unused_variables)]
#![allow(dead_code)]
//! Epoch Based Reclamation (EBR). This is the same approach that `crossbeam-epoch` is based on. It
//! is low very overhead compared to eg. Hazard Pointers.
//!
//! The scheme works as follows: there is a global number, called the `epoch`.  Each thread has a
//! local epoch, which is the latest observed global epoch.  When threads want to delete stuff, the
//! garbage is put in a list for the current epoch.
//!
//! When threads want to perform memory operations, the `pin` the current epoch.  We keep track of
//! the epochs of threads pinning. This way, if all threads pinning have seen epoch `e`, we can
//! safely destroy garbage from epoch `e-2`.
//!
//! # Inner workings
//!
//! The system works as follows: globally there is a list of `ThreadPinMarker`s, which contains all
//! threads that have ever `pin`ned somehting, as well as whether the thread is currently pinning
//! anything, and the last seen epoch of that thread. Every once in a while, when a thread pins
//! something, it walks the list and checks which epoch all threads which are pinned have seen.  If
//! they have all seen epoch `n`, the garbage from epoch `n-2` is free to be collected, so the
//! thread can free this garbage.
//!
//! Threads also have some local data, which includes a pointer to the node in the global list.
//! When `pin` is called, we use this pointer to update the `ThreadPinMarker` struct in the list.
//!
//! # Details on freeing memory
//!
//! When clients wants to free memory, they call `pin::add_garbage`, supplying a `Owned<T>`. This
//! is the only thing clients need to do, and the only thing they need to make sure of is that no
//! other thread is adding the same memory (this should be fine, since we're taking an `Owned`,
//! which _should be_ unique). This pointer is passed to `LocalState::add_garbage`, which makes a
//! `Garbage` object containing it. The garbage object is used to abstract away handling different
//! destructors for different types, so that we only worry about `Drop`ping, and not the types of
//! what we are dropping.
//!
//! `Garbage` is just a `Box<FnOnce>`, so it is a closure that is heap allocated (since it is not
//! `sized`). When we `Drop` the garbage, we first drop the closure, which in turn drops the values
//! it has captured, which includes the `Owned` we passed it. Then we drop the heap pointer. Both
//! of these values should be unique.
//!

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
use std::default::Default;
use std::mem::ManuallyDrop;

use self::atomic::Owned;
use self::list::Node;

#[derive(Debug)]
/// A marker which is used by the threads to signal if it is pinner or not, as well as which epoch
/// it has read.
struct ThreadPinMarker {
    epoch: AtomicUsize,
    thread_id: usize,
}

impl ThreadPinMarker {
    /// Make a new pin marker
    fn new() -> Self {
        let u = GLOBAL.get_next_thread_id();
        Self {
            epoch: AtomicUsize::new(0),
            thread_id: u,
        }
    }

    /// Mark the marker as pinned. This should not be called if the thread is already pinned.
    fn pin(&self, epoch: usize) {
        let current_epoch = {
            let e = self.epoch.load(Ordering::SeqCst);
            // Clear the set bit - we use this to assume that
            // the thread wasn't already pinned.
            e & !1
        };
        let epoch = (epoch << 1) | 1;
        if self.epoch.compare_and_swap(
            current_epoch,
            epoch,
            Ordering::SeqCst,
        ) != current_epoch
        {
            panic!("ThreadMarker::pin was called, but the thread is already pinned!");
        }
    }
    /// Unmark the marker as pinned.
    fn unpin(&self) {
        let pinned_epoch = self.epoch.load(Ordering::SeqCst);
        let unpinned_epoch = self.epoch.load(Ordering::SeqCst) & !1;
        if self.epoch.compare_and_swap(
            pinned_epoch,
            unpinned_epoch,
            Ordering::SeqCst,
        ) != pinned_epoch
        {
            panic!("ThreadMarker::unpin CAS failed!");
        }
    }

    /// Return the epoch read by the thread.
    fn epoch(&self, ord: Ordering) -> usize {
        self.epoch_and_pinned(ord).0
    }

    /// Return `true` if the thread is pinned, and `false` if not.
    fn is_pinned(&self, ord: Ordering) -> bool {
        self.epoch_and_pinned(ord).1
    }

    /// Return both the read epoch as well as wether the thread is pinned.
    fn epoch_and_pinned(&self, ord: Ordering) -> (usize, bool) {
        let e = self.epoch.load(ord);
        (e >> 1, e & 1 == 1)
    }
}

impl PartialEq for ThreadPinMarker {
    fn eq(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id
    }
}

/// A `Bag` of `Garbage`.
///
/// We cannot afford to allocate a new node in any data structure
/// per thing we want to garbage collect, since we'd get an infinite
/// loop of generated garbage, as the garbage list itself also needs
/// to be garbage collected. We use this `Bag` to group together some
/// number of elements, and use them as one unit.
#[derive(Debug)]
struct Bag {
    /// A list of garbage, if present.
    data: [Option<Garbage>; BAG_SIZE],
    /// The index of the first available slot in `data`.
    index: usize,
}

const BAG_SIZE: usize = 32;

/// This is one unit of garbage. We can think of it as just a T.
struct Garbage(Box<FnOnce()>);

unsafe impl Send for Garbage {}
unsafe impl Sync for Garbage {}

impl Garbage {
    /// Make a new `Garbage` object given the data `t`.
    fn new<T>(t: Owned<T>) -> Self
    where
        T: 'static,
    {
        // The data is moved to a closure so we do not have to keep track of what type the data is,
        // since this is needed to `Drop` it correctly - the closure keeps track of this for us.
        // Note that this closure is never actually called, but the destructors are ran when the
        // closure is dropped.
        let d = t.data;
        Garbage(Box::new(move || { ::std::mem::forget(t); }))
    }
}

// So we can #[derive(Debug)] on `Bag`
impl ::std::fmt::Debug for Garbage {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
        use std::fmt::Pointer;
        // TODO(7.11.17): use `debug_struct` ?
        fmt.write_str("Garbage { fn: ")?;
        self.0.fmt(fmt)?;
        fmt.write_str(" }")
    }
}

impl Bag {
    /// Make a new empty bag.
    fn new() -> Self {
        static mut COUNT: AtomicUsize = AtomicUsize::new(0);
        let count = unsafe { COUNT.fetch_add(1, Ordering::SeqCst) };
        Self {
            data: Default::default(),
            index: 0,
        }
    }

    /// Return `true` if the bag is full.
    fn is_full(&self) -> bool {
        self.index == BAG_SIZE
    }

    /// Try to insert `Garbage` into the bag. If successful we return `Ok(())`, and if not we
    /// return `Err(garbage)`.
    fn try_insert(&mut self, t: Garbage) -> Result<(), Garbage> {
        if self.is_full() {
            Err(t)
        } else {
            self.data[self.index] = Some(t);
            self.index += 1;
            Ok(())
        }
    }

    /// Call this on drop to assert that the elements in the bag are `take`n out and dropped
    /// explicitly.
    fn drop(self) {
        for i in 0..self.index {
            assert!(self.data[i].is_none());
        }
    }
}

/// The global data we need for EBR to work. This includes the global epoch, a list which threads
/// can broadcast their read epoch as well as whether they are pinned or not, and a list of
/// garbage tagged with the epoch the garbage was added to the queue in.
struct GlobalState {
    epoch: AtomicUsize,
    pins: list::List<ThreadPinMarker>,
    garbage: queue::Queue<(usize, Bag)>,
    next_thread_id: AtomicUsize,
}

impl GlobalState {
    /// Checks that all pinned threads have seen the current epoch. If one threads local epoch is
    /// less than the global epoch, we cannot increment the epoch.
    fn can_increment_epoch<'scope>(&self, pin: Pin<'scope>) -> bool {
        let global_epoch = self.epoch.load(Ordering::SeqCst);
        // We don't need to worry about the list changing as we walk through it: if we see an
        // unpinned element, and it changes, it means that the thread read the new epoch, so thats
        // OK. If we see a pinned element, in the worst case the thread may unpin and pin again,
        // and then know the latest epoch, but we're only seing that it has read a stale one. In
        // this case, we return false, even though we could have returned true, which is not so
        // bad.
        self.pins.all(
            |n| {
                let (epoch, pinned) = n.epoch_and_pinned(Ordering::SeqCst);
                if pinned { epoch == global_epoch } else { true }
            },
            pin,
        )
    }

    /// Add a bag of garbage to the global garbage list. The garbage is tagged with the global
    /// epoch.
    fn add_garbage_bag<'scope>(&self, bag: Bag, _pin: Pin<'scope>) {
        let epoch = self.epoch.load(Ordering::SeqCst);
        self.garbage.push((epoch, bag), _pin);
    }

    fn free_garbage<'scope>(&self, current_epoch: usize, pin: Pin<'scope>) {
        while let Some((e, mut bag)) =
            self.garbage.pop_if(
                |&(e, _)| current_epoch.saturating_sub(e) >= 2,
                pin,
            )
        {
            // TODO: share this among other threads. Find out a good way to do this.
            // Idea: set the global state to something, eg. use a second lower bit in `epoch`
            // to signal that we are in "freeing mode". Then all threads calling 'pin' will be
            // set to pull out garbage from the queue, as long as there is more left.
            //
            // Since we've popped the bag from the queue, this thread is the only thread
            // accessing the bag. This isn't true in general, since `pop_if` accesses the bag,
            // and can read whatever it wants. However, we only use `pop_if` in one place, and
            // that place only reads the `epoch` field.
            for i in 0..bag.index {
                if let Some(garbage) = bag.data[i].take() {
                    // This is where we free the memory of the nodes the data strucutres make.
                    // The `bag` contains `garbage`, which is either eg. `queue::Node<T>`, or
                    // it is `queue::Node<Bag>`, if it is the list we use for reclamation.
                    ::std::mem::drop(garbage);
                } else {
                    break;
                }
            }
            // assert that all bags are empty. Then dropping the bag is a noop.
            bag.drop();
        }
    }

    /// Increments the current epoch and puts all garbage in the safe-to-free garbare queue.
    fn increment_epoch<'scope>(&self, epoch: usize, pin: Pin<'scope>) {
        // Try to increment the epoch.
        // We are using `cas` instead of `fech_add`, since we would like to make sure that the
        // epoch is only once, and that the thread which increments it needs to know that it did.
        let ret = self.epoch.compare_and_swap(
            epoch,
            epoch + 1,
            Ordering::SeqCst,
        );
        if ret == epoch {
            // This is a critical section, since this thread is pinned, and has not registered
            // that we've read the newly incremented epoch. Hence, other threads only see that we
            // have seen epoch `epoch`.
            let current_epoch = epoch + 1;
            self.free_garbage(current_epoch, pin);
        }
    }

    fn get_next_thread_id(&self) -> usize {
        self.next_thread_id.fetch_add(1, Ordering::SeqCst)
    }
}

lazy_static! {
    static ref GLOBAL: GlobalState = {
        GlobalState {
            epoch: AtomicUsize::new(0),
            pins: list::List::new(),
            garbage: queue::Queue::new(),
            next_thread_id: AtomicUsize::new(0),
        }
    };
}

/// The thread local data we need for EBR. This includes the
struct LocalState {
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
                // The current bag is full. `swap` it with a new one, and push the full bag onto
                // the global list of deferred garbage.
                let mut new_bag = Bag::new();
                let res = new_bag.try_insert(o);
                // The bag is empty, so this should succeed.
                assert!(res.is_ok());
                ::std::mem::swap(&mut self.garbage_bag, &mut new_bag);
                GLOBAL.add_garbage_bag(new_bag, pin);
            }
        };
    }

    /// Returns a reference to the threads marker. Make the marker if it is not present.
    fn marker(&mut self, p: Pin) -> &'static ManuallyDrop<ThreadPinMarker> {
        let mut marker_ptr = self.thread_pin;
        if marker_ptr.is_null() {
            marker_ptr = GLOBAL.pins.insert(ThreadPinMarker::new(), p).as_raw();
            self.thread_pin = marker_ptr;
        }
        // This is safe, since we've just made sure it isn't null.
        unsafe { &(*marker_ptr).data }
    }
}

impl Drop for LocalState {
    fn drop(&mut self) {
        let p = Pin::fake();
        let m = self.marker(p);
        GLOBAL.pins.remove_with(m, p, |o| {
            // TODO: it is not safe to simply drop the Owned here, as some thread may iterate over
            // the global list.
        });
    }
}

thread_local! {
    static LOCAL_EPOCH: RefCell<LocalState> = {
        RefCell::new(LocalState {
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
    /// Return a pin without actually pinning the thread.
    /// This is useful eg. if we want to make a new queue, since we know that no other thread
    /// is accessing the memory we use.
    ///
    /// TODO: rename this, and probably mark as `unsafe`.
    pub fn fake() -> Self {
        Pin { _marker: PhantomData }
    }

    /// Add the Owned pointer as garbage. This is the first step in freeing used memory, and it is
    /// the only step for users of EBR.
    pub fn add_garbage<T>(&self, o: Owned<T>)
    where
        T: 'static,
    {
        LOCAL_EPOCH.with(|l| l.borrow_mut().add_garbage(o, *self));
    }
}

/// Pin the thread.
///
/// This is the core of EBR. When we want to do anything with memory, we need to pin the thread in
/// order for other threads to not remove memory we are accessing. We will also try to increment
/// the current epoch, before calling the supplied closure.
pub fn pin<'scope, F, R>(f: F) -> R
where
    F: FnOnce(Pin<'scope>) -> R,
{
    // Make the pin
    let p = Pin { _marker: PhantomData };
    let marker = LOCAL_EPOCH.with(|l| l.borrow_mut().marker(p));

    // Read the global epoch.
    let global_epoch = GLOBAL.epoch.load(Ordering::SeqCst);

    // We must register the pin before eventually incrementing the epoch, since we are using the
    // pin in there.
    marker.pin(global_epoch);

    // Every once in a while, try to update the global epoch.
    LOCAL_EPOCH.with(|e| {
        let pin_count = {
            let mut e = e.borrow_mut();
            e.pin_count += 1;
            e.pin_count
        };
        // TODO: reset this number to something higher
        // probably also don't use mod, but if we've pinned `n` times
        // without incrementing the epoch, we'll try?
        if pin_count % 1000 == 0 && GLOBAL.can_increment_epoch(p) {
            GLOBAL.increment_epoch(global_epoch, p);
        }
    });

    let ret = f(p);
    marker.unpin();
    ret
}



#[cfg(test)]
mod test {

    use super::*;
    #[test]
    fn add_garbage() {
        const N: usize = 1024 * 32;
        for _ in 0..N {
            pin(|pin| pin.add_garbage(atomic::Owned::new(0usize)));
        }
    }
}

mod bench {
    extern crate test;

    #[bench]
    fn pin(b: &mut test::Bencher) {
        b.iter(|| { super::pin(|pin| {}); });
    }
}
