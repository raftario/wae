use std::{io, ptr, task::Poll};

use winapi::{
    shared::{minwindef::TRUE, ws2def::WSABUF},
    um::{
        minwinbase::OVERLAPPED,
        winnt::HANDLE,
        winsock2::{WSAGetLastError, WSAGetOverlappedResult, WSASend, SOCKET},
    },
};

use crate::io::AsyncWrite;

use super::{TcpStream, WriteHalf};

pub(super) unsafe fn schedule(
    handle: HANDLE,
    overlapped: *mut OVERLAPPED,
    buf: *mut WSABUF,
) -> Poll<io::Result<usize>> {
    let socket = handle as SOCKET;

    let ret = WSASend(socket, buf, 1, ptr::null_mut(), 0, overlapped, None);
    if ret == 0 {
        let mut sent = 0;
        let mut flags = 0;
        if WSAGetOverlappedResult(socket, overlapped, &mut sent, TRUE, &mut flags) == TRUE {
            return Poll::Ready(Ok(sent as usize));
        } else {
            return Poll::Ready(Err(io::Error::last_os_error()));
        }
    }

    let err = WSAGetLastError();
    match err {
        // WSA_IO_PENDING
        997 => Poll::Pending,
        _ => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
    }
}

impl AsyncWrite for TcpStream {
    unsafe fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &crate::io::IoSlice<'_>,
    ) -> Poll<io::Result<usize>> {
        self.inner.poll_write(cx, buf.as_wsabuf())
    }

    fn cancel_write(self: std::pin::Pin<&mut Self>, wait: bool) -> io::Result<()> {
        unsafe { self.inner.cancel_write(wait) }
    }
}

impl AsyncWrite for WriteHalf {
    unsafe fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &crate::io::IoSlice<'_>,
    ) -> Poll<io::Result<usize>> {
        self.inner.inner.poll_write(cx, buf.as_wsabuf())
    }

    fn cancel_write(self: std::pin::Pin<&mut Self>, wait: bool) -> io::Result<()> {
        unsafe { self.inner.inner.cancel_write(wait) }
    }
}
