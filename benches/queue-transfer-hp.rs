extern crate comere;
extern crate bench;

use std::sync::{Arc, Barrier};
use std::env;

use comere::hp::queue::Queue;
use comere::hp;

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

    let mut b = bench::Bencher::<()>::new();
    let pre_source = source.clone();
    b.pre(move |_| for i in 0..NUM_ELEMENTS {
        pre_source.push(i);
    });
    let between_source = source.clone();
    let between_sink = sink.clone();
    b.between(move |_| for _ in 0..NUM_ELEMENTS {
        while let Some(i) = between_sink.pop() {
            between_source.push(i);
        }
    });

    b.set_n(100);
    b.bench((), |_| {
        let threads: Vec<hp::JoinHandle<()>> = (0..num_threads)
            .map(|_i| {
                let source = source.clone();
                let sink = sink.clone();
                let barrier = barrier.clone();
                hp::spawn(move || {
                    barrier.wait();
                    while let Some(i) = source.pop() {
                        sink.push(i);
                    }
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
    });

    let mut f = ::std::fs::File::create(&format!("{}-hp-{}", BENCH_NAME, num_threads)).unwrap();
    let _ = b.output_samples(&mut f);
}
