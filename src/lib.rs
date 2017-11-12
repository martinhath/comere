#![feature(test)]
// TODO: remove this
#![feature(const_atomic_usize_new, const_atomic_bool_new)]
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

#[cfg(test)]
mod test {
    use super::*;

    use std::sync::atomic::Ordering::SeqCst;
    use std::thread::spawn;
    use std::sync::{Arc, Mutex, Barrier};

    const N_THREADS: usize = 4;

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
