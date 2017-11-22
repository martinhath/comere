use std::sync::atomic::Ordering::{Relaxed, Release, SeqCst};
use std::mem::{drop, ManuallyDrop};

use super::atomic::{Owned, Atomic, Ptr, HazardPtr};

pub struct Node<T> {
    pub data: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

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

    fn data_ptr(&self) -> Ptr<T> {
        let t: &T = &*self.data;
        Ptr::from_raw(t as *const T)
    }
}

impl<T> List<T>
where
    T: 'static,
{
    pub fn new() -> Self {
        Self { head: Atomic::null() }
    }

    /// Insert into the head of the list
    pub fn insert(&self, data: T) -> Ptr<Node<T>> {
        let node = Node::new(data);
        let curr_ptr: Ptr<Node<T>> = Owned::new(node).into_ptr();
        let data_ptr: Ptr<T> = {
            let node: &Node<T> = unsafe { curr_ptr.deref() };
            Ptr::from_raw(node.data_ptr().as_raw())
        };
        let curr: &Node<T> = unsafe { curr_ptr.deref() };
        let mut head = self.head.load(Relaxed);
        loop {
            curr.next.store(head, Relaxed);
            let res = self.head.compare_and_set(head, curr_ptr, Release);
            match res {
                Ok(_) => {
                    return curr_ptr;
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

    /// Removes and returns the first element of the list, if any.
    pub fn remove_front(&self) -> Option<T> {
        let mut head_ptr: Ptr<Node<T>> = self.head.load(SeqCst);
        loop {
            if head_ptr.is_null() {
                return None;
            }
            let head_hp = head_ptr.hazard();
            {
                if self.head.load(SeqCst) != head_ptr {
                    drop(head_hp);
                    return self.remove_front();
                }
            }
            let head: &Node<T> = unsafe { head_ptr.deref() };
            let next = head.next.load(Relaxed);
            match self.head.compare_and_set(head_ptr, next, SeqCst) {
                Ok(()) => {
                    head_hp.wait();
                    // Now the head is made unreachable from the queue, and no thread has marked
                    // the pointer in the hazard list. Then we have exclusive access to it. Read
                    // the data, and free the node.
                    let data = unsafe {::std::ptr::read(&head.data)};
                    unsafe {
                        // Since we have made the node unreachable, and no thread has registered
                        // it as hazardous, it is safe to free.
                        head_hp.free();
                    }
                    return Some(ManuallyDrop::into_inner(data));
                }
                Err(new_head) => {
                    head_ptr = new_head;
                }
            }
        }
    }

    /// Return an iterator to the list.
    pub fn iter(&self) -> Iter<T> {
        Iter {
            node: self.head.load(SeqCst),
            _marker: ::std::marker::PhantomData,
        }
    }
}

impl<T> List<T>
where
    T: 'static + Eq + ::std::fmt::Debug,
{
    /// Remove the first node in the list where `node.data == key`
    ///
    /// Note that this method causes the list to not be lock-free, since threads wanting to insert
    /// a node after this or remove the next node will be stuck forever if a thread tags the
    /// current node and then dies.
    ///
    /// NOTE(6.11.17): Maybe we can fix this by having other operation help out deleting the note
    /// if they ever see one?
    ///
    /// TODO(6.11.17): Return the value! We need to do this, since it may have to be dropped. Now
    /// we just leak the value!
    pub fn remove(&self, value: &T) -> Option<Owned<Node<T>>> {
        // Rust does not have tail-call optimization guarantees, so we have to use a loop here, in
        // order not to blow the stack.
        'outer: loop {
            let mut previous_node_ptr = &self.head;
            let mut current_ptr = previous_node_ptr.load(SeqCst);
            if current_ptr.is_null() {
                return None;
            }
            let mut current: &Node<T>;
            let mut prev_handle: Option<HazardPtr<::hp::list::Node<T>>> = None;

            loop {
                let curr_hp = current_ptr.hazard();
                // validate
                {
                    if previous_node_ptr.load(SeqCst) != current_ptr {
                        drop(curr_hp); // explicit drop here. Do we need it?
                        // println!("remove::validate failed. restart.");
                        continue 'outer;
                    }
                }
                current = unsafe { current_ptr.deref() };

                if *current.data == *value {
                    // Now we want to remove the current node from the list.  We first need to mark
                    // this node as 'to-be-deleted', by tagging its next pointer. When doing this,
                    // we avoid that other threads are inserting something after the current node,
                    // and us swinging the `next` pointer of `previous` to the old `next` of the
                    // current node.
                    let next_ptr = current.next.load(SeqCst);
                    if current
                        .next
                        .compare_and_set(next_ptr, next_ptr.with_tag(1), SeqCst)
                        .is_err()
                    {
                        // Failed to mark the current node. Restart.
                        // println!("failed to mark current node. restart.");
                        continue 'outer;
                    };
                    let res = previous_node_ptr.compare_and_set(current_ptr, next_ptr, SeqCst);
                    match res {
                        Ok(_) => {
                            // Now `current` is not reachable from the list.
                            // TODO(6.11.17): have a way to do this in one operation?
                            curr_hp.wait();
                            return Some(unsafe { current_ptr.into_owned() });
                        }
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
                                // This might hit if we decide to make other threads help out on
                                // deletion.
                                panic!("couldn't untag ptr. WTF?");
                            }
                            continue 'outer;
                        }
                    }
                } else {
                    previous_node_ptr = &current.next;
                    current_ptr = current.next.load(SeqCst).with_tag(0);
                    prev_handle.take().map(::std::mem::drop);
                    prev_handle = Some(curr_hp);

                    if current_ptr.is_null() {
                        // we've reached the end of the list, without finding our value.
                        return None;
                    }
                }
            }
        }
    }

    /// Return `true` if the list contains the given value.
    pub fn contains(&self, value: &T) -> bool {
        let previous_atomic: &Atomic<Node<T>> = &self.head;
        let mut node_ptr = self.head.load(Relaxed);
        let mut node;
        while !node_ptr.is_null() {
            node = unsafe { node_ptr.deref() };
            if *node.data == *value {
                return true;
            }
            node_ptr = node.next.load(Relaxed);
        }
        false
    }
}

/// An iterator for `List`
pub struct Iter<'a, T: 'a> {
    node: Ptr<'a, Node<T>>,
    _marker: ::std::marker::PhantomData<&'a ()>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        // TODO: this also needs to use HP!
        if let Some(node) = unsafe { self.node.as_ref() } {
            self.node = node.next.load(SeqCst);
            Some(&node.data)
        } else {
            None
        }
    }
}

impl<T> Drop for List<T> {
    fn drop(&mut self) {
        unsafe {
            let mut ptr = self.head.load(SeqCst);
            if ptr.is_null() {
                return;
            }
            // The first node has no valid data - this is already returned by `pop`, and if nothing
            // is popped it is uninitialized data.
            let node = ptr.into_owned();
            let next = node.next.load(SeqCst);
            ::std::mem::drop(node);
            ptr = next;
            while !ptr.is_null() {
                let mut node: Owned<Node<T>> = ptr.into_owned();
                let next = node.next.load(SeqCst);
                ManuallyDrop::drop(&mut (*node).data);
                ::std::mem::drop(node);
                ptr = next;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::{thread_rng, Rng};

    use std::thread::spawn;
    use std::sync::Arc;

    #[test]
    fn insert() {
        let list = List::new();
        const N: usize = 32;
        for i in 0..N {
            assert!(!list.insert(i).is_null());
        }
        assert_eq!(list.iter().count(), N);
    }

    #[test]
    fn remove_front() {
        let list = List::new();
        const N: usize = 32;
        for i in 0..N {
            assert!(!list.insert(i).is_null());
        }
        for i in (0..N).rev() {
            let ret = list.remove_front();
            assert_eq!(ret, Some(i));
        }
        assert_eq!(list.iter().next(), None);
    }

    #[test]
    fn remove() {
        const N_THREADS: usize = 4;
        const N: usize = 123; //1024 * 32; // * 1024;
        const MAX: usize = 1024;

        let list: Arc<List<usize>> = Arc::new(List::new());

        let mut rng = thread_rng();

        // Prefill with some values
        for i in 0..N {
            list.insert(rng.gen_range(0, MAX));
        }
        assert_eq!(list.iter().count(), N);

        let threads = (0..N_THREADS)
            .map(|thread_id| {
                let list = list.clone();
                spawn(move || {
                    let removals = [0; N];
                    let mut rng = thread_rng();
                    for i in 0..N {
                        let a = rng.gen_range(0, MAX);
                        list.remove(&a);
                        let b = rng.gen_range(0, MAX);
                        list.insert(b);
                    }
                })
            })
            .collect::<Vec<_>>();

        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }

        while let Some(_) = list.remove_front() {}
        assert!(list.is_empty());
    }

    #[test]
    fn stress_test() {
        const N_THREADS: usize = 4;
        const N: usize = 1024 * 1024;

        // NOTE: we can replace the arc problems by using crossbeams's `scope`,
        // instead of `thread::spawn`.
        let source = Arc::new(List::new());
        let sink = Arc::new(List::new());

        // Pre-fill the source with stuff
        for n in 0..N {
            source.insert(n);
        }

        let threads = (0..N_THREADS)
            .map(|thread_id| {
                let source = source.clone();
                let sink = sink.clone();
                spawn(move || {
                    let source = source;
                    let sink = sink;

                    // Move stuff from source to sink
                    while let Some(i) = source.remove_front() {
                        sink.insert(i);
                    }
                })
            })
            .collect::<Vec<_>>();

        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }
        let mut v = Vec::with_capacity(N);
        while let Some(i) = sink.remove_front() {
            v.push(i);
        }
        v.sort();
        for (i, n) in v.into_iter().enumerate() {
            assert_eq!(i, n);
        }
    }
}
