#![feature(try_trait_v2)]
#![feature(min_specialization)]
#![warn(clippy::partial_pub_fields)]

#[macro_use]
extern crate tracing;

pub mod job;
#[cfg(feature = "http-service")]
pub mod service;
