use std::sync::atomic::Ordering::{Relaxed, Release, Acquire};
use super::atomic::{Owned, Atomic, Ptr};

pub struct Node<T> {
    data: T,
    next: Atomic<Node<T>>,
}

pub struct List<T> {
    head: Atomic<Node<T>>,
}

impl<T> Node<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            next: Atomic::null(),
        }
    }
}

impl<T> List<T> {
    pub fn new() -> Self {
        Self { head: Atomic::null() }
    }

    /// Insert into the head of the list
    pub fn insert(&self, data: T) {
        let curr_ptr: Ptr<Node<T>> = Owned::new(Node::new(data)).into_ptr();
        let curr: &Node<T> = unsafe { curr_ptr.deref() };
        let mut head = self.head.load(Relaxed);
        loop {
            curr.next.store(head, Relaxed);
            let res = self.head.compare_and_set(head, curr_ptr, Release);
            match res {
                Ok(_) => return,
                Err(new_head) => {
                    head = new_head;
                }
            }
        }
    }

    /// Removes and returns the first element of the list, if any.
    pub fn remove_front(&self) -> Option<T> {
        let mut head_ptr: Ptr<Node<T>> = self.head.load(Relaxed);
        loop {
            if head_ptr.is_null() {
                return None;
            }
            let head: &Node<T> = unsafe { head_ptr.deref() };
            let next = head.next.load(Relaxed);
            match self.head.compare_and_set(head_ptr, next, Release) {
                Ok(()) => {
                    return Some(unsafe {::std::ptr::read(&head.data)})
                }
                Err(new_head) => {
                    head_ptr = new_head;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Relaxed);
        head.is_null()
    }
}

impl<T> List<T>
where
    T: Eq,
{
    /// Remove the first node in the list where `node.data == key`
    pub fn remove(&self, key: &T) -> bool {
        let mut node_atomic: &Atomic<Node<T>> = &self.head;
        let mut node_ptr: Ptr<Node<T>> = self.head.load(Relaxed);
        let mut node: &Node<T> = unsafe { node_ptr.deref() };
        loop {
            let next_ptr = node.next.load(Acquire);
            if node.data == *key {
                let res = node_atomic.compare_and_set(node_ptr, next_ptr, Relaxed);
                match res {
                    Ok(_) => return true,
                    Err(new_) => {}
                }
            } else {
                node_atomic = &node.next;
                node_ptr = next_ptr;
                if node_ptr.is_null() {
                    break;
                }
                node = unsafe { node_ptr.deref() };
            }
        }
        false
    }
}
