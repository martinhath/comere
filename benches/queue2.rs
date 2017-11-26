#![feature(test)]
extern crate test;
extern crate comere;

mod ebr {
    use std::sync::{Arc, Barrier, Condvar, Mutex};
    use std::thread::spawn;
    use test::Bencher;
    use test::black_box;

    use comere::ebr::pin;
    use comere::ebr::queue::Queue;

    #[bench]
    pub fn transfer_1(b: &mut Bencher) {
        transfer_n(b, 1);
    }
    #[bench]
    pub fn transfer_2(b: &mut Bencher) {
        transfer_n(b, 2);
    }
    #[bench]
    pub fn transfer_4(b: &mut Bencher) {
        transfer_n(b, 4);
    }
    #[bench]
    pub fn transfer_8(b: &mut Bencher) {
        transfer_n(b, 8);
    }

    pub fn transfer_n(b: &mut Bencher, num_threads: usize) {
        const NUM_ELEMENTS: usize = 256 * 256;
        #[derive(Clone, Copy, PartialEq)]
        enum State {
            Wait,
            Run,
            Exit,
        };
        let state = Arc::new(Mutex::new(State::Wait));
        let condvar = Arc::new(Condvar::new());
        let barrier = Arc::new(Barrier::new(num_threads + 1));

        let source = Arc::new(Queue::new());
        pin(|pin| for i in 0..NUM_ELEMENTS {
            source.push(i, pin);
        });
        let sink = Arc::new(Queue::new());

        let threads = (0..num_threads)
            .map(|i| {
                let state = state.clone();
                let condvar = condvar.clone();
                let barrier = barrier.clone();
                let source = source.clone();
                let sink = sink.clone();
                println!("SPAWN");
                spawn(move || loop {
                    let mut started = state.lock().unwrap();
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

                            let mut c = 0;
                            while let Some(i) = pin(|pin| source.pop(pin)) {
                                pin(|pin| sink.push(i, pin));
                                c += 1;
                            }
                            println!("thread {} moved {} elements", i, c);

                            // BODY END HERE ///////////////////////////////////
                        }
                        State::Wait => unreachable!(),
                    }
                    barrier.wait();
                    barrier.wait();
                })
            })
            .collect::<Vec<_>>();

        b.iter(|| {
            let mut s = state.lock().unwrap();
            *s = State::Run;
            drop(s);
            condvar.notify_all();

            barrier.wait();
            *state.lock().unwrap() = State::Wait;
            barrier.wait();
        });

        let mut s = state.lock().unwrap();
        *s = State::Exit;
        drop(s);
        condvar.notify_all();

        for t in threads.into_iter() {
            t.join().unwrap();
        }
    }

}
