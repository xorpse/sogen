use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};

use bitflags::bitflags;
use sogen_sys::linux::ffi;

bitflags! {
    /// The `O_*` flags the descriptor was opened with, as the x86-64 Linux ABI defines them. Read-only access has no bit.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct OpenFlags: u32 {
        const APPEND = 0o2000;
        const ASYNC = 0o20000;
        const CLOEXEC = 0o2000000;
        const CREAT = 0o100;
        const DIRECT = 0o40000;
        const DIRECTORY = 0o200000;
        const DSYNC = 0o10000;
        const EXCL = 0o200;
        const LARGEFILE = 0o100000;
        const NOATIME = 0o1000000;
        const NOCTTY = 0o400;
        const NOFOLLOW = 0o400000;
        const NONBLOCK = 0o4000;
        const PATH = 0o10000000;
        const RDWR = 0o2;
        const SYNC = 0o4010000;
        const TMPFILE = 0o20200000;
        const TRUNC = 0o1000;
        const WRONLY = 0o1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Socket {
    domain: i32,
    socket_type: i32,
    protocol: i32,
    listening: bool,
    connected: bool,
    local_address: Option<SocketAddrV4>,
    peer_address: Option<SocketAddrV4>,
}

impl Socket {
    fn from_native(socket: &ffi::LinuxSocket) -> Self {
        let address = |address: u32, port: u16| {
            (address != 0 || port != 0).then(|| {
                SocketAddrV4::new(Ipv4Addr::from(u32::from_be(address)), u16::from_be(port))
            })
        };

        Self {
            domain: socket.domain,
            socket_type: socket.socket_type,
            protocol: socket.protocol,
            listening: socket.listening,
            connected: socket.connected,
            local_address: address(socket.bound_address, socket.bound_port),
            peer_address: address(socket.peer_address, socket.peer_port),
        }
    }

    /// The `AF_*` address family.
    pub fn domain(&self) -> i32 {
        self.domain
    }

    /// The `SOCK_*` type, without `SOCK_NONBLOCK` and `SOCK_CLOEXEC`.
    pub fn socket_type(&self) -> i32 {
        self.socket_type
    }

    pub fn protocol(&self) -> i32 {
        self.protocol
    }

    pub fn is_listening(&self) -> bool {
        self.listening
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// The IPv4 address the guest bound the socket to.
    pub fn local_address(&self) -> Option<SocketAddrV4> {
        self.local_address
    }

    /// The IPv4 address the guest connected the socket to.
    pub fn peer_address(&self) -> Option<SocketAddrV4> {
        self.peer_address
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDescriptorKind {
    Directory,
    Epoll,
    Event,
    File,
    /// A file the emulator generates, such as one under `/proc/self`.
    MemoryFile,
    PipeRead,
    PipeWrite,
    Socket(Socket),
}

/// A guest file descriptor, copied at the time it was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDescriptor {
    number: u32,
    kind: FileDescriptorKind,
    host_path: Option<PathBuf>,
    guest_path: Option<PathBuf>,
    flags: OpenFlags,
    close_on_exec: bool,
}

impl FileDescriptor {
    pub(super) fn from_native(descriptor: ffi::LinuxFileDescriptor) -> Self {
        let kind = match descriptor.kind {
            ffi::FileDescriptorType::Directory => FileDescriptorKind::Directory,
            ffi::FileDescriptorType::Epoll => FileDescriptorKind::Epoll,
            ffi::FileDescriptorType::Event => FileDescriptorKind::Event,
            ffi::FileDescriptorType::File => FileDescriptorKind::File,
            ffi::FileDescriptorType::MemoryFile => FileDescriptorKind::MemoryFile,
            ffi::FileDescriptorType::PipeRead => FileDescriptorKind::PipeRead,
            ffi::FileDescriptorType::PipeWrite => FileDescriptorKind::PipeWrite,
            ffi::FileDescriptorType::Socket => {
                FileDescriptorKind::Socket(Socket::from_native(&descriptor.socket))
            }
            _ => unreachable!("fd_type has no other values"),
        };
        let path = |path: String| (!path.is_empty()).then(|| PathBuf::from(path));

        Self {
            number: u32::try_from(descriptor.number).expect("descriptor numbers are non-negative"),
            kind,
            host_path: path(descriptor.host_path),
            guest_path: path(descriptor.guest_path),
            flags: OpenFlags::from_bits_retain(descriptor.flags as u32),
            close_on_exec: descriptor.close_on_exec,
        }
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn kind(&self) -> FileDescriptorKind {
        self.kind
    }

    /// The host file backing the descriptor; `/dev/stdin`, `/dev/stdout` and `/dev/stderr` for the standard streams.
    pub fn host_path(&self) -> Option<&Path> {
        self.host_path.as_deref()
    }

    /// The guest path the descriptor was opened with.
    pub fn guest_path(&self) -> Option<&Path> {
        self.guest_path.as_deref()
    }

    pub fn flags(&self) -> OpenFlags {
        self.flags
    }

    pub fn close_on_exec(&self) -> bool {
        self.close_on_exec
    }
}
