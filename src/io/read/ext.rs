use std::{
    fmt,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use super::{AsyncRead, IoSliceMut};
use crate::io::cancel::Cancelable;

pub trait AsyncReadExt: AsyncRead {
    fn chain<R: AsyncRead>(self, next: R) -> Chain<Self, R>
    where
        Self: Sized,
    {
        Chain {
            first: self,
            next,
            chained: false,
        }
    }

    fn read<'a>(&'a mut self, buf: impl Into<IoSliceMut<'a>>) -> Read<'a, Self>
    where
        Self: Unpin,
    {
        Read {
            io: self,
            buf: buf.into(),
        }
    }

    fn read_exact<'a>(&'a mut self, buf: impl Into<IoSliceMut<'a>>) -> ReadExact<'a, Self>
    where
        Self: Unpin,
    {
        let buf = buf.into();
        let buf2 = IoSliceMut {
            buf: buf.buf,
            _p: Default::default(),
        };
        ReadExact {
            read: self.read(buf2),
            buf,
            n: 0,
        }
    }
}

#[derive(Debug)]
pub struct Chain<T, U>
where
    T: AsyncRead,
    U: AsyncRead,
{
    first: T,
    next: U,
    chained: bool,
}

pub struct Read<'a, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    io: &'a mut T,
    buf: IoSliceMut<'a>,
}

pub struct ReadExact<'a, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    read: Read<'a, T>,
    buf: IoSliceMut<'a>,
    n: usize,
}

impl<T, U> AsyncRead for Chain<T, U>
where
    T: AsyncRead,
    U: AsyncRead,
{
    unsafe fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut IoSliceMut<'_>,
    ) -> Poll<io::Result<usize>> {
        if !self.chained {
            let poll = self
                .as_mut()
                .map_unchecked_mut(|p| &mut p.first)
                .poll_read(cx, buf);
            match poll {
                Poll::Ready(Ok(0)) => (),
                _ => return poll,
            }
        }

        self.map_unchecked_mut(|c| &mut c.next).poll_read(cx, buf)
    }

    fn cancel_read(self: Pin<&mut Self>, wait: bool) -> io::Result<()> {
        if !self.chained {
            unsafe { self.map_unchecked_mut(|c| &mut c.first) }.cancel_read(wait)
        } else {
            unsafe { self.map_unchecked_mut(|c| &mut c.next) }.cancel_read(wait)
        }
    }
}

impl<T, U> Unpin for Chain<T, U>
where
    T: AsyncRead + Unpin,
    U: AsyncRead + Unpin,
{
}

impl<T> Future for Read<'_, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        unsafe { Pin::new(&mut this.io).poll_read(cx, &mut this.buf) }
    }
}

impl<T> Cancelable for Read<'_, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    fn cancel(&mut self) -> io::Result<()> {
        Pin::new(&mut self.io).cancel_read(false)
    }
}

impl<T> Drop for Read<'_, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    fn drop(&mut self) {
        Pin::new(&mut self.io).cancel_read(true).ok();
    }
}

impl<T> Future for ReadExact<'_, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.read).poll(cx) {
            Poll::Ready(Ok(n)) => {
                self.n += n;
                if self.n == self.buf.len() {
                    Poll::Ready(Ok(()))
                } else {
                    self.read.buf.buf.len -= n as u32;
                    self.read.buf.buf.buf = unsafe { self.read.buf.buf.buf.add(n) };
                    self.poll(cx)
                }
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> Cancelable for ReadExact<'_, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    fn cancel(&mut self) -> io::Result<()> {
        self.read.cancel()
    }
}

impl<T> fmt::Debug for Read<'_, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Read").finish()
    }
}

impl<T> fmt::Debug for ReadExact<'_, T>
where
    T: AsyncRead + Unpin + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadExact").finish()
    }
}

impl<T> AsyncReadExt for T where T: AsyncRead {}
