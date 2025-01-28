#![feature(new_zeroed_alloc)]
#![no_std]

pub mod cmos;
pub mod executor;
pub mod heuristic;
pub mod mutation_engine;

extern crate alloc;
