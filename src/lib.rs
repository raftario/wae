#![cfg(windows)]

#[cfg(feature = "net")]
pub mod net;
#[cfg(any(feature = "net"))]
pub mod overlapped;
pub mod task;
pub mod threadpool;

pub(crate) mod context;
pub(crate) mod sync;
pub(crate) mod util;

pub use crate::{task::spawn, threadpool::Threadpool};

#[cfg(feature = "macros")]
#[doc(inline)]
pub use wae_macros::*;
