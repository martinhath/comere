#[macro_use]
extern crate clap;
extern crate crossbeam;
extern crate comere;
extern crate rand;
#[macro_use]
extern crate lazy_static;
extern crate bench;

use std::io::Write;
use std::fs::File;
use std::path::Path;

mod benches;
use benches::{nothing, hp, ebr, crossbeam as cb};
pub const NUM_ELEMENTS: usize = 256 * 256;
pub const NUM_ELEMENTS_NOTHING: usize = 256 * 256;
pub const NUM_ELEMENTS_SMALLER: usize = 256 * 4;


/// We need this, as somehow `(fn, String)` is not okay, while `(F(fn), String)` is.
pub struct F(pub fn(usize) -> bench::BenchStats);

impl F {
    pub fn call(&self, u: usize) -> bench::BenchStats {
        (self.0)(u)
    }
}

macro_rules! S {
  ($($f:expr),*) => {
    vec![$(
        (F($f), stringify!($f).to_string()),
      )*
    ]
  }
}

fn main() {
    let benches = S!(
        nothing::nop,
        nothing::queue_push,
        nothing::queue_pop,
        nothing::queue_transfer,
        cb::nop,
        cb::queue_push,
        cb::queue_pop,
        cb::queue_transfer,
        ebr::nop,
        ebr::list_remove,
        ebr::queue_push,
        ebr::queue_pop,
        ebr::queue_transfer,
        hp::nop,
        hp::list_remove,
        hp::queue_push,
        hp::queue_pop,
        hp::queue_transfer
    );

    let matches = clap_app!(benchmark_runner =>
        (version: "1.0")
        (author: "Martin Hafskjold Thoresen <martinhath@gmail.com")
        (@arg num_threads: -t +takes_value "Sets the number of threads in the benchmark")
        (@arg output_dir: -d +takes_value "Sets the output directory")
        (@arg name: +takes_value "The name of the benchmarks that is ran")
    ).get_matches();

    let num_threads: usize = value_t!(matches, "num_threads", String)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let filter_name = value_t!(matches, "name", String).unwrap_or("".to_string());
    let output_dir = value_t!(matches, "output_dir", String).unwrap_or(".".to_string());

    let stats: Vec<bench::BenchStats> = benches
        .iter()
        .filter(|&&(_, ref name)| name.contains(&filter_name))
        .map(|&(ref f, _)| f.call(num_threads))
        .collect();
    if stats.len() == 0 {
        panic!(
            "No benchmarks were left after matching with the pattern '{}'",
            filter_name
        );
    }

    for stat in stats.iter() {
        let output_filename = format!(
            "s:{}-b:{}-t:{}",
            stat.variant(),
            stat.name(),
            stat.threads()
        );

        let mut file = File::create(Path::new(&output_dir).join(output_filename)).unwrap();

        for sample in stat.samples() {
            write!(&mut file, "{}\n", sample).unwrap();
        }
    }
}
