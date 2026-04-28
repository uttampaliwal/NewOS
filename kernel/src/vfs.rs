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
            self.entries.push(VfsEntry {
                name: String::from("initramfs.txt"),
                file_type: FileType::Regular,
                data: Some(data),
            });
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

    pub fn read(&self, fd_index: usize, buf: &mut [u8]) -> Option<usize> {
        if fd_index >= self.open_files.len() {
            return None;
        }
        let fd = self.open_files[fd_index].as_ref()?;
        let entry = self.entries.iter().find(|e| e.name == fd.name)?;
        if let Some(data) = entry.data {
            let len = buf.len().min(data.len());
            buf[..len].copy_from_slice(&data[..len]);
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
