use std::{
    ffi::c_void,
    io, ptr,
    sync::Arc,
    task::{Context, Poll},
    thread,
};
use winapi::{
    shared::ws2def::WSABUF,
    um::{
        minwinbase::OVERLAPPED,
        threadpoolapiset::{CloseThreadpoolIo, CreateThreadpoolIo, StartThreadpoolIo},
        winnt::{HANDLE, PTP_CALLBACK_INSTANCE, PTP_IO, TP_CALLBACK_ENVIRON_V3},
    },
};

use atomic_waker::AtomicWaker;
use cache_padded::CachePadded;

use super::{IoResult, IoState};
use crate::threadpool::Handle;

type ScheduleFn = unsafe fn(HANDLE, *mut OVERLAPPED, *mut WSABUF) -> Poll<io::Result<usize>>;
type CancelFn = unsafe fn(HANDLE, *mut OVERLAPPED, bool) -> io::Result<()>;
type CloseFn = unsafe fn(HANDLE);

pub(crate) struct IoHandle {
    pub(crate) handle: HANDLE,
    ptp_io: PTP_IO,
    close: CloseFn,
    read: CachePadded<IoHalf>,
    write: CachePadded<IoHalf>,
}

unsafe impl Send for IoHandle {}
unsafe impl Sync for IoHandle {}

struct IoHalf {
    state: IoState,
    result: IoResult,
    waker: AtomicWaker,
    overlapped: OVERLAPPED,
    schedule: ScheduleFn,
    cancel: CancelFn,
}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    overlapped: *mut c_void,
    result: u32,
    transferred: usize,
    _io: PTP_IO,
) {
    let context = &*(context as *const IoHandle);
    let overlapped = overlapped as *const OVERLAPPED;
    let half = if overlapped == &context.read.overlapped {
        &*context.read
    } else if overlapped == &context.write.overlapped {
        &*context.write
    } else {
        unreachable!()
    };

    if half.state.callback_pending() || half.state.callback_cancelled_nowait() {
        half.result.set(result, transferred);
        half.state.set_ready();
        half.waker.wake();
    } else if half.state.callback_cancelled_wait() {
        half.state.set_ready()
    }
}

impl IoHandle {
    pub(crate) fn new(
        handle: HANDLE,
        close: CloseFn,
        schedule_read: ScheduleFn,
        cancel_read: CancelFn,
        schedule_write: ScheduleFn,
        cancel_write: CancelFn,
        callback_environ: &TP_CALLBACK_ENVIRON_V3,
    ) -> io::Result<Arc<Self>> {
        let mut this = Arc::new(IoHandle {
            handle,
            ptp_io: ptr::null_mut(),
            read: CachePadded::new(IoHalf::new(schedule_read, cancel_read)),
            write: CachePadded::new(IoHalf::new(schedule_write, cancel_write)),
            close,
        });

        let ptp_io = unsafe {
            CreateThreadpoolIo(
                handle,
                Some(callback),
                &*this as *const Self as *mut c_void,
                callback_environ as *const TP_CALLBACK_ENVIRON_V3 as *mut TP_CALLBACK_ENVIRON_V3,
            )
        };
        if ptp_io.is_null() {
            return Err(io::Error::last_os_error());
        }

        Arc::get_mut(&mut this).unwrap().ptp_io = ptp_io;
        Ok(this)
    }

    unsafe fn poll(
        &self,
        half: &IoHalf,
        cx: &mut Context<'_>,
        buf: *mut WSABUF,
    ) -> Poll<io::Result<usize>> {
        if half.state.finish() {
            let result = half.result.get();
            half.state.set_idle();
            Poll::Ready(result)
        } else if half.state.schedule() {
            StartThreadpoolIo(self.ptp_io);
            match (half.schedule)(
                self.handle,
                &half.overlapped as *const OVERLAPPED as *mut OVERLAPPED,
                buf,
            ) {
                Poll::Ready(result) => {
                    half.waker.register(cx.waker());
                    half.state.set_idle();
                    Poll::Ready(result)
                }
                Poll::Pending => {
                    half.state.set_pending();
                    Poll::Pending
                }
            }
        } else {
            half.waker.register(cx.waker());
            Poll::Pending
        }
    }

    pub(crate) unsafe fn poll_read(
        &self,
        cx: &mut Context<'_>,
        buf: *mut WSABUF,
    ) -> Poll<io::Result<usize>> {
        self.poll(&self.read, cx, buf)
    }

    pub(crate) unsafe fn poll_write(
        &self,
        cx: &mut Context<'_>,
        buf: *const WSABUF,
    ) -> Poll<io::Result<usize>> {
        self.poll(&self.write, cx, buf as *mut WSABUF)
    }

    unsafe fn cancel(&self, half: &IoHalf, wait: bool) -> io::Result<()> {
        if !half.state.is_cancellable() {
            return Ok(());
        }
        let cancel = half.state.cancel(wait);

        if wait {
            Handle::try_current().map(|h| h.may_block());
        }
        let ret = if cancel {
            (half.cancel)(
                self.handle,
                &half.overlapped as *const OVERLAPPED as *mut OVERLAPPED,
                wait,
            )
        } else {
            Ok(())
        };
        while wait && half.state.is_busy() {
            thread::yield_now();
        }
        ret
    }

    pub(crate) unsafe fn cancel_read(&self, wait: bool) -> io::Result<()> {
        self.cancel(&self.read, wait)
    }

    pub(crate) unsafe fn cancel_write(&self, wait: bool) -> io::Result<()> {
        self.cancel(&self.write, wait)
    }
}

impl IoHalf {
    fn new(schedule: ScheduleFn, cancel: CancelFn) -> Self {
        Self {
            state: IoState::new(),
            result: IoResult::new(),
            waker: AtomicWaker::new(),
            overlapped: Default::default(),
            schedule,
            cancel,
        }
    }
}

impl Drop for IoHandle {
    fn drop(&mut self) {
        unsafe {
            self.cancel_read(true).unwrap();
            self.cancel_write(true).unwrap();

            if !self.ptp_io.is_null() {
                CloseThreadpoolIo(self.ptp_io);
            }
            (self.close)(self.handle);
        }
    }
}
