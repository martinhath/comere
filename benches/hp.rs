extern crate comere;
extern crate bench;

#[macro_use]
mod common;
use common::*;

use std::env;

use comere::hp;
use comere::hp::queue::Queue;

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
            state.queue.push(i as u32);
        }
    }

    let mut b = bench::ThreadBencher::<State, hp::JoinHandle<()>>::new(state, num_threads);
    b.before(|state| for i in 0..NUM_ELEMENTS / state.num_threads {
        state.queue.push(i as u32)
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
    b.into_stats()
}

fn nop(num_threads: usize) -> bench::BenchStats {
    #[inline(never)]
    fn nop(_s: &()) {}
    let mut b = bench::ThreadBencher::<(), hp::JoinHandle<()>>::new((), num_threads);
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

    println!("HP");
    println!("name;{}", bench::BenchStats::csv_header());
    for &(ref stats, ref name) in &stats {
        println!("{};{}", name, stats.csv());
    }
}