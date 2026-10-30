use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, SystemTime};
use std::{env, fs, process, thread};

use sogen::windows::{
    AddressFamily, Allocation, ApiSignature, Application, Architecture, CallingConvention, Clock,
    Connection, Datagram, DirectoryEntry, Emulator, File, FileInfo, FilesystemMode,
    FilesystemProvider, HostMemory, MmioDevice, Mount, NetworkMode, OpenMode, Readiness,
    RegistryValue, Settings, Socket, SocketActivity, SocketFactory, SocketKind,
};
use sogen::{
    Backend, CallContinuation, DescriptorTable, Error, Guest, MemoryPermission, MemoryRegionKind,
    X86Register, X86Segment,
};

const SAMPLE_GUEST_PATH: &str = r"C:\test-sample.exe";
const READ_WRITE: MemoryPermission = MemoryPermission::READ.union(MemoryPermission::WRITE);

fn emulator_root() -> Option<PathBuf> {
    let root = env::var_os("EMULATOR_ROOT").map(PathBuf::from);
    if root.is_none() {
        eprintln!("skipping: EMULATOR_ROOT is not set");
    }
    root
}

/// The emulation root with the analysis sample available as `C:\test-sample.exe`, as test.py prepares it.
fn sample_root() -> Option<PathBuf> {
    let root = emulator_root()?;
    let Some(sample) = env::var_os("ANALYSIS_SAMPLE").map(PathBuf::from) else {
        eprintln!("skipping: ANALYSIS_SAMPLE is not set");
        return None;
    };
    assert!(sample.exists(), "{} does not exist", sample.display());

    let filesys = root.join("filesys").join("c");
    fs::create_dir_all(&filesys).unwrap();
    let copy = filesys.join("test-sample.exe");
    if !copy.exists() {
        fs::copy(&sample, &copy).unwrap();
    }

    Some(root)
}

fn capture_stdout(emulator: &Emulator) -> Arc<Mutex<Vec<u8>>> {
    let output = Arc::new(Mutex::new(Vec::new()));
    let captured = output.clone();
    emulator.on_stdout(move |_, data| captured.lock().unwrap().extend_from_slice(data));
    output
}

fn text(output: &Mutex<Vec<u8>>) -> String {
    String::from_utf8_lossy(&output.lock().unwrap()).into_owned()
}

/// A writable `C:\a.txt` backed by a fresh host directory, which the sample's I/O tests need.
fn scratch_mount(name: &str) -> (PathBuf, Mount) {
    let directory = env::temp_dir().join(format!("sogen-rust-{name}-{}", process::id()));
    fs::create_dir_all(&directory).unwrap();
    let mount =
        Mount::new(directory.join("a.txt"), r"C:\a.txt").with_mode(FilesystemMode::ReadWrite);
    (directory, mount)
}

/// Answers every read with bytes derived from their offset and records every written byte by offset.
#[derive(Clone, Default)]
struct RecordingDevice {
    written: Arc<Mutex<BTreeMap<u64, u8>>>,
}

impl RecordingDevice {
    fn byte(offset: u64) -> u8 {
        offset as u8 ^ 0x5a
    }
}

impl MmioDevice for RecordingDevice {
    fn read(&self, offset: u64, data: &mut [u8]) {
        for (byte, offset) in data.iter_mut().zip(offset..) {
            *byte = Self::byte(offset);
        }
    }

    fn write(&self, offset: u64, data: &[u8]) {
        self.written
            .lock()
            .unwrap()
            .extend((offset..).zip(data.iter().copied()));
    }
}

/// Runs `mov rax, [source]; mov [target], rax` on the sample's main thread, once it is scheduled.
fn run_copy(emulator: &mut Emulator, source: u64, target: u64) {
    let mut code = vec![0x48, 0xa1];
    code.extend(source.to_le_bytes());
    code.extend([0x48, 0xa3]);
    code.extend(target.to_le_bytes());

    let base = emulator
        .allocate_memory(&Allocation::new(
            0x1000,
            MemoryPermission::READ | MemoryPermission::EXEC,
        ))
        .unwrap()
        .unwrap();
    emulator.write_memory(base, code).unwrap();
    emulator.run_instructions(1).unwrap();
    emulator.write_register(X86Register::Rip, base).unwrap();
    emulator.run_instructions(2).unwrap();
}

fn sample_emulator(root: &Path, backend: Backend) -> Emulator {
    let settings = Settings::new()
        .with_emulation_root(root)
        .with_backend(backend);
    Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap()
}

const GUEST_TIME_NANOSECONDS: u64 = 1_700_000_000_123_456_000;

/// A fixed wall clock whose steady time and timestamp counter advance by one tick per reading.
#[derive(Default)]
struct SteppingClock {
    ticks: AtomicU64,
}

impl Clock for SteppingClock {
    fn system_time(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_nanos(GUEST_TIME_NANOSECONDS)
    }

    fn steady_time(&self) -> Duration {
        Duration::from_micros(self.ticks.fetch_add(1, Ordering::Relaxed))
    }

    fn timestamp_counter(&self) -> u64 {
        self.ticks.fetch_add(1, Ordering::Relaxed)
    }
}

struct Packet {
    source: SocketAddr,
    destination: SocketAddr,
    data: Vec<u8>,
}

/// An in-process IPv4 datagram network.
#[derive(Default)]
struct LoopbackNetwork {
    sockets: AtomicUsize,
    in_flight: Mutex<Vec<Packet>>,
    delivered: Mutex<Vec<Vec<u8>>>,
}

#[derive(Clone, Default)]
struct LoopbackFactory(Arc<LoopbackNetwork>);

impl SocketFactory for LoopbackFactory {
    fn create_socket(&self, family: AddressFamily, kind: SocketKind) -> Option<Box<dyn Socket>> {
        if family != AddressFamily::Ipv4 || kind != SocketKind::Datagram {
            return None;
        }

        let index = self.0.sockets.fetch_add(1, Ordering::Relaxed);
        Some(Box::new(LoopbackSocket {
            network: self.0.clone(),
            local: SocketAddr::from(([127, 0, 0, 1], 50_000 + index as u16)),
        }))
    }
}

struct LoopbackSocket {
    network: Arc<LoopbackNetwork>,
    local: SocketAddr,
}

impl Socket for LoopbackSocket {
    fn accept(&mut self) -> io::Result<Connection> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn bind(&mut self, address: SocketAddr) -> io::Result<()> {
        self.local = address;
        Ok(())
    }

    fn connect(&mut self, _address: SocketAddr) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn is_listening(&self) -> bool {
        false
    }

    fn listen(&mut self, _backlog: u32) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn local_address(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }

    fn readiness(&mut self) -> Readiness {
        let in_flight = self.network.in_flight.lock().unwrap();
        if in_flight
            .iter()
            .any(|packet| packet.destination == self.local)
        {
            Readiness::READABLE | Readiness::WRITABLE
        } else {
            Readiness::WRITABLE
        }
    }

    fn receive(&mut self, _data: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::NotConnected.into())
    }

    fn receive_from(&mut self, data: &mut [u8]) -> io::Result<Datagram> {
        let mut in_flight = self.network.in_flight.lock().unwrap();
        let index = in_flight
            .iter()
            .position(|packet| packet.destination == self.local)
            .ok_or(io::ErrorKind::WouldBlock)?;
        let packet = in_flight.remove(index);
        let length = packet.data.len().min(data.len());
        data[..length].copy_from_slice(&packet.data[..length]);
        self.network.delivered.lock().unwrap().push(packet.data);
        Ok(Datagram::new(length, packet.source))
    }

    fn send(&mut self, _data: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::NotConnected.into())
    }

    fn send_to(&mut self, data: &[u8], destination: SocketAddr) -> io::Result<usize> {
        self.network.in_flight.lock().unwrap().push(Packet {
            source: self.local,
            destination,
            data: data.to_vec(),
        });
        Ok(data.len())
    }

    fn take_error(&mut self) -> io::Result<Option<io::Error>> {
        Ok(None)
    }
}

/// A flat directory of files held in memory, recording how each file was opened.
#[derive(Default)]
struct MemoryFiles {
    files: Mutex<BTreeMap<String, Arc<Mutex<Vec<u8>>>>>,
    opened: Mutex<Vec<(String, OpenMode)>>,
}

#[derive(Clone, Default)]
struct MemoryProvider(Arc<MemoryFiles>);

impl FilesystemProvider for MemoryProvider {
    fn file_info(&self, path: &str) -> Option<FileInfo> {
        if path.is_empty() {
            return Some(FileInfo::directory());
        }

        let files = self.0.files.lock().unwrap();
        let size = files.get(path)?.lock().unwrap().len();
        Some(FileInfo::file(size as u64))
    }

    fn read_directory(&self, path: &str) -> io::Result<Vec<DirectoryEntry>> {
        if !path.is_empty() {
            return Err(io::ErrorKind::NotFound.into());
        }

        let files = self.0.files.lock().unwrap();
        Ok(files
            .iter()
            .map(|(name, data)| {
                let size = data.lock().unwrap().len() as u64;
                DirectoryEntry::new(name, false).with_info(FileInfo::file(size))
            })
            .collect())
    }

    fn open(&self, path: &str, mode: OpenMode) -> io::Result<Box<dyn File>> {
        if path.is_empty() {
            return Err(io::ErrorKind::IsADirectory.into());
        }

        self.0.opened.lock().unwrap().push((path.to_owned(), mode));
        let mut files = self.0.files.lock().unwrap();
        let data = match mode {
            OpenMode::Read | OpenMode::ReadWrite => {
                files.get(path).cloned().ok_or(io::ErrorKind::NotFound)?
            }
            OpenMode::Write | OpenMode::WriteRead => {
                let data = files.entry(path.to_owned()).or_default().clone();
                data.lock().unwrap().clear();
                data
            }
            OpenMode::Append | OpenMode::AppendRead => {
                files.entry(path.to_owned()).or_default().clone()
            }
        };
        Ok(Box::new(MemoryFile {
            data,
            position: 0,
            append: matches!(mode, OpenMode::Append | OpenMode::AppendRead),
        }))
    }

    fn create_directory(&self, _path: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let mut files = self.0.files.lock().unwrap();
        let data = files.remove(from).ok_or(io::ErrorKind::NotFound)?;
        files.insert(to.to_owned(), data);
        Ok(())
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        let mut files = self.0.files.lock().unwrap();
        files.remove(path).ok_or(io::ErrorKind::NotFound)?;
        Ok(())
    }
}

struct MemoryFile {
    data: Arc<Mutex<Vec<u8>>>,
    position: u64,
    append: bool,
}

impl Read for MemoryFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let data = self.data.lock().unwrap();
        let start = (self.position as usize).min(data.len());
        let count = buffer.len().min(data.len() - start);
        buffer[..count].copy_from_slice(&data[start..start + count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl Write for MemoryFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut data = self.data.lock().unwrap();
        if self.append {
            self.position = data.len() as u64;
        }

        let start = self.position as usize;
        let end = start + buffer.len();
        if data.len() < end {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(buffer);
        self.position = end as u64;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for MemoryFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let length = self.data.lock().unwrap().len() as i64;
        let target = match position {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::Current(offset) => self.position as i64 + offset,
            SeekFrom::End(offset) => length + offset,
        };
        self.position = u64::try_from(target).map_err(|_| io::ErrorKind::InvalidInput)?;
        Ok(self.position)
    }
}

impl File for MemoryFile {
    fn set_len(&mut self, size: u64) -> io::Result<()> {
        self.data.lock().unwrap().resize(size as usize, 0);
        Ok(())
    }

    fn info(&mut self) -> io::Result<FileInfo> {
        Ok(FileInfo::file(self.data.lock().unwrap().len() as u64))
    }
}

#[test]
fn empty_emulator_state_round_trips() {
    let Some(root) = emulator_root() else {
        return;
    };

    let settings = Settings::new().with_emulation_root(root);
    let mut emulator = Emulator::empty(&settings, Architecture::X86_64).unwrap();

    let base = emulator
        .allocate_memory(&Allocation::new(0x1000, READ_WRITE))
        .unwrap()
        .unwrap();
    let region = emulator.memory_region(base).unwrap();
    assert_eq!(region.start(), base);
    assert_eq!(region.allocation_base(), base);
    assert_eq!(region.permissions(), READ_WRITE);
    assert_eq!(region.kind(), MemoryRegionKind::PrivateAllocation);
    assert!(region.is_committed());
    assert!(emulator.memory_stats().committed_memory() >= 0x1000);

    emulator.write_memory(base, b"ABCD").unwrap();
    let state = emulator.serialise_state().unwrap();
    emulator.write_memory(base, b"WXYZ").unwrap();
    emulator.deserialise_state(&state).unwrap();
    let mut data = [0; 4];
    emulator.read_memory(base, &mut data).unwrap();
    assert_eq!(&data, b"ABCD");

    emulator.save_snapshot().unwrap();
    emulator.write_memory(base, b"1234").unwrap();
    emulator.restore_snapshot().unwrap();
    emulator.read_memory(base, &mut data).unwrap();
    assert_eq!(&data, b"ABCD");

    assert!(
        emulator
            .protect_memory(base, 0x1000, MemoryPermission::READ)
            .unwrap()
    );
    assert_eq!(
        emulator.memory_region(base).unwrap().permissions(),
        MemoryPermission::READ
    );

    let reserved = emulator
        .allocate_memory(&Allocation::new(0x2000, READ_WRITE).with_reserve_only(true))
        .unwrap()
        .unwrap();
    let region = emulator.memory_region(reserved).unwrap();
    assert!(region.is_reserved());
    assert!(!region.is_committed());
    assert!(
        emulator
            .commit_memory(reserved, 0x1000, READ_WRITE)
            .unwrap()
    );
    assert!(emulator.memory_region(reserved).unwrap().is_committed());
    assert!(emulator.release_memory(reserved, 0).unwrap());
    assert_eq!(
        emulator.memory_region(reserved).unwrap().kind(),
        MemoryRegionKind::Free
    );
    assert!(emulator.memory_region(0).is_none());

    assert_eq!(emulator.host_port(28970), None);
    assert!(matches!(emulator.map_port(28970, 0), Err(Error::ZeroPort)));
    emulator.map_port(28970, 28971).unwrap();
    assert_eq!(emulator.host_port(28970), Some(28971));
    assert_eq!(emulator.emulator_port(28971), Some(28970));
    emulator.map_port(28970, 28970).unwrap();
    assert_eq!(emulator.emulator_port(28971), None);

    assert!(emulator.process().exit_status().is_none());
    assert!(emulator.current_thread().is_none());
    assert!(emulator.modules().is_empty());

    let moved = thread::spawn(move || {
        let mut data = [0; 4];
        emulator.read_memory(base, &mut data).unwrap();
        data
    })
    .join()
    .unwrap();
    assert_eq!(&moved, b"ABCD");
}

#[test]
fn analysis_sample_exits_cleanly_with_hooks_and_a_dns_resolver() {
    let Some(root) = sample_root() else {
        return;
    };

    let (mapped_dir, mount) = scratch_mount("analysis");

    let resolved = Arc::new(Mutex::new(Vec::new()));
    let recorded_lookups = resolved.clone();
    let settings = Settings::new()
        .with_emulation_root(&root)
        .with_mount(mount)
        .with_network(NetworkMode::Live)
        .with_port_mapping(28970, 28980)
        .with_dns_resolver(move |hostname, _| {
            recorded_lookups.lock().unwrap().push(hostname.to_owned());
            hostname
                .eq_ignore_ascii_case("google.com")
                .then(|| vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))])
        });

    let mut emulator =
        Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap();
    let stdout = capture_stdout(&emulator);

    let sockets = Arc::new(Mutex::new(Vec::new()));
    let recorded_sockets = sockets.clone();
    emulator.on_socket_activity(move |_, activity, address| {
        recorded_sockets.lock().unwrap().push((activity, address));
    });

    let modules = Arc::new(Mutex::new(Vec::new()));
    let recorded_modules = modules.clone();
    emulator.on_module_load(move |_, module| {
        recorded_modules
            .lock()
            .unwrap()
            .push(module.name().to_ascii_lowercase());
    });

    let sleeps = Arc::new(Mutex::new(Vec::new()));
    let recorded_sleeps = sleeps.clone();
    emulator
        .hook_api(
            "Sleep",
            ApiSignature::new(CallingConvention::Stdcall, 1),
            move |_, call, arguments| {
                assert_eq!(call.name(), "Sleep");
                recorded_sleeps.lock().unwrap().push(arguments[0]);
                CallContinuation::RunOriginal
            },
        )
        .unwrap();

    emulator.run().unwrap();
    let output = text(&stdout);
    eprintln!(
        "{output}\nexit_status={:?} stop_reason={:?} stop_detail={} executed_instructions={} backend={}",
        emulator.process().exit_status(),
        emulator.last_stop_reason(),
        emulator.last_stop_detail(),
        emulator.executed_instructions(),
        emulator.backend_name(),
    );
    fs::remove_dir_all(&mapped_dir).ok();

    assert_eq!(emulator.process().exit_status(), Some(0));
    assert!(output.contains("Running test 'Socket': Success"));

    let sleeps = sleeps.lock().unwrap();
    assert!(!sleeps.is_empty());
    assert!(sleeps.iter().all(|&milliseconds| milliseconds <= 1));

    let expected = SocketAddr::from(([127, 0, 0, 1], 28970));
    let sockets = sockets.lock().unwrap();
    assert!(
        sockets.contains(&(SocketActivity::Bind, expected)),
        "{sockets:?}"
    );
    assert!(
        sockets.contains(&(SocketActivity::SendTo, expected)),
        "{sockets:?}"
    );

    assert!(
        resolved
            .lock()
            .unwrap()
            .iter()
            .any(|hostname| hostname.eq_ignore_ascii_case("google.com"))
    );
    assert!(
        modules
            .lock()
            .unwrap()
            .iter()
            .any(|name| name == "kernel32.dll")
    );
    assert!(emulator.find_module_by_name("ntdll.dll").is_some());
}

#[test]
fn intercepted_api_returns_to_the_guest_with_the_hook_value() {
    let Some(root) = sample_root() else {
        return;
    };

    const DNS_ERROR_RCODE_NAME_ERROR: u64 = 9003;

    let (mapped_dir, mount) = scratch_mount("intercept");
    let settings = Settings::new()
        .with_emulation_root(&root)
        .with_mount(mount)
        .with_network(NetworkMode::Live);
    let mut emulator =
        Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap();
    let stdout = capture_stdout(&emulator);

    let hits = Arc::new(Mutex::new(Vec::new()));
    let recorded = hits.clone();
    emulator
        .hook_api(
            "DNSAPI!DnsQuery_A",
            ApiSignature::new(CallingConvention::Stdcall, 6),
            move |context, call, _| {
                recorded.lock().unwrap().push(call.module().to_owned());
                assert!(context.unhook_api("DNSAPI!DnsQuery_A"));
                call.set_return_value(DNS_ERROR_RCODE_NAME_ERROR);
                CallContinuation::Intercept
            },
        )
        .unwrap();

    emulator.run().unwrap();
    let output = text(&stdout);
    eprintln!(
        "{output}\nexit_status={:?} stop_reason={:?}",
        emulator.process().exit_status(),
        emulator.last_stop_reason()
    );
    fs::remove_dir_all(&mapped_dir).ok();

    assert_eq!(
        hits.lock().unwrap().as_slice(),
        ["dnsapi.dll".to_owned()].as_slice()
    );
    let lines = output.lines().collect::<Vec<_>>();
    let dns = format!("Running test 'DNS': DnsQuery_A failed: {DNS_ERROR_RCODE_NAME_ERROR}");
    assert!(lines.windows(2).any(|pair| pair == [dns.as_str(), "Fail"]));
    assert_eq!(lines.iter().filter(|line| **line == "Fail").count(), 1);
    assert!(lines.contains(&"Running test 'GDI DIB': Success"));
    assert_eq!(emulator.process().exit_status(), Some(1));
}

#[test]
fn intercepting_hook_ends_dispatch_at_its_address() {
    let Some(root) = sample_root() else {
        return;
    };

    let (mapped_dir, mount) = scratch_mount("same-address");
    let settings = Settings::new()
        .with_emulation_root(&root)
        .with_mount(mount)
        .with_network(NetworkMode::Live)
        .with_port_mapping(28970, 28990)
        .with_dns_resolver(|hostname, _| {
            hostname
                .eq_ignore_ascii_case("google.com")
                .then(|| vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))])
        });
    let mut emulator =
        Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap();
    let stdout = capture_stdout(&emulator);

    let signature = ApiSignature::new(CallingConvention::Stdcall, 1);
    let intercepted = Arc::new(AtomicUsize::new(0));
    let counted = intercepted.clone();
    emulator
        .hook_api("kernel32!Sleep", signature, move |_, _, _| {
            counted.fetch_add(1, Ordering::Relaxed);
            CallContinuation::Intercept
        })
        .unwrap();
    let shadowed = Arc::new(AtomicUsize::new(0));
    let counted = shadowed.clone();
    emulator
        .hook_api("kernel32.dll!Sleep", signature, move |_, _, _| {
            counted.fetch_add(1, Ordering::Relaxed);
            CallContinuation::Intercept
        })
        .unwrap();

    emulator.run().unwrap();
    let output = text(&stdout);
    eprintln!(
        "{output}\nexit_status={:?} stop_reason={:?}",
        emulator.process().exit_status(),
        emulator.last_stop_reason()
    );
    fs::remove_dir_all(&mapped_dir).ok();

    assert!(intercepted.load(Ordering::Relaxed) > 0);
    assert_eq!(shadowed.load(Ordering::Relaxed), 0);
    assert_eq!(emulator.process().exit_status(), Some(0));
}

#[test]
fn first_execution_of_the_sample_text_section_is_reported() {
    let Some(root) = sample_root() else {
        return;
    };

    let settings = Settings::new().with_emulation_root(&root);
    let mut emulator =
        Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap();

    let executions = Arc::new(Mutex::new(Vec::new()));
    let recorded = executions.clone();
    emulator.on_section_first_execution(move |context, module, section, address| {
        if module.name() == "test-sample.exe" {
            recorded.lock().unwrap().push((
                section.name().to_owned(),
                section.contains(address),
                section.first_execution(),
                address,
            ));
            context.stop();
        }
    });

    emulator.run().unwrap();

    let executions = executions.lock().unwrap();
    let Some((name, contained, first_execution, address)) = executions.first() else {
        panic!("no section of the sample was executed");
    };
    assert_eq!(name, ".text");
    assert!(contained);
    assert_eq!(*first_execution, Some(*address));

    let module = emulator.find_module_by_name("test-sample.exe").unwrap();
    let text = module
        .sections()
        .iter()
        .find(|section| section.name() == ".text")
        .unwrap();
    assert_eq!(text.first_execution(), Some(*address));
}

#[test]
fn registry_values_round_trip() {
    let Some(root) = emulator_root() else {
        return;
    };

    const PARENT: &str = r"\Registry\Machine\Software";
    const KEY: &str = r"\Registry\Machine\Software\SogenRustBindings\Test";

    let emulator = Emulator::empty(
        &Settings::new().with_emulation_root(root),
        Architecture::X86_64,
    )
    .unwrap();
    assert!(matches!(
        emulator.registry_value(KEY, "Name"),
        Err(Error::RegistryKeyNotFound(_))
    ));
    assert!(emulator.create_registry_key(KEY).unwrap());

    let values = [
        ("Name", RegistryValue::String("sogen ✓".to_owned())),
        (
            "Path",
            RegistryValue::ExpandString(r"%SystemRoot%\System32".to_owned()),
        ),
        ("Count", RegistryValue::Dword(0xdead_beef)),
        ("Total", RegistryValue::Qword(0x0123_4567_89ab_cdef)),
        ("Blob", RegistryValue::Binary(vec![0, 1, 2, 0xff])),
        (
            "List",
            RegistryValue::MultiString(vec!["one".to_owned(), "two".to_owned()]),
        ),
    ];
    for (name, value) in &values {
        emulator.set_registry_value(KEY, name, value).unwrap();
    }

    for (name, value) in &values {
        assert_eq!(
            emulator.registry_value(KEY, name).unwrap().as_ref(),
            Some(value)
        );
    }
    assert_eq!(emulator.registry_value(KEY, "Missing").unwrap(), None);

    let entries = emulator.registry_values(KEY).unwrap();
    assert_eq!(entries.len(), values.len());
    for (name, value) in &values {
        assert!(
            entries
                .iter()
                .any(|entry| entry.name() == *name && entry.value() == value)
        );
    }

    assert!(
        emulator
            .registry_subkeys(PARENT)
            .unwrap()
            .iter()
            .any(|name| name == "SogenRustBindings")
    );
}

#[test]
fn snapshot_file_restores_the_process() {
    let Some(root) = sample_root() else {
        return;
    };

    let settings = Settings::new().with_emulation_root(&root);
    let mut emulator =
        Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap();
    emulator.run_instructions(10_000).unwrap();

    let base = emulator
        .allocate_memory(&Allocation::new(0x1000, READ_WRITE))
        .unwrap()
        .unwrap();
    emulator.write_memory(base, b"snapshot").unwrap();

    let path = env::temp_dir().join(format!("sogen-rust-snapshot-{}.snap", process::id()));
    emulator.write_snapshot(&path).unwrap();

    let restored = Emulator::snapshot(&settings, &path);
    let other_settings = settings.clone().with_root_mode(FilesystemMode::ReadWrite);
    let mismatched = Emulator::snapshot(&other_settings, &path);
    fs::remove_file(&path).ok();

    let restored = restored.unwrap();
    let mut data = [0; 8];
    restored.read_memory(base, &mut data).unwrap();
    assert_eq!(&data, b"snapshot");
    assert_eq!(restored.current_thread(), emulator.current_thread());
    assert_eq!(
        restored
            .modules()
            .iter()
            .map(|module| module.image_base())
            .collect::<Vec<_>>(),
        emulator
            .modules()
            .iter()
            .map(|module| module.image_base())
            .collect::<Vec<_>>()
    );
    assert!(matches!(mismatched, Err(Error::Native(_))));
}

#[test]
fn thread_enumeration_includes_the_current_thread() {
    let Some(root) = sample_root() else {
        return;
    };

    let settings = Settings::new().with_emulation_root(&root);
    let mut emulator =
        Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap();
    emulator.run_instructions(10_000).unwrap();

    let current = emulator.current_thread().unwrap();
    let threads = emulator.threads();
    assert!(threads.contains(&current), "{threads:?}");
    assert_eq!(emulator.thread(current.id()).as_ref(), Some(&current));
    assert!(current.teb_address().is_some());
    assert!(current.stack().size() > 0);
    assert_eq!(current.wait_state(), None);
    assert_eq!(current.id(), emulator.process().initial_thread_id());

    assert_eq!(
        emulator.thread_registers(current.id()).unwrap(),
        Some(emulator.save_registers().unwrap())
    );
    assert_eq!(emulator.thread(u32::MAX), None);
}

#[test]
fn memory_regions_contain_allocations() {
    let Some(root) = emulator_root() else {
        return;
    };

    let emulator = Emulator::empty(
        &Settings::new().with_emulation_root(root),
        Architecture::X86_64,
    )
    .unwrap();
    let base = emulator
        .allocate_memory(&Allocation::new(0x3000, READ_WRITE).with_reserve_only(true))
        .unwrap()
        .unwrap();
    assert!(
        emulator
            .commit_memory(base + 0x1000, 0x1000, READ_WRITE)
            .unwrap()
    );

    let regions = emulator
        .memory_regions()
        .into_iter()
        .filter(|region| region.allocation_base() == base)
        .map(|region| (region.start(), region.length(), region.is_committed()))
        .collect::<Vec<_>>();
    assert_eq!(
        regions,
        [
            (base, 0x1000, false),
            (base + 0x1000, 0x1000, true),
            (base + 0x2000, 0x1000, false),
        ]
    );

    let fixed = emulator.find_free_allocation_base(0x1000, None).unwrap();
    assert!(
        emulator
            .allocate_memory_at(fixed, &Allocation::new(0x1000, READ_WRITE))
            .unwrap()
    );
    assert!(
        !emulator
            .allocate_memory_at(fixed, &Allocation::new(0x1000, READ_WRITE))
            .unwrap()
    );
    assert!(
        emulator
            .memory_regions()
            .iter()
            .any(|region| region.start() == fixed && region.is_committed())
    );
}

#[test]
fn x86_descriptor_tables_and_segment_bases_round_trip() {
    let Some(root) = emulator_root() else {
        return;
    };

    let emulator = Emulator::empty(
        &Settings::new().with_emulation_root(root),
        Architecture::X86_64,
    )
    .unwrap();
    let gdt = emulator
        .allocate_memory(&Allocation::new(0x1000, READ_WRITE))
        .unwrap()
        .unwrap();
    emulator.load_gdt(gdt, 0xfff).unwrap();

    let loaded = emulator.descriptor_table(DescriptorTable::Gdt).unwrap();
    assert_eq!((loaded.base(), loaded.limit()), (gdt, 0xfff));
    assert_ne!(
        emulator.descriptor_table(DescriptorTable::Idt).unwrap(),
        loaded
    );

    emulator
        .set_segment_base(X86Segment::Fs, 0x1234_5000)
        .unwrap();
    emulator
        .set_segment_base(X86Segment::Gs, 0x5678_9000)
        .unwrap();
    assert_eq!(emulator.segment_base(X86Segment::Fs).unwrap(), 0x1234_5000);
    assert_eq!(emulator.segment_base(X86Segment::Gs).unwrap(), 0x5678_9000);
}

#[test]
fn emulators_run_concurrently_on_separate_threads() {
    let Some(root) = sample_root() else {
        return;
    };

    const WORKERS: usize = 8;

    let settings = Settings::new().with_emulation_root(&root);
    let emulators: Vec<_> = (0..WORKERS)
        .map(|_| Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap())
        .collect();

    let ready = Barrier::new(WORKERS);
    thread::scope(|scope| {
        let workers: Vec<_> = emulators
            .into_iter()
            .map(|mut emulator| {
                let ready = &ready;
                scope.spawn(move || {
                    ready.wait();
                    for _ in 0..200 {
                        emulator.run_instructions(100).unwrap();
                    }
                    emulator.executed_instructions()
                })
            })
            .collect();

        for worker in workers {
            assert!(worker.join().unwrap() > 0);
        }
    });
}

#[derive(Debug, PartialEq, Eq)]
struct CallbackObservation {
    terminated: bool,
    vcpu_count: u32,
    current_vcpu: u32,
    current_thread_id: Option<u32>,
}

#[test]
fn threads_terminate_from_inside_and_outside_callbacks() {
    let Some(root) = sample_root() else {
        return;
    };

    let settings = Settings::new().with_emulation_root(&root);
    let mut emulator =
        Emulator::application(&settings, &Application::new(SAMPLE_GUEST_PATH)).unwrap();
    emulator.run_instructions(10_000).unwrap();

    let main = emulator.current_thread().unwrap();
    let suspended = emulator
        .create_thread(main.start_address(), 0, true)
        .unwrap();

    let terminated = Arc::new(Mutex::new(Vec::new()));
    let recorded_terminations = terminated.clone();
    emulator.on_thread_terminated(move |_, _, id| recorded_terminations.lock().unwrap().push(id));

    let observation = Arc::new(Mutex::new(None));
    let recorded_observation = observation.clone();
    emulator
        .hook_basic_block(move |context, _| {
            let mut observation = recorded_observation.lock().unwrap();
            if observation.is_none() {
                *observation = Some(CallbackObservation {
                    terminated: context.terminate_thread(suspended, 7).unwrap(),
                    vcpu_count: context.vcpu_count(),
                    current_vcpu: context.current_vcpu(),
                    current_thread_id: context.current_thread_id(),
                });
                context.stop();
            }
        })
        .unwrap();
    emulator.run().unwrap();

    assert_eq!(
        observation.lock().unwrap().take(),
        Some(CallbackObservation {
            terminated: true,
            vcpu_count: 1,
            current_vcpu: 0,
            current_thread_id: Some(main.id()),
        })
    );
    assert_eq!(emulator.thread(suspended).unwrap().exit_status(), Some(7));

    assert!(emulator.terminate_thread(main.id(), 9).unwrap());
    assert_eq!(emulator.thread(main.id()).unwrap().exit_status(), Some(9));
    assert!(!emulator.terminate_thread(u32::MAX, 0).unwrap());
    assert_eq!(*terminated.lock().unwrap(), [suspended, main.id()]);
}

#[test]
fn multiple_vcpus_are_rejected_where_the_backend_runs_one() {
    let Some(root) = emulator_root() else {
        return;
    };

    let settings = Settings::new().with_emulation_root(&root);
    assert_eq!(settings.vcpu_count(), NonZeroU32::MIN);
    let emulator = Emulator::empty(&settings, Architecture::X86_64).unwrap();
    assert_eq!(emulator.vcpu_count(), 1);
    assert_eq!(emulator.current_vcpu(), 0);

    let two = NonZeroU32::new(2).unwrap();
    let settings = settings.with_backend(Backend::Unicorn).with_vcpu_count(two);
    assert!(matches!(
        Emulator::empty(&settings, Architecture::X86_64),
        Err(Error::UnsupportedVcpuCount(count)) if count == two
    ));
}

#[test]
fn mmio_device_sees_guest_reads_and_writes() {
    let Some(root) = sample_root() else {
        return;
    };

    let icicle = env::var_os("GHIDRA_SRC").map(|_| Backend::Icicle);
    if icicle.is_none() {
        eprintln!("skipping Icicle: GHIDRA_SRC is not set");
    }

    for backend in [Some(Backend::Unicorn), icicle].into_iter().flatten() {
        let mut emulator = sample_emulator(&root, backend);
        let device = RecordingDevice::default();
        let mmio = 0x5000_0000;
        assert!(emulator.map_mmio(mmio, 0x1000, device.clone()).unwrap());
        assert!(!emulator.map_mmio(mmio, 0x1000, device.clone()).unwrap());

        run_copy(&mut emulator, mmio + 0x10, mmio + 0x20);

        let expected = (0x10..0x18).map(RecordingDevice::byte).collect::<Vec<_>>();
        let rax = emulator.read_register(X86Register::Rax).unwrap();
        assert_eq!(rax.to_le_bytes(), *expected, "{backend:?}");

        let expected_written = (0x20..).zip(expected).collect::<BTreeMap<_, _>>();
        assert_eq!(
            *device.written.lock().unwrap(),
            expected_written,
            "{backend:?}"
        );
    }
}

#[test]
fn host_memory_is_shared_with_the_guest() {
    let Some(root) = sample_root() else {
        return;
    };

    let mut emulator = sample_emulator(&root, Backend::Unicorn);
    let memory = HostMemory::new(0x1000);
    assert_eq!(memory.size(), 0x4000);
    memory.write(0, b"from host");

    let guest = emulator
        .map_host_memory(&memory, READ_WRITE)
        .unwrap()
        .unwrap();
    let mut data = [0; 9];
    emulator.read_memory(guest, &mut data).unwrap();
    assert_eq!(&data, b"from host");

    run_copy(&mut emulator, guest, guest + 0x3ff8);
    memory.read(0x3ff8, &mut data[..8]);
    assert_eq!(&data[..8], b"from hos");

    drop(memory);
    emulator.write_memory(guest, b"still mapped").unwrap();
    assert!(emulator.release_memory(guest, 0).unwrap());
    assert!(
        emulator
            .memory_region(guest)
            .is_none_or(|region| !region.is_committed())
    );
}

#[test]
fn guest_reads_the_injected_clock() {
    let Some(root) = sample_root() else {
        return;
    };

    let settings = Settings::new()
        .with_emulation_root(&root)
        .with_clock(SteppingClock::default());
    let application = Application::new(SAMPLE_GUEST_PATH).with_argument("-time");
    let mut emulator = Emulator::application(&settings, &application).unwrap();
    let stdout = capture_stdout(&emulator);

    emulator.run().unwrap();

    assert_eq!(emulator.process().exit_status(), Some(0));
    assert_eq!(
        text(&stdout).trim(),
        format!("Time: {GUEST_TIME_NANOSECONDS}")
    );
}

#[test]
fn sample_runs_on_injected_sockets_and_a_provider_mount() {
    let Some(root) = sample_root() else {
        return;
    };

    let network = LoopbackFactory::default();
    let settings = Settings::new()
        .with_emulation_root(&root)
        .with_socket_factory(network.clone())
        .with_dns_resolver(|hostname, _| {
            hostname
                .eq_ignore_ascii_case("google.com")
                .then(|| vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])
        });
    let application = Application::new(SAMPLE_GUEST_PATH).with_working_directory(r"C:\provided");
    let mut emulator = Emulator::application(&settings, &application).unwrap();
    let stdout = capture_stdout(&emulator);

    let provider = MemoryProvider::default();
    let notes = b"provider notes";
    provider
        .0
        .files
        .lock()
        .unwrap()
        .insert("notes.txt".to_owned(), Arc::new(Mutex::new(notes.to_vec())));
    emulator
        .mount_provider(r"C:\provided", FilesystemMode::ReadWrite, provider.clone())
        .unwrap();

    emulator.run().unwrap();
    let output = text(&stdout);
    eprintln!("{output}");

    assert_eq!(emulator.process().exit_status(), Some(0));
    for test in ["I/O", "File Locking", "Working Directory", "Socket"] {
        assert!(
            output.contains(&format!("Running test '{test}': Success")),
            "{test}"
        );
    }

    assert!(network.0.sockets.load(Ordering::Relaxed) >= 2);
    assert_eq!(
        *network.0.delivered.lock().unwrap(),
        [b"Hello World".to_vec()]
    );
    assert_eq!(
        provider.0.opened.lock().unwrap().first(),
        Some(&("a.txt".to_owned(), OpenMode::WriteRead))
    );
    assert!(provider.0.files.lock().unwrap().keys().eq(["notes.txt"]));

    assert!(emulator.file_info(r"C:\provided").unwrap().is_directory());
    assert_eq!(
        emulator.file_info(r"C:\Provided\NOTES.TXT").unwrap().size(),
        notes.len() as u64
    );
    let entries = emulator.read_directory(r"C:\provided").unwrap();
    let listed = entries
        .iter()
        .find(|entry| entry.name() == "notes.txt")
        .unwrap();
    assert!(!listed.is_directory());
    assert_eq!(listed.info().map(FileInfo::size), Some(notes.len() as u64));
}
