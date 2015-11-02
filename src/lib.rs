#![feature(read_exact)]

extern crate mio;
extern crate crossbeam;

const TOKEN_NOTIFY: mio::Token = mio::Token(::std::usize::MAX);

pub mod address;
pub mod service;
pub mod connection;
mod pipe;

#[cfg(test)]
mod tests {
    use service::Service;
    use std::thread;
    use std::io::{Read, Write};
    use crossbeam;

    #[test]
    fn ping_self() {
        let service = Service::new();
        let mut acceptor = service.acceptor().unwrap();
        crossbeam::scope(|scope| {
            let (r, c) = acceptor.start().unwrap();
            let jg0 = scope.spawn(move || {
                for conn in r {
                    let mut conn = conn.unwrap();
                    conn.write(b"hello").unwrap();
                };
            });
            let jg1 = scope.spawn(move || {
                {
                    let (ev_reactor, _ev_controller) = c.listener_events();
                    c.add_listener("0.0.0.0:45666").unwrap();
                    let (_, event) = ev_reactor.next_event().unwrap();

                    match event {
                        ::service::ListenerEvent::StartListening(addr) => match addr {
                            ::address::PeerAddr::Tcp(tcp_addr) => {
                                let mut socket = ::mio::tcp::TcpStream::connect(&tcp_addr).unwrap();
                                let mut buf = String::new();
                                // give acceptor time to reply
                                thread::sleep_ms(100);
                                socket.read_to_string(&mut buf).unwrap();
                                assert_eq!(&buf[..], "hello");
                            },
                        },
                    }
                }
                drop(c);
            });
            jg0.join();
            jg1.join();
        });
    }
}

