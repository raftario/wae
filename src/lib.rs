#![cfg(windows)]

#[cfg(feature = "net")]
pub mod net;
pub mod task;
pub mod threadpool;

pub(crate) mod context;
#[cfg(any(feature = "net"))]
pub(crate) mod overlapped;
pub(crate) mod sync;
pub(crate) mod util;

pub use crate::{context::current as context, task::spawn, threadpool::Threadpool};

#[cfg(feature = "macros")]
#[doc(inline)]
pub use wae_macros::*;
