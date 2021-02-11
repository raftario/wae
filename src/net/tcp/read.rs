use std::{
    future::Future,
    io,
    pin::Pin,
    ptr,
    task::{Context, Poll},
};

use winapi::{
    shared::{minwindef::TRUE, ws2def::WSABUF},
    um::{
        minwinbase::OVERLAPPED,
        winsock2::{WSAGetLastError, WSAGetOverlappedResult, WSARecv},
    },
};

use futures_io::AsyncRead;
use pin_utils::pin_mut;

use super::{socket::TcpSocket, ReadHalf, TcpStream};
use crate::overlapped::io::IO;

fn schedule(
    socket: &TcpSocket,
    buf: *mut WSABUF,
    overlapped: *mut OVERLAPPED,
) -> Poll<io::Result<usize>> {
    let mut flags = 0;

    let ret = unsafe {
        WSARecv(
            **socket,
            buf,
            1,
            ptr::null_mut(),
            &mut flags,
            overlapped,
            None,
        )
    };
    if ret == 0 {
        let mut recvd = 0;
        if unsafe { WSAGetOverlappedResult(**socket, overlapped, &mut recvd, TRUE, &mut flags) }
            == TRUE
        {
            return Poll::Ready(Ok(recvd as usize));
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

impl AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        unsafe {
            self.inner
                .poll_read(cx, buf.as_mut_ptr(), buf.len(), false, schedule)
        }
    }
}

impl AsyncRead for ReadHalf {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let inner = &mut self.inner;
        pin_mut!(inner);
        AsyncRead::poll_read(inner, cx, buf)
    }
}

struct Peek<'a> {
    inner: &'a IO<TcpSocket>,
    buf: &'a mut [u8],
}

impl Future for Peek<'_> {
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            self.inner
                .poll_read(cx, self.buf.as_mut_ptr(), self.buf.len(), true, schedule)
        }
    }
}

impl TcpStream {
    #[inline]
    pub async fn peek(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Peek {
            inner: &self.inner,
            buf,
        }
        .await
    }

    pub fn poll_peek(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        unsafe {
            self.inner
                .poll_read(cx, buf.as_mut_ptr(), buf.len(), true, schedule)
        }
    }

    pub fn read_capacity(&self) -> (usize, bool) {
        self.inner.read_capacity()
    }

    pub fn set_read_capacity(&mut self, capacity: impl Into<Option<usize>>, fixed: bool) -> bool {
        unsafe { self.inner.set_read_capacity(capacity.into(), fixed) }
    }
}

impl ReadHalf {
    #[inline]
    pub async fn peek(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.peek(buf).await
    }

    pub fn poll_peek(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        self.inner.poll_peek(cx, buf)
    }

    pub fn set_read_capacity(&mut self, capacity: impl Into<Option<usize>>, fixed: bool) -> bool {
        self.inner.set_read_capacity(capacity, fixed)
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        unsafe {
            let recv_buf = buf.unfilled_mut();
            match self.inner.poll_read(
                cx,
                recv_buf.as_mut_ptr() as *mut u8,
                recv_buf.len(),
                false,
                schedule,
            ) {
                Poll::Ready(Ok(recvd)) => {
                    buf.assume_init(recvd);
                    buf.advance(recvd);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for ReadHalf {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let inner = &mut self.inner;
        pin_mut!(inner);
        tokio::io::AsyncRead::poll_read(inner, cx, buf)
    }
}
