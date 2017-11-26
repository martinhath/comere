#[macro_use]
extern crate bencher;
extern crate comere;
extern crate crossbeam;
extern crate time;

use bencher::Bencher;

mod nothing {
    //! See comment in `benches/list.rs:nothing`.
    use super::Bencher;
    use comere::nothing::queue::Queue;
    use comere::nothing::atomic::Owned;
    use comere::nothing::queue::Node;

    pub fn push(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let queue = Queue::new();
            let mut ptrs: Vec<Owned<Node<usize>>> = Vec::with_capacity(N as usize);
            let ptr = ptrs.as_mut_ptr();
            let mut i = 0;
            _b.iter(|| {
                queue.push(0usize, Some(unsafe { ptr.offset(i) }));
                i += 1;
            });
            unsafe {
                ptrs.set_len(N as usize);
            }
        });
    }

    pub fn pop(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let queue = Queue::new();
            let mut ptrs: Vec<Owned<Node<u64>>> = Vec::with_capacity(N as usize);
            let ptr = ptrs.as_mut_ptr();
            let mut c = 0;
            for i in 0..N {
                queue.push(i, Some(unsafe { ptr.offset(c) }));
                c += 1
            }
            _b.iter(|| {
                let ret = queue.pop();
                assert!(ret.unwrap() < N);
            });
            unsafe {
                ptrs.set_len(N as usize);
            }
        });
    }
}

mod hp {
    use super::Bencher;
    use comere::hp::queue::Queue;
    use comere::hp::*;

    use std::sync::{Arc, Condvar, Mutex};
    use std::mem::drop;

    pub fn push(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let queue = Queue::new();
            _b.iter(|| { queue.push(0usize); });
        });
    }

    pub fn pop(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let queue = Queue::new();
            for i in 0..N {
                queue.push(i);
            }
            _b.iter(|| {
                let ret = queue.pop();
                assert!(ret.unwrap() < N);
            });
        });
    }

    pub fn transfer_n(b: &mut Bencher, n_threads: usize) {
        b.bench_n(1, |_b| {
            const NUM_ELEMENTS: usize = 256 * 256;
            let source = Arc::new(Queue::new());
            for i in 0..NUM_ELEMENTS {
                source.push(i);
            }
            let sink = Arc::new(Queue::new());
            let pair = Arc::new((Mutex::new(false), Condvar::new()));
            let mut threads = Vec::with_capacity(n_threads);
            for _ in 0..n_threads {
                let p = pair.clone();
                let source = source.clone();
                let sink = sink.clone();
                let handle = ::std::thread::spawn(move || {
                    let &(ref lock, ref cvar) = &*p;
                    let mut started = lock.lock().unwrap();
                    while !*started {
                        started = cvar.wait(started).unwrap();
                    }
                    drop(started);
                    while let Some(i) = source.pop() {
                        sink.push(i);
                    }
                });
                threads.push(handle);
            }
            _b.iter(|| {
                let &(ref lock, ref cvar) = &*pair;
                let mut started = lock.lock().unwrap();
                *started = true;
                drop(started);
                cvar.notify_all();
                for i in (0..n_threads).rev() {
                    let t = threads.remove(i);
                    let _ = t.join();
                }
            });

        });
    }

    macro_rules! transfer_ {
        ($name:ident, $n:expr) => {
            pub fn $name(b: &mut Bencher) { transfer_n(b, $n); }
        }
    }

    transfer_!(transfer_1, 1);
    transfer_!(transfer_2, 2);
    transfer_!(transfer_4, 4);
    transfer_!(transfer_8, 8);
    transfer_!(transfer_16, 16);
    transfer_!(transfer_32, 32);
}

mod ebr {
    use super::Bencher;
    use comere::ebr::queue::Queue;
    use comere::ebr::pin;

    use std::sync::{Arc, Condvar, Mutex};
    use std::mem::drop;

    pub fn push(b: &mut Bencher) {
        let queue = Queue::new();
        b.iter(|| { pin(|pin| { queue.push(0usize, pin); }); })
    }

    pub fn pop(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let queue = Queue::new();
            pin(|pin| for i in 0..N {
                queue.push(i, pin);
            });
            _b.iter(|| {
                let ret = pin(|pin| queue.pop(pin));
                assert!(ret.unwrap() < N);
            });
        });
    }

    pub fn pop_pin_outer(b: &mut Bencher) {
        const N: u64 = 1024;
        b.bench_n(N, |_b| {
            let queue = Queue::new();
            pin(|pin| for i in 0..N {
                queue.push(i, pin);
            });
            pin(|pin| {
                _b.iter(|| {
                    let ret = queue.pop(pin);
                    assert!(ret.unwrap() < N);
                });
            })
        });
    }

    pub fn transfer_n(b: &mut Bencher, n_threads: usize) {
        b.bench_n(1, |_b| {
            const NUM_ELEMENTS: usize = 256 * 256;
            let source = Arc::new(Queue::new());
            pin(|pin| for i in 0..NUM_ELEMENTS {
                source.push(i, pin);
            });
            let sink = Arc::new(Queue::new());
            let pair = Arc::new((Mutex::new(false), Condvar::new()));
            let mut threads = Vec::with_capacity(n_threads);
            for i in 0..n_threads {
                let p = pair.clone();
                let source = source.clone();
                let sink = sink.clone();
                let handle = ::std::thread::spawn(move || {
                    let &(ref lock, ref cvar) = &*p;
                    let mut started = lock.lock().unwrap();
                    while !*started {
                        started = cvar.wait(started).unwrap();
                    }
                    drop(started);
                    while let Some(i) = pin(|pin| source.pop(pin)) {
                        pin(|pin| sink.push(i, pin));
                    }
                });
                threads.push(handle);
            }
            _b.iter(|| {
                let &(ref lock, ref cvar) = &*pair;
                let mut started = lock.lock().unwrap();
                *started = true;
                drop(started);
                cvar.notify_all();
                for i in (0..n_threads).rev() {
                    let t = threads.remove(i);
                    let _ = t.join();
                }
            });

        });
    }
    macro_rules! transfer_ {
        ($name:ident, $n:expr) => {
            pub fn $name(b: &mut Bencher) { transfer_n(b, $n); }
        }
    }

    transfer_!(transfer_1, 1);
    transfer_!(transfer_2, 2);
    transfer_!(transfer_4, 4);
    transfer_!(transfer_8, 8);
    transfer_!(transfer_16, 16);
    transfer_!(transfer_32, 32);
}

mod crossbeam_bench {
    use super::Bencher;
    use crossbeam::sync::MsQueue;

    use std::sync::{Arc, Condvar, Mutex};
    use std::mem::drop;

    fn time() -> u64 {
        ::time::precise_time_ns()
    }

    pub fn transfer_n(b: &mut Bencher, n_threads: usize) {
        b.bench_n(1, |_b| {
            const NUM_ELEMENTS: usize = 256 * 256;
            let source = Arc::new(MsQueue::new());
            for i in 0..NUM_ELEMENTS {
                source.push(i);
            }
            let sink = Arc::new(MsQueue::new());
            let pair = Arc::new((Mutex::new(false), Condvar::new()));
            let mut threads = Vec::with_capacity(n_threads);
            for i in 0..n_threads {
                let p = pair.clone();
                let source = source.clone();
                let sink = sink.clone();
                let handle = ::std::thread::spawn(move || {
                    let &(ref lock, ref cvar) = &*p;
                    let mut started = lock.lock().unwrap();
                    while !*started {
                        started = cvar.wait(started).unwrap();
                    }
                    drop(started);
                    let t0 = time();
                    while let Some(i) = source.try_pop() {
                        sink.push(i);
                    }
                    let t1 = time();
                    // println!("[b] thread {:2} finished in {:10}ns", i, t1 - t0);
                });
                threads.push(handle);
            }
            _b.iter(|| {
                let t0 = time();
                let &(ref lock, ref cvar) = &*pair;
                let mut started = lock.lock().unwrap();
                *started = true;
                drop(started);
                cvar.notify_all();
                for i in (0..n_threads).rev() {
                    let t = threads.remove(i);
                    let _ = t.join();
                }
                let t1 = time();
                // println!("[b] main      finished in {:10}ns\n", t1 - t0);
            });

        });
    }

    pub fn transfer_n_barrier(b: &mut Bencher, n_threads: usize) {
        b.bench_n(1, |_b| {
            const NUM_ELEMENTS: usize = 256 * 256;
            let source = Arc::new(MsQueue::new());
            for i in 0..NUM_ELEMENTS {
                source.push(i);
            }
            let sink = Arc::new(MsQueue::new());
            use std::sync::Barrier;
            let barrier = Arc::new(Barrier::new(n_threads + 1));
            let mut threads = Vec::with_capacity(n_threads);
            for i in 0..n_threads {
                let source = source.clone();
                let sink = sink.clone();
                let barrier = barrier.clone();
                let handle = ::std::thread::spawn(move || {
                    barrier.wait();
                    let t0 = time();
                    while let Some(i) = source.try_pop() {
                        sink.push(i);
                    }
                    let t1 = time();
                    // println!("[b] thread {:2} finished in {:10}ns", i, t1 - t0);
                });
                threads.push(handle);
            }
            _b.iter(|| {
                let t0 = time();
                barrier.wait();
                for i in (0..n_threads).rev() {
                    let t = threads.remove(i);
                    let _ = t.join();
                }
                let t1 = time();
                // println!("[b] main      finished in {:10}ns\n", t1 - t0);
            });

        });
    }

    macro_rules! transfer_ {
        ($name:ident, $n:expr) => {
            pub fn $name(b: &mut Bencher) { transfer_n(b, $n); }
        }
    }

    transfer_!(transfer_1, 1);
    transfer_!(transfer_2, 2);
    transfer_!(transfer_4, 4);
    transfer_!(transfer_8, 8);
    transfer_!(transfer_16, 16);
    transfer_!(transfer_32, 32);

    pub fn aransfer_barrier_1(b: &mut Bencher) {
        transfer_n_barrier(b, 1);
    }
    pub fn aransfer_barrier_2(b: &mut Bencher) {
        transfer_n_barrier(b, 2);
    }
    pub fn aransfer_barrier_4(b: &mut Bencher) {
        transfer_n_barrier(b, 4);
    }
}

benchmark_group!(nothing_queue, nothing::push, nothing::pop);
benchmark_group!(
    hp_queue,
    hp::push,
    hp::pop,
    hp::transfer_1,
    hp::transfer_2,
    hp::transfer_4 // hp::transfer_8,
                   // hp::transfer_16,
                   // hp::transfer_32
);
benchmark_group!(
    ebr_queue,
    ebr::push,
    ebr::pop,
    ebr::pop_pin_outer,
    ebr::transfer_1,
    ebr::transfer_2,
    ebr::transfer_4 // ebr::transfer_8,
                    // ebr::transfer_16,
                    // ebr::transfer_32
);
benchmark_group!(
    crossbeam_bench,
    crossbeam_bench::aransfer_barrier_1,
    crossbeam_bench::aransfer_barrier_2,
    crossbeam_bench::aransfer_barrier_4,

    crossbeam_bench::transfer_1,
    crossbeam_bench::transfer_2,
    crossbeam_bench::transfer_4 // crossbeam_bench::transfer_8,
                                // crossbeam_bench::transfer_16,
                                // crossbeam_bench::transfer_32
);
benchmark_main!(hp_queue, ebr_queue, nothing_queue, crossbeam_bench);
