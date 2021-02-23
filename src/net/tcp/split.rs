use std::fmt;

use super::TcpStream;

pub struct ReadHalf {
    pub(crate) inner: TcpStream,
}

pub struct WriteHalf {
    pub(crate) inner: TcpStream,
}

impl TcpStream {
    pub fn split(self) -> (ReadHalf, WriteHalf) {
        (
            ReadHalf {
                inner: Self {
                    inner: self.inner.clone(),
                },
            },
            WriteHalf { inner: self },
        )
    }
}

impl AsRef<TcpStream> for ReadHalf {
    fn as_ref(&self) -> &TcpStream {
        &self.inner
    }
}

impl AsRef<TcpStream> for WriteHalf {
    fn as_ref(&self) -> &TcpStream {
        &self.inner
    }
}

impl fmt::Debug for ReadHalf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_struct("ReadHalf");

        if let Ok(addr) = self.inner.local_addr() {
            dbg.field("addr", &addr);
        }
        if let Ok(addr) = self.inner.peer_addr() {
            dbg.field("peer", &addr);
        }

        dbg.finish()
    }
}

impl fmt::Debug for WriteHalf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_struct("WriteHalf");

        if let Ok(addr) = self.inner.local_addr() {
            dbg.field("addr", &addr);
        }
        if let Ok(addr) = self.inner.peer_addr() {
            dbg.field("peer", &addr);
        }

        dbg.finish()
    }
}
