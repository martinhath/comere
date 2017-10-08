use std::sync::atomic::Ordering::{Relaxed, Release, SeqCst};
use super::atomic::{Owned, Atomic, Ptr};

// Avoid debug and cmp generic problems for now
type T = u32;

#[derive(Debug)]
pub struct Node<T> {
    data: T,
    next: Atomic<Node<T>>,
}

#[derive(Debug)]
pub struct List {
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

impl List {
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
                Ok(_) => {
                    return;
                }
                Err(new_head) => {
                    head = new_head;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Relaxed);
        let ret = head.is_null();
        if !ret {
            let mut node = unsafe { head.deref() };
            let mut next = node.next.load(SeqCst);
            while !next.is_null() {
                node = unsafe { next.deref() };
                next = node.next.load(SeqCst);
            }
        }
        ret
    }

    /// Return `true` if the list contains the given value.
    pub fn contains(&self, value: &T) -> bool {
        let previous_atomic: &Atomic<Node<T>> = &self.head;
        let mut node_ptr = self.head.load(Relaxed);
        let mut node;
        while !node_ptr.is_null() {
            node = unsafe { node_ptr.deref() };
            if node.data == *value {
                return true;
            }
            node_ptr = node.next.load(Relaxed);
        }
        false
    }

    /// Remove the first node in the list where `node.data == key`
    ///
    /// Note that this method causes the list to not be lock-free, since
    /// threads wanting to insert a node after this or remove the next node
    /// will be stuck forever if a thread tags the current node and then dies.
    pub fn remove(&self, value: &T) -> bool {
        // Rust does not have tail-call optimization guarantees,
        // so we have to use a loop here, in order not to blow the stack.
        'outer: loop {
            let mut previous_node_ptr = &self.head;
            let mut current_ptr = self.head.load(SeqCst);
            if current_ptr.is_null() {
                return false;
            }
            let mut current: &Node<T> = unsafe { current_ptr.deref() };

            loop {
                let next_ptr = current.next.load(SeqCst).with_tag(0);
                if current.data == *value {
                    // Now we want to remove the current node from the list.
                    // We first need to mark this node as 'to-be-deleted',
                    // by tagging its next pointer. When doing this, we avoid
                    // that other threads are inserting something after the
                    // current node, and us swinging the `next` pointer of
                    // `previous` to the old `next` of the current node.
                    let next_ptr = current.next.load(SeqCst);
                    if current
                        .next
                        .compare_and_set(next_ptr, next_ptr.with_tag(1), SeqCst)
                        .is_err()
                    {
                        // Failed to mark the current node. Restart.
                        continue 'outer;
                    };
                    let res = previous_node_ptr.compare_and_set(current_ptr, next_ptr, SeqCst);
                    match res {
                        Ok(_) => return true,
                        Err(_) => {
                            let pnp = previous_node_ptr.load(SeqCst);
                            // Some new node in inserted behind us.
                            // Unmark and restart.
                            let res = current.next.compare_and_set(
                                next_ptr.with_tag(1),
                                next_ptr,
                                SeqCst,
                            );
                            if res.is_err() {
                                panic!("coulnd't untag ptr. WTF?");
                            }
                            continue 'outer;
                        }
                    }
                } else {
                    previous_node_ptr = &current.next;
                    current_ptr = next_ptr;
                    if current_ptr.is_null() {
                        // we've reached the end of the list, without finding our value.
                        return false;
                    }
                    current = unsafe { current_ptr.deref() };
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
}
