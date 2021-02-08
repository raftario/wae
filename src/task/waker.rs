use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{RawWaker, RawWakerVTable, Waker};
use winapi::um::synchapi::{WaitOnAddress, WakeByAddressAll};
use winapi::um::winbase::INFINITE;
use winapi::um::winnt::PVOID;

pub struct InlineWaker {
    state: AtomicU32,
}

impl InlineWaker {
    // as_mut_ptr() is not available on atomics on stable. So until it is, replicate it here.
    fn get_mut_ptr(&self) -> *mut u32 {
        unsafe { (&*(&self.state as *const AtomicU32 as *const UnsafeCell<u32>)).get() }
    }

    pub fn new() -> InlineWaker {
        InlineWaker {
            state: AtomicU32::new(0),
        }
    }

    pub fn get_waker(&self) -> Waker {
        let state = self as *const InlineWaker;
        unsafe { Waker::from_raw(clone_fn(state as *const ())) }
    }

    pub fn wake(&self) {
        if self
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe {
                WakeByAddressAll(self.get_mut_ptr() as PVOID);
            }
        }
    }

    pub fn wait(&self) {
        unsafe {
            while self.state.load(Ordering::Acquire) != 1 {
                let mut wait: u32 = 0;
                WaitOnAddress(
                    self.get_mut_ptr() as PVOID,
                    &mut wait as *mut u32 as PVOID,
                    std::mem::size_of::<u32>(),
                    INFINITE,
                );
            }
        }
    }
}

impl Default for InlineWaker {
    fn default() -> Self {
        Self::new()
    }
}

unsafe fn clone_fn(ptr: *const ()) -> RawWaker {
    RawWaker::new(
        ptr,
        &RawWakerVTable::new(clone_fn, wake_fn, wake_fn, drop_fn),
    )
}
unsafe fn wake_fn(ptr: *const ()) {
    let p = &*(ptr as *const InlineWaker);
    p.wake();
}

unsafe fn drop_fn(_ptr: *const ()) {}
