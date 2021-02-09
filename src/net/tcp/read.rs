use std::{
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

use super::{TcpSocket, TcpStream};

fn schedule(
    socket: TcpSocket,
    buf: *mut WSABUF,
    overlapped: *mut OVERLAPPED,
) -> Poll<io::Result<usize>> {
    let mut flags = 0;

    let ret = unsafe {
        WSARecv(
            socket.0,
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
        if unsafe { WSAGetOverlappedResult(socket.0, overlapped, &mut recvd, TRUE, &mut flags) }
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
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        unsafe {
            self.inner
                .poll_read(cx, buf.as_mut_ptr(), buf.len(), schedule)
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
            match self.inner.poll_read(
                cx,
                recv_buf.as_mut_ptr() as *mut u8,
                recv_buf.len(),
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
