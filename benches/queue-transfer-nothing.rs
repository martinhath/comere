extern crate comere;
extern crate bench;

use std::sync::{Arc, Barrier};
use std::env;

use comere::nothing::queue::Queue;

const BENCH_NAME: &str = "queue-transfer";

fn main() {
    let num_threads: usize = env::args()
        .nth(1)
        .unwrap_or("4".to_string())
        .parse()
        .unwrap_or(4);

    const NUM_ELEMENTS: usize = 256 * 256;
    let barrier = Arc::new(Barrier::new(num_threads + 1));
    let source = Arc::new(Queue::new());
    let sink = Arc::new(Queue::new());

    struct State {
        threads: Vec<hp::JoinHandle<()>>,
    };

    let mut b = bench::Bencher::<State>::new();
    let pre_source = source.clone();
    let pre_sink = sink.clone();
    let pre_barrier = barrier.clone();
    b.pre(move |state| {
        for i in 0..NUM_ELEMENTS {
            pre_source.push(i);
        }
        state.threads.extend((0..num_threads).map(|_i| {
            let barrier = pre_barrier.clone();
            let source = pre_source.clone();
            let sink = pre_sink.clone();
            hp::spawn(move || {
                barrier.wait();
                while let Some(i) = source.pop() {
                    sink.push(i);
                }
            })
        }));
    });
    let between_barrier = barrier.clone();
    let between_source = source.clone();
    let between_sink = sink.clone();
    b.between(move |state| {
        for _ in 0..NUM_ELEMENTS {
            while let Some(i) = between_sink.pop() {
                between_source.push(i);
            }
        }
        state.threads.extend((0..num_threads).map(|_i| {
            let barrier = between_barrier.clone();
            let source = between_source.clone();
            let sink = between_sink.clone();
            hp::spawn(move || {
                barrier.wait();
                while let Some(i) = source.pop() {
                    sink.push(i);
                }
            })
        }));

    });

    b.set_n(100);
    b.bench(State { threads: vec![] }, |state| {
        barrier.wait();
        while let Some(thread) = state.threads.pop() {
            thread.join().unwrap();
        }
    });

    let mut f = ::std::fs::File::create(&format!("{}-hp-{}", BENCH_NAME, num_threads)).unwrap();
    let _ = b.output_samples(&mut f);
}
