use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB},
};

/// Initialize a new OffsetPageTable.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    unsafe {
        let level_4_table = active_level_4_table(physical_memory_offset);
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

/// Returns a mutable reference to the active level 4 table.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once to
/// avoid aliasing `&mut` references (which is undefined behavior).
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

// NOTE: this is generic because we can't name the iterator type we produce
// in [`make_allocator`].
pub struct BootInfoFrameAllocator<I> {
    iter: I,
}

/// Create a FrameAllocator from the passed memory map.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the passed
/// memory map is valid. The main requirement is that all frames that are marked
/// as `USABLE` in it are really unused.
// NOTE: this is not in an impl block because we don't actually need or want
// the I generic parameter, we have a concrete type here, even though it cannot
// be named.
pub unsafe fn make_allocator(
    memory_regions: &'static MemoryRegions,
) -> BootInfoFrameAllocator<impl Iterator<Item = PhysFrame>> {
    let regions = memory_regions.iter();
    let usable_regions = regions.filter(|r| r.kind == MemoryRegionKind::Usable);
    let addr_ranges = usable_regions.map(|r| r.start..r.end);
    let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));
    let iter = frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)));
    BootInfoFrameAllocator { iter }
}

unsafe impl<I> FrameAllocator<Size4KiB> for BootInfoFrameAllocator<I>
where
    I: Iterator<Item = PhysFrame>,
{
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.iter.next()
    }
}
