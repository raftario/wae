use std::{io, ptr, task::Poll};
use winapi::{
    shared::{minwindef::TRUE, ws2def::WSABUF},
    um::{
        minwinbase::OVERLAPPED,
        winnt::HANDLE,
        winsock2::{WSAGetLastError, WSAGetOverlappedResult, WSARecv, SOCKET},
    },
};

use super::{ReadHalf, TcpStream};
use crate::io::AsyncRead;

pub(super) unsafe fn schedule(
    handle: HANDLE,
    overlapped: *mut OVERLAPPED,
    buf: *mut WSABUF,
) -> Poll<io::Result<usize>> {
    let socket = handle as SOCKET;
    let mut flags = 0;

    let ret = WSARecv(
        socket,
        buf,
        1,
        ptr::null_mut(),
        &mut flags,
        overlapped,
        None,
    );
    if ret == 0 {
        let mut recvd = 0;
        if WSAGetOverlappedResult(socket, overlapped, &mut recvd, TRUE, &mut flags) == TRUE {
            return Poll::Ready(Ok(recvd as usize));
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

impl AsyncRead for TcpStream {
    unsafe fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::io::IoSliceMut<'_>,
    ) -> Poll<io::Result<usize>> {
        self.inner.poll_read(cx, buf.as_mut_wsabuf())
    }

    fn cancel_read(self: std::pin::Pin<&mut Self>, wait: bool) -> io::Result<()> {
        unsafe { self.inner.cancel_read(wait) }
    }
}

impl AsyncRead for ReadHalf {
    unsafe fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::io::IoSliceMut<'_>,
    ) -> Poll<io::Result<usize>> {
        self.inner.inner.poll_read(cx, buf.as_mut_wsabuf())
    }

    fn cancel_read(self: std::pin::Pin<&mut Self>, wait: bool) -> io::Result<()> {
        unsafe { self.inner.inner.cancel_read(wait) }
    }
}
