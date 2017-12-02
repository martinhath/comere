#!/bin/bash

THREADS="1 2 4 8"
BENCHES="queue-transfer"
VARIANTS="crossbeam ebr hp"

for variant in $(echo "$VARIANTS"); do
  for bench in $(echo "$BENCHES"); do
    files=""
    for n in $(echo "$THREADS"); do
      cargo run --release --bin "$bench-$variant" -- "$n"
      files+="$bench-$variant-$n "
    done
    paste -d" " $files > "$bench-$variant"
    rm $files
  done
done

# gnuplot -persist gnuplot
