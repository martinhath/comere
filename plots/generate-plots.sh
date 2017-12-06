#!/bin/bash

# Example usage:
# $ ./generate-plots.sh "crossbeam hp" "1 2"

if [[ "$1" =~ ^$ ]]; then
  VARIANTS="crossbeam ebr hp hp-spin"
else
  VARIANTS="$1"
fi

if [[ "$2" =~ ^$ ]]; then
  THREADS="1 2 4 8"
else
  THREADS="$2"
fi

for n in $(echo "$THREADS"); do
  files=""
  for variant in $(echo "$VARIANTS"); do
    files+="$variant-$n.data "
  done
  ./gnuplot-merge.awk $files

  # `col-1` is the data file for each benchmark. It is important that these
  # benchmarks are in the same order here as they are in the `.rs` files.
  # TODO: Maybe we should use `make` for this after all?
  gnuplot -e "data='col-2'; cols=4; title='Queue::Push, t=$n';     output='queue-push-$n.pdf'" box.gp
  gnuplot -e "data='col-3'; cols=4; title='Queue::Pop, t=$n';      output='queue-pop-$n.pdf'" box.gp
  gnuplot -e "data='col-4'; cols=4; title='Queue::Transfer, t=$n'; output='queue-transfer-$n.pdf'" box.gp
  gnuplot -e "data='col-5'; cols=3; title='List::Remove, t=$n';    output='list-remove-$n.pdf'" box.gp
done
