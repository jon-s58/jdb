// storage/src/file/mod.rs

use crate::page::{Page, PageType, PAGE_SIZE};
use crate::{Result, StorageError};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Magic number to identify our database files
const DB_MAGIC: [u8; 4] = *b"JDB1"; // JDB version 1

const FILE_VERSION: u32 = 1;

const HEADER_SIZE: usize = 512;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FileHeader {
    // Core identification (16 bytes)
    magic: [u8; 4],   // "JDB1"
    version: u32,     // File format version
    header_size: u32, // Size of this header (512)
    page_size: u32,   // Page size (8192)

    // Page management (16 bytes)
    page_count: u32,      // Total pages in file
    free_list_head: u32,  // Head of free page list (0 = no free pages)
    first_data_page: u32, // First page with user data (0 = none)
    last_data_page: u32,  // Last page with user data (0 = none)

    // Timestamps (16 bytes)
    created_at: u64,    // Creation timestamp
    last_modified: u64, // Last modification timestamp

    // Integrity (8 bytes)
    header_checksum: u32,    // CRC32 of header
    data_checksum_flag: u32, // 0 = off, 1 = on for data pages

    // Future expansion
    _reserved: [u8; 456], // 512 - 56 = 456 bytes for future use
}

impl FileHeader {
    fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            magic: DB_MAGIC,
            version: FILE_VERSION,
            header_size: HEADER_SIZE as u32,
            page_size: PAGE_SIZE as u32,

            page_count: 1, // Start with 1 to account for header page
            free_list_head: 0,
            first_data_page: 0,
            last_data_page: 0,

            created_at: now,
            last_modified: now,

            header_checksum: 0,
            data_checksum_flag: 1, // Enable checksums by default

            _reserved: [0; 456],
        }
    }

    fn validate(&self) -> Result<()> {
        if self.magic != DB_MAGIC {
            return Err(StorageError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid database file: wrong magic number",
            )));
        }

        if self.version > FILE_VERSION {
            return Err(StorageError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported file version: {}", self.version),
            )));
        }

        if self.page_size != PAGE_SIZE as u32 {
            return Err(StorageError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Invalid page size: expected {}, got {}",
                    PAGE_SIZE, self.page_size
                ),
            )));
        }

        Ok(())
    }

    fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut bytes = [0u8; HEADER_SIZE];

        // Core identification (16 bytes)
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..8].copy_from_slice(&self.version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.header_size.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.page_size.to_le_bytes());

        // Page management (16 bytes)
        bytes[16..20].copy_from_slice(&self.page_count.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.free_list_head.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.first_data_page.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.last_data_page.to_le_bytes());

        // Timestamps (16 bytes)
        bytes[32..40].copy_from_slice(&self.created_at.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.last_modified.to_le_bytes());

        // Integrity (8 bytes)
        bytes[48..52].copy_from_slice(&self.header_checksum.to_le_bytes());
        bytes[52..56].copy_from_slice(&self.data_checksum_flag.to_le_bytes());

        // Reserved bytes
        bytes[56..512].copy_from_slice(&self._reserved);

        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(StorageError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid file header size",
            )));
        }

        let header = Self {
            magic: bytes[0..4].try_into().unwrap(),
            version: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            header_size: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            page_size: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),

            page_count: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            free_list_head: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            first_data_page: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            last_data_page: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),

            created_at: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
            last_modified: u64::from_le_bytes(bytes[40..48].try_into().unwrap()),

            header_checksum: u32::from_le_bytes(bytes[48..52].try_into().unwrap()),
            data_checksum_flag: u32::from_le_bytes(bytes[52..56].try_into().unwrap()),

            _reserved: bytes[56..512].try_into().unwrap(),
        };

        header.validate()?;
        Ok(header)
    }

    fn update_checksum(&mut self) {
        use crc32fast::Hasher;

        self.header_checksum = 0;
        let bytes = self.to_bytes();

        let mut hasher = Hasher::new();
        hasher.update(&bytes[0..48]); // Hash everything before checksum field
        hasher.update(&bytes[52..]); // Hash everything after checksum field

        self.header_checksum = hasher.finalize();
    }

    fn verify_checksum(&self) -> bool {
        let stored_checksum = self.header_checksum;
        let mut temp = *self;
        temp.header_checksum = 0;

        let bytes = temp.to_bytes();
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&bytes[0..48]);
        hasher.update(&bytes[52..]);

        hasher.finalize() == stored_checksum
    }
}

pub struct PageFile {
    file: File,
    header: FileHeader,
}

impl PageFile {
    pub fn create_new(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(StorageError::Io)?;

        let mut header = FileHeader::new();
        header.update_checksum();

        let mut page_file = Self { file, header };

        // Write the header
        page_file.write_header()?;

        Ok(page_file)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(StorageError::Io)?;

        let header = Self::read_header(&mut file)?;

        Ok(Self { file, header })
    }

    pub fn write_page(&mut self, page: &Page) -> Result<()> {
        let page_id = page.header().page_id;

        // Page 0 is reserved for the file header
        if page_id == 0 {
            return Err(StorageError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot write to page 0 (reserved for file header)",
            )));
        }

        // Seek to page position
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(StorageError::Io)?;

        // Write page data
        self.file
            .write_all(page.as_bytes())
            .map_err(StorageError::Io)?;

        // Update header if this extends the file
        if page_id >= self.header.page_count {
            self.header.page_count = page_id + 1;
            self.update_modified_time();
            self.write_header()?;
        }

        Ok(())
    }

    pub fn read_page(&mut self, page_id: u32) -> Result<Page> {
        if page_id == 0 {
            return Err(StorageError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot read page 0 as a data page (it's the header)",
            )));
        }

        if page_id >= self.header.page_count {
            return Err(StorageError::PageNotFound(page_id));
        }

        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(StorageError::Io)?;

        let mut buffer = [0u8; PAGE_SIZE];
        self.file
            .read_exact(&mut buffer)
            .map_err(StorageError::Io)?;

        let page = Page::from_bytes(&buffer)?;

        // Verify checksum if enabled
        if self.header.data_checksum_flag != 0 && !page.verify_checksum() {
            return Err(StorageError::ChecksumMismatch(page_id));
        }

        Ok(page)
    }

    pub fn allocate_page(&mut self) -> Result<u32> {
        // For now, just append a new page
        // TODO: Later implement free list management
        let page_id = self.header.page_count;
        self.header.page_count += 1;

        // Create and write an empty page
        let mut page = Page::new(page_id, PageType::Free);
        if self.header.data_checksum_flag != 0 {
            page.update_checksum();
        }

        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(StorageError::Io)?;
        self.file
            .write_all(page.as_bytes())
            .map_err(StorageError::Io)?;

        self.update_modified_time();
        self.write_header()?;

        Ok(page_id)
    }

    pub fn page_count(&self) -> u32 {
        self.header.page_count
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.sync_all().map_err(StorageError::Io)
    }

    fn write_header(&mut self) -> Result<()> {
        self.header.update_checksum();

        // Create a full page for the header (for alignment)
        let mut header_page = [0u8; PAGE_SIZE];
        let header_bytes = self.header.to_bytes();
        header_page[0..HEADER_SIZE].copy_from_slice(&header_bytes);

        self.file
            .seek(SeekFrom::Start(0))
            .map_err(StorageError::Io)?;
        self.file
            .write_all(&header_page)
            .map_err(StorageError::Io)?;

        Ok(())
    }

    fn read_header(file: &mut File) -> Result<FileHeader> {
        file.seek(SeekFrom::Start(0)).map_err(StorageError::Io)?;

        let mut buffer = [0u8; PAGE_SIZE];
        file.read_exact(&mut buffer).map_err(StorageError::Io)?;

        let header = FileHeader::from_bytes(&buffer[0..HEADER_SIZE])?;

        // Verify checksum
        if !header.verify_checksum() {
            return Err(StorageError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File header checksum mismatch",
            )));
        }

        Ok(header)
    }

    fn update_modified_time(&mut self) {
        use std::time::{SystemTime, UNIX_EPOCH};

        self.header.last_modified = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
}
