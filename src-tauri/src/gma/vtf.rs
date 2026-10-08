use byteorder::{ByteOrder, LittleEndian};
use image::{
	codecs::{
		dxt::{DxtDecoder, DxtVariant},
		png::PngEncoder,
	},
	ColorType, ImageDecoder,
};

const INVALID: &str = "ERR_FILE_PREVIEW_VTF_INVALID";
const PIXEL_LIMIT: u64 = 4096 * 4096;

#[derive(Clone, Copy)]
enum Format {
	Rgba,
	Abgr,
	Rgb,
	Bgr,
	Rgb565,
	Intensity,
	IntensityAlpha,
	Alpha,
	Argb,
	Bgra,
	Dxt1,
	Dxt3,
	Dxt5,
	Bgrx,
	Bgr565,
	Bgrx5551,
	Bgra4444,
	Dxt1Alpha,
	Bgra5551,
}

impl Format {
	fn from_id(id: u32) -> Result<Self, String> {
		Ok(match id {
			0 => Self::Rgba,
			1 => Self::Abgr,
			2 => Self::Rgb,
			3 => Self::Bgr,
			4 => Self::Rgb565,
			5 => Self::Intensity,
			6 => Self::IntensityAlpha,
			8 => Self::Alpha,
			11 => Self::Argb,
			12 => Self::Bgra,
			13 => Self::Dxt1,
			14 => Self::Dxt3,
			15 => Self::Dxt5,
			16 => Self::Bgrx,
			17 => Self::Bgr565,
			18 => Self::Bgrx5551,
			19 => Self::Bgra4444,
			20 => Self::Dxt1Alpha,
			21 => Self::Bgra5551,
			_ => return Err(format!("ERR_FILE_PREVIEW_VTF_FORMAT:{id}")),
		})
	}

	fn bytes_per_pixel(self) -> usize {
		match self {
			Self::Intensity | Self::Alpha => 1,
			Self::Rgb565 | Self::IntensityAlpha | Self::Bgr565 | Self::Bgrx5551 | Self::Bgra4444 | Self::Bgra5551 => 2,
			Self::Rgb | Self::Bgr => 3,
			_ => 4,
		}
	}

	fn size(self, width: u32, height: u32) -> u64 {
		match self {
			Self::Dxt1 | Self::Dxt1Alpha => u64::from(width.div_ceil(4)) * u64::from(height.div_ceil(4)) * 8,
			Self::Dxt3 | Self::Dxt5 => u64::from(width.div_ceil(4)) * u64::from(height.div_ceil(4)) * 16,
			_ => u64::from(width) * u64::from(height) * self.bytes_per_pixel() as u64,
		}
	}
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], String> {
	bytes
		.get(offset..offset + N)
		.and_then(|slice| slice.try_into().ok())
		.ok_or_else(|| INVALID.into())
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
	Ok(u32::from_le_bytes(field(bytes, offset)?))
}

fn short(bytes: &[u8], offset: usize) -> Result<u32, String> {
	Ok(u16::from_le_bytes(field(bytes, offset)?) as u32)
}

pub(super) fn preview(bytes: &[u8]) -> Result<Vec<u8>, String> {
	if &field::<4>(bytes, 0)? != b"VTF\0" {
		return Err(INVALID.into());
	}
	let major = word(bytes, 4)?;
	let minor = word(bytes, 8)?;
	if major != 7 || minor > 5 {
		return Err(format!("ERR_FILE_PREVIEW_VTF_VERSION:{major}.{minor}"));
	}
	let header_size = word(bytes, 12)? as usize;
	let minimum_header = if minor >= 2 { 80 } else { 64 };
	if header_size < minimum_header || header_size > bytes.len() {
		return Err(INVALID.into());
	}
	let width = short(bytes, 16)?;
	let height = short(bytes, 18)?;
	let flags = word(bytes, 20)?;
	let frames = short(bytes, 24)?;
	let first_frame = short(bytes, 26)?;
	let format = Format::from_id(word(bytes, 52)?)?;
	let mipmaps = u32::from(field::<1>(bytes, 56)?[0]);
	let depth = if minor >= 2 { short(bytes, 63)? } else { 1 };
	if !width.is_power_of_two()
		|| !height.is_power_of_two()
		|| !depth.is_power_of_two()
		|| frames == 0
		|| mipmaps == 0
		|| mipmaps > width.max(height).max(depth).ilog2() + 1
	{
		return Err(INVALID.into());
	}
	if u64::from(width.max(4)) * u64::from(height.max(4)) > PIXEL_LIMIT {
		return Err("ERR_FILE_PREVIEW_VTF_LIMIT".into());
	}
	let faces = if flags & 0x4000 == 0 {
		1
	} else if minor < 5 && first_frame != 0xffff {
		7
	} else {
		6
	};
	let resources = if minor >= 3 { word(bytes, 68)? as usize } else { 0 };
	let mut offset = if resources == 0 {
		let thumbnail_width = u32::from(field::<1>(bytes, 61)?[0]);
		let thumbnail_height = u32::from(field::<1>(bytes, 62)?[0]);
		let thumbnail_size = if thumbnail_width == 0 && thumbnail_height == 0 {
			0
		} else {
			if thumbnail_width == 0 || thumbnail_height == 0 || word(bytes, 57)? != 13 {
				return Err(INVALID.into());
			}
			Format::Dxt1.size(thumbnail_width, thumbnail_height)
		};
		header_size as u64 + thumbnail_size
	} else {
		if resources > 32 || 80 + resources * 8 > header_size {
			return Err(INVALID.into());
		}
		let mut image_offset = None;
		for index in 0..resources {
			let entry = 80 + index * 8;
			if field::<3>(bytes, entry)? == [0x30, 0, 0] {
				if field::<1>(bytes, entry + 3)?[0] != 0 || image_offset.is_some() {
					return Err(INVALID.into());
				}
				image_offset = Some(u64::from(word(bytes, entry + 4)?));
			}
		}
		image_offset.ok_or(INVALID)?
	};
	if offset < header_size as u64 || offset > bytes.len() as u64 {
		return Err(INVALID.into());
	}
	let mut first_image = 0;
	for mip in (0..mipmaps).rev() {
		if mip == 0 {
			first_image = offset;
		}
		let size = format
			.size((width >> mip).max(1), (height >> mip).max(1))
			.checked_mul(u64::from((depth >> mip).max(1)))
			.and_then(|size| size.checked_mul(u64::from(frames) * faces))
			.ok_or(INVALID)?;
		offset = offset.checked_add(size).ok_or(INVALID)?;
		if offset > bytes.len() as u64 {
			return Err(INVALID.into());
		}
	}
	let start = first_image as usize;
	let data = &bytes[start..start + format.size(width, height) as usize];
	let rgba = match format {
		Format::Dxt1 | Format::Dxt1Alpha | Format::Dxt3 | Format::Dxt5 => decode_dxt(data, width, height, format, flags & 0x1000 != 0)?,
		_ => decode_pixels(data, format),
	};
	let mut png = Vec::new();
	PngEncoder::new(&mut png)
		.encode(&rgba, width, height, ColorType::Rgba8)
		.map_err(|error| format!("ERR_FILE_PREVIEW_VTF_INVALID:{error}"))?;
	Ok(png)
}

fn decode_dxt(data: &[u8], width: u32, height: u32, format: Format, one_bit_alpha: bool) -> Result<Vec<u8>, String> {
	let variant = match format {
		Format::Dxt3 => DxtVariant::DXT3,
		Format::Dxt5 => DxtVariant::DXT5,
		_ => DxtVariant::DXT1,
	};
	let padded_width = width.max(4);
	let decoder = DxtDecoder::new(data, padded_width, height.max(4), variant).map_err(|_| INVALID)?;
	let channels = decoder.color_type().bytes_per_pixel() as usize;
	let mut decoded = vec![0; decoder.total_bytes() as usize];
	decoder.read_image(&mut decoded).map_err(|_| INVALID)?;
	let transparent = matches!(format, Format::Dxt1Alpha) || one_bit_alpha;
	let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
	for y in 0..height as usize {
		for x in 0..width as usize {
			let pixel = (y * padded_width as usize + x) * channels;
			rgba.extend_from_slice(&decoded[pixel..pixel + 3]);
			let alpha = if channels == 4 {
				decoded[pixel + 3]
			} else if transparent {
				let block = (y / 4 * (padded_width as usize / 4) + x / 4) * 8;
				let color0 = LittleEndian::read_u16(&data[block..]);
				let color1 = LittleEndian::read_u16(&data[block + 2..]);
				let indices = LittleEndian::read_u32(&data[block + 4..]);
				if color0 <= color1 && (indices >> (2 * (y % 4 * 4 + x % 4))) & 3 == 3 {
					0
				} else {
					255
				}
			} else {
				255
			};
			rgba.push(alpha);
		}
	}
	Ok(rgba)
}

fn decode_pixels(data: &[u8], format: Format) -> Vec<u8> {
	let stride = format.bytes_per_pixel();
	let mut rgba = Vec::with_capacity(data.len() / stride * 4);
	for pixel in data.chunks_exact(stride) {
		let color = match format {
			Format::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
			Format::Abgr => [pixel[3], pixel[2], pixel[1], pixel[0]],
			Format::Argb => [pixel[3], pixel[0], pixel[1], pixel[2]],
			Format::Bgra => [pixel[2], pixel[1], pixel[0], pixel[3]],
			Format::Bgrx | Format::Bgr => [pixel[2], pixel[1], pixel[0], 255],
			Format::Rgb => [pixel[0], pixel[1], pixel[2], 255],
			Format::Intensity => [pixel[0], pixel[0], pixel[0], 255],
			Format::IntensityAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
			Format::Alpha => [255, 255, 255, pixel[0]],
			_ => {
				let value = LittleEndian::read_u16(pixel);
				let bits = |shift: u32, mask: u16| (((value >> shift) & mask) * 255 / mask) as u8;
				match format {
					Format::Rgb565 => [bits(0, 31), bits(5, 63), bits(11, 31), 255],
					Format::Bgr565 => [bits(11, 31), bits(5, 63), bits(0, 31), 255],
					Format::Bgra4444 => [bits(8, 15), bits(4, 15), bits(0, 15), bits(12, 15)],
					Format::Bgra5551 => [bits(10, 31), bits(5, 31), bits(0, 31), bits(15, 1)],
					Format::Bgrx5551 => [bits(10, 31), bits(5, 31), bits(0, 31), 255],
					_ => unreachable!(),
				}
			}
		};
		rgba.extend_from_slice(&color);
	}
	rgba
}

#[cfg(test)]
pub(super) mod tests {
	use super::*;

	pub(crate) fn texture(format: u32, width: u16, height: u16, data: &[u8]) -> Vec<u8> {
		let mut bytes = vec![0; 80];
		bytes[..4].copy_from_slice(b"VTF\0");
		LittleEndian::write_u32(&mut bytes[4..], 7);
		LittleEndian::write_u32(&mut bytes[8..], 2);
		LittleEndian::write_u32(&mut bytes[12..], 80);
		LittleEndian::write_u16(&mut bytes[16..], width);
		LittleEndian::write_u16(&mut bytes[18..], height);
		LittleEndian::write_u16(&mut bytes[24..], 1);
		LittleEndian::write_u32(&mut bytes[52..], format);
		bytes[56] = 1;
		LittleEndian::write_u32(&mut bytes[57..], u32::MAX);
		LittleEndian::write_u16(&mut bytes[63..], 1);
		bytes.extend_from_slice(data);
		bytes
	}

	fn pixels(bytes: &[u8]) -> image::RgbaImage {
		image::load_from_memory(&preview(bytes).unwrap()).unwrap().to_rgba8()
	}

	#[test]
	fn preserves_channel_order_and_alpha() {
		for (format, input, expected) in [
			(0, vec![10, 20, 30, 40], [10, 20, 30, 40]),
			(1, vec![40, 30, 20, 10], [10, 20, 30, 40]),
			(2, vec![10, 20, 30], [10, 20, 30, 255]),
			(3, vec![30, 20, 10], [10, 20, 30, 255]),
			(4, vec![31, 0], [255, 0, 0, 255]),
			(5, vec![50], [50, 50, 50, 255]),
			(6, vec![50, 40], [50, 50, 50, 40]),
			(8, vec![40], [255, 255, 255, 40]),
			(11, vec![20, 30, 40, 10], [10, 20, 30, 40]),
			(12, vec![30, 20, 10, 40], [10, 20, 30, 40]),
			(16, vec![30, 20, 10, 0], [10, 20, 30, 255]),
			(17, vec![0, 248], [255, 0, 0, 255]),
			(18, vec![0, 124], [255, 0, 0, 255]),
			(19, vec![0x23, 0x41], [17, 34, 51, 68]),
			(21, vec![0, 124], [255, 0, 0, 0]),
			(21, vec![0, 252], [255, 0, 0, 255]),
		] {
			assert_eq!(pixels(&texture(format, 1, 1, &input)).get_pixel(0, 0).0, expected, "format {format}");
		}
	}

	#[test]
	fn decodes_dxt_blocks_and_small_textures() {
		let red = [0, 248, 0, 0, 0, 0, 0, 0];
		for (format, alpha, expected) in [(13, vec![], 255), (14, vec![0x88; 8], 136), (15, vec![128, 0, 0, 0, 0, 0, 0, 0], 128)] {
			let data = [alpha, red.to_vec()].concat();
			for (width, height) in [(1, 1), (2, 4), (4, 2), (4, 4)] {
				let decoded = pixels(&texture(format, width, height, &data));
				assert_eq!(decoded.dimensions(), (width.into(), height.into()));
				assert!(decoded.pixels().all(|pixel| pixel.0 == [255, 0, 0, expected]));
			}
		}
		let blue = [31, 0, 0, 0, 0, 0, 0, 0];
		let decoded = pixels(&texture(13, 8, 8, &[red, blue, blue, red].concat()));
		assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
		assert_eq!(decoded.get_pixel(4, 0).0, [0, 0, 255, 255]);
		assert_eq!(decoded.get_pixel(0, 4).0, [0, 0, 255, 255]);
		assert_eq!(decoded.get_pixel(4, 4).0, [255, 0, 0, 255]);
	}

	#[test]
	fn preserves_dxt1_transparency_without_turning_opaque_black_transparent() {
		let block = [0, 0, 255, 255, 3, 0, 0, 0];
		for (format, flags, alpha) in [(13, 0, 255), (20, 0, 0), (13, 0x1000, 0)] {
			let mut bytes = texture(format, 4, 4, &block);
			LittleEndian::write_u32(&mut bytes[20..], flags);
			let decoded = pixels(&bytes);
			assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, alpha]);
			assert_eq!(decoded.get_pixel(1, 0).0, [0, 0, 0, 255]);
		}
	}

	#[test]
	fn skips_thumbnail_and_smaller_mips_and_shows_first_frame() {
		let mut data = vec![0; 8];
		data.extend_from_slice(&[0, 0, 255, 255, 0, 255, 0, 255]);
		data.extend_from_slice(&[255, 0, 0, 128].repeat(4));
		data.extend_from_slice(&[0, 255, 0, 255].repeat(4));
		let mut bytes = texture(0, 2, 2, &data);
		LittleEndian::write_u16(&mut bytes[24..], 2);
		LittleEndian::write_u16(&mut bytes[26..], 1);
		bytes[56] = 2;
		LittleEndian::write_u32(&mut bytes[57..], 13);
		bytes[61] = 4;
		bytes[62] = 4;
		for minor in 0..=5 {
			LittleEndian::write_u32(&mut bytes[8..], minor);
			let decoded = pixels(&bytes);
			assert_eq!(decoded.dimensions(), (2, 2));
			assert!(decoded.pixels().all(|pixel| pixel.0 == [255, 0, 0, 128]));
		}
	}

	#[test]
	fn reads_resource_offsets_and_skips_cubemap_faces_and_volume_slices() {
		for (minor, start_frame, faces, depth) in [(3, 0, 7, 1), (4, 65535, 6, 1), (5, 0, 6, 1), (5, 0, 1, 4)] {
			let mut bytes = texture(0, 2, 2, &[]);
			LittleEndian::write_u32(&mut bytes[8..], minor);
			LittleEndian::write_u32(&mut bytes[12..], 96);
			LittleEndian::write_u32(&mut bytes[20..], if faces > 1 { 0x4000 } else { 0 });
			LittleEndian::write_u16(&mut bytes[24..], 2);
			LittleEndian::write_u16(&mut bytes[26..], start_frame);
			LittleEndian::write_u16(&mut bytes[63..], depth);
			LittleEndian::write_u32(&mut bytes[68..], 2);
			bytes[56] = 2;
			bytes.extend_from_slice(&[b'C', b'R', b'C', 2, 1, 2, 3, 4]);
			bytes.extend_from_slice(&[0x30, 0, 0, 0, 104, 0, 0, 0]);
			bytes.resize(104 + 4 * 2 * faces * (depth as usize / 2).max(1), 0);
			bytes.extend_from_slice(&[10, 20, 30, 40].repeat(4));
			bytes.resize(bytes.len() + 16 * (2 * faces * depth as usize - 1), 0);
			assert!(pixels(&bytes).pixels().all(|pixel| pixel.0 == [10, 20, 30, 40]));
			bytes.pop();
			assert_eq!(preview(&bytes).unwrap_err(), INVALID);
		}
	}

	#[test]
	fn rejects_truncation_invalid_headers_and_excessive_sizes() {
		let valid = texture(0, 1, 1, &[10, 20, 30, 40]);
		for length in 0..valid.len() {
			assert!(preview(&valid[..length]).is_err(), "length {length}");
		}
		for (offset, value) in [(0, 0), (12, 63), (12, u32::MAX), (16, 0), (16, 3), (24, 0), (56, 0), (56, 255), (63, 0)] {
			let mut bytes = valid.clone();
			LittleEndian::write_u32(&mut bytes[offset..], value);
			assert_eq!(preview(&bytes).unwrap_err(), INVALID, "offset {offset}");
		}
		let mut bytes = texture(13, 8192, 8192, &[]);
		assert_eq!(preview(&bytes).unwrap_err(), "ERR_FILE_PREVIEW_VTF_LIMIT");
		LittleEndian::write_u32(&mut bytes[8..], 6);
		assert_eq!(preview(&bytes).unwrap_err(), "ERR_FILE_PREVIEW_VTF_VERSION:7.6");
		assert_eq!(preview(&texture(7, 1, 1, &[0])).unwrap_err(), "ERR_FILE_PREVIEW_VTF_FORMAT:7");
	}

	#[test]
	fn rejects_missing_duplicate_inline_and_out_of_bounds_image_resources() {
		let mut valid = texture(0, 1, 1, &[]);
		LittleEndian::write_u32(&mut valid[8..], 5);
		LittleEndian::write_u32(&mut valid[12..], 88);
		LittleEndian::write_u32(&mut valid[68..], 1);
		valid.extend_from_slice(&[0x30, 0, 0, 0, 88, 0, 0, 0, 10, 20, 30, 40]);
		assert_eq!(pixels(&valid).get_pixel(0, 0).0, [10, 20, 30, 40]);
		for (offset, value) in [(68, 33), (68, 2), (80, 0x01), (80, 0x02000030), (84, 80), (84, u32::MAX)] {
			let mut bytes = valid.clone();
			LittleEndian::write_u32(&mut bytes[offset..], value);
			assert_eq!(preview(&bytes).unwrap_err(), INVALID, "offset {offset}");
		}
		let mut duplicate = valid;
		LittleEndian::write_u32(&mut duplicate[12..], 96);
		LittleEndian::write_u32(&mut duplicate[68..], 2);
		duplicate.splice(88..88, [0x30, 0, 0, 0, 96, 0, 0, 0]);
		assert_eq!(preview(&duplicate).unwrap_err(), INVALID);
	}
}
