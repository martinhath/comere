/// This crate implements reference counting as a means of concurrent memory
/// reclamation for a Michael-Scott Queue. All nodes in the queue are allocated
/// on the heap, and we have pointers pointing at them. The nodes themselves
/// contain the data, the reference count, and the `next` node in the queue.
///
use std::sync::atomic::{AtomicUsize, AtomicPtr};
use std::marker::PhantomData;
use std::sync::atomic::Ordering::{self, SeqCst};
use std::default::Default;

#[derive(Debug)]
pub struct Queue<T> {
    head: AtomicPtr<Node<T>>,
    tail: AtomicPtr<Node<T>>,
}

#[derive(Debug)]
pub struct Node<T> {
    count: AtomicUsize,
    // TODO: Use `std::mem::ManuallyDrop` instead,
    // as in `crossbeam-epoch`
    data: Option<T>,
    next: AtomicPtr<Node<T>>,
}

impl<T> Node<T> {
    pub fn empty() -> Self {
        Self {
            count: AtomicUsize::new(0),
            data: None,
            next: Default::default(),
        }
    }
}

struct Owned<T> {
    ptr: usize,
    _marker: PhantomData<Box<T>>,
}

impl<T> Owned<T> {
    pub fn new(t: T) -> Self {
        let b = Box::into_raw(Box::new(t));
        Self {
            ptr: b as usize,
            _marker: PhantomData,
        }
    }

    pub fn as_mut_ptr(&self) -> *mut T {
        self.ptr as *mut T
    }
}

impl<T> Queue<T> {
    pub fn new() -> Self {
        let sentinel = Owned::new(Node {
            count: AtomicUsize::new(0),
            data: None,
            next: Default::default(),
        });
        let ptr = sentinel.as_mut_ptr();
        let q = Queue {
            head: AtomicPtr::new(ptr),
            tail: AtomicPtr::new(ptr),
        };
        q
    }

    pub fn push(&self, t: T) {
        let node = Owned::new(Node {
            count: AtomicUsize::new(0),
            data: Some(t),
            next: Default::default(),
        });
        loop {
            let tail: *mut Node<T> = self.tail.load(SeqCst);
        }
    }

    pub fn len(&self) -> usize {
        let mut len = 0;
        let node_ptr: *mut Node<T> = self.head.load(SeqCst);
        unsafe {
            let mut node = &*node_ptr;
            let mut next_ptr = node.next.load(SeqCst);
            while next_ptr as usize != 0 {
                node = &*next_ptr;
                next_ptr = node.next.load(SeqCst);
                len += 1;
            }
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
    fn queue_push() {
        let mut q: Queue<Payload> = Queue::new();
        q.push(Payload::new());
        q.push(Payload::new());
        q.push(Payload::new());
    }

    #[test]
    fn queue_len() {
        let mut q: Queue<Payload> = Queue::new();
        for i in 0..10 {
            q.push(Payload::new());
        }
        assert_eq!(q.len(), 10);
    }
}
