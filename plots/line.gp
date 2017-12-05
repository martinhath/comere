#!/bin/gnuplot
# Generate a line plot
# Input data should be as follows:
#  ---------------------------
# |  scheme   scheme  scheme
# |    1        1       1
# |    2        2       2

# Note: this **MUST** be the same order as in `./generate-plots.sh`
schemes = "crossbeam ebr hp hp-spin"

set terminal pdf size 10cm,10cm
set title title
set output output
plot for [i=1:cols] data using i title word(schemes, i) with lines smooth bezier
