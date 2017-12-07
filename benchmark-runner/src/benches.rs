use bench;

use std::thread;
use super::*;

pub mod hp {

    #[cfg(feature = "hp-wait")]
    const NAME: &str = "hpspin";
    #[cfg(not(feature = "hp-wait"))]
    const NAME: &str = "hp";

    use super::*;
    use rand::Rng;
    use comere::hp;
    use comere::hp::queue::Queue;
    use comere::hp::list::List;

    pub fn queue_push(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: Queue<u32>,
            num_threads: usize,
        }

        let state = State {
            queue: Queue::new(),
            num_threads,
        };

        fn queue_push(state: &State) {
            for i in 0..NUM_ELEMENTS / state.num_threads {
                state.queue.push(i as u32);
            }
        }

        let mut b = bench::ThreadBencher::<State, hp::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| while let Some(_) = state.queue.pop() {});
        b.thread_bench(queue_push);
        b.into_stats(format!("{}::queue::push::{}", NAME, num_threads))
    }

    pub fn queue_pop(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: Queue<u32>,
        }

        let state = State { queue: Queue::new() };

        fn queue_pop(state: &State) {
            while let Some(_) = state.queue.pop() {}
        }

        let mut b = bench::ThreadBencher::<State, hp::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            while let Some(_) = state.queue.pop() {}
            for i in 0..NUM_ELEMENTS {
                state.queue.push(i as u32);
            }
        });
        b.thread_bench(queue_pop);
        b.into_stats(format!("{}::queue::pop::{}", NAME, num_threads))
    }

    pub fn queue_transfer(num_threads: usize) -> bench::BenchStats {
        struct State {
            source: Queue<u32>,
            sink: Queue<u32>,
        }

        let state = State {
            source: Queue::new(),
            sink: Queue::new(),
        };

        fn transfer(state: &State) {
            while let Some(i) = state.source.pop() {
                state.sink.push(i);
            }
        }

        let mut b = bench::ThreadBencher::<State, hp::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            while let Some(_) = state.sink.pop() {}
            for i in 0..NUM_ELEMENTS {
                state.source.push(i as u32);
            }
        });
        b.thread_bench(transfer);
        b.into_stats(format!("{}::queue::transfer::{}", NAME, num_threads))
    }

    pub fn list_remove(num_threads: usize) -> bench::BenchStats {
        struct State {
            list: List<u32>,
            num_threads: usize,
        }

        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::cell::RefCell;
        lazy_static! {
            static ref THREAD_COUNTER: AtomicUsize = { AtomicUsize::new(0) };
        }

        thread_local! {
            static THREAD_ID: RefCell<usize> = {
                RefCell::new(THREAD_COUNTER.fetch_add(1, Ordering::SeqCst))
            }
        }

        let state = State {
            list: List::new(),
            num_threads,
        };

        fn remove(state: &State) {
            let ti = THREAD_ID.with(|t| *t.borrow());
            for i in 0..NUM_ELEMENTS_SMALLER / state.num_threads {
                let n = (i * state.num_threads + ti) as u32;
                let ret = state.list.remove(&n);
                assert!(ret.is_some());
            }
        }

        let mut b = bench::ThreadBencher::<State, hp::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            let mut rng = rand::thread_rng();
            let mut n: Vec<u32> = (0..NUM_ELEMENTS_SMALLER as u32).collect();
            rng.shuffle(&mut n);
            for &i in &n {
                state.list.insert(i);
            }
        });

        THREAD_COUNTER.store(0, Ordering::SeqCst);

        b.thread_bench(remove);
        b.into_stats(format!("{}::list::remove::{}", NAME, num_threads))
    }

    pub fn nop(num_threads: usize) -> bench::BenchStats {
        #[inline(never)]
        fn nop(_s: &()) {}
        let mut b = bench::ThreadBencher::<(), hp::JoinHandle<()>>::new((), num_threads);
        b.thread_bench(nop);
        b.into_stats(format!("{}::nop::{}", NAME, num_threads))
    }
}

pub mod ebr {
    use super::*;
    use comere::ebr;
    use comere::ebr::queue::Queue;
    use comere::ebr::list::List;

    use rand::Rng;

    pub fn queue_push(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: Queue<u32>,
            num_threads: usize,
        }

        let state = State {
            queue: Queue::new(),
            num_threads,
        };

        fn queue_push(state: &State) {
            for i in 0..NUM_ELEMENTS / state.num_threads {
                ebr::pin(|pin| state.queue.push(i as u32, pin))
            }
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            ebr::pin(|pin| while let Some(_) = state.queue.pop(pin) {});
        });
        b.thread_bench(queue_push);
        b.into_stats(format!("ebr::queue::push::{}", num_threads))
    }

    pub fn queue_pop(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: Queue<u32>,
        }

        let state = State { queue: Queue::new() };

        fn queue_pop(state: &State) {
            while let Some(_) = ebr::pin(|pin| state.queue.pop(pin)) {}
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            ebr::pin(|pin| {
                while let Some(_) = state.queue.pop(pin) {}
                for i in 0..NUM_ELEMENTS {
                    state.queue.push(i as u32, pin);
                }
            });
        });
        b.thread_bench(queue_pop);
        b.into_stats(format!("ebr::queue::pop::{}", num_threads))
    }

    pub fn queue_transfer(num_threads: usize) -> bench::BenchStats {
        struct State {
            source: Queue<u32>,
            sink: Queue<u32>,
        }

        let state = State {
            source: Queue::new(),
            sink: Queue::new(),
        };

        fn transfer(state: &State) {
            while let Some(i) = ebr::pin(|pin| state.source.pop(pin)) {
                ebr::pin(|pin| state.sink.push(i, pin));
            }
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            ebr::pin(|pin| {
                while let Some(_) = state.sink.pop(pin) {}
                for i in 0..NUM_ELEMENTS {
                    state.source.push(i as u32, pin);
                }
            });
        });
        b.thread_bench(transfer);
        b.into_stats(format!("ebr::queue::transfer::{}", num_threads))
    }

    pub fn list_remove(num_threads: usize) -> bench::BenchStats {
        struct State {
            list: List<u32>,
            num_threads: usize,
        }

        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::cell::RefCell;
        lazy_static! {
            static ref THREAD_COUNTER: AtomicUsize = { AtomicUsize::new(0) };
        }

        thread_local! {
            static THREAD_ID: RefCell<usize> = {
                RefCell::new(THREAD_COUNTER.fetch_add(1, Ordering::SeqCst))
            }
        }

        let state = State {
            list: List::new(),
            num_threads,
        };

        fn remove(state: &State) {
            let ti = THREAD_ID.with(|t| *t.borrow());
            for i in 0..NUM_ELEMENTS_SMALLER / state.num_threads {
                let n = (i * state.num_threads + ti) as u32;
                let ret = ebr::pin(|pin| state.list.remove(&n, pin));
                assert!(ret.is_some());
            }
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            let mut rng = rand::thread_rng();
            let mut n: Vec<u32> = (0..NUM_ELEMENTS_SMALLER as u32).collect();
            rng.shuffle(&mut n);
            ebr::pin(|pin| for &i in &n {
                state.list.insert(i, pin);
            });
        });

        THREAD_COUNTER.store(0, Ordering::SeqCst);

        b.thread_bench(remove);
        b.into_stats(format!("ebr::list::remove::{}", num_threads))
    }

    pub fn nop(num_threads: usize) -> bench::BenchStats {
        #[inline(never)]
        fn nop(_s: &()) {}
        let mut b = bench::ThreadBencher::<(), thread::JoinHandle<()>>::new((), num_threads);
        b.thread_bench(nop);
        b.into_stats(format!("ebr::nop::{}", num_threads))
    }
}

pub mod crossbeam {
    use super::*;
    use crossbeam::sync::MsQueue;

    pub fn queue_push(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: MsQueue<u32>,
            num_threads: usize,
        }

        let state = State {
            queue: MsQueue::new(),
            num_threads,
        };

        fn queue_push(state: &State) {
            for i in 0..NUM_ELEMENTS / state.num_threads {
                state.queue.push(i as u32);
            }
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| while let Some(_) = state.queue.try_pop() {});
        b.thread_bench(queue_push);
        b.into_stats(format!("crossbeam::queue::push::{}", num_threads))
    }

    pub fn queue_pop(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: MsQueue<u32>,
        }

        let state = State { queue: MsQueue::new() };

        fn queue_pop(state: &State) {
            while let Some(_) = state.queue.try_pop() {}
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            while let Some(_) = state.queue.try_pop() {}
            for i in 0..NUM_ELEMENTS {
                state.queue.push(i as u32);
            }
        });
        b.thread_bench(queue_pop);
        b.into_stats(format!("crossbeam::queue::pop::{}", num_threads))
    }

    pub fn queue_transfer(num_threads: usize) -> bench::BenchStats {
        struct State {
            source: MsQueue<u32>,
            sink: MsQueue<u32>,
        }

        let state = State {
            source: MsQueue::new(),
            sink: MsQueue::new(),
        };

        fn transfer(state: &State) {
            while let Some(i) = state.source.try_pop() {
                state.sink.push(i);
            }
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            while let Some(_) = state.sink.try_pop() {}
            for i in 0..NUM_ELEMENTS {
                state.source.push(i as u32);
            }
        });
        b.thread_bench(transfer);
        b.into_stats(format!("crossbeam::queue::transfer::{}", num_threads))
    }

    pub fn nop(num_threads: usize) -> bench::BenchStats {
        #[inline(never)]
        fn nop(_s: &()) {}
        let mut b = bench::ThreadBencher::<(), thread::JoinHandle<()>>::new((), num_threads);
        b.thread_bench(nop);
        b.into_stats(format!("crossbeam::nop::{}", num_threads))
    }
}

pub mod nothing {
    use super::*;
    use comere::nothing::queue::Queue;

    pub fn queue_push(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: Queue<u32>,
            num_threads: usize,
        }

        let state = State {
            queue: Queue::new(),
            num_threads,
        };

        fn queue_push(state: &State) {
            for i in 0..NUM_ELEMENTS_NOTHING / state.num_threads {
                state.queue.push(i as u32, None);
            }
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| while let Some(i) = state.queue.pop() {
            bench::black_box(i);
        });
        b.thread_bench(queue_push);
        b.into_stats(format!("nothing::queue::push::{}", num_threads))
    }

    pub fn queue_pop(num_threads: usize) -> bench::BenchStats {
        struct State {
            queue: Queue<u32>,
        }

        let state = State { queue: Queue::new() };

        fn queue_pop(state: &State) {
            while let Some(i) = state.queue.pop() {
                bench::black_box(i);
            }
            bench::black_box(&state);
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            while let Some(_) = state.queue.pop() {}
            for i in 0..NUM_ELEMENTS_NOTHING {
                state.queue.push(i as u32, None);
            }
            bench::black_box(&state);
        });
        b.thread_bench(queue_pop);
        b.into_stats(format!("nothing::queue::pop::{}", num_threads))
    }

    pub fn queue_transfer(num_threads: usize) -> bench::BenchStats {
        struct State {
            source: Queue<u32>,
            sink: Queue<u32>,
        }

        let state = State {
            source: Queue::new(),
            sink: Queue::new(),
        };

        fn transfer(state: &State) {
            while let Some(i) = state.source.pop() {
                state.sink.push(i, None);
            }
        }

        let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
        b.before(|state| {
            while let Some(_) = state.sink.pop() {}
            for i in 0..NUM_ELEMENTS {
                state.source.push(i as u32, None);
            }
        });
        b.thread_bench(transfer);
        b.into_stats(format!("nothing::queue::transfer::{}", num_threads))
    }

    pub fn nop(num_threads: usize) -> bench::BenchStats {
        #[inline(never)]
        fn nop(_s: &()) {
            bench::black_box(_s);
        }
        let mut b = bench::ThreadBencher::<(), thread::JoinHandle<()>>::new((), num_threads);
        b.thread_bench(nop);
        b.into_stats(format!("nothing::nop::{}", num_threads))
    }
}
