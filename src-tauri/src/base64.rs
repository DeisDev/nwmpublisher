use base64::{engine::general_purpose::STANDARD, DecodeError, Engine};

pub fn encode<T: AsRef<[u8]>>(bytes: T) -> String {
	STANDARD.encode(bytes)
}

pub fn decode<T: AsRef<[u8]>>(text: T) -> Result<Vec<u8>, DecodeError> {
	STANDARD.decode(text)
}
