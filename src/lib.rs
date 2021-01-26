#![cfg(windows)]

pub mod error;
pub mod task;
pub mod threadpool;

mod context;

mod sync {
    #[cfg(feature = "parking_lot")]
    pub use parking_lot::Once;
    #[cfg(not(feature = "parking_lot"))]
    pub use std::sync::Once;
}

pub use crate::{task::spawn, threadpool::Threadpool};

#[cfg(feature = "macros")]
#[doc(inline)]
pub use wae_macros::*;
