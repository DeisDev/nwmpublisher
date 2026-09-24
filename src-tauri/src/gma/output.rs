use std::{ffi::OsStr, fs::{self, File}, io, path::{Component, Path, PathBuf}, sync::Arc};

// Unix operations are relative to held directory descriptors. Windows pins every
// ancestor without FILE_SHARE_DELETE and opens reparse points themselves.
#[derive(Clone)]
pub(super) struct Directory {
	file: Arc<File>,
	pub path: PathBuf,
	#[cfg(windows)]
	ancestors: Vec<Arc<File>>,
}

#[derive(PartialEq)]
pub(super) enum Kind { File, Directory }

fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, "links, reparse points and non-regular extraction targets are not allowed") }

fn name(path: &Path) -> io::Result<&OsStr> {
	let mut parts = path.components();
	match (parts.next(), parts.next()) {
		(Some(Component::Normal(name)), None) => Ok(name),
		_ => Err(invalid()),
	}
}

#[cfg(unix)]
fn c_name(path: &Path) -> io::Result<std::ffi::CString> {
	use std::os::unix::ffi::OsStrExt;
	std::ffi::CString::new(name(path)?.as_bytes()).map_err(|_| invalid())
}

#[cfg(unix)]
fn cvt(result: libc::c_int) -> io::Result<()> {
	if result == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

impl Directory {
	pub fn children(&self) -> io::Result<Vec<PathBuf>> {
		#[cfg(windows)] { fs::read_dir(&self.path)?.map(|entry| entry.map(|entry| PathBuf::from(entry.file_name()))).collect() }
		#[cfg(unix)] {
			use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
			let fd = unsafe { libc::openat(self.file.as_raw_fd(), c".".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
			if fd == -1 { return Err(io::Error::last_os_error()); }
			let directory = unsafe { libc::fdopendir(fd) };
			if directory.is_null() { let error = io::Error::last_os_error(); unsafe { libc::close(fd); } return Err(error); }
			let mut children = Vec::new();
			loop {
				#[cfg(target_os = "linux")]
				let errno = unsafe { libc::__errno_location() };
				#[cfg(target_os = "macos")]
				let errno = unsafe { libc::__error() };
				unsafe { *errno = 0; }
				let entry = unsafe { libc::readdir(directory) };
				if entry.is_null() {
					let error = unsafe { *errno };
					unsafe { libc::closedir(directory); }
					return if error == 0 { Ok(children) } else { Err(io::Error::from_raw_os_error(error)) };
				}
				let entry = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
				if entry != b"." && entry != b".." { children.push(PathBuf::from(OsStr::from_bytes(entry))); }
			}
		}
	}

	pub fn remove_tree(&self, child: &Path) -> io::Result<()> {
		if self.kind(child)? == Some(Kind::Directory) {
			let directory = self.child(child, false)?;
			for entry in directory.children()? { directory.remove_tree(&entry)?; }
			drop(directory);
			self.remove(child, true)
		} else { self.remove(child, false) }
	}
	pub fn open(path: &Path, create: bool) -> io::Result<Self> {
		let absolute = std::path::absolute(path)?;
		let mut normalized = PathBuf::new();
		for part in absolute.components() {
			match part { Component::ParentDir => { normalized.pop(); }, Component::CurDir => {}, _ => normalized.push(part) }
		}
		let mut root = PathBuf::new();
		let mut parts = normalized.components().peekable();
		while matches!(parts.peek(), Some(Component::Prefix(_) | Component::RootDir)) { root.push(parts.next().unwrap()); }
		#[cfg(unix)]
		let file = {
			use std::os::unix::fs::OpenOptionsExt;
			fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&root)?
		};
		#[cfg(windows)]
		let file = Self::open_windows(&root)?;
		let mut directory = Self {
			file: Arc::new(file), path: root,
			#[cfg(windows)] ancestors: Vec::new(),
		};
		for part in parts {
			if let Component::Normal(part) = part { directory = directory.child(Path::new(part), create)?; }
			else { return Err(invalid()); }
		}
		Ok(directory)
	}

	#[cfg(windows)]
	fn open_windows(path: &Path) -> io::Result<File> {
		use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
		let file = fs::OpenOptions::new().access_mode(0).share_mode(0x1)
			.custom_flags(0x02000000 | 0x00200000).open(path)
			.map_err(|error| io::Error::new(error.kind(), crate::IoError::new("open extraction directory", path, error)))?; // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
		let metadata = file.metadata().map_err(|error| io::Error::new(error.kind(), crate::IoError::new("read extraction directory metadata", path, error)))?;
		if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 { return Err(invalid()); }
		Ok(file)
	}

	pub fn child(&self, child: &Path, create: bool) -> io::Result<Self> {
		name(child)?;
		if create {
			match self.mkdir(child) { Ok(()) => {}, Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}, Err(error) => return Err(error) }
		}
		#[cfg(unix)]
		let file = {
			use std::os::fd::{AsRawFd, FromRawFd};
			let child = c_name(child)?;
			let fd = unsafe { libc::openat(self.file.as_raw_fd(), child.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
			if fd == -1 { return Err(io::Error::last_os_error()); }
			unsafe { File::from_raw_fd(fd) }
		};
		#[cfg(windows)]
		let file = Self::open_windows(&self.path.join(child))?;
		Ok(Self {
			file: Arc::new(file), path: self.path.join(child),
			#[cfg(windows)] ancestors: { let mut parents = self.ancestors.clone(); parents.push(self.file.clone()); parents },
		})
	}

	pub fn mkdir(&self, child: &Path) -> io::Result<()> {
		name(child)?;
		#[cfg(unix)] {
			use std::os::fd::AsRawFd;
			let child = c_name(child)?;
			cvt(unsafe { libc::mkdirat(self.file.as_raw_fd(), child.as_ptr(), 0o700) })
		}
		#[cfg(windows)] { fs::create_dir(self.path.join(child)) }
	}

	pub fn create_file(&self, child: &Path) -> io::Result<File> {
		name(child)?;
		#[cfg(unix)] {
			use std::os::fd::{AsRawFd, FromRawFd};
			let child = c_name(child)?;
			let fd = unsafe { libc::openat(self.file.as_raw_fd(), child.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600 as libc::mode_t) };
			if fd == -1 { return Err(io::Error::last_os_error()); }
			Ok(unsafe { File::from_raw_fd(fd) })
		}
		#[cfg(windows)] { fs::OpenOptions::new().write(true).create_new(true).open(self.path.join(child)) }
	}

	pub fn kind(&self, child: &Path) -> io::Result<Option<Kind>> {
		name(child)?;
		#[cfg(unix)] {
			use std::os::fd::AsRawFd;
			let child = c_name(child)?;
			let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
			let result = unsafe { libc::fstatat(self.file.as_raw_fd(), child.as_ptr(), stat.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
			if result == -1 {
				let error = io::Error::last_os_error();
				return if error.kind() == io::ErrorKind::NotFound { Ok(None) } else { Err(error) };
			}
			match unsafe { stat.assume_init() }.st_mode & libc::S_IFMT {
				libc::S_IFREG => Ok(Some(Kind::File)), libc::S_IFDIR => Ok(Some(Kind::Directory)), _ => Err(invalid()),
			}
		}
		#[cfg(windows)] {
			use std::os::windows::fs::MetadataExt;
			let metadata = match fs::symlink_metadata(self.path.join(child)) {
				Ok(metadata) => metadata, Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None), Err(error) => return Err(error),
			};
			if metadata.file_attributes() & 0x400 != 0 { return Err(invalid()); }
			if metadata.is_file() { Ok(Some(Kind::File)) } else if metadata.is_dir() { Ok(Some(Kind::Directory)) } else { Err(invalid()) }
		}
	}

	pub fn rename(&self, source: &Path, target: &Self, destination: &Path) -> io::Result<()> {
		name(source)?; name(destination)?;
		#[cfg(unix)] {
			use std::os::fd::AsRawFd;
			let source = c_name(source)?; let destination = c_name(destination)?;
			cvt(unsafe { libc::renameat(self.file.as_raw_fd(), source.as_ptr(), target.file.as_raw_fd(), destination.as_ptr()) })
		}
		#[cfg(windows)] { fs::rename(self.path.join(source), target.path.join(destination)) }
	}

	pub fn remove(&self, child: &Path, directory: bool) -> io::Result<()> {
		name(child)?;
		#[cfg(unix)] {
			use std::os::fd::AsRawFd;
			let child = c_name(child)?;
			cvt(unsafe { libc::unlinkat(self.file.as_raw_fd(), child.as_ptr(), if directory { libc::AT_REMOVEDIR } else { 0 }) })
		}
		#[cfg(windows)] {
			if directory { fs::remove_dir(self.path.join(child)) } else { fs::remove_file(self.path.join(child)) }
		}
	}
}
