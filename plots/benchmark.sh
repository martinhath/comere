#!/bin/bash

# Example usage:
# $ ./generate-plots.sh "crossbeam hp" "1 2"

COLUMN="column -s';' -t -N NAME,AVG,VAR,MIN,MAX,NABOVE,NBELOW -R AVG,VAR,MIN,MAX,NABOVE,NBELOW"

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

for variant in $(echo "$VARIANTS"); do
  files=""
  for n in $(echo "$THREADS"); do
    if [[ $variant =~ hp-spin ]]; then
      cargo run --release --bin "benches-$variant" --features "hp-wait"\
        -- "$n" "$variant-$n.data" | $COLUMN
    else
      cargo run --release --bin "benches-$variant" -- "$n" "$variant-$n.data" | $COLUMN
    fi
  done
done
