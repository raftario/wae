use std::{cell::UnsafeCell, io, mem::MaybeUninit, num::NonZeroU32};

pub(crate) struct IoResult(UnsafeCell<MaybeUninit<Result<usize, NonZeroU32>>>);

unsafe impl Send for IoResult {}
unsafe impl Sync for IoResult {}

impl IoResult {
    pub(crate) const fn new() -> Self {
        Self(UnsafeCell::new(MaybeUninit::uninit()))
    }

    /// # Safety
    /// This must be the only active reference to the result
    pub(crate) unsafe fn set(&self, result: u32, transferred: usize) {
        let val = match NonZeroU32::new(result) {
            None => Ok(transferred),
            Some(err) => Err(err),
        };
        (*self.0.get()).as_mut_ptr().write(val)
    }

    /// # Safety
    /// This must be the only active reference to the result and it must have been previously set
    pub(crate) unsafe fn get(&self) -> io::Result<usize> {
        let val = (*self.0.get()).as_ptr().read();
        val.map_err(|err| io::Error::from_raw_os_error(err.get() as i32))
    }
}
