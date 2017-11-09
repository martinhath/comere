#[allow(unused_variables)]
#[allow(dead_code)]
/// A Michael-Scott Queue.

use std::sync::atomic::Ordering::{Release, Relaxed, Acquire, SeqCst};
use std::default::Default;
use std::mem::{ManuallyDrop, drop};

use super::*;

use super::atomic::{Owned, Atomic, Ptr};

#[derive(Debug)]
pub struct Queue<T> {
    head: Atomic<Node<T>>,
    tail: Atomic<Node<T>>,
}

#[derive(Debug)]
pub struct Node<T> {
    data: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

impl<T> Node<T> {
    pub fn new(data: T) -> Self {
        Self {
            data: ManuallyDrop::new(data),
            next: Default::default(),
        }
    }

    pub fn empty() -> Self {
        Self {
            data: unsafe { ::std::mem::uninitialized() },
            next: Default::default(),
        }
    }
}

impl<T> Queue<T> {
    pub fn new() -> Self {
        let sentinel = Owned::new(Node::empty());
        let ptr = sentinel.into_ptr();
        let q = Queue {
            head: Atomic::null(),
            tail: Atomic::null(),
        };
        q.head.store(ptr, Relaxed);
        q.tail.store(ptr, Relaxed);
        q
    }

    pub fn push(&self, t: T) {
        let node = Owned::new(Node::new(t));
        let new_node = node.into_ptr();
        loop {
            let tail: Ptr<Node<T>> = self.tail.load(Acquire);
            let tail_hp = register_hp(tail.as_raw());
            {
                if self.tail.load(Acquire) != tail {
                    drop(tail_hp);
                    continue;
                }
            }
            let t = unsafe { tail.deref() };
            let next = t.next.load(Acquire);
            if unsafe { next.as_ref().is_some() } {
                // tail wasnt't tail after all.
                // We try to help out by moving the tail pointer
                // on queue to the real tail we've seen, which is `next`.
                let _ = self.tail.compare_and_set(tail, next, Release);
            } else {
                let succ = t.next
                    .compare_and_set(Ptr::null(), new_node, Release)
                    .is_ok();
                if succ {
                    // the CAS succeded, and the new node is linked into the list.
                    // Update `queue.tail`. If we fail here it's OK, since another
                    // thread could have helped by moving the tail pointer.
                    let _ = self.tail.compare_and_set(tail, new_node, Release);
                    break;
                }
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        let head: Ptr<Node<T>> = self.head.load(Acquire);
        let head_hp = register_hp(head.as_raw()).expect("Failed to register HP");
        // validate:
        {
            let new_head: Ptr<Node<T>> = self.head.load(Acquire);
            // If head changed after registering, restart.
            if head != new_head {
                drop(head_hp);
                return self.pop();
            }
        }
        let h: &Node<T> = unsafe { head.deref() };
        let next: Ptr<Node<T>> = h.next.load(Acquire);
        if next.is_null() {
            return None;
        }
        let next_hp = register_hp(next.as_raw()).expect("Failed to register HP");
        {
            if h.next.load(Acquire) != next {
                drop(next_hp);
                return self.pop();
            }
        }
        // Register the `next` pointer as hazardous
        match unsafe { next.as_ref() } {
            Some(node) => unsafe {
                // NOTE(martin): We don't really return the correct node here:
                // we CAS the old sentinel node out, and make the first data
                // node the new sentinel node, but return the data of `node`,
                // instead of `head`. In other words, the data we return
                // belongs on the node that is the new sentinel node.
                let res = self.head.compare_and_set(head, next, SeqCst);
                match res {
                    Ok(()) => {
                        let ret = Some(ManuallyDrop::into_inner(::std::ptr::read(&node.data)));
                        drop(next_hp);
                        drop(head_hp);
                        // While someone is using the head pointer, keep it here.
                        while scan(head.as_raw()) {
                            ::std::thread::yield_now();
                        }
                        // Drop it when we can; `head` is no longer reachable.
                        ::std::mem::drop(head.to_owned());
                        ret
                    }
                    // TODO: we would rather want to loop here, instead of
                    // giving up if there is contention?
                    Err(e) => None,
                }
            },
            None => None,
        }
    }

    /// Count the number of elements in the queue.
    /// This is typically not a operation we need,
    /// but it is practical to have for testing
    /// purposes.
    pub fn len(&self) -> usize {
        let mut len = 0;
        let mut node = unsafe { self.head.load(Acquire).deref() };
        while let Some(next) = unsafe { node.next.load(Relaxed).as_ref() } {
            node = next;
            len += 1;
        }
        len
    }

    /// Returns `true` if the queue is empty.
    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Acquire);
        let h = unsafe { head.deref() };
        h.next.load(Acquire).is_null()
    }
}



#[cfg(test)]
mod test {
    use super::*;

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
        let q: Queue<Payload> = Queue::new();
    }

    #[test]
    fn st_queue_push() {
        let q: Queue<Payload> = Queue::new();
        q.push(Payload::new());
        q.push(Payload::new());
        q.push(Payload::new());
    }

    #[test]
    fn st_queue_push_pop() {
        let q: Queue<u32> = Queue::new();
        q.push(1);
        let r = q.pop();
        assert_eq!(r, Some(1));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn st_queue_push_pop_many() {
        let q: Queue<u32> = Queue::new();
        for i in 0..100 {
            q.push(i);
        }
        for i in 0..100 {
            assert_eq!(q.pop(), Some(i));
        }
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn st_queue_len() {
        let q: Queue<Payload> = Queue::new();
        for i in 0..10 {
            q.push(Payload::new());
        }
        assert_eq!(q.len(), 10);
    }

    struct LargeStruct {
        b: [u8; 1024 * 4],
    }

    impl LargeStruct {
        fn new() -> Self {
            Self { b: [0; 1024 * 4] }
        }
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
            q.push(NoDrop);
            let r = q.pop().unwrap();
            ::std::mem::forget(r);
        }
        // NoDrop panics on drop, so if we get here, it's good.
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
            q.push(SingleDrop(false));
            q.pop();
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
            q.push(MustDrop(&ATOMIC_COUNT));
            q.pop();
        }
        assert_eq!(ATOMIC_COUNT.load(Ordering::SeqCst), iters);
    }


    use std::thread::spawn;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn is_unique_receiver() {
        const N_THREADS: usize = 16;
        const ELEMS: usize = 512 * 512;

        let q = Arc::new(Queue::new());
        // Markers to check.
        let markers = Arc::new(
            (0..ELEMS)
                .map(|_| AtomicBool::new(false))
                .collect::<Vec<_>>(),
        );
        // Fill the queue with all numbers
        for i in 0..ELEMS {
            q.push(i);
        }

        // Each threads pops something until the queue is empty,
        // and CASes the element they got back in `markers`.
        // If any CAS fails, we've returned the same element to two
        // threads, which should not happen, since all nubmers are only
        // once in the queue.
        let threads = (0..N_THREADS)
            .map(|i| {
                let markers = markers.clone();
                let q = q.clone();
                spawn(move || while let Some(i) = q.pop() {
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

    // #[test]
    // fn is_unique_receiver_if() {
    //     const N_THREADS: usize = 16;
    //     const ELEMS: usize = 512 * 512;

    //     let q = Arc::new(Queue::new());
    //     // Markers to check.
    //     let markers = Arc::new(
    //         (0..ELEMS)
    //             .map(|_| AtomicBool::new(false))
    //             .collect::<Vec<_>>(),
    //     );
    //     // Fill the queue with all numbers
    //     for i in 0..ELEMS {
    //         q.push(i);
    //     }

    //     // Each threads pops something until the queue is empty,
    //     // and CASes the element they got back in `markers`.
    //     // If any CAS fails, we've returned the same element to two
    //     // threads, which should not happen, since all nubmers are only
    //     // once in the queue.
    //     let threads = (0..N_THREADS)
    //         .map(|i| {
    //             let markers = markers.clone();
    //             let q = q.clone();
    //             spawn(move || {
    //                 while let Some(i) = q.pop_if(|_| true) {
    //                     let ret = markers[i].compare_and_swap(false, true, Ordering::SeqCst);
    //                     assert_eq!(ret, false);
    //                 }
    //             })
    //         })
    //         .collect::<Vec<_>>();

    //     // Wait for all threads to finish
    //     for t in threads.into_iter() {
    //         assert!(t.join().is_ok());
    //     }

    //     // Check that all elements were returned from the queue
    //     for m in markers.iter() {
    //         assert!(m.load(Ordering::SeqCst));
    //     }
    // }

    #[test]
    fn stress_test() {
        const N_THREADS: usize = 16;
        const N: usize = 1024 * 1024;

        // NOTE: we can replace the arc problems by using crossbeams's `scope`,
        // instead of `thread::spawn`.
        let source = Arc::new(Queue::new());
        let sink = Arc::new(Queue::new());

        // Pre-fill the source with stuff
        for n in 0..N {
            source.push(n);
        }

        let threads = (0..N_THREADS)
            .map(|thread_id| {
                let source = source.clone();
                let sink = sink.clone();
                spawn(move || {
                    register_thread(thread_id);
                    let source = source;
                    let sink = sink;

                    // Move stuff from source to sink
                    while let Some(i) = source.pop() {
                        sink.push(i);
                    }
                })
            })
            .collect::<Vec<_>>();

        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }
        let mut v = Vec::with_capacity(N);
        while let Some(i) = sink.pop() {
            v.push(i);
        }
        v.sort();
        for (i, n) in v.into_iter().enumerate() {
            assert_eq!(i, n);
        }
    }

    #[test]
    fn pop_if_push() {
        const N_THREADS: usize = 16;
        const N: usize = 1024 * 1024;

        let q = Arc::new(Queue::new());

        let threads = (0..N_THREADS)
            .map(|thread_id| {
                let q = q.clone();
                spawn(move || {
                    let push = thread_id % 2 == 0;

                    if push {
                        q.push(thread_id);
                    } else {
                        if let Some(i) = q.pop() {
                            // register
                        }
                    }
                })
            })
            .collect::<Vec<_>>();

        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }
    }
}
