//! Epoch Based Reclamation (EBR). This is the same approach that `crossbeam-epoch`
//! is based on. It is low very overhead compared to eg. Hazard Pointers.

use std::marker::PhantomData;

pub mod atomic;
pub mod queue;

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
