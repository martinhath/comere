#!/bin/bash

THREADS="1 2 4 8"
EXECS="queue-crossbeam queue-ebr queue-hp"

for e in $(echo "$EXECS"); do
  for n in $(echo "$THREADS"); do
    printf "Run %s with %s threads\n" "$n" "$e"
    cargo run --release --bin "$e" -- "$n"
  done
done

gnuplot -persist gnuplot
