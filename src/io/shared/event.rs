use atomic_waker::AtomicWaker;
use std::{
    ffi::c_void,
    io, ptr,
    task::{Context, Poll},
    thread,
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

use crate::threadpool::Handle;

use super::{IoResult, IoState};

pub(crate) struct IoEvent {
    wait: PTP_WAIT,
    state: IoState,
    result: IoResult,
    waker: AtomicWaker,
    overlapped: OVERLAPPED,
}

unsafe impl Send for IoEvent {}
unsafe impl Sync for IoEvent {}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _wait: PTP_WAIT,
    result: TP_WAIT_RESULT,
) {
    let context = context as *const IoEvent;
    let event = &*context;
    if event.state.callback_pending() {
        event.result.set(result, 0);
        event.state.set_ready();
        event.waker.wake();
    }
}

impl IoEvent {
    pub(crate) fn new(callback_environ: &TP_CALLBACK_ENVIRON_V3) -> io::Result<Box<Self>> {
        let event = unsafe { CreateEventW(ptr::null_mut(), TRUE, FALSE, ptr::null_mut()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut this = Box::new(Self {
            wait: ptr::null_mut(),
            state: IoState::new(),
            result: IoResult::new(),
            waker: AtomicWaker::new(),
            overlapped: OVERLAPPED {
                hEvent: event,
                ..Default::default()
            },
        });

        let wait = unsafe {
            CreateThreadpoolWait(
                Some(callback),
                &mut *this as *mut Self as *mut c_void,
                callback_environ as *const TP_CALLBACK_ENVIRON_V3 as *mut TP_CALLBACK_ENVIRON_V3,
            )
        };
        if wait.is_null() {
            return Err(io::Error::last_os_error());
        }
        this.wait = wait;
        unsafe {
            SetThreadpoolWait(wait, event, ptr::null_mut());
        }

        Ok(this)
    }

    pub(crate) fn poll<S>(
        &self,
        cx: &mut Context<'_>,
        handle: Option<HANDLE>,
        schedule: S,
    ) -> Poll<io::Result<()>>
    where
        S: FnOnce(*mut OVERLAPPED) -> Poll<io::Result<()>>,
    {
        if self.state.finish() {
            let mut ret = unsafe { ResetEvent(self.overlapped.hEvent) };
            if ret == 0 {
                self.state.set_idle();
                return Poll::Ready(Err(io::Error::last_os_error()));
            }
            unsafe {
                SetThreadpoolWait(self.wait, self.overlapped.hEvent, ptr::null_mut());
            }

            let result = unsafe { self.result.get() };
            match (result, handle) {
                (Err(err), _) => {
                    self.state.set_idle();
                    Poll::Ready(Err(err))
                }
                (_, Some(handle)) => {
                    ret = unsafe {
                        GetOverlappedResult(
                            handle,
                            &self.overlapped as *const OVERLAPPED as *mut OVERLAPPED,
                            &mut 0,
                            TRUE,
                        )
                    };

                    self.state.set_idle();
                    if ret != 0 {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Ready(Err(io::Error::last_os_error()))
                    }
                }
                (_, None) => {
                    self.state.set_idle();
                    Poll::Ready(Ok(()))
                }
            }
        } else if self.state.schedule() {
            let poll = schedule(&self.overlapped as *const OVERLAPPED as *mut OVERLAPPED);
            match poll {
                Poll::Ready(result) => {
                    self.state.set_idle();
                    Poll::Ready(result)
                }
                Poll::Pending => {
                    self.waker.register(cx.waker());
                    self.state.set_pending();
                    Poll::Pending
                }
            }
        } else {
            self.waker.register(cx.waker());
            Poll::Pending
        }
    }
}

impl Drop for IoEvent {
    fn drop(&mut self) {
        Handle::try_current().map(|h| h.may_block());
        while self.state.is_busy() {
            thread::yield_now();
        }
        unsafe {
            if !self.wait.is_null() {
                CloseThreadpoolWait(self.wait);
            }
            CloseHandle(self.overlapped.hEvent);
        }
    }
}
