#![cfg(windows)]

pub mod error;
#[cfg(feature = "net")]
pub mod net;
pub mod task;
pub mod threadpool;

mod context;

mod sync {
    #[cfg(feature = "parking_lot")]
    pub(crate) use parking_lot::Once;
    #[cfg(not(feature = "parking_lot"))]
    pub(crate) use std::sync::Once;

    #[cfg(feature = "parking_lot")]
    pub(crate) use parking_lot::Mutex;
    #[cfg(not(feature = "parking_lot"))]
    pub(crate) struct Mutex<T>(std::sync::Mutex<T>);
    #[cfg(not(feature = "parking_lot"))]
    impl<T> Mutex<T> {
        pub(crate) fn new(t: T) -> Self {
            Self(std::sync::Mutex::new(t))
        }

        pub(crate) fn lock(&self) -> std::sync::MutexGuard<T> {
            self.0.lock().unwrap()
        }
    }
}

pub use crate::{task::spawn, threadpool::Threadpool};

#[cfg(feature = "macros")]
#[doc(inline)]
pub use wae_macros::*;
