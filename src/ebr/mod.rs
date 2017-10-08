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

struct GlobalState {
    epoch: AtomicUsize,
    pins: list::List,
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
}

thread_local! {
    static LOCAL_EPOCH: RefCell<LocalState> = {
        RefCell::new(LocalState {
            epoch: 0,
        })
    }
}

/// A marker value, used as a proof that Ptr functions are
/// only used when the current epoch is pinned (read).
pub struct Pin<'scope> {
    _marker: PhantomData<&'scope ()>,
}

pub fn pin<'scope, F, R>(f: F) -> R
where
    F: Fn(Pin<'scope>) -> R,
{
    let p = Pin { _marker: PhantomData };
    f(p)
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
