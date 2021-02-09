use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct State(AtomicU64);

#[repr(u64)]
pub(crate) enum Status {
    Idle = 0b00,
    Busy = 0b01,
    Ready = 0b10,
    Canceled = 0b11,
}

impl State {
    const STATUS_MASK: u64 = 0b11 << 62;
    const LEN_MASK: u64 = !Self::STATUS_MASK;

    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub(crate) fn state(&self) -> (Status, usize) {
        let val = self.0.load(Ordering::Relaxed);
        (Status::from_u64(val), (val & Self::LEN_MASK) as usize)
    }

    pub(crate) fn status(&self) -> Status {
        let val = self.0.load(Ordering::Relaxed);
        Status::from_u64(val)
    }

    pub(crate) fn set_idle(&self) {
        let val = (Status::Idle as u64) << 62;
        self.0.store(val, Ordering::Relaxed);
    }

    pub(crate) fn set_busy(&self) {
        let val = (Status::Busy as u64) << 62;
        self.0.store(val, Ordering::Relaxed);
    }

    pub(crate) fn set_ready(&self) {
        let val = (Status::Ready as u64) << 62;
        self.0.store(val, Ordering::Relaxed);
    }

    pub(crate) fn set_canceled(&self) {
        let val = (Status::Canceled as u64) << 62;
        self.0.store(val, Ordering::Relaxed);
    }

    pub(crate) fn set_ready_with(&self, len: usize) {
        let val = ((Status::Ready as u64) << 62) | (len as u64);
        self.0.store(val, Ordering::Relaxed);
    }

    pub(crate) fn is_busy(&self) -> bool {
        matches!(self.status(), Status::Busy)
    }

    pub(crate) fn is_ready(&self) -> bool {
        matches!(self.status(), Status::Ready)
    }
}

impl Status {
    fn from_u64(val: u64) -> Self {
        match val >> 62 {
            0b00 => Self::Idle,
            0b01 => Self::Busy,
            0b10 => Self::Ready,
            _ => unreachable!(),
        }
    }
}
