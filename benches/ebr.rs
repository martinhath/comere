extern crate comere;
extern crate bench;
extern crate rand;
#[macro_use]
extern crate lazy_static;


#[macro_use]
mod common;
use common::*;

use std::env;
use std::thread;

use comere::ebr;
use comere::ebr::queue::Queue;
use comere::ebr::list::List;

use rand::Rng;

fn queue_push(num_threads: usize) -> bench::BenchStats {
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
    b.into_stats()
}

fn queue_pop(num_threads: usize) -> bench::BenchStats {
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
    b.into_stats()
}

fn queue_transfer(num_threads: usize) -> bench::BenchStats {
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
    b.into_stats()
}

fn list_remove(num_threads: usize) -> bench::BenchStats {
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
        ebr::pin(|pin| {
            for &i in &n {
                state.list.insert(i, pin);
            }
        });
    });

    b.thread_bench(remove);
    b.into_stats()
}

fn nop(num_threads: usize) -> bench::BenchStats {
    #[inline(never)]
    fn nop(_s: &()) {}
    let mut b = bench::ThreadBencher::<(), thread::JoinHandle<()>>::new((), num_threads);
    b.thread_bench(nop);
    b.into_stats()
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    let num_threads: usize = args.get(1)
        .ok_or(())
        .and_then(|s| s.parse().map_err(|_| ()))
        .unwrap_or(4);

    let gnuplot_output = args.get(2);

    let stats = run!(num_threads,
                     nop,
                     list_remove,
                     queue_push,
                     queue_pop,
                     queue_transfer
                     );

    println!("EBR");
    println!("name;{}", bench::BenchStats::csv_header());
    for &(ref stats, ref name) in &stats {
        println!("{};{}", name, stats.csv());
    }

    if let Some(fname) = gnuplot_output {
        use std::io::Write;
        use std::fs::File;
        let mut f = File::create(fname).unwrap();
        f.write_all(bench::gnuplot(&stats).as_bytes()).unwrap();
    }
}
