#!/bin/gnuplot -persist

# Generate a box plot.

set style fill solid 0.25 border -1
set style boxplot outliers pointtype 7
set style data boxplot
set size square

set pointsize 0.3

set xtics ('Crossbeam' 1, 'EBR' 2, 'HP' 3, 'HP-Spin' 4)

set terminal pdf size 10cm,10cm
set title title
set output output
plot for [i=1:cols] data using (i):i notitle
