#![cfg(windows)]
#![warn(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

#[cfg(feature = "io")]
pub mod io;
#[cfg(feature = "net")]
pub mod net;
pub mod task;
pub mod threadpool;

pub(crate) mod context;
pub(crate) mod util;

pub use crate::{context::current as context, task::spawn, threadpool::Threadpool};

#[cfg(feature = "macros")]
#[doc(inline)]
pub use wae_macros::*;
