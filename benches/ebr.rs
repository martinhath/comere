extern crate comere;
extern crate bench;

#[macro_use]
mod common;
use common::*;

use std::env;
use std::thread;

use comere::ebr;
use comere::ebr::queue::Queue;

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

fn nop(num_threads: usize) -> bench::BenchStats {
    #[inline(never)]
    fn nop(_s: &()) {}
    let mut b = bench::ThreadBencher::<(), thread::JoinHandle<()>>::new((), num_threads);
    b.thread_bench(nop);
    b.into_stats()
}

fn main() {
    let num_threads: usize = env::args()
        .nth(1)
        .unwrap_or("4".to_string())
        .parse()
        .unwrap_or(4);

    let stats = run!(num_threads, nop, queue_push, queue_pop, queue_transfer);

    println!("EBR");
    println!("name;{}", bench::BenchStats::csv_header());
    for &(ref stats, ref name) in &stats {
        println!("{};{}", name, stats.csv());
    }
}
