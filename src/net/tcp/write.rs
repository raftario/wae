use std::{
    io,
    pin::Pin,
    ptr,
    task::{Context, Poll},
};

use futures_io::AsyncWrite;
use winapi::{
    shared::{minwindef::TRUE, ws2def::WSABUF},
    um::{
        minwinbase::OVERLAPPED,
        winsock2::{shutdown, WSAGetLastError, WSAGetOverlappedResult, WSASend, SD_SEND},
    },
};

use super::{socket::TcpSocket, TcpStream};

fn schedule(
    socket: &TcpSocket,
    buf: *mut WSABUF,
    overlapped: *mut OVERLAPPED,
) -> Poll<io::Result<usize>> {
    let ret = unsafe { WSASend(**socket, buf, 1, ptr::null_mut(), 0, overlapped, None) };
    if ret == 0 {
        let mut sent = 0;
        let mut flags = 0;
        if unsafe { WSAGetOverlappedResult(**socket, overlapped, &mut sent, TRUE, &mut flags) }
            == TRUE
        {
            return Poll::Ready(Ok(sent as usize));
        } else {
            return Poll::Ready(Err(io::Error::last_os_error()));
        }
    }

    let err = unsafe { WSAGetLastError() };
    match err {
        // WSA_IO_PENDING
        997 => Poll::Pending,
        _ => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        unsafe { self.inner.poll_write(cx, buf.as_ptr(), buf.len(), schedule) }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match unsafe { shutdown(**self.inner.handle(), SD_SEND) } {
            0 => Poll::Ready(Ok(())),
            _ => Poll::Ready(Err(io::Error::last_os_error())),
        }
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(self, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(self, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(self, cx)
    }
}
