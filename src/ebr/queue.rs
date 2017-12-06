#[allow(unused_variables)]
#[allow(dead_code)]
/// A Michael-Scott Queue.

use std::sync::atomic::Ordering::{Relaxed, Acquire, SeqCst};
use std::default::Default;
use std::mem::ManuallyDrop;

use super::Pin;

use super::atomic::{Owned, Atomic, Ptr};


#[derive(Debug)]
pub struct Queue<T> {
    head: Atomic<Node<T>>,
    tail: Atomic<Node<T>>,
}

impl<T> Drop for Queue<T> {
    // TODO: find out what happens if we share the queue between threads. Is it possible that the
    // threads is dropped in multiple threads? Also, if we drop the queue when other threads are
    // reading the stuff, we should add the nodes to garbage. However, we also need to drop the
    // data. What to do?
    fn drop(&mut self) {
        unsafe {
            let pin = Pin::fake();
            let mut ptr = self.head.load(SeqCst, pin);
            // The first node has no valid data - this is already returned by `pop`, and if nothing
            // is popped it is uninitialized data.
            let node = ptr.into_owned();
            let next = node.next.load(SeqCst, pin);
            ::std::mem::drop(node);
            ptr = next;
            while !ptr.is_null() {
                let mut node = ptr.into_owned();
                let next = node.next.load(SeqCst, pin);
                ManuallyDrop::drop(node.data_mut());
                ::std::mem::drop(node);
                ptr = next;
            }
        }
    }
}

#[derive(Debug)]
pub struct Node<T> {
    // We don't want to drop the data of the node when we drop the node itself; dropping the data
    // is the responsibility of the caller.
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

    fn data_mut(&mut self) -> &mut ManuallyDrop<T> {
        &mut self.data
    }
}

impl<T> Queue<T>
where
    T: 'static + ::std::fmt::Debug,
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
            let tail = self.tail.load(SeqCst, _pin);
            let t = unsafe { tail.deref() };
            let next = t.next.load(SeqCst, _pin);
            if unsafe { next.as_ref().is_some() } {
                // tail wasnt't tail after all.
                // We try to help out by moving the tail pointer
                // on queue to the real tail we've seen, which is `next`.
                let _ = self.tail.compare_and_set(tail, next, SeqCst, _pin);
            } else {
                let succ = t.next
                    .compare_and_set(Ptr::null(), new_node, SeqCst, _pin)
                    .is_ok();
                if succ {
                    // the CAS succeded, and the new node is linked into the list.
                    // Update `queue.tail`. If we fail here it's OK, since another
                    // thread could have helped by moving the tail pointer.
                    let _ = self.tail.compare_and_set(tail, new_node, SeqCst, _pin);
                    break;
                }
            }
        }
    }

    pub fn pop<'scope>(&self, _pin: Pin<'scope>) -> Option<T> {
        'outer: loop {
            let head: Ptr<Node<T>> = self.head.load(SeqCst, _pin);
            let h: &Node<T> = unsafe { head.deref() };
            let next: Ptr<Node<T>> = h.next.load(SeqCst, _pin);
            match unsafe { next.as_ref() } {
                Some(node) => unsafe {
                    // NOTE(martin): We don't really return the correct node here: we CAS the old
                    // sentinel node out, and make the first data node the new sentinel node, but
                    // return the data of `node`, instead of `head`. In other words, the data we return
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
                    // Remember that the first node is the sentinel node, so its data isn't really in
                    // the queue.
                    //
                    // This is where we leak memory: when we CAS out `head`, it is no longer reachable
                    // by the queue.
                    let res = self.head.compare_and_set(head, next, SeqCst, _pin);
                    match res {
                        Ok(()) => {
                        let data = ::std::ptr::read(&node.data);
                        _pin.add_garbage(head.into_owned());
                        return Some(ManuallyDrop::into_inner(data));
                    }
                        Err(e) => continue 'outer,
                    }
                },
                None => return None,
            }
        }
    }

    /// Pop the first element of the queue if `F(head)` evaluates
    /// to `true`.
    pub fn pop_if<'scope, F>(&self, f: F, _pin: Pin<'scope>) -> Option<T>
    where
        F: Fn(&T) -> bool,
    {
        let head: Ptr<Node<T>> = self.head.load(SeqCst, _pin);
        let h: &Node<T> = unsafe { head.deref() };
        let next: Ptr<Node<T>> = h.next.load(SeqCst, _pin);
        match unsafe { next.as_ref() } {
            Some(node) => {
                let data = unsafe { ::std::ptr::read(&node.data) };
                if f(&*data) {
                    unsafe {
                        let res = self.head.compare_and_set(head, next, SeqCst, _pin);
                        match res {
                            Ok(()) => {
                                _pin.add_garbage(head.into_owned());
                                Some(ManuallyDrop::into_inner(data))
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

    use super::super::pin;

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

    #[derive(Debug)]
    struct NoDrop;
    impl Drop for NoDrop {
        fn drop(&mut self) {
            panic!("did drop!");
        }
    }

    #[test]
    fn no_drop() {
        let q = Queue::new();
        let iters = 1024 * 1024;
        for i in 0..iters {
            pin(|pin| {
                q.push(NoDrop, pin);
                let r = q.pop(pin).unwrap();
                ::std::mem::forget(r);
            })
        }
    }

    #[derive(Debug)]
    struct SingleDrop(bool);
    impl Drop for SingleDrop {
        fn drop(&mut self) {
            if self.0 {
                panic!("Dropped before!");
            }
            self.0 = true;
        }
    }

    #[test]
    fn single_drop() {
        let q = Queue::new();
        let iters = 1024 * 1024;
        for i in 0..iters {
            pin(|pin| {
                q.push(SingleDrop(false), pin);
                q.pop(pin);
            })
        }
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Debug)]
    struct MustDrop<'a>(&'a AtomicUsize);

    impl<'a> Drop for MustDrop<'a> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    lazy_static! {
        static ref ATOMIC_COUNT: AtomicUsize = {
            AtomicUsize::new(0)
        };
    }

    #[test]
    fn do_drop() {
        let q = Queue::new();
        let iters = 1024 * 1024;
        for i in 0..iters {
            let q = &q;
            pin(move |pin| {
                q.push(MustDrop(&ATOMIC_COUNT), pin);
                q.pop(pin);
            });
        }
        assert_eq!(ATOMIC_COUNT.load(Ordering::SeqCst), iters);
    }


    use std::thread::spawn;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn is_unique_receiver() {
        const N_THREADS: usize = 16;
        const ELEMS: usize = 1024 * 512;

        let q = Arc::new(Queue::new());
        // Markers to check.
        let markers = Arc::new(
            (0..ELEMS)
                .map(|_| AtomicBool::new(false))
                .collect::<Vec<_>>(),
        );
        // Fill the queue with all numbers
        pin(|pin| for i in 0..ELEMS {
            q.push(i, pin);
        });

        // Each threads pops something until the queue is empty, and CASes the element they got
        // back in `markers`.  If any CAS fails, we've returned the same element to two threads,
        // which should not happen, since all nubmers are only once in the queue.
        let threads = (0..N_THREADS)
            .map(|i| {
                let markers = markers.clone();
                let q = q.clone();
                spawn(move || while let Some(i) = pin(|pin| q.pop(pin)) {
                    assert!(i < ELEMS);
                    let ret = markers[i].compare_and_swap(false, true, Ordering::SeqCst);
                    assert_eq!(ret, false);
                })
            })
            .collect::<Vec<_>>();

        // Wait for all threads to finish
        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }

        // Check that all elements were returned from the queue
        for m in markers.iter() {
            assert!(m.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn is_unique_receiver_if() {
        const N_THREADS: usize = 16;
        const ELEMS: usize = 1024 * 512;

        let q = Arc::new(Queue::new());
        // Markers to check.
        let markers = Arc::new(
            (0..ELEMS)
                .map(|_| AtomicBool::new(false))
                .collect::<Vec<_>>(),
        );
        // Fill the queue with all numbers
        pin(|pin| for i in 0..ELEMS {
            q.push(i, pin);
        });

        // Each threads pops something until the queue is empty,
        // and CASes the element they got back in `markers`.
        // If any CAS fails, we've returned the same element to two
        // threads, which should not happen, since all nubmers are only
        // once in the queue.
        let threads = (0..N_THREADS)
            .map(|i| {
                let markers = markers.clone();
                let q = q.clone();
                spawn(move || while let Some(i) = pin(
                    |pin| q.pop_if(|_| true, pin),
                )
                {
                    let ret = markers[i].compare_and_swap(false, true, Ordering::SeqCst);
                    assert_eq!(ret, false);
                })
            })
            .collect::<Vec<_>>();

        // Wait for all threads to finish
        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }

        // Check that all elements were returned from the queue
        for m in markers.iter() {
            assert!(m.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn stress_test() {
        const N_THREADS: usize = 16;
        const N: usize = 1024 * 1024;

        let source = Arc::new(Queue::new());
        let sink = Arc::new(Queue::new());

        pin(|pin| for n in 0..N {
            source.push(n, pin);
        });

        let threads = (0..N_THREADS)
            .map(|thread_id| {
                let source = source.clone();
                let sink = sink.clone();
                spawn(move || {
                    let source = source;
                    let sink = sink;

                    while let Some(i) = pin(|pin| source.pop(pin)) {
                        pin(|pin| sink.push(i, pin));
                    }
                })
            })
            .collect::<Vec<_>>();

        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }
        let mut v = Vec::with_capacity(N);
        pin(|pin| while let Some(i) = sink.pop(pin) {
            v.push(i);
        });
        v.sort();
        for (i, n) in v.into_iter().enumerate() {
            assert_eq!(i, n);
        }
    }
}
