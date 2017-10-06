//! Epoch Based Reclamation (EBR). This is the same approach that `crossbeam-epoch`
//! is based on. It is low very overhead compared to eg. Hazard Pointers.

pub mod atomic;
