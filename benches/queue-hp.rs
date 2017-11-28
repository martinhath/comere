extern crate comere;
extern crate bench;

use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::thread::spawn;
use std::cell::UnsafeCell;
use std::env;

use comere::hp::queue::Queue;

fn main() {
    let num_threads: usize = env::args()
        .nth(1)
        .unwrap_or("4".to_string())
        .parse()
        .unwrap_or(4);
    const NUM_ELEMENTS: usize = 256 * 256;
    struct BenchState {
        state: Arc<Mutex<State>>,
        condvar: Arc<Condvar>,
        barrier: Arc<Barrier>,
        source: UnsafeCell<Queue<usize>>,
        sink: UnsafeCell<Queue<usize>>,
        threads: Vec<::std::thread::JoinHandle<()>>,
    };
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Wait,
        Run,
        Exit,
    };
    let bench_state = BenchState {
        state: Arc::new(Mutex::new(State::Wait)),
        condvar: Arc::new(Condvar::new()),
        barrier: Arc::new(Barrier::new(num_threads + 1)),
        source: UnsafeCell::new(Queue::new()),
        sink: UnsafeCell::new(Queue::new()),
        threads: vec![],
    };

    let mut b = bench::Bencher::<BenchState>::new();

    // Before the benchmark, fill the source up with elements, and spawn the threads that are to do
    // the work.
    b.pre(move |state| {
        for i in 0..NUM_ELEMENTS {
            unsafe { (*state.source.get()).push(i) };
        }
        for i in 0..num_threads {
            let bench_state = state.state.clone();
            let condvar = state.condvar.clone();
            let barrier = state.barrier.clone();
            let (source, sink) = unsafe {
                let source: &Queue<_> = &*state.source.get();
                let sink: &Queue<_> = &*state.sink.get();
                (source, sink)
            };
            state.threads.push(spawn(move || loop {
                let mut started = bench_state.lock().unwrap();
                while *started == State::Wait {
                    started = condvar.wait(started).unwrap();
                }
                let state = *started;
                drop(started);
                match state {
                    State::Exit => {
                        break;
                    }
                    State::Run => {
                        // BODY BEGINS HERE! ///////////////////////////////

                        // let mut c = 0;
                        while let Some(i) = source.pop() {
                            sink.push(i);
                            // c += 1;
                        }
                        // println!("thread {} moved {} elements", i, c);

                        // BODY END HERE ///////////////////////////////////
                    }
                    State::Wait => unreachable!(),
                }
                barrier.wait();
                barrier.wait();
            }));
        }
    });

    b.post(|state| {
        let mut s = state.state.lock().unwrap();
        *s = State::Exit;
    });

    b.between(|state| {
        let (source, sink) = unsafe {
            let source: &mut Queue<_> = &mut *state.source.get();
            let sink: &mut Queue<_> = &mut *state.sink.get();
            (source, sink)
        };
        while let Some(e) = source.pop() {
            sink.push(e);
        }
        unsafe {
            // We know that no other thread is reading this data when we swap it. Therefore,
            // this is safe.
            ::std::ptr::swap(sink, source);
        }
    });

    b.set_n(100);
    b.bench(bench_state, |state| {
        let mut s = state.state.lock().unwrap();
        *s = State::Run;
        drop(s);
        state.condvar.notify_all();

        state.barrier.wait();
        *state.state.lock().unwrap() = State::Wait;
        state.barrier.wait();
    });

    let mut f = ::std::fs::File::create(&format!("hp-{}", num_threads)).unwrap();
    let _ = b.output_samples(&mut f);
}
