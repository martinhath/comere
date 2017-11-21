#[allow(unused_variables)]
#[allow(dead_code)]
/// A Michael-Scott Queue.

use std::sync::atomic::Ordering::{Release, Relaxed, Acquire};
use std::default::Default;

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
    data: Option<T>,
    next: Atomic<Node<T>>,
}

impl<T> Node<T> {
    pub fn empty() -> Self {
        Self {
            data: None,
            next: Default::default(),
        }
    }

    fn new(t: T) -> Self {
        Self {
            data: Some(t),
            next: Default::default(),
        }
    }
}

impl<T> Queue<T> {
    pub fn new() -> Self {
        let sentinel = Owned::new(Node {
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

    // Enqueue, as described by MS. Performs the same as push (on x86).
    fn enqueue(&self, t: T) {
        let node = Owned::new(Node::new(t)).into_ptr();
        let mut tail;
        loop {
            tail = self.tail.load(Acquire);
            let t = unsafe { tail.deref() };
            let next = t.next.load(Acquire);
            if tail == self.tail.load(Acquire) {
                if next.is_null() {
                    let ret = t.next.compare_and_set(next, node, Release);
                    if ret.is_ok() {
                        break;
                    }
                } else {
                    self.tail.compare_and_set(tail, next, Release).ok();
                }
            }
        }
        self.tail.compare_and_set(tail, node, Release).ok();
    }

    pub fn push(&self, t: T, node_ptr: Option<*mut Owned<Node<T>>>) {
        let node = Owned::new(Node {
            data: Some(t),
            next: Default::default(),
        });
        let new_node = node.into_ptr();
        if let Some(node_ptr) = node_ptr {
            unsafe {
                ::std::ptr::write(node_ptr, new_node.clone().into_owned());
            }
        }
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
        let head: Ptr<Node<T>> = self.head.load(Acquire);
        let h: &Node<T> = unsafe { head.deref() };
        let next: Ptr<Node<T>> = h.next.load(Acquire);
        match unsafe { next.as_ref() } {
            Some(node) => unsafe {
                // NOTE(martin): We don't really return the correct node here:
                // we CAS the old sentinel node out, and make the first data
                // node the new sentinel node, but return the data of `node`,
                // instead of `head`. In other words, the data we return
                // belongs on the node that is the new sentinel node.
                //
                // This is where we leak memory: when we CAS out `head`,
                // it is no longer reachable by the queue.
                self.head
                    .compare_and_set(head, next, Release)
                    .ok()
                    .and_then(|_| ::std::ptr::read(&node.data))
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
        q.push(Payload::new(), None);
        q.push(Payload::new(), None);
        q.push(Payload::new(), None);
    }

    #[test]
    fn st_queue_push_pop() {
        let q: Queue<u32> = Queue::new();
        q.push(1, None);
        let r = q.pop();
        assert_eq!(r, Some(1));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn st_queue_push_pop_many() {
        let q: Queue<u32> = Queue::new();
        for i in 0..100 {
            q.push(i, None);
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
            q.push(Payload::new(), None);
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
}
