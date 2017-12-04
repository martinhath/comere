extern crate crossbeam;
extern crate bench;

#[macro_use]
mod common;
use common::*;

use std::env;
use std::thread;

use crossbeam::sync::MsQueue;

const NUM_ELEMENTS: usize = 256 * 256;

fn queue_push(num_threads: usize) -> bench::BenchStats {
    struct State {
        queue: MsQueue<u32>,
    }

    let state = State { queue: MsQueue::new() };

    fn queue_push(state: &State) {
        while let Some(_) = state.queue.try_pop() {}
    }

    let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
    b.before(|state| {
        while let Some(_) = state.queue.try_pop() {}
        for i in 0..NUM_ELEMENTS {
            state.queue.push(i as u32);
        }
    });
    b.thread_bench(queue_push);
    b.into_stats()
}

fn queue_transfer(num_threads: usize) -> bench::BenchStats {
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

    let stats = run!(num_threads, nop, queue_push, queue_transfer);

    println!("Crossbeam");
    println!("name;{}", bench::BenchStats::csv_header());
    for &(ref stats, ref name) in &stats {
        println!("{};{}", name, stats.csv());
    }
}
