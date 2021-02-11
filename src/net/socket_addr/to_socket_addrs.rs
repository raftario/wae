use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::threadpool::Handle;

use super::get_addr_info::get_addr_info;

pub trait ToSocketAddrs {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_>;
}

impl ToSocketAddrs for SocketAddr {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_> {
        sealed::ToSocketAddrs {
            inner: sealed::ToSocketAddrsInner::Immediate { addr: *self },
        }
    }
}

macro_rules! impl_into {
    ($ty:ty) => {
        impl ToSocketAddrs for $ty {
            fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_> {
                sealed::ToSocketAddrs {
                    inner: sealed::ToSocketAddrsInner::Immediate {
                        addr: (*self).into(),
                    },
                }
            }
        }
    };
}

impl_into!(SocketAddrV4);
impl_into!(SocketAddrV6);
impl_into!((IpAddr, u16));
impl_into!((Ipv4Addr, u16));
impl_into!((Ipv6Addr, u16));

impl<'a> ToSocketAddrs for &'a [SocketAddr] {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'a> {
        sealed::ToSocketAddrs {
            inner: sealed::ToSocketAddrsInner::Slice { addrs: self },
        }
    }
}

impl ToSocketAddrs for str {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_> {
        sealed::ToSocketAddrs {
            inner: match self.parse() {
                Ok(addr) => sealed::ToSocketAddrsInner::Immediate { addr },
                Err(_) => sealed::ToSocketAddrsInner::Future {
                    future: get_addr_info(self, None, &Handle::current().callback_environ()),
                },
            },
        }
    }
}

impl ToSocketAddrs for String {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_> {
        <str as ToSocketAddrs>::to_socket_addrs(self)
    }
}

impl ToSocketAddrs for (&str, u16) {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_> {
        sealed::ToSocketAddrs {
            inner: match self.0.parse() {
                Ok(ip) => sealed::ToSocketAddrsInner::Immediate {
                    addr: SocketAddr::new(ip, self.1),
                },
                Err(_) => sealed::ToSocketAddrsInner::Future {
                    future: get_addr_info(
                        self.0,
                        Some(self.1),
                        &Handle::current().callback_environ(),
                    ),
                },
            },
        }
    }
}

impl ToSocketAddrs for (String, u16) {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_> {
        sealed::ToSocketAddrs {
            inner: match self.0.parse() {
                Ok(ip) => sealed::ToSocketAddrsInner::Immediate {
                    addr: SocketAddr::new(ip, self.1),
                },
                Err(_) => sealed::ToSocketAddrsInner::Future {
                    future: get_addr_info(
                        &self.0,
                        Some(self.1),
                        &Handle::current().callback_environ(),
                    ),
                },
            },
        }
    }
}

impl<T: ToSocketAddrs + ?Sized> ToSocketAddrs for &T {
    fn to_socket_addrs(&self) -> sealed::ToSocketAddrs<'_> {
        (&**self).to_socket_addrs()
    }
}

mod sealed {
    use std::{
        fmt,
        future::Future,
        io,
        iter::{self, Copied, Once},
        net::SocketAddr,
        pin::Pin,
        slice::Iter,
        task::{Context, Poll},
    };

    use pin_project_lite::pin_project;

    use crate::net::socket_addr::get_addr_info::{GetAddrInfoFuture, GetAddrInfoIter};

    pin_project! {
        pub struct ToSocketAddrs<'a> {
            #[pin] pub(super) inner: ToSocketAddrsInner<'a>,
        }
    }

    pin_project! {
        #[project = ToSocketAddrsProj]
        pub(super) enum ToSocketAddrsInner<'a> {
            Immediate { addr: SocketAddr },
            Slice { addrs: &'a [SocketAddr] },
            Future { #[pin] future: GetAddrInfoFuture },
        }
    }

    impl<'a> Future for ToSocketAddrs<'a> {
        type Output = io::Result<ToSocketAddrsIter<'a>>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            match self.project().inner.project() {
                ToSocketAddrsProj::Immediate { addr } => Poll::Ready(Ok(ToSocketAddrsIter {
                    inner: ToSocketAddrsIterInner::Immediate(iter::once(*addr)),
                })),
                ToSocketAddrsProj::Slice { addrs } => Poll::Ready(Ok(ToSocketAddrsIter {
                    inner: ToSocketAddrsIterInner::Slice(addrs.iter().copied()),
                })),
                ToSocketAddrsProj::Future { future } => match future.poll(cx) {
                    Poll::Ready(Ok(iter)) => Poll::Ready(Ok(ToSocketAddrsIter {
                        inner: ToSocketAddrsIterInner::Future(iter),
                    })),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Pending => Poll::Pending,
                },
            }
        }
    }

    impl fmt::Debug for ToSocketAddrs<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("ToSocketAddrs").finish()
        }
    }

    pub struct ToSocketAddrsIter<'a> {
        inner: ToSocketAddrsIterInner<'a>,
    }

    enum ToSocketAddrsIterInner<'a> {
        Immediate(Once<SocketAddr>),
        Slice(Copied<Iter<'a, SocketAddr>>),
        Future(GetAddrInfoIter),
    }

    impl Iterator for ToSocketAddrsIter<'_> {
        type Item = SocketAddr;

        fn next(&mut self) -> Option<Self::Item> {
            match &mut self.inner {
                ToSocketAddrsIterInner::Immediate(iter) => iter.next(),
                ToSocketAddrsIterInner::Slice(iter) => iter.next(),
                ToSocketAddrsIterInner::Future(iter) => iter.next(),
            }
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            match &self.inner {
                ToSocketAddrsIterInner::Immediate(iter) => iter.size_hint(),
                ToSocketAddrsIterInner::Slice(iter) => iter.size_hint(),
                ToSocketAddrsIterInner::Future(iter) => iter.size_hint(),
            }
        }
    }

    impl fmt::Debug for ToSocketAddrsIter<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("ToSocketAddrsIter").finish()
        }
    }
}
