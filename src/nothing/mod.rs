/// This module contains implementation of some concurrent data structures
/// without any memory reclamation. This is used as a baseline in comparing
/// different memory reclamation schemes.
#[allow(unused_variables)]
#[allow(dead_code)]
mod atomic;

pub mod queue;
pub mod list;
