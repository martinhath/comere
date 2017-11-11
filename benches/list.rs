#[macro_use]
extern crate bencher;
extern crate comere;

use bencher::Bencher;

pub mod nothing {
    use super::Bencher;
    use comere::nothing::list::List;
    pub fn insert(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let list = List::new();
            _b.iter(|| { list.insert(0usize); });
        });
    }

    pub fn remove_front(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        let list = List::new();
        for i in 0..N {
            list.insert(i);
        }
        b.iter(|| { list.remove_front(); });
    }
}

mod hp {
    use super::Bencher;
    use comere::hp::list::List;
    pub fn insert(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let list = List::new();
            _b.iter(|| { list.insert(0usize); });
        });
    }

    pub fn remove_front(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let list = List::new();
            for i in 0..N {
                list.insert(i);
            }
            _b.iter(|| {
                let ret = list.remove_front();
                assert!(ret.unwrap() < N);
            });
        });
    }
}

mod ebr {
    use super::Bencher;
    use comere::ebr::list::List;
    use comere::ebr::pin;
    pub fn insert(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let list = List::new();
            _b.iter(|| pin(|pin| list.insert(0usize, pin)));
        });
    }

    pub fn remove_front(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let list = List::new();
            for i in 0..N {
                pin(|pin| list.insert(i, pin));
            }
            _b.iter(|| {
                let ret = pin(|pin| list.remove_front(pin));
                assert!(ret.unwrap() < N);
            });
        });
    }
}

benchmark_group!(nothing_list, nothing::insert, nothing::remove_front);
benchmark_group!(hp_list, hp::insert, hp::remove_front);
benchmark_group!(ebr_list, ebr::insert, ebr::remove_front);
benchmark_main!(hp_list, ebr_list, nothing_list);
