use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

pub(crate) struct IoState(AtomicUsize);

impl IoState {
    const IDLE: usize = 0;
    const SCHEDULING: usize = 1;
    const PENDING: usize = 2;
    const CALLBACK: usize = 3;
    const READY: usize = 4;
    const CANCELLED_NOWAIT: usize = 5;
    const CANCELLED_WAIT: usize = 6;

    pub(crate) const fn new() -> Self {
        Self(AtomicUsize::new(Self::IDLE))
    }

    pub(crate) fn set_idle(&self) {
        self.0.store(Self::IDLE, Ordering::Release)
    }

    pub(crate) fn schedule(&self) -> bool {
        self.0
            .compare_exchange(
                Self::IDLE,
                Self::SCHEDULING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub(crate) fn set_pending(&self) {
        self.0.store(Self::PENDING, Ordering::Release)
    }

    pub(crate) fn cancel(&self, wait: bool) -> bool {
        let old = if !wait {
            self.0.swap(Self::CANCELLED_NOWAIT, Ordering::Relaxed)
        } else {
            self.0.swap(Self::CANCELLED_WAIT, Ordering::Relaxed)
        };
        old != Self::CANCELLED_NOWAIT
    }

    pub(crate) fn callback_pending(&self) -> bool {
        while self.0.load(Ordering::Relaxed) == Self::SCHEDULING {
            thread::yield_now();
        }
        self.0
            .compare_exchange(
                Self::PENDING,
                Self::CALLBACK,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub(crate) fn callback_cancelled_nowait(&self) -> bool {
        self.0
            .compare_exchange(
                Self::CANCELLED_NOWAIT,
                Self::CALLBACK,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub(crate) fn callback_cancelled_wait(&self) -> bool {
        self.0
            .compare_exchange(
                Self::CANCELLED_WAIT,
                Self::CALLBACK,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub(crate) fn set_ready(&self) {
        self.0.store(Self::READY, Ordering::Release)
    }

    pub(crate) fn finish(&self) -> bool {
        self.0
            .compare_exchange(
                Self::READY,
                Self::READY,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub(crate) fn is_cancellable(&self) -> bool {
        self.0.load(Ordering::Relaxed) == Self::PENDING
    }

    pub(crate) fn is_busy(&self) -> bool {
        !matches!(self.0.load(Ordering::Relaxed), Self::IDLE | Self::READY)
    }
}
