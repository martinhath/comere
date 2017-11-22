use std::sync::atomic::Ordering::{Relaxed, Release, SeqCst};
use super::atomic::{Owned, Atomic, Ptr};
use std::mem::ManuallyDrop;
use super::{Pin, pin};

pub struct Node<T> {
    pub data: ManuallyDrop<T>,
    pub next: Atomic<Node<T>>,
}

pub struct List<T>
where
    T: 'static,
{
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
    pub fn insert<'scope>(&self, data: T, _pin: Pin<'scope>) -> Ptr<'scope, Node<T>> {
        let curr_ptr: Ptr<Node<T>> = Owned::new(Node::new(data)).into_ptr(_pin);
        let curr: &Node<T> = unsafe { curr_ptr.deref() };
        let mut head = self.head.load(Relaxed, _pin);
        loop {
            curr.next.store(head, Relaxed);
            let res = self.head.compare_and_set(head, curr_ptr, Release, _pin);
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

    pub fn is_empty<'scope>(&self, _pin: Pin<'scope>) -> bool {
        let head = self.head.load(Relaxed, _pin);
        let ret = head.is_null();
        if !ret {
            let mut node = unsafe { head.deref() };
            let mut next = node.next.load(SeqCst, _pin);
            while !next.is_null() {
                node = unsafe { next.deref() };
                next = node.next.load(SeqCst, _pin);
            }
        }
        ret
    }

    /// Removes and returns the first element of the list, if any.
    pub fn remove_front<'scope>(&self, pin: Pin<'scope>) -> Option<T> {
        let mut head_ptr: Ptr<Node<T>> = self.head.load(Relaxed, pin);
        loop {
            if head_ptr.is_null() {
                return None;
            }
            let head: &Node<T> = unsafe { head_ptr.deref() };
            let next = head.next.load(Relaxed, pin).with_tag(0);
            match self.head.compare_and_set(head_ptr, next, Release, pin) {
                Ok(()) => {
                    let data = unsafe {::std::ptr::read(&head.data)};
                    // add garbage here!!
                    pin.add_garbage(unsafe{head_ptr.into_owned()});
                    return Some(ManuallyDrop::into_inner(data))
                }
                Err(new_head) => {
                    head_ptr = new_head;
                }
            }
        }
    }

    /// Return `true` if `f` evaluates to `true` for all the elements
    /// in the list
    pub fn all<'scope, F>(&self, f: F, _pin: Pin<'scope>) -> bool
    where
        F: Fn(&T) -> bool,
    {
        let previous_atomic: &Atomic<Node<T>> = &self.head;
        let mut node_ptr = self.head.load(Relaxed, _pin);
        let mut node;
        while !node_ptr.is_null() {
            node = unsafe { node_ptr.deref() };
            if !f(&node.data) {
                return false;
            }
            node_ptr = node.next.load(Relaxed, _pin);
        }
        true
    }

    /// Return an iterator to the list.
    pub fn iter<'scope>(&self, pin: Pin<'scope>) -> Iter<'scope, T> {
        Iter {
            node: self.head.load(SeqCst, pin),
            pin: pin,
            _marker: ::std::marker::PhantomData,
        }
    }
}

/// An iterator for `List`
pub struct Iter<'scope, T: 'scope> {
    node: Ptr<'scope, Node<T>>,
    pin: Pin<'scope>,
    _marker: ::std::marker::PhantomData<&'scope ()>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        // TODO: this also needs to use HP!
        if let Some(node) = unsafe { self.node.as_ref() } {
            self.node = node.next.load(SeqCst, self.pin);
            Some(&node.data)
        } else {
            None
        }
    }
}

impl<T: ::std::cmp::PartialEq> List<T> {
    /// Remove the first node in the list where `node.data == key`
    ///
    /// Note that this method causes the list to not be lock-free, since
    /// threads wanting to insert a node after this or remove the next node
    /// will be stuck forever if a thread tags the current node and then dies.
    pub fn remove<'scope>(&self, value: &T, _pin: Pin<'scope>) -> Option<Owned<Node<T>>> {
        // Rust does not have tail-call optimization guarantees,
        // so we have to use a loop here, in order not to blow the stack.
        'outer: loop {
            let mut previous_node_ptr = &self.head;
            let mut current_ptr = self.head.load(SeqCst, _pin);
            if current_ptr.is_null() {
                return None;
            }
            let mut current: &Node<T> = unsafe { current_ptr.deref() };

            loop {
                let next_ptr = current.next.load(SeqCst, _pin).with_tag(0);
                if *current.data == *value {
                    // Now we want to remove the current node from the list.
                    // We first need to mark this node as 'to-be-deleted',
                    // by tagging its next pointer. When doing this, we avoid
                    // that other threads are inserting something after the
                    // current node, and us swinging the `next` pointer of
                    // `previous` to the old `next` of the current node.
                    let next_ptr = current.next.load(SeqCst, _pin);
                    if current
                        .next
                        .compare_and_set(next_ptr, next_ptr.with_tag(1), SeqCst, _pin)
                        .is_err()
                    {
                        // Failed to mark the current node. Restart.
                        continue 'outer;
                    };
                    let res =
                        previous_node_ptr.compare_and_set(current_ptr, next_ptr, SeqCst, _pin);
                    match res {
                        Ok(_) => {
                            return Some(unsafe { current_ptr.into_owned() });
                        }
                        Err(_) => {
                            let pnp = previous_node_ptr.load(SeqCst, _pin);
                            // Some new node in inserted behind us.
                            // Unmark and restart.
                            let res = current.next.compare_and_set(
                                next_ptr.with_tag(1),
                                next_ptr,
                                SeqCst,
                                _pin,
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
                        return None;
                    }
                    current = unsafe { current_ptr.deref() };
                }
            }
        }
    }

    /// Return `true` if the list contains the given value.
    pub fn contains<'scope>(&self, value: &T, _pin: Pin<'scope>) -> bool {
        let previous_atomic: &Atomic<Node<T>> = &self.head;
        let mut node_ptr = self.head.load(Relaxed, _pin);
        let mut node;
        while !node_ptr.is_null() {
            node = unsafe { node_ptr.deref() };
            if *node.data == *value {
                return true;
            }
            node_ptr = node.next.load(Relaxed, _pin);
        }
        false
    }
}

impl<T> Drop for List<T>
where
    T: 'static,
{
    fn drop(&mut self) {
        unsafe {
            pin(|pin| {
                let head = {
                    let head_ptr: Ptr<Node<T>> = self.head.load(SeqCst, pin);
                    if head_ptr.is_null() {
                        return;
                    }
                    // TODO: this is debug only! remove
                    // swap some random ptr as head, so other threads fail.  If we get an error
                    // that `128` is not a valid pointer, we have problems.
                    let p = Ptr::from_raw(128 as *const Node<T>);
                    let ret = self.head.compare_and_set(head_ptr, p, SeqCst, pin);
                    if ret.is_err() {
                        // someone changed head - we are not alone.
                        panic!("we are fucked!");
                    }
                    head_ptr.into_owned()
                };
                // The first node has no valid data - this is already returned by `pop`, and if
                // nothing is popped it is uninitialized data.
                let next = head.next.load(SeqCst, pin);
                // when we drop, no other thread should operate on the list (?), which means that
                // all tags should be 0.
                assert_eq!(next.tag(), 0);
                pin.add_garbage(head);
                let mut ptr = next;
                while !ptr.is_null() {
                    let mut node: Owned<Node<T>> = ptr.into_owned();
                    let next = node.next.load(SeqCst, pin);
                    {
                        let data: &mut ManuallyDrop<T> = &mut (*node).data;
                        ManuallyDrop::drop(data);
                    }
                    pin.add_garbage(node);
                    ptr = next;
                }
            })
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
            pin(|pin| {
                assert!(!list.insert(i, pin).is_null());
            });
        }
        pin(|pin| assert_eq!(list.iter(pin).count(), N));
    }

    #[test]
    fn remove_front() {
        let list = List::new();
        const N: usize = 32;
        pin(|pin| for i in 0..N {
            assert!(!list.insert(i, pin).is_null());
        });
        for i in (0..N).rev() {
            let ret = pin(|pin| list.remove_front(pin));
            assert_eq!(ret, Some(i));
        }
        pin(|pin| assert_eq!(list.iter(pin).next(), None));
    }

    #[test]
    fn remove() {
        let list = List::new();
        const N: usize = 32;
        pin(|pin| for i in 0..N {
            assert!(!list.insert(i, pin).is_null());
        });
        let mut ids = (0..N).collect::<Vec<_>>();
        let mut rng = thread_rng();
        rng.shuffle(&mut ids);

        for i in (0..N).rev() {
            let ret = pin(|pin| list.remove(&ids[i], pin));
            assert!(ret);
        }
        pin(|pin| assert_eq!(list.iter(pin).next(), None));
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
        pin(|pin| for n in 0..N {
            source.insert(n, pin);
        });

        let threads = (0..N_THREADS)
            .map(|thread_id| {
                let source = source.clone();
                let sink = sink.clone();
                spawn(move || {
                    let source = source;
                    let sink = sink;

                    // Move stuff from source to sink
                    while let Some(i) = pin(|pin| source.remove_front(pin)) {
                        pin(|pin| sink.insert(i, pin));
                    }
                })
            })
            .collect::<Vec<_>>();

        for t in threads.into_iter() {
            assert!(t.join().is_ok());
        }
        let mut v = Vec::with_capacity(N);
        pin(|pin| while let Some(i) = sink.remove_front(pin) {
            v.push(i);
        });
        v.sort();
        for (i, n) in v.into_iter().enumerate() {
            assert_eq!(i, n);
        }
    }
}
