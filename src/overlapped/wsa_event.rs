use std::{
    cell::Cell,
    ffi::c_void,
    io,
    num::NonZeroU32,
    ptr,
    task::{Context, Poll, Waker},
};

use winapi::{
    shared::minwindef::TRUE,
    um::{
        minwinbase::OVERLAPPED,
        threadpoolapiset::{CloseThreadpoolWait, CreateThreadpoolWait, SetThreadpoolWait},
        winnt::{PTP_CALLBACK_INSTANCE, PTP_WAIT, TP_WAIT_RESULT},
        winsock2::{
            WSACloseEvent, WSACreateEvent, WSAGetOverlappedResult, WSAResetEvent, SOCKET,
            WSA_INVALID_EVENT,
        },
    },
};

use super::state::{State, Status};
use crate::{sync::Mutex, threadpool::Handle, util::HeapAllocated};

pub(crate) struct WsaEvent {
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
    let context = context as *const WsaEvent;
    let event = &*context;
    event.set_error(result);
    event.state.set_ready();
    if let Some(waker) = &*event.waker.lock() {
        waker.wake_by_ref();
    }
}

impl WsaEvent {
    #[allow(clippy::new_ret_no_self)]
    pub(crate) fn new<H>() -> io::Result<H>
    where
        H: HeapAllocated<Self>,
    {
        let event = unsafe { WSACreateEvent() };
        if event == WSA_INVALID_EVENT {
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
                &mut Handle::current().callback_environ,
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
        &mut self,
        cx: &mut Context,
        socket: Option<SOCKET>,
        schedule: S,
    ) -> Poll<io::Result<()>>
    where
        S: FnOnce(Option<SOCKET>, *mut OVERLAPPED) -> Poll<io::Result<()>>,
    {
        match self.state.status() {
            Status::Idle => match schedule(socket, self.overlapped()) {
                Poll::Pending => {
                    self.set_waker(cx.waker().clone());
                    self.state.set_busy();
                    Poll::Pending
                }
                Poll::Ready(ret) => Poll::Ready(ret),
            },
            Status::Busy => {
                self.set_waker(cx.waker().clone());
                Poll::Pending
            }
            Status::Ready => match (self.error.get(), socket) {
                (Some(err), _) => Poll::Ready(Err(io::Error::from_raw_os_error(err.get() as i32))),
                (None, Some(socket)) => {
                    if unsafe {
                        WSAGetOverlappedResult(socket, self.overlapped(), &mut 0, TRUE, &mut 0)
                    } == TRUE
                    {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Ready(Err(io::Error::last_os_error()))
                    }
                }
                (None, None) => Poll::Ready(Ok(())),
            },
            _ => unreachable!(),
        }
    }

    pub(crate) fn set_waker(&self, waker: Waker) {
        self.waker.lock().replace(waker);
    }

    pub(crate) fn reset(&mut self) -> io::Result<bool> {
        if self.state.is_ready() {
            if unsafe { WSAResetEvent(self.overlapped.hEvent) } == TRUE {
                self.state.set_idle();
                Ok(true)
            } else {
                Err(io::Error::last_os_error())
            }
        } else {
            Ok(false)
        }
    }

    fn set_error(&self, error: u32) {
        self.error.set(NonZeroU32::new(error))
    }

    fn overlapped(&self) -> *mut OVERLAPPED {
        &self.overlapped as *const OVERLAPPED as *mut OVERLAPPED
    }
}

impl Drop for WsaEvent {
    fn drop(&mut self) {
        unsafe {
            if !self.wait.is_null() {
                CloseThreadpoolWait(self.wait);
            }
            WSACloseEvent(self.overlapped.hEvent);
        }
    }
}

unsafe impl Send for WsaEvent {}
unsafe impl Sync for WsaEvent {}
