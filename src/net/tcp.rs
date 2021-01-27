use std::{
    alloc::{self, Layout},
    ffi::c_void,
    io,
    net::ToSocketAddrs,
    os::windows::prelude::{AsRawSocket, IntoRawSocket},
    pin::Pin,
    ptr::{self, NonNull},
    task::{Context, Poll, Waker},
};

use winapi::{
    shared::ws2def::WSABUF,
    um::{
        threadpoolapiset::{CloseThreadpoolWait, CreateThreadpoolWait},
        winnt::{PTP_CALLBACK_INSTANCE, PTP_WAIT, TP_WAIT_RESULT},
        winsock2::{
            recv, WSACloseEvent, WSACreateEvent, WSAEventSelect, WSAGetLastError, WSARecv, FD_READ,
            FD_WRITE, SOCKET, WSAEVENT, WSA_INVALID_EVENT,
        },
    },
};

use futures_io::{AsyncRead, AsyncWrite};

use crate::{sync::Mutex, threadpool::Handle};

pub struct TcpStream {
    socket: SOCKET,

    read_event: WSAEVENT,
    read_wait: PTP_WAIT,
    read_waker: NonNull<Mutex<Option<Waker>>>,

    write_event: WSAEVENT,
    write_wait: PTP_WAIT,
    write_waker: NonNull<Mutex<Option<Waker>>>,
}

impl TcpStream {
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<TcpStream> {
        let handle = Handle::current();
        handle.may_block();

        let std_stream = std::net::TcpStream::connect(addr)?;
        std_stream.set_nonblocking(true)?;
        let socket = std_stream.as_raw_socket() as SOCKET;

        let stream = unsafe {
            let read_event = WSACreateEvent();
            if read_event == WSA_INVALID_EVENT {
                return Err(io::Error::last_os_error());
            }
            if WSAEventSelect(socket, read_event, FD_READ) != 0 {
                WSACloseEvent(read_event);
                return Err(io::Error::last_os_error());
            }

            let write_event = WSACreateEvent();
            if write_event == WSA_INVALID_EVENT {
                WSACloseEvent(read_event);
                return Err(io::Error::last_os_error());
            }
            if WSAEventSelect(socket, write_event, FD_WRITE) != 0 {
                WSACloseEvent(write_event);
                WSACloseEvent(read_event);
                return Err(io::Error::last_os_error());
            }

            let layout = Layout::new::<Mutex<Option<Waker>>>();
            let (layout, write_offset) = layout.extend(layout).unwrap();

            let read_ptr = alloc::alloc(layout);
            let write_ptr = read_ptr.add(write_offset);

            let read_ptr = read_ptr as *mut Mutex<Option<Waker>>;
            let write_ptr = write_ptr as *mut Mutex<Option<Waker>>;
            read_ptr.write(Mutex::new(None));
            write_ptr.write(Mutex::new(None));

            let mut callback_environ = handle.callback_environ;

            let read_wait = CreateThreadpoolWait(
                Some(callback),
                read_ptr as *mut c_void,
                &mut callback_environ,
            );
            if read_wait.is_null() {
                write_ptr.drop_in_place();
                read_ptr.drop_in_place();
                alloc::dealloc(read_ptr as *mut u8, layout);
                WSACloseEvent(write_event);
                WSACloseEvent(read_event);
                return Err(io::Error::last_os_error());
            }

            let write_wait = CreateThreadpoolWait(
                Some(callback),
                write_ptr as *mut c_void,
                &mut callback_environ,
            );
            if write_wait.is_null() {
                CloseThreadpoolWait(read_wait);
                write_ptr.drop_in_place();
                read_ptr.drop_in_place();
                alloc::dealloc(read_ptr as *mut u8, layout);
                WSACloseEvent(write_event);
                WSACloseEvent(read_event);
                return Err(io::Error::last_os_error());
            }

            TcpStream {
                socket: std_stream.into_raw_socket() as SOCKET,

                read_event,
                read_wait,
                read_waker: NonNull::new_unchecked(read_ptr),

                write_event,
                write_wait,
                write_waker: NonNull::new_unchecked(write_ptr),
            }
        };

        crate::task::yield_now().await;
        Ok(stream)
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        unsafe {
            let recv = recv(
                self.socket,
                buf.as_mut_ptr() as *mut i8,
                buf.len() as i32,
                0,
            );
            match recv {
                // SOCKET_ERROR
                -1 => match WSAGetLastError() {
                    // WSAEWOULDBLOCK
                    10035 => {
                        let mutex = self.write_waker.as_ref();
                        mutex.lock().replace(cx.waker().clone());
                        Poll::Pending
                    }
                    err => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                },
                n => Poll::Ready(Ok(n as usize)),
            }
        }
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context,
        bufs: &mut [io::IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        unsafe {
            let mut recv = 0;
            let ret = WSARecv(
                self.socket,
                bufs.as_mut_ptr() as *mut WSABUF,
                bufs.len() as u32,
                &mut recv,
                ptr::null_mut(),
                ptr::null_mut(),
                None,
            );
            match ret {
                // SOCKET_ERROR
                -1 => match WSAGetLastError() {
                    // WSAEWOULDBLOCK
                    10035 => {
                        let mutex = self.write_waker.as_ref();
                        mutex.lock().replace(cx.waker().clone());
                        Poll::Pending
                    }
                    err => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                },
                _ => Poll::Ready(Ok(recv as usize)),
            }
        }
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut tokio::io::ReadBuf,
    ) -> Poll<io::Result<()>> {
        unsafe {
            let recv_buf = buf.unfilled_mut();
            let recv = recv(
                self.socket,
                recv_buf.as_mut_ptr() as *mut i8,
                recv_buf.len() as i32,
                0,
            );
            match recv {
                // SOCKET_ERROR
                -1 => match WSAGetLastError() {
                    // WSAEWOULDBLOCK
                    10035 => {
                        let mutex = self.write_waker.as_ref();
                        mutex.lock().replace(cx.waker().clone());
                        Poll::Pending
                    }
                    err => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                },
                n => {
                    let n = n as usize;
                    buf.assume_init(n);
                    buf.advance(n);
                    Poll::Ready(Ok(()))
                }
            }
        }
    }
}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _wait: PTP_WAIT,
    _wait_result: TP_WAIT_RESULT,
) {
    let context = context as *const Mutex<Option<Waker>>;
    let mutex = &*context;
    if let Some(waker) = &*mutex.lock() {
        waker.wake_by_ref();
    }
}
