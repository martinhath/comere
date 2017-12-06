extern crate crossbeam;
extern crate bench;

#[macro_use]
use std::env;
use std::thread;

use crossbeam::sync::MsQueue;


// fn main() {
//     let args = env::args().collect::<Vec<_>>();
//     let num_threads: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
//     let gnuplot_output = args.get(2);
// 
//     let stats: Vec<bench::BenchStats> = [nop, queue_push, queue_pop, queue_transfer]
//         .iter()
//         .map(|f| f(num_threads))
//         .collect();
// 
//     if let Some(fname) = gnuplot_output {
//         use std::io::Write;
//         use std::fs::File;
//         let mut f = File::create(fname).unwrap();
//         f.write_all(bench::gnuplot(&stats).as_bytes()).unwrap();
//     }
// }
