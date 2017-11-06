#![feature(alloc_system, global_allocator, allocator_api)]
extern crate alloc_system;
use alloc_system::System;
#[global_allocator]
static A: System = System;


#[macro_use]
extern crate lazy_static;

#[cfg(test)]
extern crate rand;

#[allow(unused_variables)]
#[allow(dead_code)]
pub mod nothing;
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod ebr;
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod hp;

pub trait Queue<T> {
    fn new() -> Self;
    fn push(&self, T);
    fn pop(&self) -> Option<T>;
    fn is_empty(&self) -> bool;
}

impl<T> Queue<T> for nothing::queue::Queue<T> {
    fn new() -> Self {
        nothing::queue::Queue::new()
    }
    fn push(&self, val: T) {
        nothing::queue::Queue::push(self, val);
    }
    fn pop(&self) -> Option<T> {
        nothing::queue::Queue::pop(self)
    }
    fn is_empty(&self) -> bool {
        nothing::queue::Queue::is_empty(self)
    }
}

// TODO: remove this
type T = u32;

pub trait List {
    fn new() -> Self;
    fn insert(&self, T);
    // fn remove(&self) -> Option<T>;
    // fn is_empty(&self) -> bool;
}

impl List for nothing::list::List {
    fn new() -> Self {
        nothing::list::List::new()
    }
    fn insert(&self, val: T) {
        nothing::list::List::insert(self, val);
    }
    // fn remove(&self) -> Option<T> {
    //     nothing::list::List::remove(self)
    // }
    // fn is_empty(&self) -> bool {
    //     nothing::list::List::is_empty(self)
    // }
}


#[cfg(test)]
mod test {
    use super::*;

    use std::sync::atomic::Ordering::SeqCst;
    use std::thread::spawn;
    use std::sync::{Arc, Mutex, Barrier};


    const N_THREADS: usize = 16;

    macro_rules! correctness_queue {($Q:ident) => {
        $Q.push(123);
        assert!(!$Q.is_empty());
        assert_eq!($Q.pop(), Some(123));
        assert!($Q.is_empty());
        for i in 0..200 {
            $Q.push(i);
        }
        assert!(!$Q.is_empty());
        for i in 0..200 {
            assert_eq!($Q.pop(), Some(i));
        }
        assert!($Q.is_empty());

        let iter_count = 1_000_000;
        let sync_interval = 10000;

        let thread_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(N_THREADS));
        let q = Arc::new($Q);
        let removals = Arc::new(Mutex::new([0; N_THREADS]));
        {
            let mut threads = vec![];
            for _ in 0..N_THREADS {
                let thread_count = thread_count.clone();
                let q = q.clone();
                let removals = removals.clone();
                let barrier = barrier.clone();
                threads.push(spawn(move || {
                    // All threads find their id.
                    // Some threads `push`es, and the other `pop`s.
                    // The poppers register what they find.
                    let mut local_removals = [0; N_THREADS];
                    let thread_id = thread_count.fetch_add(1, SeqCst) as u32;
                    let push_thread = thread_id % 2 == 0;

                    barrier.wait();
                    for i in 0..iter_count {
                        if push_thread {
                            q.push(thread_id);
                        } else {
                            if let Some(res) = q.pop() {
                                local_removals[res as usize] += 1;
                            }
                        }
                        // Every now and then, sync up the threads.
                        // This seems to cause errors more often
                        if i % sync_interval == 0 {
                            barrier.wait();
                        }
                    }
                    // Wait for everyone to finish
                    barrier.wait();
                    // Remove remaining elements, if any.
                    // Each thread updates the global removals count
                    if !push_thread {
                        if thread_id == 1 {
                            while let Some(res) = q.pop() {
                                local_removals[res as usize] += 1;
                            }
                        }
                        let mut removals = removals.lock().unwrap();
                        for i in 0..N_THREADS {
                            removals[i] += local_removals[i];
                        }
                    }
                }));
            }

            // Finish all threads
            for t in threads {
                t.join().unwrap();
            }

            assert!(q.is_empty());
            // Confirm the counts
            for (i, &n) in removals.lock().unwrap().iter().enumerate() {
                let push_thread = i % 2 == 0;
                if push_thread {
                    assert_eq!(n, iter_count);
                }
            }
        }
    }}

    macro_rules! correctness_list {($L:ident) => {
        assert!($L.is_empty());
        $L.insert(1);
        assert!(!$L.is_empty());
        assert_eq!($L.remove_front(), Some(1));
        assert!($L.is_empty());
        for i in 0..200 {
            $L.insert(i);
        }
        assert!(!$L.is_empty());
        for i in (0..200).rev() {
            assert_eq!($L.remove_front(), Some(i));
        }
        assert!($L.is_empty());

        let iter_count = 10_000;

        let thread_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(N_THREADS));
        let l = Arc::new($L);
        {
            // Have odd threads insert their thread_id, and even
            // threads remove them.
            // Queue should be empty at the end, and all removals
            // should succeed.
            let mut threads = vec![];
            for _ in 0..N_THREADS {
                let thread_count = thread_count.clone();
                let l = l.clone();
                let barrier = barrier.clone();
                threads.push(spawn(move || {
                    // All threads find their id.
                    // Some threads `push`es, and the other `pop`s.
                    // The poppers register what they find.
                    let thread_id = thread_count.fetch_add(1, SeqCst) as u32;
                    let even = thread_id % 2 == 0;

                    for _ in 0..iter_count {
                        barrier.wait();
                        if !even {
                            l.insert(thread_id);
                        }
                        barrier.wait();
                        if even {
                            let remove_id = thread_id + 1;
                            assert!(l.contains(&remove_id));
                            assert!(l.remove(&remove_id));
                        }
                        barrier.wait();
                        if thread_id == 1 {
                            assert!(l.is_empty());
                        }
                    }
                }));
            }

            // Finish all threads
            for t in threads {
                assert!(t.join().is_ok());
            }
            // assert!(l.is_empty());
        }
    }}

    #[test]
    fn correct_queue_nothing() {
        let q: nothing::queue::Queue<u32> = Queue::new();
        correctness_queue!(q);
    }

    #[test]
    fn correct_list_nothing() {
        let l: nothing::list::List = List::new();
        correctness_list!(l);
    }

    #[test]
    fn correct_queue_ebr() {
        const N_THREADS: usize = 16;

        use ebr::{pin, register_thread};
        let q = ebr::queue::Queue::new();

        pin(|pin| {
            q.push(123, pin);
            assert!(!q.is_empty(pin));
            assert_eq!(q.pop(pin), Some(123));
            assert!(q.is_empty(pin));
        });
        pin(|pin| {
            for i in 0..200 {
                q.push(i, pin);
            }
            assert!(!q.is_empty(pin));
            for i in 0..200 {
                assert_eq!(q.pop(pin), Some(i));
            }
            assert!(q.is_empty(pin));
        });

        let iter_count = 100_000;
        let sync_interval = 10000;

        let thread_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(N_THREADS));
        let q = Arc::new(q);
        let removals = Arc::new(Mutex::new([0; N_THREADS]));
        {
            let mut threads = vec![];
            for _ in 0..N_THREADS {
                let thread_count = thread_count.clone();
                let q = q.clone();
                let removals = removals.clone();
                let barrier = barrier.clone();
                threads.push(spawn(move || {
                    // All threads find their id.
                    // Some threads `push`es, and the other `pop`s.
                    // The poppers register what they find.
                    let mut local_removals = [0; N_THREADS];
                    let thread_id = thread_count.fetch_add(1, SeqCst);
                    register_thread(thread_id);
                    let push_thread = thread_id % 2 == 0;

                    barrier.wait();
                    for i in 0..iter_count {
                        pin(|pin| if push_thread {
                            q.push(thread_id, pin);
                        } else {
                            if let Some(res) = q.pop(pin) {
                                local_removals[res as usize] += 1;
                            }
                        });
                        // Every now and then, sync up the threads.
                        // This seems to cause errors more often
                        if i % sync_interval == 0 {
                            barrier.wait();
                        }
                    }
                    // Wait for everyone to finish
                    barrier.wait();
                    // Remove remaining elements, if any.
                    // Each thread updates the global removals count
                    if !push_thread {
                        pin(|pin| {
                            if thread_id == 1 {
                                while let Some(res) = q.pop(pin) {
                                    local_removals[res as usize] += 1;
                                }
                            }
                            let mut removals = removals.lock().unwrap();
                            for i in 0..N_THREADS {
                                removals[i] += local_removals[i];
                            }
                        });
                    }
                }));
            }

            // Finish all threads
            for t in threads {
                t.join().unwrap();
            }

            pin(|pin| {
                assert!(q.is_empty(pin));
            });
            // Confirm the counts
            for (i, &n) in removals.lock().unwrap().iter().enumerate() {
                let push_thread = i % 2 == 0;
                if push_thread {
                    assert_eq!(n, iter_count);
                }
            }
        }
    }

    #[test]
    fn correct_list_ebr() {
        let list = ebr::list::List::new();

        ebr::pin(|pin| {
            assert!(list.is_empty(pin));
            list.insert(1, pin);
            assert!(!list.is_empty(pin));
            assert_eq!(list.remove_front(pin), Some(1));
            assert!(list.is_empty(pin));
            for i in 0..200 {
                list.insert(i, pin);
            }
            assert!(!list.is_empty(pin));
            for i in (0..200).rev() {
                assert_eq!(list.remove_front(pin), Some(i));
            }
            assert!(list.is_empty(pin));
        });

        let iter_count = 10_000;

        let thread_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(N_THREADS));
        let l = Arc::new(list);
        {
            // Have odd threads insert their thread_id, and even
            // threads remove them.
            // Queue should be empty at the end, and all removals
            // should succeed.
            let mut threads = vec![];
            for _ in 0..N_THREADS {
                let thread_count = thread_count.clone();
                let l = l.clone();
                let barrier = barrier.clone();
                threads.push(spawn(move || {
                    // All threads find their id.
                    // Some threads `push`es, and the other `pop`s.
                    // The poppers register what they find.
                    let thread_id = thread_count.fetch_add(1, SeqCst) as u32;
                    let even = thread_id % 2 == 0;

                    for _ in 0..iter_count {
                        barrier.wait();
                        if !even {
                            ebr::pin(|pin| { l.insert(thread_id, pin); });
                        }
                        barrier.wait();
                        if even {
                            let remove_id = thread_id + 1;
                            ebr::pin(|pin| {
                                assert!(l.contains(&remove_id, pin));
                                assert!(l.remove(&remove_id, pin));
                            });
                        }
                        barrier.wait();
                        if thread_id == 1 {
                            ebr::pin(|pin| {
                                assert!(l.is_empty(pin));
                            });
                        }
                    }
                }));
            }

            // Finish all threads
            for t in threads {
                assert!(t.join().is_ok());
            }
            // assert!(l.is_empty());
        }
    }

}
