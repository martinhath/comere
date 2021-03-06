use std::sync::atomic::Ordering::SeqCst;
use super::atomic::{Owned, Atomic, Ptr};
use std::mem::ManuallyDrop;
use super::{Pin, pin};

const STUCK_N: usize = 100_000;

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
        let mut head = self.head.load(SeqCst, _pin);
        assert_eq!(head.tag(), 0);
        let mut c = 0;
        loop {
            c += 1;
            if c > STUCK_N {
                println!("stuck in ebr::list::insert! c={}", c);
            }
            curr.next.store(head, SeqCst);
            let res = self.head.compare_and_set(head, curr_ptr, SeqCst, _pin);
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
        let head = self.head.load(SeqCst, _pin);
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
        let mut head_ptr: Ptr<Node<T>> = self.head.load(SeqCst, pin);
        'outer: loop {
            if head_ptr.is_null() {
                return None;
            }
            let head: &Node<T> = unsafe { head_ptr.deref() };
            let next = head.next.load(SeqCst, pin);
            if next.tag() != 0 {
                head_ptr = self.head.load(SeqCst, pin);
                continue 'outer;
            }
            let tag_res = head.next.compare_and_set(next, next.with_tag(1), SeqCst, pin);
            if tag_res.is_err() {
                continue 'outer;
            }
            match self.head.compare_and_set(head_ptr, next, SeqCst, pin) {
                Ok(()) => {
                    let data = unsafe {::std::ptr::read(&head.data)};
                    // add garbage here!!
                    pin.add_garbage(unsafe{head_ptr.into_owned()});
                    return Some(ManuallyDrop::into_inner(data))
                }
                Err(new_head) => {
                    let _res = head.next.compare_and_set(
                        next.with_tag(1),
                        next,
                        SeqCst,
                        pin
                    );
                    head_ptr = new_head;
                }
            }
        }
    }

    // TODO: we don't need this anymore, since we've got `iter`.
    /// Return `true` if `f` evaluates to `true` for all the elements
    /// in the list
    pub fn all<'scope, F>(&self, f: F, _pin: Pin<'scope>) -> bool
    where
        F: Fn(&T) -> bool,
    {
        let previous_atomic: &Atomic<Node<T>> = &self.head;
        let mut node_ptr = self.head.load(SeqCst, _pin);
        let mut node;
        while !node_ptr.is_null() {
            node = unsafe { node_ptr.deref() };
            if !f(&node.data) {
                return false;
            }
            node_ptr = node.next.load(SeqCst, _pin);
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
    pub fn remove<'scope>(&self, value: &T, pin: Pin<'scope>) -> Option<T> {
        // Rust does not have tail-call optimization guarantees, so we have to use a loop here, in
        // order not to blow the stack.
        let mut outer_count = 0;
        let mut last_continue = 0;
        'outer: loop {
            outer_count += 1;
            if outer_count > STUCK_N {
                println!("possibly stuck in ebr::list::remove (outer) (last_continue={})", last_continue);
            }
            let mut current_atomic_ptr = &self.head;
            // NOTE: here we assume that we never tag the head pointer, which is probably correct?
            let mut current_ptr = current_atomic_ptr.load(SeqCst, pin);
            if current_ptr.is_null() {
                return None;
            }
            let mut current_node: &Node<T>;

            let mut inner_count = 0;
            loop {
                inner_count += 1;
                current_node = unsafe { current_ptr.deref() };

                if *current_node.data == *value {
                    // Now we want to remove the current node from the list.  We first need to mark
                    // this node as 'to-be-deleted', by tagging its next pointer. When doing this,
                    // we avoid that other threads are inserting something after the current node,
                    // and us swinging the `next` pointer of `previous` to the old `next` of the
                    // current node.
                    let next_ptr = current_node.next.load(SeqCst, pin).with_tag(0);
                    if current_node
                        .next
                        .compare_and_set(next_ptr, next_ptr.with_tag(1), SeqCst, pin)
                        .is_err()
                    {
                        // Failed to mark the current node. Restart.
                            if outer_count > STUCK_N {
                        println!("couldn't mark current node.");
                            }
                        last_continue = 1;
                        continue 'outer;
                    };
                    let res = current_atomic_ptr.compare_and_set(current_ptr.with_tag(0), next_ptr, SeqCst, pin);
                    match res {
                        Ok(_) => unsafe {
                            // Now `current_node` is not reachable from the list.
                            let data = ::std::ptr::read(&current_node.data);
                            pin.add_garbage(current_ptr.into_owned());
                            return Some(ManuallyDrop::into_inner(data));
                        }
                        Err(_) => {
                            // Some new node in inserted behind us.
                            // Unmark and restart.
                            let res = current_node.next.compare_and_set(
                                next_ptr.with_tag(1),
                                next_ptr,
                                SeqCst,
                                pin
                            );
                            if res.is_err() {
                                // This might hit if we decide to make other threads help out on
                                // deletion.
                                // panic!("couldn't untag ptr. WTF?");
                            }
                            if outer_count > STUCK_N {
                           println!("Tried to untag pointer. Success? {}", res.is_ok());
                            }
                        last_continue = 2;
                            continue 'outer;
                        }
                    }
                } else {
                    current_atomic_ptr = &current_node.next;
                    current_ptr = current_node.next.load(SeqCst, pin);
                    if current_ptr.tag() != 0 {
                        // Some other thread have deleted us! This means that the next node might
                        // have already been free'd.
                        if outer_count > STUCK_N {
                            println!("want to skip this node, but it is marked! Danger!");
                        }
                        last_continue = 3;
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

    pub fn remove_with<'scope, F>(&self, value: &T, _pin: Pin<'scope>, f: F) -> bool
    where
        F: FnOnce(Owned<Node<T>>),
    {
        // Rust does not have tail-call optimization guarantees,
        // so we have to use a loop here, in order not to blow the stack.
        'outer: loop {
            let mut previous_node_ptr = &self.head;
            let mut current_ptr = self.head.load(SeqCst, _pin);
            if current_ptr.is_null() {
                return false;
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
                            let o = unsafe { current_ptr.into_owned() };
                            f(o);
                            return true;
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
                        return false;
                    }
                    current = unsafe { current_ptr.deref() };
                }
            }
        }
    }

    /// Return `true` if the list contains the given value.
    pub fn contains<'scope>(&self, value: &T, _pin: Pin<'scope>) -> bool {
        let mut c = 0;
        let mut last_iter_before_stuck = 0;
        'outer: loop {
            c += 1;
            if c > STUCK_N {
                println!("stuck in ebr::list::contains c={} ({})", c, last_iter_before_stuck);
            }
            let previous_atomic: &Atomic<Node<T>> = &self.head;
            let mut node_ptr = self.head.load(SeqCst, _pin);
            let mut node;

            let mut inner_count = 0;
            while !node_ptr.is_null() {
                inner_count += 1;
                node = unsafe { node_ptr.deref() };
                if *node.data == *value {
                    return true;
                }
                node_ptr = node.next.load(SeqCst, _pin);
                if node_ptr.tag() != 0 {
                    // restart, as we're being (or has been) removed
                    last_iter_before_stuck = inner_count;
                    continue 'outer;
                }
            }
            return false
        }
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
            assert!(ret.is_some());
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
