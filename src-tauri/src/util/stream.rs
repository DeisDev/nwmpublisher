use std::{
	io::{BufRead, ErrorKind, Read, Seek, SeekFrom, Write},
	sync::Arc,
};

use byteorder::WriteBytesExt;

pub fn stream_len<F: Seek + ?Sized>(f: &mut F) -> Result<u64, std::io::Error> {
	let old_pos = f.stream_position()?;
	let len = f.seek(SeekFrom::End(0))?;
	if old_pos != len {
		f.seek(SeekFrom::Start(old_pos))?;
	}

	Ok(len)
}

#[derive(Debug)]
pub enum StreamError {
	Read(std::io::Error),
	Write(std::io::Error),
}

pub fn stream_bytes<R: BufRead + ?Sized, W: Write>(r: &mut R, w: &mut W, mut bytes: u64, mut progress: impl FnMut(u64)) -> Result<(), StreamError> {
	let total = bytes;
	while bytes > 0 {
		let data = match r.fill_buf() {
			Ok([]) => {
				return Err(StreamError::Read(std::io::Error::new(
					ErrorKind::UnexpectedEof,
					"archive entry is truncated",
				)))
			}
			Ok(data) => data,
			Err(error) if error.kind() == ErrorKind::Interrupted => continue,
			Err(error) => return Err(StreamError::Read(error)),
		};
		let consumed = bytes.min(data.len() as u64) as usize;
		w.write_all(&data[..consumed]).map_err(StreamError::Write)?;
		r.consume(consumed);
		bytes -= consumed as u64;
		progress(total - bytes);
	}
	Ok(())
}

pub trait NTStringReader: BufRead + Seek {
	fn read_nt_string(&mut self) -> Result<String, std::io::Error> {
		let mut buf = vec![];
		let bytes_read = (&mut *self).take(1024 * 1024 + 1).read_until(0, &mut buf)?;
		if bytes_read > 1024 * 1024 { return Err(std::io::Error::new(ErrorKind::InvalidData, "archive string exceeds 1 MiB")); }
		if bytes_read == 0 || buf.last() != Some(&0) {
			return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "unterminated archive string"));
		}
		let nt_string = &buf[0..bytes_read - 1];

		Ok(match std::str::from_utf8(nt_string) {
			Ok(str) => str.to_owned(),
			Err(_) => {
				// Some file paths aren't UTF-8 encoded, usually due to Windows NTFS
				// This will simply guess the text encoding and decode it with that instead
				let mut decoder = chardetng::EncodingDetector::new();
				decoder.feed(nt_string, true);
				let encoding = decoder.guess(None, false);
				let (str, _, _) = encoding.decode(nt_string);
				str.to_string()
			}
		})
	}

	fn skip_nt_string(&mut self) -> Result<usize, std::io::Error> {
		let mut buf = vec![];
		let bytes = (&mut *self).take(1024 * 1024 + 1).read_until(0, &mut buf)?;
		if bytes > 1024 * 1024 { return Err(std::io::Error::new(ErrorKind::InvalidData, "archive string exceeds 1 MiB")); }
		if buf.last() != Some(&0) {
			return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "unterminated archive string"));
		}
		Ok(bytes)
	}
}

pub trait NTStringWriter: Write {
	fn write_nt_string<S: AsRef<str>>(&mut self, str: S) -> Result<(), std::io::Error> {
		self.write_all(str.as_ref().as_bytes())?;
		self.write_u8(0)?;
		Ok(())
	}
}
impl NTStringWriter for Vec<u8> {}

#[derive(derive_more::Deref, derive_more::DerefMut, Clone, Debug)]
pub struct ArcBytes(Arc<Vec<u8>>);
impl AsRef<[u8]> for ArcBytes {
	fn as_ref(&self) -> &[u8] {
		self.0.as_ref()
	}
}
impl From<Vec<u8>> for ArcBytes {
	fn from(bytes: Vec<u8>) -> Self {
		ArcBytes(Arc::new(bytes))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::{self, BufReader, Cursor, Read};

	#[test]
	fn copies_exactly_the_requested_bytes_and_reports_progress() {
		let mut reader = BufReader::with_capacity(2, Cursor::new(b"abcdef"));
		let mut output = Vec::new();
		let mut progress = Vec::new();
		stream_bytes(&mut reader, &mut output, 5, |bytes| progress.push(bytes)).unwrap();
		assert_eq!(output, b"abcde");
		assert_eq!(progress, [2, 4, 5]);
		assert_eq!(reader.fill_buf().unwrap(), b"f");
		stream_bytes(&mut reader, &mut output, 0, |_| panic!("zero-byte copy progressed")).unwrap();
	}

	#[test]
	fn rejects_truncated_entries_and_strings() {
		let error = stream_bytes(&mut Cursor::new(b"short"), &mut Vec::new(), 6, |_| {}).unwrap_err();
		assert!(matches!(error, StreamError::Read(error) if error.kind() == ErrorKind::UnexpectedEof));
		for bytes in [Vec::new(), b"unterminated".to_vec()] {
			let mut reader = Cursor::new(ArcBytes::from(bytes));
			assert_eq!(reader.read_nt_string().unwrap_err().kind(), ErrorKind::UnexpectedEof);
			reader.set_position(0);
			assert_eq!(reader.skip_nt_string().unwrap_err().kind(), ErrorKind::UnexpectedEof);
		}
	}

	struct FailingIo;
	impl Read for FailingIo {
		fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
			Err(io::Error::new(ErrorKind::PermissionDenied, "read denied"))
		}
	}
	impl Write for FailingIo {
		fn write(&mut self, _: &[u8]) -> io::Result<usize> {
			Err(io::Error::new(ErrorKind::WriteZero, "disk full"))
		}
		fn flush(&mut self) -> io::Result<()> {
			Ok(())
		}
	}

	#[test]
	fn distinguishes_read_and_write_failures() {
		let error = stream_bytes(&mut BufReader::new(FailingIo), &mut Vec::new(), 1, |_| {}).unwrap_err();
		assert!(matches!(error, StreamError::Read(error) if error.to_string() == "read denied"));
		let error = stream_bytes(&mut Cursor::new(b"x"), &mut FailingIo, 1, |_| {}).unwrap_err();
		assert!(matches!(error, StreamError::Write(error) if error.to_string() == "disk full"));
	}
}
