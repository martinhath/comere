extern crate comere;
extern crate bench;

use std::env;

use comere::hp;
use comere::hp::queue::Queue;

const NUM_ELEMENTS: usize = 256 * 256;

fn queue_push(num_threads: usize) {
    struct State {
        queue: Queue<u32>,
    }

    let state = State { queue: Queue::new() };

    fn queue_push(state: &State) {
        while let Some(_) = state.queue.pop() {}
    }

    let mut b = bench::ThreadBencher::<State, hp::JoinHandle<()>>::new(state, num_threads);
    b.before(|state| {
        while let Some(_) = state.queue.pop() {}
        for i in 0..NUM_ELEMENTS {
            state.queue.push(i as u32);
        }
    });
    b.thread_bench(queue_push);

    print!("Queue::Push ");
    println!("{}", b.report());
}

fn queue_transfer(num_threads: usize) {
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

    print!("Queue::Transfer ");
    println!("{}", b.report());
}

fn main() {
    let num_threads: usize = env::args()
        .nth(1)
        .unwrap_or("4".to_string())
        .parse()
        .unwrap_or(4);

    // Run a bench without doing anything, just to see how large the benching overhead is.
    {
        #[inline(never)]
        fn nop(_s: &()) {}
        let mut b = bench::ThreadBencher::<(), hp::JoinHandle<()>>::new((), 1);
        b.thread_bench(nop);
        print!("Queue::nop ");
        println!("{}", b.report());
    }
    queue_push(num_threads);
    queue_transfer(num_threads);
}
