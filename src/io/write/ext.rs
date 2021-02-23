use std::{
    fmt,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use super::{AsyncWrite, IoSlice};
use crate::io::cancel::Cancelable;

pub trait AsyncWriteExt: AsyncWrite {
    fn write<'a>(&'a mut self, buf: impl Into<IoSlice<'a>>) -> Write<'a, Self>
    where
        Self: Unpin,
    {
        Write {
            io: self,
            buf: buf.into(),
        }
    }

    fn write_all<'a>(&'a mut self, buf: impl Into<IoSlice<'a>>) -> WriteAll<'a, Self>
    where
        Self: Unpin,
    {
        let buf = buf.into();
        let buf2 = IoSlice {
            buf: buf.buf,
            _p: Default::default(),
        };
        WriteAll {
            write: self.write(buf2),
            buf,
            n: 0,
        }
    }
}

pub struct Write<'a, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    io: &'a mut T,
    buf: IoSlice<'a>,
}

pub struct WriteAll<'a, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    write: Write<'a, T>,
    buf: IoSlice<'a>,
    n: usize,
}

impl<T> Future for Write<'_, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        unsafe { Pin::new(&mut this.io).poll_write(cx, &this.buf) }
    }
}

impl<T> Cancelable for Write<'_, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    fn cancel(&mut self) -> io::Result<()> {
        Pin::new(&mut self.io).cancel_write(false)
    }
}

impl<T> Drop for Write<'_, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    fn drop(&mut self) {
        Pin::new(&mut self.io).cancel_write(true).unwrap();
    }
}

impl<T> Future for WriteAll<'_, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.write).poll(cx) {
            Poll::Ready(Ok(n)) => {
                self.n += n;
                if self.n == self.buf.len() {
                    Poll::Ready(Ok(()))
                } else {
                    self.write.buf.buf.len -= n as u32;
                    self.write.buf.buf.buf = unsafe { self.write.buf.buf.buf.add(n) };
                    self.poll(cx)
                }
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> Cancelable for WriteAll<'_, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    fn cancel(&mut self) -> io::Result<()> {
        self.write.cancel()
    }
}

impl<T> fmt::Debug for Write<'_, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Write").finish()
    }
}

impl<T> fmt::Debug for WriteAll<'_, T>
where
    T: AsyncWrite + Unpin + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteExact").finish()
    }
}

impl<T> AsyncWriteExt for T where T: AsyncWrite {}
