# comere
Concurrent Memory Reclamation using Reference Counting

This repository contains different memory reclamation schemes for
concurrent data structures in Rust.
The goal of the repository is to provide benchmarks for the different
schemes, in differnet use cases.

Some of the code, especially the `atomic.rs` files, are more or less
borrowed (copied) from 
[`crossbeam-epoch`](http://www.github.com/crossbeam-rs/crossbeam-epoch).
The data structures also draws heavy inspiration from the crossbeam project.
