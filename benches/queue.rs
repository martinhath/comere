#[macro_use]
extern crate bencher;
extern crate comere;

use bencher::Bencher;

mod nothing {
    use super::Bencher;
    use comere::nothing::queue::Queue;

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
}

mod hp {
    use super::Bencher;
    use comere::hp::queue::Queue;

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
}

mod ebr {
    use super::Bencher;
    use comere::ebr::queue::Queue;
    use comere::ebr::pin;

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
}

benchmark_group!(nothing_queue, nothing::push, nothing::pop);
benchmark_group!(hp_queue, hp::push, hp::pop);
benchmark_group!(ebr_queue, ebr::push, ebr::pop, ebr::pop_pin_outer);
benchmark_main!(hp_queue, ebr_queue, nothing_queue);
