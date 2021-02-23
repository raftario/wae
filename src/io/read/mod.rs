#[cfg(feature = "io-ext")]
mod ext;
#[cfg(feature = "io-ext")]
pub use ext::*;

use std::{
    fmt, io,
    marker::PhantomData,
    mem::{self, MaybeUninit},
    ops::{Deref, DerefMut},
    pin::Pin,
    slice,
    task::{Context, Poll},
};
use winapi::shared::ws2def::WSABUF;

pub trait AsyncRead {
    /// # Safety
    /// The given buffer is guaranteed to be valid and stay the same until the function returns `Poll::Ready`
    unsafe fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut IoSliceMut<'_>,
    ) -> Poll<io::Result<usize>>;
    fn cancel_read(self: Pin<&mut Self>, wait: bool) -> io::Result<()>;
}

#[repr(transparent)]
pub struct IoSliceMut<'a> {
    buf: WSABUF,
    _p: PhantomData<&'a mut [MaybeUninit<u8>]>,
}

unsafe impl Send for IoSliceMut<'_> {}
unsafe impl Sync for IoSliceMut<'_> {}

impl IoSliceMut<'_> {
    pub fn as_wsabuf(&self) -> &WSABUF {
        unsafe { &*(self as *const Self as *const WSABUF) }
    }

    pub fn as_mut_wsabuf(&mut self) -> &mut WSABUF {
        unsafe { &mut *(self as *mut Self as *mut WSABUF) }
    }
}

impl<'a> From<&'a mut [u8]> for IoSliceMut<'a> {
    fn from(buf: &'a mut [u8]) -> Self {
        Self {
            buf: WSABUF {
                len: buf.len() as u32,
                buf: buf.as_mut_ptr() as *mut i8,
            },
            _p: Default::default(),
        }
    }
}

impl<'a> From<&'a mut [MaybeUninit<u8>]> for IoSliceMut<'a> {
    fn from(buf: &'a mut [MaybeUninit<u8>]) -> Self {
        Self {
            buf: WSABUF {
                len: buf.len() as u32,
                buf: buf.as_mut_ptr() as *mut i8,
            },
            _p: Default::default(),
        }
    }
}

impl<'a> From<io::IoSliceMut<'a>> for IoSliceMut<'a> {
    fn from(buf: io::IoSliceMut<'a>) -> Self {
        unsafe { mem::transmute::<io::IoSliceMut<'a>, Self>(buf) }
    }
}

impl<'a, T> From<&'a mut T> for IoSliceMut<'a>
where
    T: Deref<Target = [u8]> + DerefMut,
{
    fn from(buf: &'a mut T) -> Self {
        buf.deref_mut().into()
    }
}

impl Deref for IoSliceMut<'_> {
    type Target = [MaybeUninit<u8>];

    fn deref(&self) -> &Self::Target {
        unsafe {
            slice::from_raw_parts(
                self.buf.buf as *const MaybeUninit<u8>,
                self.buf.len as usize,
            )
        }
    }
}

impl DerefMut for IoSliceMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            slice::from_raw_parts_mut(self.buf.buf as *mut MaybeUninit<u8>, self.buf.len as usize)
        }
    }
}

impl fmt::Debug for IoSliceMut<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<P> AsyncRead for Pin<P>
where
    P: DerefMut + Unpin,
    P::Target: AsyncRead,
{
    unsafe fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut IoSliceMut<'_>,
    ) -> Poll<io::Result<usize>> {
        self.get_mut().as_mut().poll_read(cx, buf)
    }

    fn cancel_read(self: Pin<&mut Self>, wait: bool) -> io::Result<()> {
        self.get_mut().as_mut().cancel_read(wait)
    }
}

impl<T> AsyncRead for &mut T
where
    T: AsyncRead + Unpin + ?Sized,
{
    unsafe fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut IoSliceMut<'_>,
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut **self).poll_read(cx, buf)
    }

    fn cancel_read(mut self: Pin<&mut Self>, wait: bool) -> io::Result<()> {
        Pin::new(&mut **self).cancel_read(wait)
    }
}
