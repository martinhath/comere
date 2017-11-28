#![feature(test)]
// TODO: remove this
#![feature(const_fn, const_atomic_usize_new)]
#![feature(alloc_system, global_allocator, allocator_api)]
extern crate alloc_system;
use alloc_system::System;
#[global_allocator]
static A: System = System;

extern crate bench;

#[macro_use]
extern crate lazy_static;

#[cfg(test)]
extern crate rand;

#[allow(unused_variables)]
#[allow(dead_code)]
pub mod nothing;
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod ebr;
#[allow(unused_variables)]
#[allow(dead_code)]
pub mod hp;
