use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use bitflags::bitflags;
use sogen_sys::windows::{SocketBridge, SocketFactoryBridge, ffi};

use super::settings::AddressFamily;

/// Creates the guest's sockets for guest syscalls and snapshot restores. Emulators built from the same settings share it
/// and may call it concurrently; it must not use the emulator. Returning `None` defers to the socket factory of the
/// emulator's [`NetworkMode`](super::NetworkMode), like a [`DnsResolver`](super::DnsResolver).
pub trait SocketFactory: Send + Sync + 'static {
    fn create_socket(&self, family: AddressFamily, kind: SocketKind) -> Option<Box<dyn Socket>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SocketKind {
    Datagram,
    Stream,
}

/// A non-blocking guest socket, used by one emulator thread at a time: operations that cannot complete yet fail with
/// [`io::ErrorKind::WouldBlock`], and the guest waits by polling [`Socket::readiness`]. Errors carrying an OS error code
/// reach the guest with that code; otherwise their kind selects the matching socket error. It must not use the emulator.
pub trait Socket: Send + 'static {
    fn accept(&mut self) -> io::Result<Connection>;
    fn bind(&mut self, address: SocketAddr) -> io::Result<()>;
    /// An in-progress connection fails with [`io::ErrorKind::WouldBlock`] and completes once writable.
    fn connect(&mut self, address: SocketAddr) -> io::Result<()>;
    fn is_listening(&self) -> bool;
    fn listen(&mut self, backlog: u32) -> io::Result<()>;
    fn local_address(&self) -> io::Result<SocketAddr>;
    fn readiness(&mut self) -> Readiness;
    fn receive(&mut self, data: &mut [u8]) -> io::Result<usize>;
    fn receive_from(&mut self, data: &mut [u8]) -> io::Result<Datagram>;
    fn send(&mut self, data: &[u8]) -> io::Result<usize>;
    fn send_to(&mut self, data: &[u8], destination: SocketAddr) -> io::Result<usize>;
    /// The pending asynchronous error, such as a refused connection.
    fn take_error(&mut self) -> io::Result<Option<io::Error>>;
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Readiness: u8 {
        const ERROR = 1;
        const HANG_UP = 1 << 1;
        const READABLE = 1 << 2;
        const WRITABLE = 1 << 3;
    }
}

/// An accepted connection.
pub struct Connection {
    socket: Box<dyn Socket>,
    peer: SocketAddr,
}

impl Connection {
    pub fn new(socket: impl Socket, peer: SocketAddr) -> Self {
        Self {
            socket: Box::new(socket),
            peer,
        }
    }

    pub fn socket(&self) -> &dyn Socket {
        &*self.socket
    }

    pub fn peer(&self) -> SocketAddr {
        self.peer
    }
}

/// A received datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Datagram {
    length: usize,
    source: SocketAddr,
}

impl Datagram {
    /// `length` is the number of bytes the datagram stored in the receive buffer.
    pub fn new(length: usize, source: SocketAddr) -> Self {
        Self { length, source }
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn source(&self) -> SocketAddr {
        self.source
    }
}

pub(super) struct SocketFactoryAdapter(pub(super) Arc<dyn SocketFactory>);

impl SocketFactoryBridge for SocketFactoryAdapter {
    fn create_socket(&self, family: u8, kind: ffi::SocketType) -> Option<Box<dyn SocketBridge>> {
        let family = match family {
            6 => AddressFamily::Ipv6,
            _ => AddressFamily::Ipv4,
        };
        let kind = match kind {
            ffi::SocketType::Stream => SocketKind::Stream,
            _ => SocketKind::Datagram,
        };
        let socket = self.0.create_socket(family, kind)?;
        Some(Box::new(SocketAdapter(socket)))
    }
}

struct SocketAdapter(Box<dyn Socket>);

impl SocketBridge for SocketAdapter {
    fn accept(
        &mut self,
        peer: &mut ffi::SocketAddress,
    ) -> Result<Box<dyn SocketBridge>, ffi::SocketStatus> {
        let connection = self.0.accept().map_err(|error| failure(&error))?;
        *peer = connection.peer.into();
        Ok(Box::new(SocketAdapter(connection.socket)))
    }

    fn bind(&mut self, address: &ffi::SocketAddress) -> ffi::SocketStatus {
        status(self.0.bind(address.into()).map(|()| 0))
    }

    fn connect(&mut self, address: &ffi::SocketAddress) -> ffi::SocketStatus {
        status(self.0.connect(address.into()).map(|()| 0))
    }

    fn is_listening(&self) -> bool {
        self.0.is_listening()
    }

    fn listen(&mut self, backlog: u32) -> ffi::SocketStatus {
        status(self.0.listen(backlog).map(|()| 0))
    }

    fn local_address(&self, address: &mut ffi::SocketAddress) -> bool {
        let Ok(local) = self.0.local_address() else {
            return false;
        };
        *address = local.into();
        true
    }

    fn readiness(&mut self) -> ffi::SocketReadiness {
        let readiness = self.0.readiness();
        ffi::SocketReadiness {
            error: readiness.contains(Readiness::ERROR),
            hang_up: readiness.contains(Readiness::HANG_UP),
            readable: readiness.contains(Readiness::READABLE),
            writable: readiness.contains(Readiness::WRITABLE),
        }
    }

    fn receive(&mut self, data: &mut [u8]) -> ffi::SocketStatus {
        status(self.0.receive(data))
    }

    fn receive_from(
        &mut self,
        data: &mut [u8],
        source: &mut ffi::SocketAddress,
    ) -> ffi::SocketStatus {
        status(self.0.receive_from(data).map(|datagram| {
            *source = datagram.source.into();
            datagram.length
        }))
    }

    fn send(&mut self, data: &[u8]) -> ffi::SocketStatus {
        status(self.0.send(data))
    }

    fn send_to(&mut self, data: &[u8], destination: &ffi::SocketAddress) -> ffi::SocketStatus {
        status(self.0.send_to(data, destination.into()))
    }

    fn take_error(&mut self, pending: &mut ffi::SocketStatus) -> bool {
        let Ok(error) = self.0.take_error() else {
            return false;
        };
        *pending = error.map_or(ffi::SocketStatus::success(0), |error| failure(&error));
        true
    }
}

fn status(result: io::Result<usize>) -> ffi::SocketStatus {
    match result {
        Ok(count) => ffi::SocketStatus::success(count as u64),
        Err(error) => failure(&error),
    }
}

fn failure(error: &io::Error) -> ffi::SocketStatus {
    if let Some(code) = error.raw_os_error() {
        return ffi::SocketStatus {
            error: ffi::SocketErrorKind::Os,
            os_error: code,
            count: 0,
        };
    }

    let kind = match error.kind() {
        io::ErrorKind::WouldBlock => ffi::SocketErrorKind::WouldBlock,
        io::ErrorKind::ConnectionRefused => ffi::SocketErrorKind::ConnectionRefused,
        io::ErrorKind::ConnectionReset => ffi::SocketErrorKind::ConnectionReset,
        io::ErrorKind::AddrInUse => ffi::SocketErrorKind::AddressInUse,
        io::ErrorKind::AddrNotAvailable => ffi::SocketErrorKind::AddressNotAvailable,
        io::ErrorKind::NotConnected => ffi::SocketErrorKind::NotConnected,
        io::ErrorKind::InvalidInput => ffi::SocketErrorKind::InvalidInput,
        _ => ffi::SocketErrorKind::Other,
    };
    ffi::SocketStatus {
        error: kind,
        os_error: 0,
        count: 0,
    }
}
