#!/bin/bash
CSV="column -s';' -te"
THREADS="1 2 4 8 16"
NAME=$(date -Iseconds)
export RUSTFLAGS="-C target-cpu=native"


for t in $(echo $THREADS); do
  echo "RUNNING WITH $t THREADS" >> "$NAME"
  cargo run --release --bin benches-hp | $CSV >> "$NAME"
  cargo run --release --bin benches-hp-spin --features hp-wait | $CSV >> "$NAME"
  cargo run --release --bin benches-ebr | $CSV >> "$NAME"
  cargo run --release --bin benches-crossbeam | $CSV >> "$NAME"
  cargo run --release --bin benches-nothing | $CSV >> "$NAME"
done
