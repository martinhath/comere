#![feature(asm)]
#![allow(dead_code)]
/// A Benchmark runner.
///
/// We use this instead of `rustc-test` or `bencher` in order to make it exactly as we want it to
/// behave, as we need very specific things to happen, in order to go around the thread cleanup
/// problem.

extern crate time;

pub struct Bencher<S> {
    samples: Vec<u64>,
    n: usize,
    pre: Box<Fn(&mut S)>,
    post: Box<Fn(&mut S)>,
    between: Box<Fn(&mut S)>,
}

pub fn black_box<T>(dummy: T) -> T {
    // we need to "use" the argument in some way LLVM can't
    // introspect.
    unsafe { asm!("" : : "r"(&dummy)) }
    dummy
}

impl<S> Bencher<S> {
    fn new() -> Self {
        Bencher {
            samples: vec![],
            n: 10_000,
            pre: Box::new(|_| {}),
            post: Box::new(|_| {}),
            between: Box::new(|_| {}),
        }
    }

    fn bench<F: Fn(&S)>(&mut self, mut state: S, f: F) {
        (self.pre)(&mut state);
        for _ in 0..self.n {
            let t0 = time::precise_time_ns();
            black_box(f(&state));
            let t1 = time::precise_time_ns();
            self.samples.push((t1 - t0) / 1000);
            (self.between)(&mut state);
        }
        (self.post)(&mut state);
        self.print();
    }

    fn print(&self) {
        let len = self.samples.len() as u64;
        let sum = self.samples.iter().cloned().sum::<u64>();
        let avg = sum / len;
        let var = self.samples
            .iter()
            .cloned()
            .map(|s| (if s < avg { (avg - s) } else { s - avg }).pow(2))
            .sum::<u64>() / len;
        println!(
            "Bench: ................  {} ns/iter (+/- {})",
            fmt_thousands_sep(avg),
            fmt_thousands_sep(var)
        );
    }

    fn pre<F: 'static + Fn(&mut S)>(&mut self, f: F) {
        self.pre = Box::new(f);
    }

    fn post<F: 'static + Fn(&mut S)>(&mut self, f: F) {
        self.post = Box::new(f);
    }

    fn between<F: 'static + Fn(&mut S)>(&mut self, f: F) {
        self.between = Box::new(f);
    }
}

fn fmt_thousands_sep(mut n: u64) -> String {
    let sep = ',';
    use std::fmt::Write;
    let mut output = String::new();
    let mut trailing = false;
    for &pow in &[9, 6, 3, 0] {
        let base = 10u64.pow(pow);
        if pow == 0 || trailing || n / base != 0 {
            if !trailing {
                output.write_fmt(format_args!("{}", n / base)).unwrap();
            } else {
                output.write_fmt(format_args!("{:03}", n / base)).unwrap();
            }
            if pow != 0 {
                output.push(sep);
            }
            trailing = true;
        }
        n %= base;
    }

    output
}

mod test {
    use super::*;

    #[test]
    fn state_bencher() {
        struct State {
            num: i32,
        };

        let mut b = Bencher::new();
        b.bench(State { num: 123 }, |state| {});
    }

    #[test]
    fn give_closures() {
        struct State {
            num: i32,
        };
        let mut b = Bencher::<State>::new();
        b.pre(|s| s.num += 1);
        b.post(|s| assert!(s.num == 1i32));
        b.bench(State { num: 0i32 }, |_| {});
    }
}
