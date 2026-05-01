use alloc::string::String;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    pub static ref VFS: Mutex<Vfs> = Mutex::new(Vfs::new());
}

pub const MAX_OPEN_FILES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Device,
    Pipe,
}

#[derive(Debug, Clone)]
pub struct FileStat {
    pub size: u64,
    pub file_type: FileType,
}

#[derive(Debug, Clone)]
pub struct FileDescriptor {
    pub name: String,
    pub file_type: FileType,
    pub offset: u64,
}

#[derive(Debug, Clone)]
pub struct VfsEntry {
    pub name: String,
    pub file_type: FileType,
    pub data: Option<&'static [u8]>,
}

pub struct Vfs {
    entries: Vec<VfsEntry>,
    open_files: [Option<FileDescriptor>; MAX_OPEN_FILES],
}

impl Vfs {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            open_files: [const { None }; MAX_OPEN_FILES],
        }
    }

    pub fn init_from_ramdisk(&mut self, addr: u64, size: u64) {
        self.init_defaults();

        if addr != 0 && size > 0 {
            let data = unsafe { core::slice::from_raw_parts(addr as *const u8, size as usize) };

            let mut offset = 0;
            while offset + 72 <= data.len() {
                // Read 64-byte filename
                let name_bytes = &data[offset..offset + 64];
                let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(64);
                let name = core::str::from_utf8(&name_bytes[..name_len]).unwrap_or("unknown");

                // Read 8-byte size
                let mut size_bytes = [0u8; 8];
                size_bytes.copy_from_slice(&data[offset + 64..offset + 72]);
                let file_size = u64::from_le_bytes(size_bytes) as usize;

                offset += 72;

                if offset + file_size <= data.len() {
                    let file_data = &data[offset..offset + file_size];
                    self.entries.push(VfsEntry {
                        name: String::from(name),
                        file_type: FileType::Regular,
                        data: Some(unsafe { core::mem::transmute(file_data) }),
                    });
                    offset += file_size;
                } else {
                    break;
                }
            }
        }
    }

    fn init_defaults(&mut self) {
        self.entries.push(VfsEntry {
            name: String::from("."),
            file_type: FileType::Directory,
            data: None,
        });
        self.entries.push(VfsEntry {
            name: String::from("dev"),
            file_type: FileType::Directory,
            data: None,
        });
        self.entries.push(VfsEntry {
            name: String::from("null"),
            file_type: FileType::Device,
            data: None,
        });
    }

    pub fn open(&mut self, path: &str) -> Option<usize> {
        for entry in &self.entries {
            if entry.name == path {
                let fd = FileDescriptor {
                    name: String::from(path),
                    file_type: entry.file_type,
                    offset: 0,
                };
                for j in 0..self.open_files.len() {
                    if self.open_files[j].is_none() {
                        self.open_files[j] = Some(fd);
                        return Some(j);
                    }
                }
            }
        }
        None
    }

    pub fn read(&mut self, fd_index: usize, buf: &mut [u8]) -> Option<usize> {
        if fd_index >= self.open_files.len() {
            return None;
        }
        let fd = self.open_files[fd_index].as_mut()?;
        let entry = self.entries.iter().find(|e| e.name == fd.name)?;
        if let Some(data) = entry.data {
            let start = fd.offset as usize;
            if start >= data.len() {
                return Some(0); // EOF
            }
            let available = data.len() - start;
            let len = buf.len().min(available);
            buf[..len].copy_from_slice(&data[start..start + len]);
            fd.offset += len as u64;
            return Some(len);
        }
        None
    }

    pub fn stat(&self, path: &str) -> Option<FileStat> {
        let entry = self.entries.iter().find(|e| e.name == path)?;
        Some(FileStat {
            size: entry.data.map(|d| d.len() as u64).unwrap_or(0),
            file_type: entry.file_type,
        })
    }

    pub fn list_dir(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.name.clone()).collect()
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}
