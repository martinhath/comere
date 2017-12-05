extern crate comere;
extern crate bench;

#[macro_use]
mod common;
use common::*;

use std::env;
use std::thread;

use comere::nothing::queue::{Queue, Node};
use comere::nothing::atomic::Owned;

const NUM_ELEMENTS_NOTHING: usize = 256 * 256;

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
        for i in 0..NUM_ELEMENTS_NOTHING / state.num_threads {
            state.queue.push(i as u32, None);
        }
    }

    let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
    b.before(|state| while let Some(i) = state.queue.pop() {
        bench::black_box(i);
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
    b.into_stats()
}

fn nop(num_threads: usize) -> bench::BenchStats {
    #[inline(never)]
    fn nop(_s: &()) { bench::black_box(_s); }
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

    let stats = run!(num_threads, nop, queue_push, queue_pop, queue_transfer);

    println!("Nothing");
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
