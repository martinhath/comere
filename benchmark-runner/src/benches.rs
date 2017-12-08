use bench;

use std::thread;
use super::*;

use bench::{black_box, StdThread};

use rand::Rng;

pub mod hp {

    #[cfg(feature = "hp-wait")]
    const NAME: &str = "hpspin";
    #[cfg(not(feature = "hp-wait"))]
    const NAME: &str = "hp";

    use super::*;
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

    pub fn list_real(num_threads: usize) -> bench::BenchStats {
        struct State {
            list: List<u32>,
            num_threads: usize,
        }

        let state = State {
            list: List::new(),
            num_threads,
        };

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

        fn real(state: &State) {
            let mut rng = rand::thread_rng();
            let ti = THREAD_ID.with(|ti| *ti.borrow());
            for i in 0..NUM_ELEMENTS_SMALLER {
                // println!("real {} {}", i, ti);
                use super::Operation::*;
                match random_op(&mut rng) {
                    Insert(n) => {
                        // println!("{} insert", ti);
                        let r = state.list.insert(n);
                        black_box(r);
                    }
                    Search(n) => {
                        // println!("{} search", ti);
                        let r = state.list.contains(&n);
                        black_box(r);
                    }
                    Remove(n) => {
                        // println!("{} remove", ti);
                        let r = state.list.remove(&n);
                        black_box(r);
                    }
                    PopFront => {
                        // println!("{} pop_front", ti);
                        let r = state.list.remove_front();
                        black_box(r);
                    }
                }
                // println!("{} ok", ti);
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

        // println!("before thread_bench");
        b.thread_bench(real);
        // println!("before into_stats");
        let s = b.into_stats(format!("nothing::list::remove::{}", num_threads));
        // println!("End of bench function");
        s
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

    pub fn list_real(num_threads: usize) -> bench::BenchStats {
        struct State {
            list: List<u32>,
            num_threads: usize,
        }

        let state = State {
            list: List::new(),
            num_threads,
        };

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

        fn real(state: &State) {
            let ti = THREAD_ID.with(|ti| *ti.borrow());
            let mut rng = rand::thread_rng();
            for i in 0..NUM_ELEMENTS_SMALLER {
                use super::Operation::*;
                let op = random_op(&mut rng);
                // println!("{} {:?}", ti, op);
                ebr::pin(|pin| match op {
                    Insert(n) => {
                        // println!("insert");
                        let r = state.list.insert(n, pin);
                        black_box(r);
                    }
                    Search(n) => {
                        // println!("search");
                        let r = state.list.contains(&n, pin);
                        black_box(r);
                    }
                    Remove(n) => {
                        // println!("remove");
                        let r = state.list.remove(&n, pin);
                        black_box(r);
                    }
                    PopFront => {
                        // println!("pop_front");
                        let r = state.list.remove_front(pin);
                        black_box(r);
                    }
                });
                // println!("{} OK", ti);
            }
        }

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
        b.before(|state| {
            let mut rng = rand::thread_rng();
            let mut n: Vec<u32> = (0..NUM_ELEMENTS_SMALLER as u32).collect();
            rng.shuffle(&mut n);
            ebr::pin(|pin| for &i in &n {
                state.list.insert(i, pin);
            });
        });

        b.thread_bench(real);
        b.into_stats(format!("nothing::list::remove::{}", num_threads))
    }

    pub fn nop(num_threads: usize) -> bench::BenchStats {
        #[inline(never)]
        fn nop(_s: &()) {}
        let mut b = bench::ThreadBencher::<(), StdThread<()>>::new((), num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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
        let mut b = bench::ThreadBencher::<(), StdThread<()>>::new((), num_threads);
        b.thread_bench(nop);
        b.into_stats(format!("crossbeam::nop::{}", num_threads))
    }
}

pub mod nothing {
    use super::*;
    use comere::nothing::queue::Queue;
    use comere::nothing::list::List;

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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
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

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
        b.before(|state| {
            while let Some(_) = state.sink.pop() {}
            for i in 0..NUM_ELEMENTS {
                state.source.push(i as u32, None);
            }
        });
        b.thread_bench(transfer);
        b.into_stats(format!("nothing::queue::transfer::{}", num_threads))
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
                assert!(ret);
            }
        }

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
        b.before(|state| {
            let mut rng = rand::thread_rng();
            let mut n: Vec<u32> = (0..NUM_ELEMENTS_SMALLER as u32).collect();
            rng.shuffle(&mut n);
            for &i in &n {
                state.list.insert(i, None);
            }
        });

        THREAD_COUNTER.store(0, Ordering::SeqCst);

        b.thread_bench(remove);
        b.into_stats(format!("nothing::list::remove::{}", num_threads))
    }

    pub fn list_real(num_threads: usize) -> bench::BenchStats {
        struct State {
            list: List<u32>,
        }

        let state = State { list: List::new() };

        fn real(state: &State) {
            let mut rng = rand::thread_rng();
            for _ in 0..NUM_ELEMENTS_SMALLER {
                use super::Operation::*;
                match random_op(&mut rng) {
                    Insert(n) => {
                        let r = state.list.insert(n, None);
                        black_box(r);
                    }
                    Search(n) => {
                        let r = state.list.contains(&n);
                        black_box(r);
                    }
                    Remove(n) => {
                        let r = state.list.remove(&n);
                        black_box(r);
                    }
                    PopFront => {
                        let r = state.list.remove_front();
                        black_box(r);
                    }
                }
            }
        }

        let mut b = bench::ThreadBencher::<State, StdThread<()>>::new(state, num_threads);
        b.before(|state| {
            let mut rng = rand::thread_rng();
            let mut n: Vec<u32> = (0..NUM_ELEMENTS_SMALLER as u32).collect();
            rng.shuffle(&mut n);
            for &i in &n {
                state.list.insert(i, None);
            }
        });

        b.thread_bench(real);
        b.into_stats(format!("nothing::list::remove::{}", num_threads))
    }

    pub fn nop(num_threads: usize) -> bench::BenchStats {
        #[inline(never)]
        fn nop(_s: &()) {
            bench::black_box(_s);
        }
        let mut b = bench::ThreadBencher::<(), StdThread<()>>::new((), num_threads);
        b.thread_bench(nop);
        b.into_stats(format!("nothing::nop::{}", num_threads))
    }
}

#[derive(Debug)]
enum Operation {
    Insert(u32),
    Search(u32),
    Remove(u32),
    PopFront,
}

fn random_op<R: Rng>(rng: &mut R) -> Operation {
    let r = rng.gen_range(0, 10);
    let n = rng.gen_range(0, NUM_ELEMENTS_SMALLER as u32);
    if r < 4 {
        Operation::Insert(n)
    } else if r < 6 {
        Operation::Search(n)
    } else if r < 8 {
        Operation::Remove(n)
    } else {
        Operation::PopFront
    }
}
