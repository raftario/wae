#[cfg(feature = "io-ext")]
mod ext;
#[cfg(feature = "io-ext")]
pub use ext::*;

use std::{
    fmt, io,
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    pin::Pin,
    slice,
    task::{Context, Poll},
};
use winapi::shared::ws2def::WSABUF;

pub trait AsyncWrite {
    /// # Safety
    /// The given buffer is guaranteed to be valid and stay the same until the function returns `Poll::Ready`
    unsafe fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &IoSlice<'_>,
    ) -> Poll<io::Result<usize>>;
    fn cancel_write(self: Pin<&mut Self>, wait: bool) -> io::Result<()>;
}

#[repr(transparent)]
pub struct IoSlice<'a> {
    buf: WSABUF,
    _p: PhantomData<&'a [u8]>,
}

unsafe impl Send for IoSlice<'_> {}
unsafe impl Sync for IoSlice<'_> {}

impl IoSlice<'_> {
    pub fn as_wsabuf(&self) -> &WSABUF {
        unsafe { &*(self as *const Self as *const WSABUF) }
    }
}

impl<'a> From<&'a [u8]> for IoSlice<'a> {
    fn from(buf: &'a [u8]) -> Self {
        Self {
            buf: WSABUF {
                len: buf.len() as u32,
                buf: buf.as_ptr() as *mut i8,
            },
            _p: Default::default(),
        }
    }
}

impl<'a> From<io::IoSlice<'a>> for IoSlice<'a> {
    fn from(buf: io::IoSlice<'a>) -> Self {
        unsafe { mem::transmute::<io::IoSlice<'a>, Self>(buf) }
    }
}

impl<'a, T> From<&'a T> for IoSlice<'a>
where
    T: Deref<Target = [u8]>,
{
    fn from(buf: &'a T) -> Self {
        buf.deref().into()
    }
}

impl Deref for IoSlice<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.buf.buf as *const u8, self.buf.len as usize) }
    }
}

impl fmt::Debug for IoSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<P> AsyncWrite for Pin<P>
where
    P: DerefMut + Unpin,
    P::Target: AsyncWrite,
{
    unsafe fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &IoSlice<'_>,
    ) -> Poll<io::Result<usize>> {
        self.get_mut().as_mut().poll_write(cx, buf)
    }

    fn cancel_write(self: Pin<&mut Self>, wait: bool) -> io::Result<()> {
        self.get_mut().as_mut().cancel_write(wait)
    }
}

impl<T> AsyncWrite for &mut T
where
    T: AsyncWrite + Unpin + ?Sized,
{
    unsafe fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &IoSlice<'_>,
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut **self).poll_write(cx, buf)
    }

    fn cancel_write(mut self: Pin<&mut Self>, wait: bool) -> io::Result<()> {
        Pin::new(&mut **self).cancel_write(wait)
    }
}
