use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct State(AtomicU64);

impl State {
    const BUSY: u64 = 1 << 63;
    const READY: u64 = 1 << 62;
    const CANCELED: u64 = 1 << 61;
    const LEN_MASK: u64 = !(Self::BUSY | Self::READY | Self::CANCELED);

    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub(crate) fn enter_idle(&self) -> bool {
        self.0.compare_and_swap(0, Self::BUSY, Ordering::Relaxed) == 0
    }

    pub(crate) fn enter_ready(&self) -> Option<usize> {
        let prev = self.0.fetch_or(Self::BUSY, Ordering::Relaxed);
        if (prev & Self::READY != 0) && (prev & Self::BUSY == 0) {
            Some((prev & Self::LEN_MASK) as usize)
        } else {
            self.0.store(prev, Ordering::Relaxed);
            None
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.0.load(Ordering::Relaxed) & Self::BUSY != 0
    }

    pub(crate) fn is_canceled(&self) -> bool {
        self.0.load(Ordering::Relaxed) & Self::CANCELED != 0
    }

    pub(crate) fn set_idle(&self) {
        self.0.store(0, Ordering::Relaxed);
    }

    pub(crate) fn set_ready(&self) {
        self.0.store(Self::READY, Ordering::Relaxed);
    }

    pub(crate) fn set_ready_with(&self, len: usize) {
        self.0.store(Self::READY | len as u64, Ordering::Relaxed);
    }

    pub(crate) fn set_canceled(&self) {
        self.0.fetch_or(Self::CANCELED, Ordering::Relaxed);
    }
}
