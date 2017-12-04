extern crate bench;

pub struct F(pub fn(usize) -> bench::BenchStats);

impl F {
    pub fn call(self, u: usize) -> bench::BenchStats {
        (self.0)(u)
    }
}

macro_rules! run {
  ($num_threads:expr, $($f:expr),*) => {
    vec![$(
        (F($f), stringify!($f).to_string()),
      )*
    ].into_iter()
     .map(|(f, name)| (f.call($num_threads), name))
     .collect::<Vec<(bench::BenchStats, String)>>()
  }
}

pub const NUM_ELEMENTS: usize = 256 * 256;
