use std::io;

pub trait Cancelable {
    fn cancel(&mut self) -> io::Result<()>;
}
