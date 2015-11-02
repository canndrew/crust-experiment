use std::marker::PhantomData;
use std::io;
use mio;
use service::Service;

#[derive(Debug)]
pub struct Connection<'s> {
    service: PhantomData<&'s Service>,
    inner: ConnectionInner,
}

impl<'s> Connection<'s> {
    pub fn from_tcp(tcp_stream: mio::tcp::TcpStream) -> Connection<'s> {
        Connection {
            service: PhantomData,
            inner: ConnectionInner::Tcp(tcp_stream),
        }
    }
}

#[derive(Debug)]
enum ConnectionInner {
    Tcp(mio::tcp::TcpStream),
}

impl<'s> io::Write for Connection<'s> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.inner {
            ConnectionInner::Tcp(ref mut s) => s.write(buf),
        }
    }
    
    fn flush(&mut self) -> io::Result<()> {
        match self.inner {
            ConnectionInner::Tcp(ref mut s) => s.flush(),
        }
    }
}

