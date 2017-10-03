/// This crate implements reference counting as a means of concurrent memory
/// reclamation for a Michael-Scott Queue. All nodes in the queue are allocated
/// on the heap, and we have pointers pointing at them. The nodes themselves
/// contain the data, the reference count, and the `next` node in the queue.
///
// use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Release, Relaxed, Acquire};
use std::default::Default;

pub mod atomic;

use self::atomic::{Owned, Atomic, Ptr};

#[derive(Debug)]
pub struct Queue<T> {
    head: Atomic<Node<T>>,
    tail: Atomic<Node<T>>,
}

#[derive(Debug)]
pub struct Node<T> {
    // count: AtomicUsize,
    // TODO: Use `std::mem::ManuallyDrop` instead,
    // as in `crossbeam-epoch`
    data: Option<T>,
    next: Atomic<Node<T>>,
}

impl<T> Node<T> {
    pub fn empty() -> Self {
        Self {
            // count: AtomicUsize::new(0),
            data: None,
            next: Default::default(),
        }
    }
}

impl<T> Queue<T> {
    pub fn new() -> Self {
        let sentinel = Owned::new(Node {
            // count: AtomicUsize::new(0),
            data: None,
            next: Default::default(),
        });
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
        let node = Owned::new(Node {
            // count: AtomicUsize::new(0),
            data: Some(t),
            next: Default::default(),
        });
        let new_node = node.into_ptr();
        loop {
            let tail = self.tail.load(Acquire);
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
        let head = self.head.load(Acquire);
        let h = unsafe { head.deref() };
        let next = h.next.load(Acquire);
        match unsafe { next.as_ref() } {
            Some(node) => unsafe {
                self.head
                    .compare_and_set(head, next, Release)
                    .ok()
                    .and_then(|_| ::std::ptr::read(&node.data))
            },
            None => None,
        }
    }

    pub fn len(&self) -> usize {
        let mut len = 0;
        let mut node = unsafe { self.head.load(Acquire).deref() };
        while let Some(next) = unsafe { node.next.load(Relaxed).as_ref() } {
            node = next;
            len += 1;
        }
        len
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
        let mut q: Queue<Payload> = Queue::new();
        q.push(Payload::new());
        q.push(Payload::new());
        q.push(Payload::new());
    }

    #[test]
    fn st_queue_push_pop() {
        let mut q: Queue<u32> = Queue::new();
        q.push(1);
        let r = q.pop();
        assert_eq!(r, Some(1));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn st_queue_push_pop_many() {
        let mut q: Queue<u32> = Queue::new();
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
        let mut q: Queue<Payload> = Queue::new();
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

    #[test]
    fn memory_usage() {
        let mut q: Queue<LargeStruct> = Queue::new();
        // This will leak
        for i in 0..(1024 * 1024) {
            q.push(LargeStruct::new());
            q.pop();
        }
    }
}
