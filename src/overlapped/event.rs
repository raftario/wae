use std::{
    cell::Cell,
    ffi::c_void,
    io,
    num::NonZeroU32,
    ptr,
    task::{Context, Poll, Waker},
};

use winapi::{
    shared::minwindef::{FALSE, TRUE},
    um::{
        handleapi::CloseHandle,
        ioapiset::GetOverlappedResult,
        minwinbase::OVERLAPPED,
        synchapi::{CreateEventW, ResetEvent},
        threadpoolapiset::{CloseThreadpoolWait, CreateThreadpoolWait, SetThreadpoolWait},
        winnt::{HANDLE, PTP_CALLBACK_INSTANCE, PTP_WAIT, TP_CALLBACK_ENVIRON_V3, TP_WAIT_RESULT},
    },
};

use super::state::State;
use crate::{sync::Mutex, util::HeapAllocated};

pub(crate) struct Event {
    overlapped: OVERLAPPED,
    wait: PTP_WAIT,
    state: State,
    waker: Mutex<Option<Waker>>,
    error: Cell<Option<NonZeroU32>>,
}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _wait: PTP_WAIT,
    result: TP_WAIT_RESULT,
) {
    let context = context as *const Event;
    let event = &*context;
    if event.state.is_busy() {
        event.set_error(result);
        event.state.set_ready();
        event.wake();
    }
}

impl Event {
    #[allow(clippy::new_ret_no_self)]
    pub(crate) fn new<H>(callback_environ: &TP_CALLBACK_ENVIRON_V3) -> io::Result<H>
    where
        H: HeapAllocated<Self>,
    {
        let event = unsafe { CreateEventW(ptr::null_mut(), TRUE, FALSE, ptr::null_mut()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let hevent = H::new(Self {
            overlapped: OVERLAPPED {
                hEvent: event,
                ..Default::default()
            },
            wait: ptr::null_mut(),
            error: Cell::new(None),
            state: State::new(),
            waker: Mutex::new(None),
        });

        let wait = unsafe {
            CreateThreadpoolWait(
                Some(callback),
                hevent.inner_ptr() as *mut c_void,
                callback_environ as *const TP_CALLBACK_ENVIRON_V3 as *mut TP_CALLBACK_ENVIRON_V3,
            )
        };
        if wait.is_null() {
            return Err(io::Error::last_os_error());
        }

        unsafe {
            (*(hevent.inner_ptr() as *mut Self)).wait = wait;
            SetThreadpoolWait(wait, event, ptr::null_mut());
        }

        Ok(hevent)
    }

    pub(crate) fn poll<S>(
        &self,
        cx: &mut Context,
        handle: Option<HANDLE>,
        schedule: S,
    ) -> Poll<io::Result<()>>
    where
        S: FnOnce(*mut OVERLAPPED) -> Poll<io::Result<()>>,
    {
        if self.state.enter_idle() {
            self.set_waker(cx.waker().clone());
            match schedule(self.overlapped()) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(ret) => {
                    self.reset()?;
                    Poll::Ready(ret)
                }
            }
        } else if self.state.enter_ready().is_some() {
            self.reset()?;
            match (self.error.get(), handle) {
                (Some(err), _) => Poll::Ready(Err(io::Error::from_raw_os_error(err.get() as i32))),
                (None, Some(handle)) => {
                    if unsafe { GetOverlappedResult(handle, self.overlapped(), &mut 0, TRUE) } != 0
                    {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Ready(Err(io::Error::last_os_error()))
                    }
                }
                (None, None) => Poll::Ready(Ok(())),
            }
        } else {
            self.set_waker(cx.waker().clone());
            Poll::Pending
        }
    }

    fn reset(&self) -> io::Result<()> {
        self.state.set_idle();
        if unsafe { ResetEvent(self.overlapped.hEvent) } != 0 {
            unsafe {
                SetThreadpoolWait(self.wait, self.overlapped.hEvent, ptr::null_mut());
            }
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn set_waker(&self, waker: Waker) {
        self.waker.lock().replace(waker);
    }

    fn set_error(&self, error: u32) {
        self.error.set(NonZeroU32::new(error))
    }

    fn overlapped(&self) -> *mut OVERLAPPED {
        &self.overlapped as *const OVERLAPPED as *mut OVERLAPPED
    }

    fn wake(&self) {
        if let Some(waker) = &*self.waker.lock() {
            waker.wake_by_ref();
        }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            if !self.wait.is_null() {
                CloseThreadpoolWait(self.wait);
            }
            CloseHandle(self.overlapped.hEvent);
        }
    }
}

unsafe impl Send for Event {}
unsafe impl Sync for Event {}
