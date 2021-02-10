use std::{
    alloc::{self, Layout},
    cell::{Cell, UnsafeCell},
    ffi::c_void,
    io,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    num::NonZeroU32,
    ops::Deref,
    ptr, slice,
    task::{Context, Poll, Waker},
    thread,
};

use winapi::{
    shared::{minwindef::TRUE, ws2def::WSABUF},
    um::{
        ioapiset::CancelIoEx,
        minwinbase::OVERLAPPED,
        threadpoolapiset::{CloseThreadpoolIo, CreateThreadpoolIo, StartThreadpoolIo},
        winnt::{HANDLE, PTP_CALLBACK_INSTANCE, PTP_IO, TP_CALLBACK_ENVIRON_V3},
    },
};

use super::state::State;
use crate::{sync::Mutex, util::HeapAllocated};

pub(crate) struct IO<T: Handle> {
    handle: HANDLE,
    tp_io: PTP_IO,
    read_half: UnsafeCell<IOHalf>,
    write_half: UnsafeCell<IOHalf>,
    _marker: PhantomData<T>,
}

struct IOHalf {
    state: State,
    buffer: WSABUF,
    capacity: usize,
    fixed: bool,
    waker: Mutex<Option<Waker>>,
    error: Cell<Option<NonZeroU32>>,
    overlapped: OVERLAPPED,
}

pub(crate) enum Operation {
    Read,
    Write,
}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    overlapped: *mut c_void,
    result: u32,
    bytes_transferred: usize,
    _io: PTP_IO,
) {
    let context = &*(context as *const IO<()>);
    match context.operation(overlapped) {
        Operation::Read => {
            let half = context.read_half();
            if half.state.is_busy() {
                half.set_error(result);
                half.state.set_ready_with(bytes_transferred);
                half.wake();
            }
        }
        Operation::Write => {
            let half = context.write_half();
            if half.state.is_canceled() {
                half.state.set_idle();
                half.wake();
            } else if half.state.is_busy() {
                half.set_error(result);
                half.state.set_ready_with(bytes_transferred);
                half.wake();
            }
        }
    }
}

impl<T: Handle> IO<T> {
    #[allow(clippy::new_ret_no_self)]
    pub(crate) unsafe fn new<H>(
        handle: T,
        read_capacity: usize,
        write_capacity: usize,
        callback_environ: &TP_CALLBACK_ENVIRON_V3,
    ) -> io::Result<H>
    where
        H: HeapAllocated<Self>,
    {
        let handle = ManuallyDrop::new(handle);
        let handle = handle.as_handle();
        let read_half = UnsafeCell::new(IOHalf::new(read_capacity));
        let write_half = UnsafeCell::new(IOHalf::new(write_capacity));
        let overlapped = H::new(Self {
            handle,
            tp_io: ptr::null_mut(),
            read_half,
            write_half,
            _marker: PhantomData::default(),
        });

        let ptr = overlapped.inner_ptr();
        let tpio = CreateThreadpoolIo(
            handle,
            Some(callback),
            ptr as *mut c_void,
            callback_environ as *const TP_CALLBACK_ENVIRON_V3 as *mut TP_CALLBACK_ENVIRON_V3,
        );
        if tpio.is_null() {
            return Err(io::Error::last_os_error());
        }
        (*(ptr as *mut Self)).tp_io = tpio;

        Ok(overlapped)
    }

    pub(crate) unsafe fn poll_read<S>(
        &self,
        cx: &mut Context,
        ptr: *mut u8,
        len: usize,
        schedule: S,
    ) -> Poll<io::Result<usize>>
    where
        S: FnOnce(&T, *mut WSABUF, *mut OVERLAPPED) -> Poll<io::Result<usize>>,
    {
        let half = &mut *self.read_half.get();
        if half.state.enter_idle() {
            half.fit(len);
            half.buffer.len = usize::min(half.capacity, len) as u32;
            half.set_waker(cx.waker().clone());

            StartThreadpoolIo(self.tp_io);
            match schedule(
                &*ManuallyDrop::new(T::from_handle(self.handle)),
                &mut half.buffer,
                &mut half.overlapped,
            ) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(n)) => {
                    ptr::copy_nonoverlapping(half.buffer.buf as *mut u8, ptr, n);
                    half.state.set_idle();
                    Poll::Ready(Ok(n))
                }
                Poll::Ready(Err(err)) => {
                    half.state.set_idle();
                    Poll::Ready(Err(err))
                }
            }
        } else if let Some(n) = half.state.enter_ready() {
            if let Some(err) = half.error.get_mut().take() {
                half.state.set_idle();
                return Poll::Ready(Err(io::Error::from_raw_os_error(err.get() as i32)));
            }

            let read = usize::min(n, len);
            ptr::copy_nonoverlapping(half.buffer.buf as *mut u8, ptr, read);

            if read < n {
                let rem = n - read;
                ptr::copy(half.buffer.buf.add(read), half.buffer.buf, rem);
                half.state.set_ready_with(rem);
            } else {
                half.state.set_idle();
            }

            Poll::Ready(Ok(read))
        } else {
            half.set_waker(cx.waker().clone());
            Poll::Pending
        }
    }

    pub(crate) unsafe fn poll_write<S>(
        &self,
        cx: &mut Context,
        ptr: *const u8,
        len: usize,
        schedule: S,
    ) -> Poll<io::Result<usize>>
    where
        S: FnOnce(&T, *mut WSABUF, *mut OVERLAPPED) -> Poll<io::Result<usize>>,
    {
        let half = &mut *self.write_half.get();
        if half.state.enter_idle() {
            half.fit(len);
            let write = usize::min(half.capacity, len);
            ptr::copy_nonoverlapping(ptr as *const i8, half.buffer.buf, write);
            half.buffer.len = write as u32;
            half.set_waker(cx.waker().clone());

            StartThreadpoolIo(self.tp_io);
            match schedule(
                &*ManuallyDrop::new(T::from_handle(self.handle)),
                &mut half.buffer,
                &mut half.overlapped,
            ) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(ret) => {
                    half.state.set_idle();
                    Poll::Ready(ret)
                }
            }
        } else if let Some(n) = half.state.enter_ready() {
            half.state.set_idle();

            if let Some(err) = half.error.get_mut().take() {
                return Poll::Ready(Err(io::Error::from_raw_os_error(err.get() as i32)));
            }

            if len < n
                || slice::from_raw_parts(ptr, n)
                    != slice::from_raw_parts(half.buffer.buf as *const u8, n)
            {
                return self.poll_write(cx, ptr, len, schedule);
            }

            Poll::Ready(Ok(n))
        } else if half.state.is_canceled() {
            half.set_waker(cx.waker().clone());
            Poll::Pending
        } else {
            half.set_waker(cx.waker().clone());

            let prev_len = half.buffer.len as usize;
            if len < prev_len
                || slice::from_raw_parts(ptr, prev_len)
                    != slice::from_raw_parts(half.buffer.buf as *const u8, prev_len)
            {
                if CancelIoEx(self.handle, &mut half.overlapped) == TRUE {
                    half.state.set_canceled();
                } else {
                    return Poll::Ready(Err(io::Error::last_os_error()));
                }
            }

            Poll::Pending
        }
    }

    pub(crate) fn handle(&self) -> impl Deref<Target = T> {
        ManuallyDrop::new(T::from_handle(self.handle))
    }

    pub(crate) fn read_capacity(&self) -> (usize, bool) {
        self.read_half().capacity()
    }

    pub(crate) fn write_capacity(&self) -> (usize, bool) {
        self.write_half().capacity()
    }

    fn read_half(&self) -> &IOHalf {
        unsafe { &*self.read_half.get() }
    }

    fn write_half(&self) -> &IOHalf {
        unsafe { &*self.write_half.get() }
    }

    fn operation(&self, overlapped: *mut c_void) -> Operation {
        let overlapped = overlapped as *const OVERLAPPED;
        if overlapped == &self.read_half().overlapped as *const OVERLAPPED {
            Operation::Read
        } else if overlapped == &self.write_half().overlapped as *const OVERLAPPED {
            Operation::Write
        } else {
            unreachable!()
        }
    }
}

impl IOHalf {
    fn new(capacity: usize) -> Self {
        let buf_layout = Layout::array::<u8>(capacity).unwrap();
        let buf = unsafe { alloc::alloc(buf_layout) };
        if buf.is_null() {
            alloc::handle_alloc_error(buf_layout);
        }

        Self {
            state: State::new(),
            buffer: WSABUF {
                len: capacity as u32,
                buf: buf as *mut i8,
            },
            capacity,
            fixed: false,
            waker: Mutex::new(None),
            error: Cell::new(None),
            overlapped: OVERLAPPED::default(),
        }
    }

    fn capacity(&self) -> (usize, bool) {
        (self.capacity, self.fixed)
    }

    fn set_capacity(&mut self, capacity: usize) -> bool {
        if self.state.is_busy() {
            return false;
        }

        let buf_layout = Layout::array::<u8>(self.capacity).unwrap();
        let buf = unsafe { alloc::realloc(self.buffer.buf as *mut u8, buf_layout, capacity) };
        if buf.is_null() {
            alloc::handle_alloc_error(buf_layout);
        }

        self.capacity = capacity;
        self.buffer.len = capacity as u32;
        self.buffer.buf = buf as *mut i8;

        true
    }

    fn fit(&mut self, len: usize) {
        if !self.fixed && len > self.capacity {
            let capacity = usize::max(self.capacity * 2, len);
            self.set_capacity(capacity);
        }
    }

    fn set_waker(&self, waker: Waker) {
        self.waker.lock().replace(waker);
    }

    fn set_error(&self, error: u32) {
        self.error.set(NonZeroU32::new(error))
    }

    fn wake(&self) {
        if let Some(waker) = &*self.waker.lock() {
            waker.wake_by_ref();
        }
    }
}

unsafe impl<T: Handle> Send for IO<T> {}
unsafe impl<T: Handle> Sync for IO<T> {}

impl<T: Handle> Drop for IO<T> {
    fn drop(&mut self) {
        if !self.tp_io.is_null() {
            unsafe {
                CloseThreadpoolIo(self.tp_io);
            }
        }
        mem::drop(T::from_handle(self.handle));
    }
}

impl Drop for IOHalf {
    fn drop(&mut self) {
        while self.state.is_busy() {
            thread::yield_now();
        }
        let buf_layout = Layout::array::<u8>(self.capacity as usize).unwrap();
        unsafe {
            alloc::dealloc(self.buffer.buf as *mut u8, buf_layout);
        }
    }
}

pub(crate) trait Handle {
    fn from_handle(handle: HANDLE) -> Self;
    fn as_handle(&self) -> HANDLE;
}

impl Handle for () {
    fn from_handle(_: HANDLE) -> Self {}

    fn as_handle(&self) -> HANDLE {
        ptr::null_mut()
    }
}
