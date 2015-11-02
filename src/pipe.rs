pub use self::stuff::*;

#[cfg(not(unix))]
mod stuff {
    /*
      WARNING: BROKEN ATTEMPT AT UGLY HACK

    pub type Reader = ::mio::tcp::TcpStream;
    pub type Writer = ::mio::tcp::TcpStream;

    pub fn pipe() -> ::std::io::Result<(::mio::tcp::TcpStream, ::mio::TcpStream)> {
        let mut listener = try!(::mio::tcp::TcpListener::bind(
                &::std::net::SocketAddr::V4(
                        ::std::net::SocketAddrV4::new(
                                ::std::net::Ipv4Addr::new(0, 0, 0, 0),
                                0
                        )
                )
        ));
        let addr = try!(listener.local_addr());
        ::crossbeam::scope(move |scope| {
            let reader = scope.spawn(move || {
                loop {
                    match listener.accept().unwrap() {
                        Some(stream) => return Ok(stream),
                        None         => ::std::thread::yield_now(),
                    }
                }
            });
            let writer = ::mio::tcp::TcpStream::connect(addr).unwrap();
            (reader.join(), writer)
        })
    }
    */
}

#[cfg(unix)]
mod stuff {
    pub type Reader = ::mio::unix::PipeReader;
    pub type Writer = ::mio::unix::PipeWriter;

    pub fn pipe() -> ::std::io::Result<(::mio::unix::PipeReader, ::mio::unix::PipeWriter)> {
        ::mio::unix::pipe()
    }
}

