use std::sync::atomic::Ordering::SeqCst;
use std::mem::{drop, ManuallyDrop};

use super::atomic::{Owned, Atomic, Ptr, HazardPtr};

pub struct Node<T> {
    data: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

pub struct List<T> {
    head: Atomic<Node<T>>,
}

impl<T> Node<T> {
    pub(crate) fn new(data: T) -> Self {
        Self {
            data: ManuallyDrop::new(data),
            next: Atomic::null(),
        }
    }

    pub(crate) fn data_ptr(&self) -> Ptr<T> {
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

    /// Insert into the head of the list, and return a pointer to the data.
    pub fn insert(&self, data: T) {
        let curr_ptr: Owned<Node<T>> = Owned::new(Node::new(data));
        self.insert_owned(curr_ptr)
    }

    /// Insert the Node given as the first element in the list. This is useful when we need a
    /// pointer to the data _before_ actually pushing it into the list (eg.
    /// in `ThreadLocal::marker`).
    pub(crate) fn insert_owned(&self, curr_ptr: Owned<Node<T>>) {
        let curr_ptr = curr_ptr.into_ptr();
        let curr: &Node<T> = unsafe { curr_ptr.deref() };
        let mut head = self.head.load(SeqCst);

        // let mut debug_c = 0;
        'outer: loop {
            // debug_c += 1;
            // if debug_c > 100_000 {
            //     // Hazard is never verified ??
            //     panic!("hp::list::insert_owned is not returning!");
            // }
            // We do not need to register `curr_ptr` as a HP, since it is not visible to other threads.
            let head_hp = head.hazard();
            {
                if self.head.load(SeqCst) != head {
                    drop(head_hp);
                    head = self.head.load(SeqCst);
                    continue 'outer;
                }
            }
            curr.next.store(head, SeqCst);
            let res = self.head.compare_and_set(head, curr_ptr, SeqCst);
            match res {
                Ok(_) => {
                    drop(head_hp);
                    return;
                }
                Err(new_head) => {
                    head = new_head;
                    drop(head_hp);
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
        'outer: loop {
            let mut head_ptr: Ptr<Node<T>> = self.head.load(SeqCst);
            loop {
                if head_ptr.is_null() {
                    return None;
                }
                let head_hp = head_ptr.hazard();
                {
                    if self.head.load(SeqCst) != head_ptr {
                        continue 'outer;
                    }
                }
                let head: &Node<T> = unsafe { head_ptr.deref() };
                let next = head.next.load(SeqCst);
                if next.tag() != 0 {
                    continue 'outer;
                }
                let next_hp = next.hazard();
                {
                    if head.next.load(SeqCst) != next {
                        continue 'outer;
                    }
                }
                // Mark this node as 'to be removed'. This is needed, since if we do not do this,
                // we risk that this node, as well as the next node is removed from the list, which
                // causes problems for other concurrent operations.
                let tag_res = head.next.compare_and_set(next, next.with_tag(1), SeqCst);
                if tag_res.is_err() {
                    continue 'outer;
                }
                match self.head.compare_and_set(head_ptr, next, SeqCst) {
                    Ok(()) => unsafe {
                        // Now the head is made unreachable from the queue, and no thread has marked
                        // the pointer in the hazard list. Then we have exclusive access to it. Read
                        // the data, and free the node.
                        let data = ::std::ptr::read(&head.data);
                        // Since we have made the node unreachable, and no thread has registered
                        // it as hazardous, it is safe to free.
                        drop(next_hp);
                        head_hp.free();
                        return Some(ManuallyDrop::into_inner(data));
                    }
                    Err(new_head) => {
                        // Some new node in inserted behind us. Unmark and restart.
                        let _res = head.next.compare_and_set(
                            next.with_tag(1),
                            next,
                            SeqCst,
                        );
                        head_ptr = new_head;
                    }
                }
            }
        }
    }

    /// Return an iterator to the list.
    pub fn iter(&self) -> Iter<T> {
        'outer: loop {
            let head = self.head.load(SeqCst);
            let head_hp = head.hazard();
            {
                if self.head.load(SeqCst) != head {
                    continue 'outer;
                }
            }
            return Iter {
                node: head,
                hp: head_hp,
                _marker: ::std::marker::PhantomData,

            }
        }
    }
}

impl<T> List<T>
where
    T: 'static + PartialEq + ::std::fmt::Debug,
{
    /// Remove the first node in the list where `node.data == key`
    ///
    /// Note that this method causes the list to not be lock-free, since threads wanting to insert
    /// a node after this or remove the next node will be stuck forever if a thread tags the
    /// current node and then dies.
    ///
    /// NOTE(6.11.17): Maybe we can fix this by having other operation help out deleting the note
    /// if they ever see one?
    pub fn remove(&self, value: &T) -> Option<T> {
        // Rust does not have tail-call optimization guarantees, so we have to use a loop here, in
        // order not to blow the stack.
        // let mut debug_c = 0;
        // let mut debug_place = 0;
        'outer: loop {
            // debug_c += 1;
            // if debug_c > 100_000 {
            //     panic!("hp::list::remove is never returning! Last conitnue was {}", debug_place);
            // }
            let mut current_atomic_ptr = &self.head;
            // NOTE: here we assume that we never tag the head pointer, which is probably correct?
            let mut current_ptr = current_atomic_ptr.load(SeqCst);
            if current_ptr.is_null() {
                return None;
            }
            let mut current_node: &Node<T>;
            let mut prev_hp: Option<HazardPtr<::hp::list::Node<T>>> = None;

            loop {
                let current_hp = current_ptr.hazard();
                // validate
                {
                    if let Some(ref handle) = prev_hp {
                        if handle.next.load(SeqCst) != current_ptr {
                            drop(current_hp); // explicit drop here. Do we need it?
                            // debug_place = 1;
                            continue 'outer;
                        }
                    } else {
                        // This is only the case the first iteration, when cap == head.
                        if current_atomic_ptr.load(SeqCst) != current_ptr {
                            drop(current_hp); // explicit drop here. Do we need it?
                            // debug_place = 2;
                            continue 'outer;
                        }
                    }
                }
                current_node = unsafe { current_ptr.deref() };

                if *current_node.data == *value {
                    // Now we want to remove the current node from the list.  We first need to mark
                    // this node as 'to-be-deleted', by tagging its next pointer. When doing this,
                    // we avoid that other threads are inserting something after the current node,
                    // and us swinging the `next` pointer of `previous` to the old `next` of the
                    // current node.
                    let next_ptr = current_node.next.load(SeqCst).with_tag(0);
                    // We don't need to register a HP here, because if we don't really care about
                    // the next node in the list: if it is about to be removed, this CAS will fail,
                    // after the pointer is swung. If this CAS succeeds before the pointer is
                    // swung, their CAS will fail. In either case, one thread will restart.
                    if current_node
                        .next
                        .compare_and_set(next_ptr, next_ptr.with_tag(1), SeqCst)
                        .is_err()
                    {
                        // Failed to mark the current node. Restart.
                        // debug_place = 3;
                        continue 'outer;
                    };
                    let res = current_atomic_ptr.compare_and_set(current_ptr.with_tag(0), next_ptr, SeqCst);
                    match res {
                        Ok(_) => unsafe {
                            // Now `current_node` is not reachable from the list.
                            let data = ::std::ptr::read(&current_node.data);
                            current_hp.free();
                            return Some(ManuallyDrop::into_inner(data));
                        }
                        Err(_) => {
                            // Some new node in inserted behind us.
                            // Unmark and restart.
                            let res = current_node.next.compare_and_set(
                                next_ptr.with_tag(1),
                                next_ptr,
                                SeqCst,
                            );
                            if res.is_err() {
                                // This might hit if we decide to make other threads help out on
                                // deletion.
                                panic!("couldn't untag ptr. WTF?");
                            }
                            // debug_place = 4;
                            continue 'outer;
                        }
                    }
                } else {
                    current_atomic_ptr = &current_node.next;
                    current_ptr = current_node.next.load(SeqCst);
                    if current_ptr.tag() != 0 {
                        // Some other thread have deleted us! This means that the next node might
                        // have already been free'd.
                        // debug_place = 5;
                        continue 'outer;
                    }
                    prev_hp.take().map(::std::mem::drop);
                    prev_hp = Some(current_hp);

                    if current_ptr.is_null() {
                        // we've reached the end of the list, without finding our value.
                        return None;
                    }
                }
            }
        }
    }

    pub fn remove_with_node(&self, value: &T) -> Option<Owned<Node<T>>> {
        // Rust does not have tail-call optimization guarantees, so we have to use a loop here, in
        // order not to blow the stack.
        'outer: loop {
            let mut current_atomic_ptr = &self.head;
            let mut current_ptr = current_atomic_ptr.load(SeqCst);
            if current_ptr.is_null() {
                return None;
            }
            let mut current_node: &Node<T>;
            let mut prev_hp: Option<HazardPtr<::hp::list::Node<T>>> = None;

            loop {
                let current_hp = current_ptr.hazard();
                // validate
                {
                    if let Some(ref handle) = prev_hp {
                        if handle.next.load(SeqCst) != current_ptr {
                            drop(current_hp); // explicit drop here. Do we need it?
                            continue 'outer;
                        }
                    } else {
                        if current_atomic_ptr.load(SeqCst) != current_ptr {
                            drop(current_hp); // explicit drop here. Do we need it?
                            continue 'outer;
                        }
                    }
                }
                current_node = unsafe { current_ptr.deref() };

                if *current_node.data == *value {
                    // Now we want to remove the current node from the list.  We first need to mark
                    // this node as 'to-be-deleted', by tagging its next pointer. When doing this,
                    // we avoid that other threads are inserting something after the current node,
                    // and us swinging the `next` pointer of `previous` to the old `next` of the
                    // current node.
                    let next_ptr = current_node.next.load(SeqCst);
                    if current_node
                        .next
                        .compare_and_set(next_ptr, next_ptr.with_tag(1), SeqCst)
                        .is_err()
                    {
                        // Failed to mark the current node. Restart.
                        continue 'outer;
                    };
                    let res = current_atomic_ptr.compare_and_set(current_ptr, next_ptr, SeqCst);
                    match res {
                        Ok(_) => unsafe {
                            // Now `current_node` is not reachable from the list.
                            return Some(current_ptr.into_owned());
                        }
                        Err(_) => {
                            // Some new node in inserted behind us.
                            // Unmark and restart.
                            let res = current_node.next.compare_and_set(
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
                    current_atomic_ptr = &current_node.next;
                    current_ptr = current_node.next.load(SeqCst).with_tag(0);
                    prev_hp.take().map(::std::mem::drop);
                    prev_hp = Some(current_hp);

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
        'outer: loop {
            let mut node_ptr = self.head.load(SeqCst);
            let mut node_hp = node_ptr.hazard();
            {
                if self.head.load(SeqCst) != node_ptr {
                    continue 'outer;
                }
            }
            let mut prev_hp;
            let mut node;
            while !node_ptr.is_null() {
                node = unsafe { node_ptr.deref() };
                prev_hp = node_hp;
                if *node.data == *value {
                    drop(prev_hp);
                    return true;
                }
                node_ptr = node.next.load(SeqCst);
                if node_ptr.tag() != 0 {
                    // TODO: We could probably just take one step back, instead of restarting the
                    // whole operation.
                    continue 'outer;
                }
                node_hp = node_ptr.hazard();
                {
                    if node.next.load(SeqCst) != node_ptr {
                        // TODO: we actually only need to read the last node again.
                        continue 'outer;
                    }
                }
            }
            return false
        }
    }
}

/// An iterator for `List`
pub struct Iter<'a, T: 'a> {
    node: Ptr<'a, Node<T>>,
    hp: HazardPtr<Node<T>>,
    _marker: ::std::marker::PhantomData<&'a ()>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        'outer: loop {
            if let Some(node) = unsafe { self.node.as_ref() } {
                let new_node = node.next.load(SeqCst);
                let new_hp = new_node.hazard();
                {
                    if node.next.load(SeqCst) != new_node {
                        continue 'outer;
                    }
                }
                self.node = new_node;
                self.hp = new_hp;
                return Some(&node.data)
            } else {
                return None
            }
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

    use std::thread::spawn;
    use std::sync::Arc;

    #[test]
    fn insert() {
        let list = List::new();
        const N: usize = 32;
        for i in 0..N {
            list.insert(i);
        }
        assert_eq!(list.iter().count(), N);
    }

    #[test]
    fn remove_front() {
        let list = List::new();
        const N: usize = 32;
        for i in 0..N {
            list.insert(i);
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
        const N: usize = 1024 * 32; // * 1024;

        let list: Arc<List<usize>> = Arc::new(List::new());

        // Prefill with some values
        for i in 0..N {
            list.insert(i);
        }
        assert_eq!(list.iter().count(), N);

        let threads = (0..N_THREADS)
            .map(|thread_id| {
                let list = list.clone();
                spawn(move || for i in (0..N / N_THREADS).rev() {
                    let n = i * N_THREADS + thread_id;
                    assert!(list.remove(&n).is_some());
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
            .map(|_thread_id| {
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
