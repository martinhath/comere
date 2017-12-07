#!/bin/bash

# Run all benchmarks.

SCHEMES="nothing hp hp-spin ebr cb"
THREADS="1 2 4"
DATE=`date +"%Y-%m-%d-%H:%M:%S"`

OUTPUT="output-$DATE"
if [[ ! -d "$OUTPUT" ]] ; then mkdir "$OUTPUT" ; fi

export RUSTFLAGS="-C target-cpu=native -C opt-level=3"

for t in $(echo "$THREADS"); do
  cargo run --release -- -t "$t" -d "$OUTPUT"
done

for t in $(echo "$THREADS"); do
  cargo run --release --features hp-wait -- -t "$t" -d "$OUTPUT" hp
done

# Since the date contains : we must tell tar to not interpret it as a port (or something).
tar -zc --force-local -f "$DATE".tar.gz "$OUTPUT"
