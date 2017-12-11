use std::sync::atomic::Ordering::{SeqCst};
use super::atomic::{Owned, Atomic, Ptr};

use std::mem::ManuallyDrop;

#[derive(Debug)]
pub struct Node<T> {
    data: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

#[derive(Debug)]
pub struct List<T> {
    head: Atomic<Node<T>>,
}

impl<T> Node<T> {
    fn new(data: T) -> Self {
        Self {
            data: ManuallyDrop::new(data),
            next: Atomic::null(),
        }
    }
}

impl<T> List<T> {
    pub fn new() -> Self {
        Self { head: Atomic::null() }
    }

    /// Insert into the head of the list
    pub fn insert(&self, data: T, node_ptr: Option<*mut Owned<Node<T>>>) {
        let curr_ptr: Ptr<Node<T>> = Owned::new(Node::new(data)).into_ptr();
        if let Some(node_ptr) = node_ptr {
            unsafe {
                ::std::ptr::write(node_ptr, curr_ptr.clone().into_owned());
            }
        }
        let curr: &Node<T> = unsafe { curr_ptr.deref() };
        let mut head = self.head.load(SeqCst);
        loop {
            curr.next.store(head, SeqCst);
            let res = self.head.compare_and_set(head, curr_ptr, SeqCst);
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
        let head = self.head.load(SeqCst);
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

    /// Removes and returns the first element of the list, if any.
    pub fn remove_front(&self) -> Option<T> {
        let mut head_ptr: Ptr<Node<T>> = self.head.load(SeqCst);
        'outer: loop {
            if head_ptr.is_null() {
                return None;
            }
            let head: &Node<T> = unsafe { head_ptr.deref() };
            let next = head.next.load(SeqCst);
            if next.tag() != 0 {
                head_ptr = self.head.load(SeqCst);
                continue 'outer;
            }
            let tag_res = head.next.compare_and_set(next, next.with_tag(1), SeqCst);
            if tag_res.is_err() {
                continue 'outer;
            }
            match self.head.compare_and_set(head_ptr, next, SeqCst) {
                Ok(()) => {
                    let data = unsafe {::std::ptr::read(&head.data)};
                    // leak node
                    return Some(ManuallyDrop::into_inner(data))
                }
                Err(new_head) => {
                    let _res = head.next.compare_and_set(
                        next.with_tag(1),
                        next,
                        SeqCst
                    );
                    head_ptr = new_head;
                }
            }
        }
    }
}

impl<T: PartialEq> List<T> {
    /// Return `true` if the list contains the given value.
    pub fn contains(&self, value: &T) -> bool {
        'outer: loop {
            let mut node_ptr = self.head.load(SeqCst);
            let mut node;

            while !node_ptr.is_null() {
                node = unsafe { node_ptr.deref() };
                if *node.data == *value {
                    return true;
                }
                node_ptr = node.next.load(SeqCst);
                if node_ptr.tag() != 0 {
                    // restart, as we're being (or has been) removed
                    continue 'outer;
                }
            }
            return false
        }
    }

    /// Remove the first node in the list where `node.data == key`
    ///
    /// Note that this method causes the list to not be lock-free, since
    /// threads wanting to insert a node after this or remove the next node
    /// will be stuck forever if a thread tags the current node and then dies.
    pub fn remove(&self, value: &T) -> Option<T> {
        // Rust does not have tail-call optimization guarantees, so we have to use a loop here, in
        // order not to blow the stack.
        'outer: loop {
            let mut current_atomic_ptr = &self.head;

            let mut current_ptr = current_atomic_ptr.load(SeqCst);
            if current_ptr.is_null() {
                return None;
            }
            let mut current_node: &Node<T>;

            loop {
                current_node = unsafe { current_ptr.deref() };

                if *current_node.data == *value {
                    // Now we want to remove the current node from the list.  We first need to mark
                    // this node as 'to-be-deleted', by tagging its next pointer. When doing this,
                    // we avoid that other threads are inserting something after the current node,
                    // and us swinging the `next` pointer of `previous` to the old `next` of the
                    // current node.
                    let next_ptr = current_node.next.load(SeqCst).with_tag(0);
                    if current_node
                        .next
                        .compare_and_set(next_ptr, next_ptr.with_tag(1), SeqCst)
                        .is_err()
                    {
                        // Failed to mark the current node. Restart.
                        continue 'outer;
                    };
                    let res = current_atomic_ptr.compare_and_set(current_ptr.with_tag(0), next_ptr, SeqCst);
                    match res {
                        Ok(_) => unsafe {
                            // Now `current_node` is not reachable from the list.
                            let data = ::std::ptr::read(&current_node.data);
                            // leak node
                            return Some(ManuallyDrop::into_inner(data));
                        }
                        Err(_) => {
                            // Some new node in inserted behind us.
                            // Unmark and restart.
                            let _res = current_node.next.compare_and_set(
                                next_ptr.with_tag(1),
                                next_ptr,
                                SeqCst,
                            );
                            continue 'outer;
                        }
                    }
                } else {
                    current_atomic_ptr = &current_node.next;
                    current_ptr = current_node.next.load(SeqCst);
                    if current_ptr.tag() != 0 {
                        // Some other thread have deleted us! This means that the next node might
                        // have already been free'd.
                        continue 'outer;
                    }

                    if current_ptr.is_null() {
                        // we've reached the end of the list, without finding our value.
                        return None;
                    }
                }
            }
        }
    }
}
