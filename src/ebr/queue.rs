#[allow(unused_variables)]
#[allow(dead_code)]
/// A Michael-Scott Queue.

use std::sync::atomic::Ordering::{Release, Relaxed, Acquire};
use std::default::Default;
use std::mem::ManuallyDrop;

use super::{Pin, pin};

use super::atomic::{Owned, Atomic, Ptr};

#[derive(Debug)]
pub struct Queue<T> {
    head: Atomic<Node<T>>,
    tail: Atomic<Node<T>>,
}

#[derive(Debug)]
pub struct Node<T> {
    // TODO: Use `std::mem::ManuallyDrop` instead,
    // as in `crossbeam-epoch`. This will probably
    // improve memory usage, which will in order
    // improve cache behaviour.
    data: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

impl<T> Node<T> {
    fn new(data: T) -> Self {
        Self {
            data: ManuallyDrop::new(data),
            next: Default::default(),
        }
    }

    fn empty() -> Self {
        Self {
            data: unsafe { ::std::mem::uninitialized() },
            next: Default::default(),
        }
    }
}

impl<T> Queue<T>
where
    T: 'static,
{
    pub fn new() -> Self {
        let sentinel = Owned::new(Node::empty());
        let pin = Pin::fake();
        let ptr = sentinel.into_ptr(pin);
        let q = Queue {
            head: Atomic::null(),
            tail: Atomic::null(),
        };
        q.head.store(ptr, Relaxed);
        q.tail.store(ptr, Relaxed);
        q
    }

    pub fn push<'scope>(&self, t: T, _pin: Pin<'scope>) {
        let node = Owned::new(Node::new(t));
        let new_node = node.into_ptr(_pin);
        loop {
            let tail = self.tail.load(Acquire, _pin);
            let t = unsafe { tail.deref() };
            let next = t.next.load(Acquire, _pin);
            if unsafe { next.as_ref().is_some() } {
                // tail wasnt't tail after all.
                // We try to help out by moving the tail pointer
                // on queue to the real tail we've seen, which is `next`.
                let _ = self.tail.compare_and_set(tail, next, Release, _pin);
            } else {
                let succ = t.next
                    .compare_and_set(Ptr::null(), new_node, Release, _pin)
                    .is_ok();
                if succ {
                    // the CAS succeded, and the new node is linked into the list.
                    // Update `queue.tail`. If we fail here it's OK, since another
                    // thread could have helped by moving the tail pointer.
                    let _ = self.tail.compare_and_set(tail, new_node, Release, _pin);
                    break;
                }
            }
        }
    }

    pub fn pop<'scope>(&self, _pin: Pin<'scope>) -> Option<T> {
        let head: Ptr<Node<T>> = self.head.load(Acquire, _pin);
        let h: &Node<T> = unsafe { head.deref() };
        let next: Ptr<Node<T>> = h.next.load(Acquire, _pin);
        match unsafe { next.as_ref() } {
            Some(node) => unsafe {
                // NOTE(martin): We don't really return the correct node here:
                // we CAS the old sentinel node out, and make the first data
                // node the new sentinel node, but return the data of `node`,
                // instead of `head`. In other words, the data we return
                // belongs on the node that is the new sentinel node.
                //
                // Before:
                //
                //  HEAD --:
                //         |
                //         V
                //     !-----!   !-----!   !-----!
                //     |  xx |-->|  93 |-->|  5  |---|
                //     !-----!   !-----!   !-----!
                //
                // After:  (return Some(93))
                //
                //  HEAD -----------:
                //                  |
                //                  V
                //     !-----!   !-----!   !-----!
                //     |  xx |-->|  93 |-->|  5  |---|
                //     !-----!   !-----!   !-----!
                //
                // Remember that the first node is the sentinel node,
                // so its data isn't really in the queue.
                //
                // This is where we leak memory: when we CAS out `head`,
                // it is no longer reachable by the queue.
                let res = self.head.compare_and_set(head, next, Release, _pin);
                match res {
                    Ok(n) => {
                        _pin.add_garbage(head.into_owned());
                        Some(ManuallyDrop::into_inner(::std::ptr::read(&node.data)))
                    }
                    Err(e) => None,
                }
            },
            None => None,
        }
    }

    pub fn pop_if<'scope, F>(&self, f: F, _pin: Pin<'scope>) -> Option<T>
    where
        F: Fn(&T) -> bool,
    {
        let head: Ptr<Node<T>> = self.head.load(Acquire, _pin);
        let h: &Node<T> = unsafe { head.deref() };
        let next: Ptr<Node<T>> = h.next.load(Acquire, _pin);
        match unsafe { next.as_ref() } {
            Some(node) => {
                // This `unwrap` is alright, since we know that only
                // the sentinel node, `head` here, is the only node
                // with `data = None`.
                if f(&node.data) {
                    unsafe {
                        let res = self.head.compare_and_set(head, next, Release, _pin);
                        match res {
                            Ok(n) => {
                                let o = head.into_owned();
                                let mem = ::std::mem::transmute::<Owned<Node<T>>, usize>(o);
                                let o = ::std::mem::transmute::<usize, Owned<Node<T>>>(mem);
                                _pin.add_garbage(o);
                                Some(ManuallyDrop::into_inner(::std::ptr::read(&node.data)))
                            }
                            Err(e) => None,
                        }

                    }
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Count the number of elements in the queue.
    /// This is typically not a operation we need,
    /// but it is practical to have for testing
    /// purposes
    pub fn len<'scope>(&self, _pin: Pin<'scope>) -> usize {
        let mut len = 0;
        let mut node = unsafe { self.head.load(Acquire, _pin).deref() };
        while let Some(next) = unsafe { node.next.load(Relaxed, _pin).as_ref() } {
            node = next;
            len += 1;
        }
        len
    }

    /// Returns `true` if the queue is empty.
    pub fn is_empty<'scope>(&self, _pin: Pin<'scope>) -> bool {
        let head = self.head.load(Acquire, _pin);
        let h = unsafe { head.deref() };
        h.next.load(Acquire, _pin).is_null()
    }
}



#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug)]
    struct Payload {
        data: String,
    }

    impl Payload {
        fn new() -> Self {
            Self { data: "payload".to_string() }
        }
    }

    #[test]
    fn can_construct_queue() {
        pin(|pin| { let q: Queue<Payload> = Queue::new(); });
    }

    #[test]
    fn st_queue_push() {
        pin(|pin| {
            let q: Queue<Payload> = Queue::new();
            q.push(Payload::new(), pin);
            q.push(Payload::new(), pin);
            q.push(Payload::new(), pin);
        });
    }

    #[test]
    fn st_queue_push_pop() {
        pin(|pin| {
            let q: Queue<u32> = Queue::new();
            q.push(1, pin);
            let r = q.pop(pin);
            assert_eq!(r, Some(1));
            assert_eq!(q.pop(pin), None);
        })
    }

    #[test]
    fn st_queue_push_pop_many() {
        pin(|pin| {
            let q: Queue<u32> = Queue::new();
            for i in 0..100 {
                q.push(i, pin);
            }
            for i in 0..100 {
                assert_eq!(q.pop(pin), Some(i));
            }
            assert_eq!(q.pop(pin), None);
        });
    }

    #[test]
    fn st_queue_len() {
        pin(|pin| {
            let q: Queue<Payload> = Queue::new();
            for i in 0..10 {
                q.push(Payload::new(), pin);
            }
            assert_eq!(q.len(pin), 10);
        });
    }

    struct LargeStruct {
        b: [u8; 1024 * 4],
        c: String,
    }

    impl LargeStruct {
        fn new() -> Self {
            Self {
                b: [0; 1024 * 4],
                c: "asd".to_string(),
            }
        }
    }

    #[test]
    fn memory_usage() {
        let mut q: Queue<LargeStruct> = Queue::new();
        for i in 0..(1024 * 1024) {
            pin(|pin| {
                q.push(LargeStruct::new(), pin);
                q.pop(pin);
            })
        }
    }

    struct NoDrop;
    impl Drop for NoDrop {
        fn drop(&mut self) {
            panic!("did drop!");
        }
    }

    #[test]
    fn no_drop() {
        let q = Queue::new();
        for i in 0..1024 {
            pin(|pin| {
                q.push(NoDrop, pin);
                let r = q.pop(pin).unwrap();
                ::std::mem::forget(r);
            })
        }
    }

    struct SingleDrop(bool);
    impl Drop for SingleDrop {
        fn drop(&mut self) {
            if (self.0) {
                panic!("Dropped before!");
            }
            self.0 = true;
        }
    }

    #[test]
    fn single_drop() {
        let q = Queue::new();
        for i in 0..1024 {
            pin(|pin| {
                q.push(SingleDrop(false), pin);
                q.pop(pin);
            })
        }
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct MustDrop<'a>(&'a AtomicUsize);

    impl<'a> Drop for MustDrop<'a> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    lazy_static! {
        static ref AtomicCount: AtomicUsize = {
            AtomicUsize::new(0)
        };
    }

    #[test]
    fn do_drop() {
        let q = Queue::new();
        let iters = 1024;
        for i in 0..iters {
            let q = &q;
            pin(move |pin| {
                q.push(MustDrop(&AtomicCount), pin);
                q.pop(pin);
            });
        }
        assert_eq!(AtomicCount.load(Ordering::SeqCst), iters);
    }
}
