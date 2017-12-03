#![feature(asm)]
#![allow(dead_code)]
/// A Benchmark runner.
///
/// We use this instead of `rustc-test` or `bencher` in order to make it exactly as we want it to
/// behave, as we need very specific things to happen, in order to go around the thread cleanup
/// problem.

extern crate time;

use std::sync::{Arc, Barrier};

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
    pub fn new() -> Self {
        Bencher {
            samples: vec![],
            n: 10_000,
            pre: Box::new(|_| {}),
            post: Box::new(|_| {}),
            between: Box::new(|_| {}),
        }
    }

    pub fn set_n(&mut self, n: usize) {
        self.n = n;
    }

    pub fn bench<F: Fn(&mut S)>(&mut self, mut state: S, f: F) -> S {
        (self.pre)(&mut state);
        for _ in 0..self.n {
            let t0 = time::precise_time_ns();
            black_box(f(&mut state));
            let t1 = time::precise_time_ns();
            self.samples.push(t1 - t0);
            (self.between)(&mut state);
        }
        (self.post)(&mut state);
        self.print();
        state
    }

    fn print(&self) {
        let len = self.samples.len() as u64;
        let sum = self.samples.iter().cloned().sum::<u64>();
        let avg = sum / len;
        let var = {
            let s = self.samples
                .iter()
                .cloned()
                .map(|s| (if s < avg { (avg - s) } else { s - avg }).pow(2))
                .sum::<u64>() / len;
            (s as f32).sqrt() as u64
        };
        let max = self.samples.iter().cloned().max().unwrap();
        let min = self.samples.iter().cloned().min().unwrap();
        let above = self.samples.iter().filter(|&&s| s > avg).count();
        let below = self.samples.len() - above;
        println!(
            "Bench: ................  {} ns/iter (+/- {}) min={} max={} above={} below={}",
            fmt_thousands_sep(avg),
            fmt_thousands_sep(var),
            min,
            max,
            above,
            below
        );
    }

    pub fn output_samples<W: ::std::io::Write>(
        &self,
        mut writer: W,
    ) -> Result<(), ::std::io::Error> {
        for sample in &self.samples {
            writer.write_fmt(format_args!("{}\n", sample))?;
        }
        Ok(())
    }

    pub fn pre<F: 'static + Fn(&mut S)>(&mut self, f: F) {
        self.pre = Box::new(f);
    }

    pub fn post<F: 'static + Fn(&mut S)>(&mut self, f: F) {
        self.post = Box::new(f);
    }

    pub fn between<F: 'static + Fn(&mut S)>(&mut self, f: F) {
        self.between = Box::new(f);
    }
}

pub trait Spawner {
    type Handle;
    type Return;
    type Result;

    fn spawn<F>(f: F) -> Self::Handle
    where
        F: FnOnce() -> Self::Return,
        F: Send + 'static,
        Self::Return: Send + 'static;

    fn join(self) -> Self::Result;
}

use std::sync::mpsc::{Sender, Receiver, channel};
use std::thread;

#[derive(Debug)]
struct FunctionPtr<State> {
    data: usize,
    state: *const State,
    _marker: ::std::marker::PhantomData<State>,
}

impl<S> Clone for FunctionPtr<S> {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            state: self.state,
            _marker: ::std::marker::PhantomData,
        }
    }
}

impl<S> FunctionPtr<S> {
    fn new(f: fn(&S), state: &S) -> Self {
        FunctionPtr {
            data: f as usize,
            state: &*state,
            _marker: ::std::marker::PhantomData,
        }
    }

    fn call(&mut self) {
        unsafe {
            let f = ::std::mem::transmute::<usize, fn(&S)>(self.data);
            (f)(&*self.state);
        }
    }
}

unsafe impl<S> Send for FunctionPtr<S> {}

#[derive(Debug)]
enum ThreadSignal<S> {
    Run(FunctionPtr<S>),
    Done(u64),
    End,
}

impl<T> Spawner for thread::JoinHandle<T>
where
    T: Send,
{
    type Handle = thread::JoinHandle<T>;
    type Return = T;
    type Result = thread::Result<T>;

    fn spawn<F>(f: F) -> Self::Handle
    where
        F: FnOnce() -> Self::Return,
        F: Send + 'static,
        Self::Return: Send + 'static,
    {
        thread::spawn(f)
    }

    fn join(self) -> Self::Result {
        self.join()
    }
}

pub struct ThreadBencher<S, Sp: Spawner> {
    samples: Vec<u64>,
    state: S,
    n: usize,
    threads: Vec<Sp::Handle>,
    senders: Vec<Sender<ThreadSignal<S>>>,
    receivers: Vec<Receiver<ThreadSignal<S>>>,
    before: Box<Fn(&mut S)>,
    after: Box<Fn(&mut S)>,
    barrier: Arc<Barrier>,
}

impl<St, Sp> ThreadBencher<St, Sp>
where
    St: 'static,
    Sp: Spawner,
    Sp::Return: Send + Default + 'static,
{
    pub fn new(state: St, n_threads: usize) -> Self {
        let mut senders = Vec::with_capacity(n_threads);
        let mut receivers = Vec::with_capacity(n_threads);
        let barrier = Arc::new(Barrier::new(n_threads + 1));
        // Start the threads, and give them channels for communication.
        let threads = (0..n_threads)
            .map(|_| {
                let (our_send, their_recv) = channel();
                let (their_send, our_recv) = channel();
                senders.push(our_send);
                receivers.push(our_recv);
                let barrier = barrier.clone();
                Sp::spawn(move || {
                    let recv = their_recv;
                    let send = their_send;
                    loop {
                        let signal = match recv.recv() {
                            Ok(ThreadSignal::Run(ref mut f)) => {
                                barrier.wait();
                                let t0 = time::precise_time_ns();
                                f.call();
                                let t1 = time::precise_time_ns();
                                ThreadSignal::Done(t1 - t0)
                            }
                            Ok(ThreadSignal::End) => break,
                            Ok(_) => unreachable!(),
                            Err(e) => panic!("{:?}", e),
                        };
                        assert!(send.send(signal).is_ok());
                    }
                    Default::default()
                })
            })
            .collect();
        Self {
            state,
            samples: vec![],
            n: 100,
            threads,
            senders,
            receivers,
            before: Box::new(|_| {}),
            after: Box::new(|_| {}),
            barrier,
        }
    }

    /// Start a threaded benchmark. All threads will run the function given. The state passed in is
    /// shared between all threads.
    pub fn thread_bench(&mut self, f: fn(&St)) {
        let func_ptr = FunctionPtr::new(f, &self.state);
        for sender in &self.senders {
            assert!(sender.send(ThreadSignal::Run(func_ptr.clone())).is_ok());
        }

        for _i in 0..self.n {
            (self.before)(&mut self.state);
            for sender in &self.senders {
                assert!(sender.send(ThreadSignal::Run(func_ptr.clone())).is_ok());
            }
            // TODO: this is not good: we risk waiting for a long time in `barrier.wait`
            let t0 = time::precise_time_ns();
            self.barrier.wait();
            for recv in self.receivers.iter() {
                match recv.recv() {
                    Ok(ThreadSignal::Done(_t)) => {
                        // OK
                    }
                    _ => panic!("Thread didn't return correctly"),
                }
            }
            let t1 = time::precise_time_ns();
            self.samples.push(t1 - t0);
        }
        for sender in &self.senders {
            assert!(sender.send(ThreadSignal::End).is_ok());
        }
        (self.after)(&mut self.state);
    }

    pub fn before<F: 'static + Fn(&mut St)>(&mut self, f: F) {
        self.before = Box::new(f);
    }

    pub fn after<F: 'static + Fn(&mut St)>(&mut self, f: F) {
        self.after = Box::new(f);
    }

    pub fn report(&self) -> String {
        let len = self.samples.len() as u64;
        let sum = self.samples.iter().cloned().sum::<u64>();
        let avg = sum / len;
        let var = {
            let s = self.samples
                .iter()
                .cloned()
                .map(|s| (if s < avg { (avg - s) } else { s - avg }).pow(2))
                .sum::<u64>() / len;
            (s as f32).sqrt() as u64
        };
        let max = self.samples.iter().cloned().max().unwrap();
        let min = self.samples.iter().cloned().min().unwrap();
        let above = self.samples.iter().filter(|&&s| s > avg).count();
        let below = self.samples.len() - above;
        format!(
            "Bench: ................  {} ns/iter (+/- {}) min={} max={} above={} below={}",
            fmt_thousands_sep(avg),
            fmt_thousands_sep(var),
            min,
            max,
            above,
            below
        )
    }
}

// This is borrowed from `test::Bencher` :)
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
        b.bench(State { num: 123 }, |_state| {});
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

    #[test]
    fn threaded() {
        #[derive(Debug, Default, Clone)]
        struct State;

        #[inline(never)]
        fn sample_function(_state: &State) {
            let mut s = 0;
            for i in 0..12345 {
                s += i;
            }
            black_box(s);
        }

        let mut b = ThreadBencher::<State, thread::JoinHandle<State>>::new(State, 4);
        b.thread_bench(sample_function);
        println!("{}", b.report());
    }
}
