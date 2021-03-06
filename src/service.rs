use std::io;
use std::marker::PhantomData;
use std::io::{Write, Read};
use std::sync::mpsc::{Sender, Receiver, channel};
use mio;
use connection::Connection;
use pipe;

use address::{ListenAddr, ToListenAddr, PeerAddr};

/// Our handle to the crust library. All actions are performed either directly or indirectly
/// through a `Service` and the library is guaranteed to have cleaned up all resources on
/// destruction of a `Service`.
pub struct Service;

impl Service {
    /// Create a new `Service`.
    pub fn new() -> Service {
        Service
    }

    /// Create an `Acceptor` from this `Service`.
    pub fn acceptor<'s>(&'s self) -> io::Result<Acceptor<'s>> {
        Ok(Acceptor {
            service: PhantomData,
            listen_set: ::std::sync::Mutex::new(ListenSet {
                poll: try!(mio::Poll::new()),
                fresh: true,
                listeners: Vec::new(),
            }),
            listen_set_freshened: ::std::sync::Condvar::new(),
            listener_events_reactors: ::std::sync::Mutex::new(Vec::new()),
        })
    }
}

struct ListenSet {
    poll: mio::Poll,
    fresh: bool,
    listeners: Vec<mio::tcp::TcpListener>,
}

/// Used to listen for and accept incoming connections.
pub struct Acceptor<'s> {
    service: PhantomData<&'s Service>,
    listen_set: ::std::sync::Mutex<ListenSet>,
    listen_set_freshened: ::std::sync::Condvar,
    listener_events_reactors: ::std::sync::Mutex<Vec<Sender<Option<ListenerEvent>>>>,
}

impl<'s> Acceptor<'s> {
    /// Start listening. This creates an `AcceptorReactor` that can be used to block, wait for and
    /// process incoming connections and an `AcceptorController` that can be used to control the
    /// `Acceptor`.
    pub fn start<'a>(&'a mut self) -> io::Result<(AcceptorReactor<'a, 's>, AcceptorController<'a, 's>)> {
        let (pr, pw) = try!(pipe::pipe());
        let mut guard = self.listen_set.lock().unwrap();
        try!(guard.poll.register(&pr, ::TOKEN_NOTIFY, mio::EventSet::readable(), mio::PollOpt::level()));
        let r = AcceptorReactor {
            acceptor: self,
            notify_pipe: pr,
        };
        let c = AcceptorController {
            acceptor: self,
            notify_pipe: ::std::sync::Mutex::new(pw),
        };
        Ok((r, c))
    }
}

/// Used to accept incoming connections on an `Acceptor`.
pub struct AcceptorReactor<'a, 's: 'a> {
    acceptor: &'a Acceptor<'s>,
    notify_pipe: pipe::Reader,
}

impl<'a, 's: 'a> AcceptorReactor<'a, 's> {
    /// Block until either we recieve an incoming connection or the `Acceptor` shuts down due to
    /// the corresponding `AcceptorController` being dropped.
    /// 
    /// # Returns
    ///
    /// `None` if the `Acceptor` has shutdown. Otherwise returns itself along with the new
    /// connection (or an error if there was an error accepting the connection).
    pub fn accept(mut self) -> Option<(AcceptorReactor<'a, 's>, io::Result<Connection<'s>>)> {
        let mut guard = self.acceptor.listen_set.lock().unwrap();
        loop {
            guard.fresh = false;
            match guard.poll.poll(::std::usize::MAX) {
                Err(e) => return Some((self, Err(e))),
                Ok(n)  => {
                    for index in 0..n {
                        match guard.poll.event(index).token {
                            ::TOKEN_NOTIFY => {
                                let mut c = [0u8];
                                // Unless poll in malfunctioning then we can definitely read this
                                // pipe. So unwrap()
                                self.notify_pipe.read_exact(&mut c[..]).unwrap();
                                if c[0] == 1 {
                                    return None;
                                }
                                else {
                                    while !guard.fresh {
                                        guard = self.acceptor.listen_set_freshened.wait(guard).unwrap();
                                    }
                                }
                            },
                            t => {
                                match guard.listeners[t.0].accept() {
                                    Ok(None) => (),
                                    Ok(Some(stream)) => return Some((self, Ok(Connection::from_tcp(stream)))),
                                    Err(e)           => return Some((self, Err(e))),
                                }
                            },
                        }
                    };
                },
            }
        }
    }
}

/// Used for iterating over the incoming connections of an `AcceptorReactor`.
pub struct AcceptIter<'a, 's: 'a> {
    acceptor_reactor: Option<AcceptorReactor<'a, 's>>,
}

impl<'a, 's: 'a> IntoIterator for AcceptorReactor<'a, 's> {
    type Item = io::Result<Connection<'s>>;
    type IntoIter = AcceptIter<'a, 's>;

    fn into_iter(self) -> AcceptIter<'a, 's> {
        AcceptIter {
            acceptor_reactor: Some(self),
        }
    }
}

impl<'a, 's: 'a> Iterator for AcceptIter<'a, 's> {
    type Item = io::Result<Connection<'s>>;

    fn next(&mut self) -> Option<io::Result<Connection<'s>>> {
        let x = self.acceptor_reactor.take();
        x.and_then(|a| a.accept()).map(|(a, c)| {
            self.acceptor_reactor = Some(a);
            c
        })
    }
}

/// Used to control an `Acceptor` that has been started. On dropping this object the `Acceptor`
/// will stop and any corresponding `AcceptorReactor::accept` call will unblock and return `None`.
pub struct AcceptorController<'a, 's: 'a> {
    acceptor: &'a Acceptor<'s>,
    notify_pipe: ::std::sync::Mutex<pipe::Writer>,
}

/// Errors returned by `AcceptorController::add_listener`
#[derive(Debug)]
pub enum AddListenerError<E> {
    /// The argument could not parsed as a valid address.
    ParseError(E),
    /// An IO error occured while creating the listener.
    IoError(io::Error),
}

impl<E> From<io::Error> for AddListenerError<E> {
    fn from(e: io::Error) -> AddListenerError<E> {
        AddListenerError::IoError(e)
    }
}

impl<'a, 's: 'a> AcceptorController<'a, 's> {
    /// Add an address to the `Acceptor` to listen for incoming connections on.
    pub fn add_listener<A: ToListenAddr>(&self, addr: A) -> Result<(), AddListenerError<A::Err>> {
        let addr = match addr.to_listen_addr() {
            Ok(addr) => addr,
            Err(e)   => return Err(AddListenerError::ParseError(e)),
        };

        let listener = match addr {
            ListenAddr::Tcp(tcp_addr) => try!(mio::tcp::TcpListener::bind(&tcp_addr)),
        };
        let listener_addr = PeerAddr::Tcp(try!(listener.local_addr()));

        {
            let mut notify_pipe = self.notify_pipe.lock().unwrap();
            try!(notify_pipe.write(&[0]));
        }
        {
            let mut listen_set = self.acceptor.listen_set.lock().unwrap();
            listen_set.fresh = true;
            let token = mio::Token(listen_set.listeners.len());
            try!(listen_set.poll.register(&listener, token, mio::EventSet::readable(), mio::PollOpt::level()));
            listen_set.listeners.push(listener);
            self.acceptor.listen_set_freshened.notify_one();
        }
        {
            let mut listener_events_reactors = self.acceptor.listener_events_reactors.lock().unwrap();
            let mut index = 0;
            while index < listener_events_reactors.len() {
                match listener_events_reactors[index]
                      .send(Some(ListenerEvent::StartListening(listener_addr.clone())))
                {
                    Ok(()) => {
                        index += 1;
                    },
                    Err(_) => {
                        listener_events_reactors.swap_remove(index);
                    },
                }
            }
        }
        Ok(())
    }

    /// Create a pair of objects that can be used to monitor the acceptor for listening addresses
    /// being added or removed. This allows you to keep track of what addresses the `Acceptor` is
    /// listening on. This is necessary because new addresses may be added asynchronously due to
    /// bootstrapping or the `Acceptor` detecting peers on the local network. Addresses may also be
    /// dropped when the `Acceptor` loses the ability to listen on one (such as when a network
    /// interface is unplugged).
    pub fn listener_events<'c>(&'c self) -> (ListenerEventsReactor<'c, 'a, 's>, ListenerEventsController<'c, 'a, 's>) {
        let (event_sender, event_receiver) = channel();
        {
            let mut listener_events_reactors = self.acceptor.listener_events_reactors.lock().unwrap();
            listener_events_reactors.push(event_sender.clone());
        }
        let r = ListenerEventsReactor {
            controller: PhantomData,
            event_receiver: event_receiver,
        };
        let c = ListenerEventsController {
            controller: PhantomData,
            event_sender: event_sender,
        };
        (r, c)
    }
}

impl<'a, 's: 'a> Drop for AcceptorController<'a, 's> {
    fn drop(&mut self) {
        // TODO: what could cause this to fail
        let _ = self.notify_pipe.lock().unwrap().write(&[1]);
    }
}

/// Issued when the state of an `Acceptor` has changed. See `AcceptorController::listener_events`
/// and the `ListenerEventsReactor` type for more details.
pub enum ListenerEvent {
    /// The `Acceptor` has started listening on an address.
    StartListening(PeerAddr),
}

/// Created via the `AcceptorController::listener_events` function. Used to react to changes in the
/// listener set of an `Acceptor`. Whenever an `Acceptor` starts or stops listening on an address
/// it issues a `ListenerEvent` to all `ListenerEventReactors`.
pub struct ListenerEventsReactor<'c, 'a: 'c, 's: 'a> {
    controller: PhantomData<&'c AcceptorController<'a, 's>>,
    event_receiver: Receiver<Option<ListenerEvent>>,
}

impl<'c, 'a: 'c, 's: 'a> ListenerEventsReactor<'c, 'a, 's> {
    /// Blocks until either the `Acceptor` issues a `ListenerEvent` or the
    /// `ListenerEventController` that `self` was created with is dropped.
    pub fn next_event(self) -> Option<(ListenerEventsReactor<'c, 'a, 's>, ListenerEvent)> {
        self.event_receiver.recv().unwrap().map(|e| (self, e))
    }
}

impl<'c, 'a: 'c, 's: 'a> IntoIterator for ListenerEventsReactor<'c, 'a, 's> {
    type Item = ListenerEvent;
    type IntoIter = ListenerEventsIter<'c, 'a, 's>;
    fn into_iter(self) -> ListenerEventsIter<'c, 'a, 's> {
        ListenerEventsIter {
            listener_events_reactor: Some(self),
        }
    }
}

/// Used to iterate over all `ListenerEvent`s from a `ListenerEventsReactor`.
pub struct ListenerEventsIter<'c, 'a: 'c, 's: 'a> {
    listener_events_reactor: Option<ListenerEventsReactor<'c, 'a, 's>>,
}

impl<'c, 'a: 'c, 's: 'a> Iterator for ListenerEventsIter<'c, 'a, 's> {
    type Item = ListenerEvent;
    
    fn next(&mut self) -> Option<ListenerEvent> {
        let x = self.listener_events_reactor.take();
        x.and_then(|r| r.next_event()).map(|(r, e)| {
            self.listener_events_reactor = Some(r);
            e
        })
    }
}

/// Created via the `AcceptorController::listener_events` function, this object can be dropped in
/// order to unblock the corresponding `ListenerEventsReactor`.
pub struct ListenerEventsController<'c, 'a: 'c, 's: 'a> {
    controller: PhantomData<&'c AcceptorController<'a, 's>>,
    event_sender: Sender<Option<ListenerEvent>>,
}

impl<'c, 'a: 'c, 's: 'a> Drop for ListenerEventsController<'c, 'a, 's> {
    fn drop(&mut self) {
        // don't care if the reactor has already shut down
        let _ = self.event_sender.send(None);
    }
}

