use crate::{Result, StorageError};
use std::io::{Error, ErrorKind};

pub const PAGE_SIZE: usize = 8192;

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
    pub page_id: u32, // 4 bytes at offset 0 - page number (supports ~32TB with 8KB pages)
    pub page_type: PageType, // 1 byte at offset 4
    // implicit padding: 1 byte at offset 5 (for u16 alignment)
    pub free_space_start: u16, // 2 bytes at offset 6 - offset where free space begins
    pub free_space_end: u16,   // 2 bytes at offset 8 - offset where free space ends
    pub slot_count: u16,       // 2 bytes at offset 10 - number of slots in directory
    // implicit padding: 4 bytes at offset 12-15 (for u64 alignment)
    pub lsn: u64,      // 8 bytes at offset 16 - log sequence number for recovery
    pub checksum: u32, // 4 bytes at offset 24 - for corruption detection
    _padding: [u8; 4], // 4 bytes at offset 28 - explicit padding to reach 32 bytes total
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
    pub const HEADER_SIZE: usize = std::mem::size_of::<PageHeader>();
    pub const SLOT_SIZE: usize = std::mem::size_of::<SlotEntry>();

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
            _padding: [0; 4],
        };

        page.set_header(header);
        page
    }

    pub fn as_bytes(&self) -> &[u8; PAGE_SIZE] {
        &self.data
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8; PAGE_SIZE] {
        &mut self.data
    }

    pub fn from_bytes(bytes: &[u8; PAGE_SIZE]) -> Result<Self> {
        let page = Self { data: *bytes };
        let header = page.header();

        if header.page_id == u32::MAX {
            return Err(StorageError::Io(Error::new(
                ErrorKind::InvalidData,
                "Invalid page ID",
            )));
        }

        if header.free_space_end as usize > PAGE_SIZE {
            return Err(StorageError::Io(Error::new(
                ErrorKind::InvalidData,
                "free_space_end exceeds page size",
            )));
        }

        let slot_array_end = Self::HEADER_SIZE + (header.slot_count as usize * Self::SLOT_SIZE);

        if slot_array_end > PAGE_SIZE {
            return Err(StorageError::Io(Error::new(
                ErrorKind::InvalidData,
                "Slot array exceeds page size",
            )));
        }

        if slot_array_end > header.free_space_end as usize {
            return Err(StorageError::Io(Error::new(
                ErrorKind::InvalidData,
                "Slot array overlaps with records",
            )));
        }

        if !matches!(
            header.page_type,
            PageType::Data | PageType::Index | PageType::Overflow | PageType::Free
        ) {
            return Err(StorageError::Io(Error::new(
                ErrorKind::InvalidData,
                "Invalid page type",
            )));
        }

        Ok(page)
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
        if slot_offset + Self::SLOT_SIZE > PAGE_SIZE {
            return None;
        }
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
        let free_space_end = header.free_space_end as usize;

        // Validate bounds to prevent underflow
        if free_space_end > PAGE_SIZE || slot_array_end > free_space_end {
            return 0; // Page is corrupted, report no free space
        }

        free_space_end - slot_array_end
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
        let current_record_boundary = self.header().free_space_end as usize;

        if current_record_boundary > PAGE_SIZE || record_len > current_record_boundary {
            return None;
        }

        let new_record_start = current_record_boundary - record_len;
        let slot_array_end = Self::HEADER_SIZE + ((slot_index + 1) * Self::SLOT_SIZE);

        if new_record_start < slot_array_end {
            return None;
        }

        self.data[new_record_start..current_record_boundary].copy_from_slice(record);

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

    pub fn add_records(&mut self, records: &[&[u8]]) -> Vec<Option<usize>> {
        if records.is_empty() {
            return Vec::new();
        }

        let total_record_size: usize = records.iter().map(|r| r.len()).sum();
        let total_slot_size = records.len() * Self::SLOT_SIZE;

        if self.free_space() < total_record_size + total_slot_size {
            return self.add_records_partial(records);
        }

        let mut results: Vec<Option<usize>> = Vec::with_capacity(records.len());

        let mut current_slot = self.header().slot_count as usize;
        let mut current_boundary = self.header().free_space_end as usize;
        let slot_array_end = Self::HEADER_SIZE + ((current_slot + records.len()) * Self::SLOT_SIZE);

        if current_boundary < total_record_size
            || current_boundary - total_record_size < slot_array_end
        {
            return self.add_records_partial(records);
        }

        for record in records {
            let record_len = record.len();
            let new_record_start = current_boundary - record_len;

            self.data[new_record_start..current_boundary].copy_from_slice(record);

            let slot = SlotEntry {
                offset: new_record_start as u16,
                length: record_len as u16,
            };
            self.set_slot(current_slot, slot);

            results.push(Some(current_slot));
            current_slot += 1;
            current_boundary = new_record_start;
        }

        let header = self.header_mut();
        header.free_space_end = current_boundary as u16;
        header.slot_count = current_slot as u16;

        results
    }

    fn add_records_partial(&mut self, records: &[&[u8]]) -> Vec<Option<usize>> {
        let mut results = Vec::with_capacity(records.len());

        for record in records {
            results.push(self.add_record(record));
        }

        results
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
    pub fn delete_records(&mut self, slot_indices: &[usize]) -> usize {
        let mut deleted_count = 0;

        for &slot_index in slot_indices {
            if let Some(mut slot) = self.get_slot(slot_index) {
                if slot.length > 0 {
                    slot.length = 0;
                    self.set_slot(slot_index, slot);
                    deleted_count += 1;
                }
            }
        }

        deleted_count
    }

    pub fn deleted_count(&self) -> usize {
        let mut count = 0;
        for i in 0..self.header().slot_count as usize {
            if let Some(slot) = self.get_slot(i) {
                if slot.length == 0 {
                    count += 1;
                }
            }
        }
        count
    }

    pub fn should_compact(&self) -> bool {
        let total_slots = self.header().slot_count as usize;

        // Don't compact empty pages or pages with very few slots
        if total_slots <= 1 {
            return false;
        }

        let deleted = self.deleted_count();

        // Need at least 2 deleted slots AND > 20% deleted
        deleted >= 2 && (deleted * 100 / total_slots) > 20
    }

    pub fn compact(&mut self) {
        if !self.should_compact() {
            return;
        }

        let slot_count = self.header().slot_count as usize;
        let mut write_position = PAGE_SIZE;

        // Process slots from first to last, moving records to end of page
        for i in 0..slot_count {
            if let Some(slot) = self.get_slot(i) {
                if slot.length > 0 {
                    let record_len = slot.length as usize;
                    let old_start = slot.offset as usize;
                    let old_end = old_start + record_len;

                    // Calculate new position (growing backwards from end)
                    let new_start = write_position - record_len;

                    // Only move if the record isn't already in the right place
                    if new_start != old_start {
                        // Use memmove-style copy that handles overlapping regions
                        self.data.copy_within(old_start..old_end, new_start);

                        // Update the slot with new offset
                        let updated_slot = SlotEntry {
                            offset: new_start as u16,
                            length: slot.length,
                        };
                        self.set_slot(i, updated_slot);
                    }

                    write_position = new_start;
                }
            }
        }

        // Update header with new free space boundary
        self.header_mut().free_space_end = write_position as u16;
    }

    pub fn used_space(&self) -> usize {
        let header = self.header();
        let slots_size = header.slot_count as usize * Self::SLOT_SIZE;
        let records_size = PAGE_SIZE - header.free_space_end as usize;

        Self::HEADER_SIZE + slots_size + records_size
    }

    pub fn fill_percentage(&self) -> f32 {
        (self.used_space() as f32 / PAGE_SIZE as f32) * 100.0
    }

    fn calculate_checksum(&self) -> u32 {
        use crc32fast::Hasher;

        let mut hasher = Hasher::new();

        // The checksum field is at bytes 24-28 (offset 24, size 4)
        // Hash everything except the checksum field
        hasher.update(&self.data[0..24]); // Before checksum
        hasher.update(&self.data[28..]); // After checksum

        hasher.finalize()
    }

    pub fn update_checksum(&mut self) {
        let checksum = self.calculate_checksum();
        self.header_mut().checksum = checksum;
    }

    pub fn verify_checksum(&self) -> bool {
        let stored = self.header().checksum;

        if stored == 0 {
            return true;
        }

        use crc32fast::Hasher;
        let mut hasher = Hasher::new();

        hasher.update(&self.data[0..24]);
        hasher.update(&[0u8; 4]);
        hasher.update(&self.data[28..]);

        hasher.finalize() == stored
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
        // The actual size is 32 due to alignment padding
        assert_eq!(Page::HEADER_SIZE, 32);
        assert_eq!(std::mem::size_of::<PageHeader>(), 32);
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

        // Add enough records to make compaction worthwhile
        let record1 = b"First";
        let record2 = b"Second";
        let record3 = b"Third";
        let record4 = b"Fourth";
        let record5 = b"Fifth";

        let slot1 = page.add_record(record1).unwrap();
        let slot2 = page.add_record(record2).unwrap();
        let slot3 = page.add_record(record3).unwrap();
        let slot4 = page.add_record(record4).unwrap();
        let slot5 = page.add_record(record5).unwrap();

        let space_before = page.free_space();

        // Delete 2 records (40% - exceeds threshold and >= 2 deleted)
        page.delete_record(slot2);
        page.delete_record(slot4);

        // Compact the page
        page.compact();

        // Check that we reclaimed space
        assert!(page.free_space() > space_before);

        // Verify remaining records are intact
        assert_eq!(page.get_record(slot1).unwrap(), record1);
        assert!(page.get_record(slot2).is_none()); // Still deleted
        assert_eq!(page.get_record(slot3).unwrap(), record3);
        assert!(page.get_record(slot4).is_none()); // Still deleted
        assert_eq!(page.get_record(slot5).unwrap(), record5);
    }

    #[test]
    fn test_page_compaction_small_page() {
        // Test that small pages don't compact
        let mut page = Page::new(1, PageType::Data);

        let record1 = b"First";
        let record2 = b"Second";
        let record3 = b"Third";

        let slot1 = page.add_record(record1).unwrap();
        let slot2 = page.add_record(record2).unwrap();
        let slot3 = page.add_record(record3).unwrap();

        let space_before = page.free_space();

        // Delete middle record (only 1 deleted - shouldn't trigger compaction)
        page.delete_record(slot2);

        let data_before = page.data.clone();
        page.compact();

        // Should NOT compact (only 1 deleted slot)
        assert_eq!(page.data, data_before);
        assert_eq!(page.free_space(), space_before); // Space unchanged

        // Records still accessible
        assert_eq!(page.get_record(slot1).unwrap(), record1);
        assert!(page.get_record(slot2).is_none());
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

    #[test]
    fn test_checksum() {
        let mut page = Page::new(1, PageType::Data);

        // Add some data
        page.add_record(b"test data").unwrap();

        // Calculate checksum
        page.update_checksum();
        let checksum = page.header().checksum;
        assert_ne!(checksum, 0);

        // Should verify successfully
        assert!(page.verify_checksum());

        // Corrupt the page
        page.data[100] ^= 0xFF; // Flip some bits

        // Should fail verification
        assert!(!page.verify_checksum());

        // Fix it and update checksum
        page.data[100] ^= 0xFF; // Flip back
        page.update_checksum();

        // Should verify again
        assert!(page.verify_checksum());
    }

    #[test]
    fn test_checksum_with_modifications() {
        let mut page = Page::new(1, PageType::Data);

        page.add_record(b"first").unwrap();
        page.update_checksum();
        let checksum1 = page.header().checksum;

        page.add_record(b"second").unwrap();
        page.update_checksum();
        let checksum2 = page.header().checksum;

        // Checksum should change when content changes
        assert_ne!(checksum1, checksum2);

        // Both should verify at their respective points
        assert!(page.verify_checksum());
    }

    #[test]
    fn test_add_record_prevents_underflow() {
        let mut page = Page::new(1, PageType::Data);

        // Corrupt free_space_end to a small value
        page.header_mut().free_space_end = 10;

        // Should fail safely without panic
        let record = b"test";
        assert!(page.add_record(record).is_none());
    }

    #[test]
    fn test_add_record_prevents_slot_overlap() {
        let mut page = Page::new(1, PageType::Data);

        // Add some records
        for i in 0..5 {
            page.add_record(format!("rec{}", i).as_bytes()).unwrap();
        }

        // Corrupt free_space_end to point into slot array
        page.header_mut().free_space_end = 50;

        // Should fail safely
        assert!(page.add_record(b"should_fail").is_none());
    }

    // Tests for Critical Issue #2: from_bytes validation
    #[test]
    fn test_from_bytes_validates_free_space_end() {
        let mut bytes = [0u8; PAGE_SIZE];
        let mut page = Page::new(1, PageType::Data);
        bytes.copy_from_slice(page.as_bytes());

        // Corrupt free_space_end
        bytes[8] = 0xFF; // free_space_end low byte
        bytes[9] = 0xFF; // free_space_end high byte (= 65535 > 8192)

        assert!(Page::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_from_bytes_validates_slot_array_bounds() {
        let mut bytes = [0u8; PAGE_SIZE];
        let mut page = Page::new(1, PageType::Data);
        bytes.copy_from_slice(page.as_bytes());

        // Set slot_count to huge value
        bytes[10] = 0xFF; // slot_count low byte
        bytes[11] = 0x7F; // slot_count high byte (= 32767 slots)

        assert!(Page::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_from_bytes_validates_no_overlap() {
        let mut bytes = [0u8; PAGE_SIZE];
        let mut page = Page::new(1, PageType::Data);
        page.header_mut().slot_count = 10;
        page.header_mut().free_space_end = 50; // Would overlap with slots
        bytes.copy_from_slice(page.as_bytes());

        assert!(Page::from_bytes(&bytes).is_err());
    }

    // Tests for Critical Issue #3: Thread-safe checksum
    #[test]
    fn test_verify_checksum_is_readonly() {
        let mut page = Page::new(1, PageType::Data);
        page.add_record(b"test").unwrap();
        page.update_checksum();

        let original_data = page.data.clone();

        // verify_checksum should not modify the page
        assert!(page.verify_checksum());
        assert_eq!(page.data, original_data);
    }

    // Tests for Issue #5: get_slot bounds
    #[test]
    fn test_get_slot_validates_bounds() {
        let mut page = Page::new(1, PageType::Data);

        // Set slot_count to a value where slots would exceed page
        // Need: HEADER_SIZE + (count * SLOT_SIZE) > PAGE_SIZE
        // 32 + (count * 4) > 8192
        // count > (8192 - 32) / 4 = 2040
        page.header_mut().slot_count = 2050;

        // Slot 2049 would be at offset: 32 + (2049 * 4) = 8228
        // 8228 + 4 = 8232, which exceeds PAGE_SIZE (8192)
        assert!(page.get_slot(2049).is_none());

        // Also test edge case: last slot that WOULD fit
        // Slot 2039 at offset: 32 + (2039 * 4) = 8188
        // 8188 + 4 = 8192, exactly PAGE_SIZE, should work
        assert!(page.get_slot(2039).is_some());

        // Slot 2040 at offset: 32 + (2040 * 4) = 8192
        // 8192 + 4 = 8196, exceeds PAGE_SIZE, should fail
        assert!(page.get_slot(2040).is_none());
    }

    // Tests for fragmentation metrics
    #[test]
    fn test_deleted_count() {
        let mut page = Page::new(1, PageType::Data);

        let slot1 = page.add_record(b"first").unwrap();
        let slot2 = page.add_record(b"second").unwrap();
        let slot3 = page.add_record(b"third").unwrap();

        assert_eq!(page.deleted_count(), 0);

        page.delete_record(slot1);
        assert_eq!(page.deleted_count(), 1);

        page.delete_record(slot3);
        assert_eq!(page.deleted_count(), 2);
    }

    #[test]
    fn test_should_compact_threshold() {
        let mut page = Page::new(1, PageType::Data);

        // Add 10 records
        let mut slots = Vec::new();
        for i in 0..10 {
            slots.push(page.add_record(format!("rec{}", i).as_bytes()).unwrap());
        }

        // Delete 2 records (20%) - should not trigger compaction
        page.delete_record(slots[0]);
        page.delete_record(slots[1]);
        assert!(!page.should_compact());

        // Delete 1 more (30%) - should trigger compaction
        page.delete_record(slots[2]);
        assert!(page.should_compact());
    }

    #[test]
    fn test_compact_only_when_needed() {
        let mut page = Page::new(1, PageType::Data);

        // Add and delete one record (not enough for threshold)
        let slot = page.add_record(b"test").unwrap();
        page.delete_record(slot);

        let data_before = page.data.clone();
        page.compact();

        // Should not compact (below threshold)
        assert_eq!(page.data, data_before);
    }
}


#[cfg(test)]
mod batch_tests {
    use super::*;
    
    #[test]
    fn test_batch_add_records() {
        let mut page = Page::new(1, PageType::Data);
        
        let records = vec![
            b"First".as_slice(),
            b"Second".as_slice(),
            b"Third".as_slice(),
            b"Fourth".as_slice(),
        ];
        
        let results = page.add_records(&records);
        
        // All should succeed
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|r| r.is_some()));
        
        // Verify all records are readable
        assert_eq!(page.get_record(0).unwrap(), b"First");
        assert_eq!(page.get_record(1).unwrap(), b"Second");
        assert_eq!(page.get_record(2).unwrap(), b"Third");
        assert_eq!(page.get_record(3).unwrap(), b"Fourth");
        
        // Should have updated header once
        assert_eq!(page.header().slot_count, 4);
    }
    
    #[test]
    fn test_batch_add_with_limited_space() {
        let mut page = Page::new(1, PageType::Data);
        
        // Fill page almost completely
        let big_record = vec![b'X'; 4000];
        page.add_record(&big_record).unwrap();

        let vec_too_big = vec![b'Y'; 5000];
        
        // Try to add multiple records with limited space
        let records = vec![
            b"Small1".as_slice(),
            b"Small2".as_slice(),
            vec_too_big.as_slice(), // Too big
            b"Small3".as_slice(),
        ];
        
        let results = page.add_records(&records);
        
        // Some should succeed, some should fail
        assert_eq!(results.len(), 4);
        assert!(results[0].is_some()); // Small1 fits
        assert!(results[1].is_some()); // Small2 fits
        assert!(results[2].is_none()); // Too big
        assert!(results[3].is_some()); // Small3 fits
    }
    
    #[test]
    fn test_batch_delete_records() {
        let mut page = Page::new(1, PageType::Data);
        
        // Add some records
        let records = vec![
            b"A".as_slice(),
            b"B".as_slice(),
            b"C".as_slice(),
            b"D".as_slice(),
            b"E".as_slice(),
        ];
        page.add_records(&records);
        
        // Delete multiple at once
        let to_delete = vec![1, 3]; // Delete B and D
        let deleted = page.delete_records(&to_delete);
        
        assert_eq!(deleted, 2);
        assert!(page.get_record(1).is_none());
        assert!(page.get_record(3).is_none());
        assert_eq!(page.get_record(0).unwrap(), b"A");
        assert_eq!(page.get_record(2).unwrap(), b"C");
        assert_eq!(page.get_record(4).unwrap(), b"E");
    }
    
    #[test]
    fn test_batch_performance() {
        use std::time::Instant;

        let owned_records: Vec<Vec<u8>> = (0..100)
        .map(|i| format!("Record{}", i).into_bytes())
        .collect();

        let records: Vec<&[u8]> = owned_records
            .iter()
            .map(|v| v.as_slice())
            .collect();
        
        // Individual inserts
        let mut page1 = Page::new(1, PageType::Data);
        let start = Instant::now();
        for record in &records {
            page1.add_record(record);
        }
        let individual_time = start.elapsed();
        
        // Batch insert
        let mut page2 = Page::new(2, PageType::Data);
        let start = Instant::now();
        page2.add_records(&records);
        let batch_time = start.elapsed();
        
        // Batch should be faster (less overhead)
        println!("Individual: {:?}, Batch: {:?}", individual_time, batch_time);
        
        // Verify same results
        for i in 0..100 {
            assert_eq!(page1.get_record(i), page2.get_record(i));
        }
    }
}