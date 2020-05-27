use x86_64::{VirtAddr, PhysAddr};
use x86_64::structures::paging::{PageTable, OffsetPageTable, Page, FrameAllocator, Size4KiB, PhysFrame, Mapper};
use bootloader::bootinfo::{MemoryMap, MemoryRegionType};

pub struct EmptyFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for EmptyFrameAllocator {
	fn allocate_frame(&mut self) -> Option<PhysFrame> {
		None
	}
}

/// A FrameAllocator that returns usable frames from the bootloader's memory map.
pub struct BootInfoFrameAllocator {
	memory_map: &'static MemoryMap,
	next: usize,
}

impl BootInfoFrameAllocator {
	/// Create a FrameAllocator from the passed memory map.
	///
	/// This function is unsafe because the caller must guarantee that the passed
	/// memory map is valid. The main requirement is that all frames that are marked
	/// as `USABLE` in it are really unused.
	pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
		BootInfoFrameAllocator {
			memory_map,
			next: 0,
		}
	}
	
	/// Returns an iterator over the usable frames specified in the memory map.
	fn usable_frames(&self) -> impl Iterator<Item=PhysFrame> {
		let addr_ranges =
			self.memory_map.iter()
				.filter(|r| r.region_type == MemoryRegionType::Usable)
				.map(|r| r.range.start_addr()..r.range.end_addr());
		
		// End addr is guaranteed to be a multiple of 4096 away from start addr
		let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));
		// create `PhysFrame` types from the start addresses
		frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
	}
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
	fn allocate_frame(&mut self) -> Option<PhysFrame> {
		// TODO: OPTIMIZE
		let frame = self.usable_frames().nth(self.next);
		self.next += 1;
		frame
	}
}

/// Initialize a new OffsetPageTable.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
	let level_4_table = active_level_4_table(physical_memory_offset);
	OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Returns a mutable reference to the active level 4 table.
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
	use x86_64::registers::control::Cr3;
	
	let (level_4_table_frame, _) = Cr3::read();
	
	let frame_phys_addr = level_4_table_frame.start_address();
	let frame_virt_addr = physical_memory_offset + frame_phys_addr.as_u64();
	let page_table_ptr: *mut PageTable = frame_virt_addr.as_mut_ptr();
	
	&mut *page_table_ptr
}

/// Translates the given virtual address to the mapped physical address, or
/// `None` if the address is not mapped.
pub unsafe fn _translate_addr(addr: VirtAddr, physical_memory_offset: VirtAddr) -> Option<PhysAddr> {
	use x86_64::structures::paging::page_table::FrameError;
	use x86_64::registers::control::Cr3;

	let (level_4_table_frame, _) = Cr3::read();

	let table_indexes = [
		addr.p4_index(), addr.p3_index(), addr.p2_index(), addr.p1_index()
	];
	let mut frame = level_4_table_frame;

	// traverse the multi-level page table
	for &index in &table_indexes {
		// convert the frame into a page table reference
		let virt = physical_memory_offset + frame.start_address().as_u64();
		let table_ptr: *const PageTable = virt.as_ptr();
		let table = &*table_ptr;

		// read the page table entry and update `frame`
		let entry = &table[index];
		frame = match entry.frame() {
			Ok(frame) => frame,
			Err(FrameError::FrameNotPresent) => return None,
			Err(FrameError::HugeFrame) => panic!("huge pages not supported"),
		};
	}

	// calculate the physical address by adding the page offset
	Some(frame.start_address() + u64::from(addr.page_offset()))
}