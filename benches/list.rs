#[macro_use]
extern crate bencher;
extern crate comere;

use bencher::Bencher;

pub mod nothing {
    //! The benchmarks for this module is somewhat different: since we never free any allocated
    //! memory, and the benchmarks runs a lot of time, it is very possible that we run out of
    //! memory, causing swapping which destroys any meaningful benchmark. For this reason, we
    //! allocate up front `N` pointers to `Owned<Node<T>>`, and store the allocated address of the
    //! allocated nodes in it. After `N` calls to `List::insert`, we `Drop` the `Vec`, and free all
    //! allocated memory. Since this is inside `bench_n` and not `iter`, this in not included in
    //! the benchmark timing.

    use super::Bencher;
    use comere::nothing::list::List;
    use comere::nothing::atomic::Owned;
    use comere::nothing::list::Node;

    pub fn insert(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let list = List::new();
            let mut ptrs: Vec<Owned<Node<usize>>> = Vec::with_capacity(N as usize);
            let ptr = ptrs.as_mut_ptr();
            let mut i = 0;
            _b.iter(|| {
                list.insert(0usize, Some(unsafe { ptr.offset(i) }));
                i += 1;
            });
            unsafe {
                ptrs.set_len(N as usize);
            }
        });
    }

    pub fn remove_front(b: &mut Bencher) {
        const N: u64 = 1024 * 1024;
        b.bench_n(N, |_b| {
            let list = List::new();
            let mut ptrs = Vec::with_capacity(N as usize);
            let ptr = ptrs.as_mut_ptr();
            let mut c = 0;
            for i in 0..N {
                list.insert(i, Some(unsafe { ptr.offset(c) }));
                c += 1;
            }
            _b.iter(|| {
                let ret = list.remove_front();
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
