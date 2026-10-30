//! Runs a packed 64-bit Windows executable until its unpacking stub hands over to the original program, then dumps the
//! unpacked image as a PE file.
//!
//! ```text
//! cargo run --example unpack -- <emulation root> <guest path> <dump path>
//! ```
//!
//! Two kinds of hand-over are recognised:
//!
//! - In place, as with UPX: the stub writes the original code into the executable's image and jumps to it. Stub code is
//!   executed image code the program did not write. A stub that calls into unpacked code, as UPX does for the original
//!   TLS callbacks, leaves its return address on the stack; the hand-over is a jump with the stub's frame gone.
//! - Mapped, as with reflective loaders such as Amber: the stub maps the original executable into private memory and
//!   enters it. The hand-over is execution reaching the entry point named by the PE headers at the start of a private
//!   allocation.
//!
//! The dump keeps the import directory found in memory; the import address table holds resolved addresses.

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::{env, fs, mem, process};

use object::pe::{
    IMAGE_DOS_SIGNATURE, IMAGE_NT_OPTIONAL_HDR64_MAGIC, IMAGE_NT_SIGNATURE, ImageDosHeader,
    ImageFileHeader, ImageNtHeaders64, ImageSectionHeader,
};
use object::{LittleEndian as LE, pod};
use sogen::windows::{Application, Context, Emulator, Settings};
use sogen::{Guest, MemoryRegionKind, X86Register};

const PAGE_SIZE: u64 = 0x1000;
const INSTRUCTION_BUDGET: u64 = 1_000_000_000;
const INSTRUCTION_SLICE: u64 = 10_000_000;
const HEADERS_SIZE: usize = 0x400;

struct Unpacked {
    entry_point: u64,
    image_base: u64,
    size: u64,
}

struct Allocation {
    end: u64,
    private: bool,
    entry_point: Option<u64>,
    first_execution: Option<u64>,
}

struct Unpacking {
    image_base: u64,
    written: Vec<bool>,
    previous_block_in_stub: bool,
    calls_from_stub: Vec<u64>,
    allocations: BTreeMap<u64, Allocation>,
    unpacked: Option<Unpacked>,
}

impl Unpacking {
    fn new(image_base: u64, size_of_image: usize) -> Self {
        Self {
            image_base,
            written: vec![false; size_of_image],
            previous_block_in_stub: false,
            calls_from_stub: Vec::new(),
            allocations: BTreeMap::new(),
            unpacked: None,
        }
    }

    fn offset(&self, address: u64) -> Option<usize> {
        let offset = usize::try_from(address.checked_sub(self.image_base)?).ok()?;
        (offset < self.written.len()).then_some(offset)
    }

    fn record_write(&mut self, address: u64, length: usize) {
        let Some(start) = self.offset(address) else {
            return;
        };
        let end = (start + length).min(self.written.len());
        self.written[start..end].fill(true);
    }

    fn is_stub(&self, address: u64) -> bool {
        self.offset(address)
            .is_some_and(|offset| !self.written[offset])
    }

    fn execute_block(&mut self, context: &Context, address: u64) {
        if self.unpacked.is_some() {
            return;
        }

        match self.offset(address) {
            Some(offset) => self.execute_image_block(context, address, self.written[offset]),
            None => {
                self.previous_block_in_stub = false;
                self.execute_allocation_block(context, address);
            }
        }
    }

    fn execute_image_block(&mut self, context: &Context, address: u64, unpacked: bool) {
        let from_stub = mem::replace(&mut self.previous_block_in_stub, !unpacked);
        if !unpacked || !from_stub {
            return;
        }

        let return_address =
            context
                .read_register(X86Register::Rsp)
                .ok()
                .and_then(|stack_pointer| {
                    let mut value = [0; 8];
                    context
                        .try_read_memory(stack_pointer, &mut value)
                        .then(|| u64::from_le_bytes(value))
                });
        if return_address.is_some_and(|returning_to| self.is_stub(returning_to)) {
            self.calls_from_stub.push(address);
            return;
        }

        self.unpacked = Some(Unpacked {
            entry_point: address,
            image_base: self.image_base,
            size: self.written.len() as u64,
        });
        context.stop();
    }

    fn execute_allocation_block(&mut self, context: &Context, address: u64) {
        let cached = self
            .allocations
            .range(..=address)
            .next_back()
            .filter(|(_, allocation)| address < allocation.end)
            .map(|(start, _)| *start);
        let start = match cached {
            Some(start) => start,
            None => {
                let Some(region) = context.memory_region(address) else {
                    return;
                };
                let start = region.allocation_base();
                let private = region.kind() == MemoryRegionKind::PrivateAllocation;
                self.allocations.insert(
                    start,
                    Allocation {
                        end: start + region.allocation_length(),
                        private,
                        entry_point: private
                            .then(|| mapped_entry_point(context, start))
                            .flatten(),
                        first_execution: None,
                    },
                );
                start
            }
        };

        let allocation = self
            .allocations
            .get_mut(&start)
            .expect("the allocation was just found or inserted");
        if !allocation.private {
            return;
        }

        allocation.first_execution.get_or_insert(address);
        if allocation.entry_point == Some(address) {
            self.unpacked = Some(Unpacked {
                entry_point: address,
                image_base: start,
                size: allocation.end - start,
            });
            context.stop();
        }
    }
}

/// The entry point of a 64-bit PE image whose headers start at `base`.
fn mapped_entry_point(context: &Context, base: u64) -> Option<u64> {
    let mut headers = [0; HEADERS_SIZE];
    if !context.try_read_memory(base, &mut headers) {
        return None;
    }

    let (dos_header, _) = pod::from_bytes::<ImageDosHeader>(&headers).ok()?;
    if dos_header.e_magic.get(LE) != IMAGE_DOS_SIGNATURE {
        return None;
    }

    let nt_headers_offset = usize::try_from(dos_header.nt_headers_offset()).ok()?;
    let (nt_headers, _) =
        pod::from_bytes::<ImageNtHeaders64>(headers.get(nt_headers_offset..)?).ok()?;
    let valid = nt_headers.signature.get(LE) == IMAGE_NT_SIGNATURE
        && nt_headers.optional_header.magic.get(LE) == IMAGE_NT_OPTIONAL_HDR64_MAGIC;
    let entry_point = nt_headers.optional_header.address_of_entry_point.get(LE);
    (valid && entry_point != 0).then(|| base + u64::from(entry_point))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let (Some(root), Some(guest_path), Some(dump_path)) =
        (arguments.next(), arguments.next(), arguments.next())
    else {
        eprintln!("usage: unpack <emulation root> <guest path> <dump path>");
        process::exit(2);
    };

    let settings = Settings::new()
        .with_emulation_root(root)
        .with_logging(false);
    let mut emulator = Emulator::application(&settings, &Application::new(guest_path.as_str()))?;
    emulator.setup_process_if_necessary()?;

    let file_name = guest_path.rsplit(['\\', '/']).next().unwrap_or(&guest_path);
    let image = emulator
        .modules()
        .into_iter()
        .find(|module| module.name().eq_ignore_ascii_case(file_name))
        .ok_or("the executable is not mapped")?;
    let image_base = image.image_base();
    let unpacking = Arc::new(Mutex::new(Unpacking::new(
        image_base,
        usize::try_from(image.size_of_image())?,
    )));

    let state = unpacking.clone();
    emulator.hook_memory_write(
        image_base,
        image.size_of_image(),
        move |_, address, data| {
            state
                .lock()
                .expect("callbacks abort instead of poisoning")
                .record_write(address, data.len());
        },
    )?;

    let state = unpacking.clone();
    emulator.hook_basic_block(move |context, block| {
        state
            .lock()
            .expect("callbacks abort instead of poisoning")
            .execute_block(context, block.address());
    })?;

    let mut executed = 0;
    while executed < INSTRUCTION_BUDGET
        && emulator.process().exit_status().is_none()
        && unpacking
            .lock()
            .expect("no callback is running")
            .unpacked
            .is_none()
    {
        emulator.run_instructions(INSTRUCTION_SLICE)?;
        executed += INSTRUCTION_SLICE;
    }

    let unpacking = unpacking.lock().expect("no callback is running");
    for address in &unpacking.calls_from_stub {
        println!(
            "the stub called unpacked code at RVA {:#x}",
            address - image_base
        );
    }
    for (start, allocation) in &unpacking.allocations {
        if let Some(first_execution) = allocation.first_execution {
            println!(
                "code ran in private memory at {first_execution:#x} (allocation {start:#x}, {:#x} bytes)",
                allocation.end - start
            );
        }
    }
    let Some(unpacked) = &unpacking.unpacked else {
        println!("the stub never handed over to an unpacked image");
        return Ok(());
    };

    let mut dump = vec![0; usize::try_from(unpacked.size)?];
    let mut unreadable_pages = 0;
    for (index, page) in dump.chunks_mut(PAGE_SIZE as usize).enumerate() {
        if !emulator.try_read_memory(unpacked.image_base + index as u64 * PAGE_SIZE, page) {
            unreadable_pages += 1;
        }
    }

    let entry_point = u32::try_from(unpacked.entry_point - unpacked.image_base)?;
    rebase_headers(&mut dump, unpacked.image_base, entry_point)?;
    fs::write(&dump_path, &dump)?;

    let location = if unpacked.image_base == image_base {
        "in place in the executable".to_owned()
    } else {
        format!("in an image mapped at {:#x}", unpacked.image_base)
    };
    println!(
        "original entry point at RVA {entry_point:#x} {location}, {unreadable_pages} unreadable pages"
    );
    println!("dumped {} bytes to {dump_path}", dump.len());
    Ok(())
}

/// Makes the memory image a valid PE file: every section's file data is its memory, and execution starts at the
/// original entry point.
fn rebase_headers(
    dump: &mut [u8],
    image_base: u64,
    entry_point: u32,
) -> Result<(), Box<dyn Error>> {
    let invalid = |()| "the dumped image has no valid PE headers";

    let (dos_header, _) = pod::from_bytes::<ImageDosHeader>(dump).map_err(invalid)?;
    let nt_headers_offset = usize::try_from(dos_header.nt_headers_offset())?;
    let (nt_headers, _) = pod::from_bytes_mut::<ImageNtHeaders64>(
        dump.get_mut(nt_headers_offset..)
            .ok_or("the NT headers lie outside the image")?,
    )
    .map_err(invalid)?;

    let optional_header = &mut nt_headers.optional_header;
    let section_alignment = optional_header.section_alignment.get(LE);
    optional_header.address_of_entry_point.set(LE, entry_point);
    optional_header.image_base.set(LE, image_base);
    optional_header.file_alignment.set(LE, section_alignment);

    let section_count = usize::from(nt_headers.file_header.number_of_sections.get(LE));
    let section_table_offset = nt_headers_offset
        + size_of::<u32>()
        + size_of::<ImageFileHeader>()
        + usize::from(nt_headers.file_header.size_of_optional_header.get(LE));
    let image_size = u32::try_from(dump.len())?;
    let (sections, _) = pod::slice_from_bytes_mut::<ImageSectionHeader>(
        dump.get_mut(section_table_offset..)
            .ok_or("the section table lies outside the image")?,
        section_count,
    )
    .map_err(invalid)?;

    for section in sections {
        let address = section.virtual_address.get(LE);
        let size = section
            .virtual_size
            .get(LE)
            .next_multiple_of(section_alignment)
            .min(image_size.saturating_sub(address));
        section.pointer_to_raw_data.set(LE, address);
        section.size_of_raw_data.set(LE, size);
    }

    Ok(())
}
