use std::io;
use std::net::{SocketAddr, ToSocketAddrs};

#[derive(Debug)]
pub enum ListenAddr {
    Tcp(SocketAddr),
}

pub trait ToListenAddr {
    type Err;

    fn to_listen_addr(self) -> Result<ListenAddr, Self::Err>;
}

#[derive(Debug)]
pub enum StrToListenAddrError {
    IoError(io::Error),
    NoAddress,
    MultipleAddresses,
}

impl<'a> ToListenAddr for &'a str {
    type Err = StrToListenAddrError;

    fn to_listen_addr(self) -> Result<ListenAddr, StrToListenAddrError> {
        match self.to_socket_addrs() {
            Err(e)     => Err(StrToListenAddrError::IoError(e)),
            Ok(mut sa) => match sa.next() {
                None    => Err(StrToListenAddrError::NoAddress),
                Some(a) => match sa.next() {
                    None    => Ok(ListenAddr::Tcp(a)),
                    Some(_) => Err(StrToListenAddrError::MultipleAddresses),
                },
            },
        }
    }
}

#[derive(Clone)]
pub enum PeerAddr {
    Tcp(SocketAddr),
}

