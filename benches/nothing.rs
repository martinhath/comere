extern crate comere;
extern crate bench;

#[macro_use]
mod common;
use common::*;

use std::env;
use std::thread;

use comere::nothing::queue::{Queue, Node};
use comere::nothing::atomic::Owned;

const NUM_ELEMENTS: usize = 256 * 256;

fn queue_push(num_threads: usize) -> bench::BenchStats {
    struct State {
        queue: Queue<u32>,
        ptrs: Vec<Owned<Node<u32>>>,
    }

    let state = State {
        queue: Queue::new(),
        ptrs: Vec::with_capacity(NUM_ELEMENTS),
    };

    fn queue_push(state: &State) {
        while let Some(_) = state.queue.pop() {}
    }

    let mut b = bench::ThreadBencher::<State, thread::JoinHandle<()>>::new(state, num_threads);
    b.before(|state| {
        state.ptrs.clear();
        let mut c = 0;
        while let Some(_) = state.queue.pop() {}
        unsafe {
            for i in 0..NUM_ELEMENTS {
                state.queue.push(
                    i as u32,
                    Some(state.ptrs.as_mut_ptr().offset(c as isize)),
                );
                c += 1;
            }
            state.ptrs.set_len(c);
        }
    });
    b.thread_bench(queue_push);
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

    let stats = run!(num_threads,
                     nop,
                     queue_push,
                     queue_transfer
                     );

    println!("EBR");
    println!("name;{}", bench::BenchStats::csv_header());
    for &(ref stats, ref name) in &stats {
        println!("{};{}", name, stats.csv());
    }
}
