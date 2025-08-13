const PAGE_SIZE: usize = 8192;

#[repr(u8)] // 1 byte
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageType {
    Data = 0,
    Index = 1,
    Overflow = 2,
    Free = 3,
}
#[repr(C)] // Ensure consistent memory layout
#[derive(Debug, Clone, Copy)]
pub struct PageHeader {
    pub page_id: u32,          // 4 bytes - page number (supports ~32TB with 8KB pages)
    pub page_type: PageType,   // 1 byte
    pub free_space_start: u16, // 2 bytes - offset where free space begins
    pub free_space_end: u16,   // 2 bytes - offset where free space ends
    pub slot_count: u16,       // 2 bytes - number of slots in directory
    pub lsn: u64,              // 8 bytes - log sequence number for recovery
    pub checksum: u32,         // 4 bytes - for corruption detection
    _padding: [u8; 3],         // 3 bytes - align to 26 bytes total
}

// For slotted pages, we need slot entries
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SlotEntry {
    pub offset: u16, // 2 bytes - offset from start of page
    pub length: u16, // 2 bytes - length of record
}

#[repr(C, align(8))]
pub struct Page {
    data: [u8; PAGE_SIZE], // The actual 8KB block
}

impl Page {
    const HEADER_SIZE: usize = std::mem::size_of::<PageHeader>();
    const SLOT_SIZE: usize = std::mem::size_of::<SlotEntry>();

    pub fn new(page_id: u32, page_type: PageType) -> Self {
        let mut page = Self {
            data: [0; PAGE_SIZE],
        };

        let header = PageHeader {
            page_id,
            page_type,
            free_space_start: Self::HEADER_SIZE as u16, // Right after header
            free_space_end: PAGE_SIZE as u16,           // At end of page
            slot_count: 0,
            lsn: 0,
            checksum: 0,
            _padding: [0; 3],
        };

        page.set_header(header);
        page
    }

    pub fn header(&self) -> &PageHeader {
        unsafe {
            // SAFETY:
            // - `self.data` is a [u8; PAGE_SIZE] array with PAGE_SIZE = 8192
            // - PageHeader is #[repr(C)] ensuring predictable layout
            // - Size of PageHeader (26 bytes) is less than PAGE_SIZE
            // - self.data.as_ptr() is aligned to at least 1 byte, PageHeader requires
            //   4-byte alignment (due to u32 fields), which is satisfied because
            //   self.data array starts at struct beginning which has natural alignment
            // - The lifetime of the returned reference is bound to &self, preventing
            //   use-after-free
            // - PageHeader has no invalid bit patterns (all fields are integers)
            &*(self.data.as_ptr() as *const PageHeader)
        }
    }

    pub fn header_mut(&mut self) -> &mut PageHeader {
        unsafe {
            // SAFETY:
            // - `self.data` is a [u8; PAGE_SIZE] array with PAGE_SIZE = 8192
            // - PageHeader is #[repr(C)] ensuring predictable layout
            // - Size of PageHeader (26 bytes) is less than PAGE_SIZE
            // - self.data.as_mut_ptr() is aligned to at least 1 byte, PageHeader
            //   requires 4-byte alignment (due to u32 fields), which is satisfied
            //   because self.data array starts at struct beginning
            // - The lifetime of the returned reference is bound to &mut self,
            //   ensuring exclusive access and preventing aliasing
            // - PageHeader has no invalid bit patterns (all fields are integers)
            // - Mutable access is safe as we have exclusive access to self
            &mut *(self.data.as_mut_ptr() as *mut PageHeader)
        }
    }

    fn set_header(&mut self, header: PageHeader) {
        unsafe {
            // SAFETY:
            // - `self.data` is a [u8; PAGE_SIZE] array with PAGE_SIZE = 8192
            // - PageHeader is #[repr(C)] with size 26 bytes, which fits in PAGE_SIZE
            // - self.data.as_mut_ptr() is aligned for PageHeader (see header_mut)
            // - We have exclusive access to self.data through &mut self
            // - PageHeader is Copy, so no drop concerns
            // - Write is atomic with respect to the PageHeader struct
            // - All bit patterns in PageHeader are valid (POD type)
            *(self.data.as_mut_ptr() as *mut PageHeader) = header;
        }
    }

    pub fn get_slot(&self, index: usize) -> Option<SlotEntry> {
        if index >= self.header().slot_count as usize {
            return None;
        }

        let slot_offset = Self::HEADER_SIZE + (index * Self::SLOT_SIZE);
        unsafe {
            // SAFETY:
            // - Pointer arithmetic `self.data.as_ptr().add(slot_offset)` is valid because:
            //   * slot_offset = HEADER_SIZE + (index * SLOT_SIZE)
            //   * index < slot_count (checked above)
            //   * Maximum offset = HEADER_SIZE + (slot_count - 1) * SLOT_SIZE
            //   * This must be < PAGE_SIZE - SLOT_SIZE to fit the last slot
            //   * We trust slot_count was set correctly when slots were added
            // - The resulting pointer is within the bounds of self.data[0..PAGE_SIZE]
            // - Casting to *const SlotEntry is valid because:
            //   * SlotEntry is #[repr(C)] with a defined memory layout
            //   * SlotEntry contains only u16 fields (no padding between them)
            //   * Size of SlotEntry is 4 bytes (two u16 fields)
            // - Alignment requirements are met:
            //   * SlotEntry requires 2-byte alignment (due to u16 fields)
            //   * HEADER_SIZE (26) % 2 == 0, maintaining 2-byte alignment
            //   * SLOT_SIZE (4) % 2 == 0, so all slots remain 2-byte aligned
            // - The read is valid because:
            //   * We're reading exactly 4 bytes (size of SlotEntry)
            //   * All bit patterns are valid for SlotEntry (two u16 values)
            //   * No uninitialized memory is being read (slots are written before slot_count increases)
            // - The dereference creates a copy (SlotEntry is Copy), not a reference,
            //   so there are no lifetime concerns
            Some(*(self.data.as_ptr().add(slot_offset) as *const SlotEntry))
        }
    }

    pub fn free_space(&self) -> usize {
        let header = self.header();
        let slot_array_end = Self::HEADER_SIZE + (header.slot_count as usize * Self::SLOT_SIZE);

        // Free space is between end of slot array and start of records
        (header.free_space_end as usize) - slot_array_end
    }

    pub fn has_space_for(&self, record_size: usize) -> bool {
        // Need space for the record + a new slot entry
        self.free_space() >= record_size + Self::SLOT_SIZE
    }

    pub fn get_record(&self, slot_index: usize) -> Option<&[u8]> {
        let slot = self.get_slot(slot_index)?;

        if slot.length == 0 {
            return None; // Deleted record
        }

        let start = slot.offset as usize;
        let end = start + slot.length as usize;

        if end <= PAGE_SIZE {
            Some(&self.data[start..end])
        } else {
            None
        }
    }

    /// Add a record to the page, returning the slot index if successful
    pub fn add_record(&mut self, record: &[u8]) -> Option<usize> {
        if !self.has_space_for(record.len()) {
            return None;
        }

        let record_len = record.len();
        let slot_index = self.header().slot_count as usize;

        let new_record_end = self.header().free_space_end as usize;
        let new_record_start = new_record_end - record_len;

        self.data[new_record_start..new_record_end].copy_from_slice(record);

        let slot = SlotEntry {
            offset: new_record_start as u16,
            length: record_len as u16,
        };
        self.set_slot(slot_index, slot);

        let header = self.header_mut();
        header.free_space_end = new_record_start as u16;
        header.slot_count += 1;

        Some(slot_index)
    }

    fn set_slot(&mut self, index: usize, slot: SlotEntry) {
        let slot_offset = Self::HEADER_SIZE + (index * Self::SLOT_SIZE);

        unsafe {
            // SAFETY:
            // - We know slot_offset is valid because we control when this is called
            // - slot_offset + SLOT_SIZE <= PAGE_SIZE (enforced by has_space_for check)
            // - Alignment is maintained (same as get_slot)
            // - We have exclusive access through &mut self
            *(self.data.as_mut_ptr().add(slot_offset) as *mut SlotEntry) = slot;
        }
    }

    pub fn delete_record(&mut self, slot_index: usize) -> bool {
        if let Some(mut slot) = self.get_slot(slot_index) {
            slot.length = 0;
            self.set_slot(slot_index, slot);
            // Note: We don't reclaim space yet - that would require compaction
            true
        } else {
            false
        }
    }

    pub fn compact(&mut self) {
        let mut new_data = [0u8; PAGE_SIZE];
        let header = *self.header();

        new_data[..Self::HEADER_SIZE].copy_from_slice(&self.data[..Self::HEADER_SIZE]);

        let slots_end = Self::HEADER_SIZE + (header.slot_count as usize * Self::SLOT_SIZE);
        new_data[Self::HEADER_SIZE..slots_end]
            .copy_from_slice(&self.data[Self::HEADER_SIZE..slots_end]);

        let mut current_end = PAGE_SIZE;

        for i in 0..header.slot_count as usize {
            if let Some(slot) = self.get_slot(i) {
                if slot.length > 0 {
                    let record = self.get_record(i).unwrap();
                    let new_start = current_end - slot.length as usize;

                    new_data[new_start..current_end].copy_from_slice(record);

                    let new_slot = SlotEntry {
                        offset: new_start as u16,
                        length: slot.length,
                    };

                    unsafe {
                        let slot_offset = Self::HEADER_SIZE + (i * Self::SLOT_SIZE);
                        *(new_data.as_mut_ptr().add(slot_offset) as *mut SlotEntry) = new_slot;
                    }

                    current_end = new_start;
                }
            }
        }

        self.data = new_data;
        self.header_mut().free_space_end = current_end as u16;
    }

    pub fn used_space(&self) -> usize {
        let header = self.header();
        let slots_size = header.slot_count as usize * Self::SLOT_SIZE;
        let records_size = (PAGE_SIZE - header.free_space_end as usize);

        Self::HEADER_SIZE + slots_size + records_size
    }

    pub fn fill_percentage(&self) -> f32 {
        (self.used_space() as f32 / PAGE_SIZE as f32) * 100.0
    }

    pub fn debug_layout(&self) {
        let header = self.header();
        println!("Page {} Layout:", header.page_id);
        println!("  Type: {:?}", header.page_type);
        println!("  Slots: {}", header.slot_count);
        println!("  Free space: {} bytes", self.free_space());
        println!(
            "  Free range: {}..{}",
            Self::HEADER_SIZE + (header.slot_count as usize * Self::SLOT_SIZE),
            header.free_space_end
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_creation() {
        let page = Page::new(42, PageType::Data);
        let header = page.header();

        assert_eq!(header.page_id, 42);
        assert_eq!(header.page_type, PageType::Data);
        assert_eq!(header.slot_count, 0);
        assert_eq!(header.free_space_start, Page::HEADER_SIZE as u16);
        assert_eq!(header.free_space_end, PAGE_SIZE as u16);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(Page::HEADER_SIZE, 26);
        assert_eq!(std::mem::size_of::<PageHeader>(), 26);
    }

    #[test]
    fn test_slot_size() {
        assert_eq!(Page::SLOT_SIZE, 4);
        assert_eq!(std::mem::size_of::<SlotEntry>(), 4);
    }

    #[test]
    fn test_free_space_calculation() {
        let page = Page::new(1, PageType::Data);

        // Initially: header (26 bytes) + 0 slots, rest is free
        let expected_free = PAGE_SIZE - Page::HEADER_SIZE;
        assert_eq!(page.free_space(), expected_free);
    }

    #[test]
    fn test_get_invalid_slot() {
        let page = Page::new(1, PageType::Data);

        // No slots exist yet
        assert!(page.get_slot(0).is_none());
        assert!(page.get_slot(10).is_none());
    }

    #[test]
    fn test_has_space_for() {
        let page = Page::new(1, PageType::Data);

        // Should have space for small records
        assert!(page.has_space_for(100));
        assert!(page.has_space_for(1000));

        // Shouldn't have space for huge records
        assert!(!page.has_space_for(PAGE_SIZE));
    }

    #[test]
    fn test_page_alignment() {
        // Ensure Page struct has correct alignment
        assert_eq!(std::mem::align_of::<Page>(), 8);
    }

    #[test]
    fn test_add_record() {
        let mut page = Page::new(1, PageType::Data);

        // Add first record
        let record1 = b"Hello, World!";
        let slot1 = page.add_record(record1).unwrap();

        assert_eq!(slot1, 0);
        assert_eq!(page.header().slot_count, 1);
        assert_eq!(page.get_record(slot1).unwrap(), record1);

        // Add second record
        let record2 = b"Second record with more data";
        let slot2 = page.add_record(record2).unwrap();

        assert_eq!(slot2, 1);
        assert_eq!(page.header().slot_count, 2);
        assert_eq!(page.get_record(slot2).unwrap(), record2);

        // Verify both records are still accessible
        assert_eq!(page.get_record(slot1).unwrap(), record1);
        assert_eq!(page.get_record(slot2).unwrap(), record2);
    }

    #[test]
    fn test_delete_record() {
        let mut page = Page::new(1, PageType::Data);

        let record = b"Delete me";
        let slot = page.add_record(record).unwrap();

        // Delete the record
        assert!(page.delete_record(slot));

        // Record should no longer be accessible
        assert!(page.get_record(slot).is_none());

        // Slot still exists but has length 0
        assert_eq!(page.get_slot(slot).unwrap().length, 0);
    }

    #[test]
    fn test_page_compaction() {
        let mut page = Page::new(1, PageType::Data);

        // Add three records
        let record1 = b"First";
        let record2 = b"Second";
        let record3 = b"Third";

        let slot1 = page.add_record(record1).unwrap();
        let slot2 = page.add_record(record2).unwrap();
        let slot3 = page.add_record(record3).unwrap();

        let space_before = page.free_space();

        // Delete middle record
        page.delete_record(slot2);

        // Compact the page
        page.compact();

        // Check that we reclaimed space
        assert!(page.free_space() > space_before);

        // Verify remaining records are intact
        assert_eq!(page.get_record(slot1).unwrap(), record1);
        assert!(page.get_record(slot2).is_none()); // Still deleted
        assert_eq!(page.get_record(slot3).unwrap(), record3);
    }

    #[test]
    fn test_page_fills_up() {
        let mut page = Page::new(1, PageType::Data);
        let record = vec![b'X'; 100]; // 100-byte record

        let mut count = 0;
        while page.add_record(&record).is_some() {
            count += 1;
        }

        // Should fit approximately (8192 - 26) / (100 + 4) ≈ 78 records
        assert!(count > 70);
        assert!(count < 85);

        // Page should be nearly full
        assert!(page.fill_percentage() > 95.0);
    }
}
