#![feature(asm)]
#![allow(dead_code)]
/// A Benchmark runner.
///
/// We use this instead of `rustc-test` or `bencher` in order to make it exactly as we want it to
/// behave, as we need very specific things to happen, in order to go around the thread cleanup
/// problem.

extern crate time;

use std::str::FromStr;
use std::sync::mpsc::{Sender, Receiver, channel};
use std::sync::{Arc, Barrier};
use std::thread;

const DEFAULT_NUM_SAMPLES: usize = 200;

#[derive(Debug, Clone)]
pub struct BenchStats {
    ident: BenchIdentifier,
    samples: Vec<u64>,
}

impl BenchStats {
    pub fn variant(&self) -> &str {
        &self.ident.variant
    }
    pub fn name(&self) -> &str {
        &self.ident.name
    }
    pub fn threads(&self) -> usize {
        self.ident.threads
    }

    pub fn string(&self) -> String {
        self.ident.string()
    }
}

#[derive(Debug, Clone)]
pub struct BenchIdentifier {
    variant: String,
    name: String,
    threads: usize,
}

impl BenchIdentifier {
    pub fn string(&self) -> String {
        format!("{}::{}::{:02}", self.variant, self.name, self.threads)
    }
}

impl FromStr for BenchIdentifier {
    type Err = ();
    fn from_str(s: &str) -> Result<BenchIdentifier, Self::Err> {
        let split = s.split("::").collect::<Vec<_>>();
        Ok(match split.len() {
            3 => {
                BenchIdentifier {
                    variant: split[0].to_string(),
                    name: split[1].to_string(),
                    threads: split[2].parse().map_err(|_| ())?,
                }
            }
            4 => {
                BenchIdentifier {
                    variant: split[0].to_string(),
                    name: format!("{}_{}", split[1], split[2]),
                    threads: split[3].parse().map_err(|_| ())?,
                }
            }
            _ => return Err(()),
        })
    }
}

impl BenchStats {
    fn len(&self) -> u64 {
        self.samples.len() as u64
    }

    pub fn average(&self) -> u64 {
        self.samples.iter().cloned().sum::<u64>() / self.len()
    }

    pub fn variance(&self) -> u64 {
        let avg = self.average();
        let s = self.samples
            .iter()
            .cloned()
            .map(|s| (if s < avg { (avg - s) } else { s - avg }).pow(2))
            .sum::<u64>() / self.len();
        (s as f32).sqrt() as u64
    }

    pub fn min(&self) -> u64 {
        self.samples.iter().cloned().min().unwrap()
    }

    pub fn max(&self) -> u64 {
        self.samples.iter().cloned().max().unwrap()
    }

    pub fn above_avg(&self) -> u64 {
        let avg = self.average();
        self.samples.iter().filter(|&&s| s > avg).count() as u64
    }

    pub fn below_avg(&self) -> u64 {
        let avg = self.average();
        self.samples.iter().filter(|&&s| s < avg).count() as u64
    }

    pub fn report(&self) -> String {
        format!(
            "{} ns/iter (+/- {}) min={} max={} above={} below={}",
            Self::fmt_thousands_sep(self.average()),
            Self::fmt_thousands_sep(self.variance()),
            self.min(),
            self.max(),
            self.above_avg(),
            self.below_avg()
        )
    }

    pub fn csv_header() -> String {
        format!(
            "{};{};{};{};{};{}",
            "average",
            "variance",
            "min",
            "max",
            "# above avg",
            "# below avg"
        )
    }

    pub fn csv(&self) -> String {
        format!(
            "{};{};{};{};{};{}",
            Self::fmt_thousands_sep(self.average()),
            Self::fmt_thousands_sep(self.variance()),
            self.min(),
            self.max(),
            self.above_avg(),
            self.below_avg()
        )
    }

    pub fn samples(&self) -> &[u64] {
        &self.samples
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
}

/// Turn the statistics given into a gnuplot data string.
pub fn gnuplot(stats: &[BenchStats]) -> String {
    let mut s = String::new();
    let lines = stats.iter().map(|b| b.samples.len()).max().unwrap_or(0);
    for stats in stats {
        let asd: String = stats.ident.string();
        s.push_str(&asd);
    }
    s.push('\n');
    for i in 0..lines {
        for stat in stats {
            s.push_str(&format!("{} ", stat.samples.get(i).cloned().unwrap_or(0)));
        }
        s.push('\n');
    }

    s
}

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
        state
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

    pub fn into_stats(self, name: String) -> BenchStats {
        BenchStats {
            samples: self.samples,
            ident: BenchIdentifier::from_str(&name).unwrap(),
        }
    }
}

pub trait Spawner {
    type Return;
    type Result;

    fn spawn<F>(f: F) -> Self
    where
        F: FnOnce() -> Self::Return,
        F: Send + 'static,
        Self::Return: Send + 'static;

    fn join(self) -> Self::Result;
}

pub struct StdThread<T> {
    handle: std::thread::JoinHandle<T>,
}

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

impl<T> Spawner for StdThread<T>
where
    T: Send,
{
    type Return = T;
    type Result = thread::Result<T>;

    fn spawn<F>(f: F) -> Self
    where
        F: FnOnce() -> Self::Return,
        F: Send + 'static,
        Self::Return: Send + 'static,
    {
        StdThread { handle: thread::spawn(f) }
    }

    fn join(self) -> Self::Result {
        self.handle.join()
    }
}

pub struct ThreadBencher<S, Sp: Spawner> {
    samples: Vec<u64>,
    state: S,
    n: usize,
    threads: Vec<Sp>,
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
            .map(|_thread_id| {
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
                            Ok(ThreadSignal::End) => {
                                break;
                            }
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
            n: DEFAULT_NUM_SAMPLES,
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

    pub fn into_stats(self, name: String) -> BenchStats {
        self.threads.into_iter().map(Spawner::join).count();
        BenchStats {
            samples: self.samples,
            ident: BenchIdentifier::from_str(&name).unwrap(),
        }
    }
}

fn flush() {
    use std::io::Write;
    let _ = ::std::io::stdout().flush();
}


#[cfg(test)]
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
