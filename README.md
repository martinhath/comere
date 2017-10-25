# Comere - Concurrent Memory Reclamation

This repository contains different memory reclamation schemes for
some concurrent data structures in Rust.
The goal of the repository is to provide benchmarks for the different
schemes, in differnet use cases.

Some of the code, especially the `atomic.rs` files, are more or less
borrowed (copied) from 
[`crossbeam-epoch`](http://www.github.com/crossbeam-rs/crossbeam-epoch).
The data structures also draws heavy inspiration from the crossbeam project.

This project is primarily developed in connection with a course at NTNU.
See [the report repo](https://github.com/martinhath/semester-project)
for the technical report, as well as a tentative plan + progress.
