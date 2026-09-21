use super::publishing::PublishError;
use image::{imageops, imageops::FilterType, RgbaImage};
use std::path::Path;
use steamworks::SteamId;

/// The preview size Steam recommends for Workshop items
const ICON_SIZE: u32 = 512;

/// Used when Steam can't give us the user's avatar
const FALLBACK_AVATAR: &[u8] = include_bytes!("../../../public/img/steam_anonymous.jpg");

/// Creates the default Workshop preview image from the user's square Steam avatar.
pub fn compose() -> Result<RgbaImage, PublishError> {
	compose_with(avatar()?)
}

fn compose_with(avatar: Option<RgbaImage>) -> Result<RgbaImage, PublishError> {
	let avatar = match avatar {
		Some(avatar) => avatar,
		None => fallback_avatar()?,
	};
	Ok(imageops::resize(&avatar, ICON_SIZE, ICON_SIZE, FilterType::CatmullRom))
}

/// The current user's Steam avatar, preferring the largest one Steam offers
fn avatar() -> Result<Option<RgbaImage>, PublishError> {
	if !steam!().connected() {
		return Ok(None);
	}

	let steam_id = steam!().client().steam_id;

	if let Some(avatar) = avatar_of(steam_id)? {
		return Ok(Some(avatar));
	}

	// Steam may still be downloading the avatar, so request it and give it a moment
	if steam!().client().friends().request_user_information(steam_id, false) {
		for _ in 0..10 {
			sleep_ms!(100);
			if let Some(avatar) = avatar_of(steam_id)? {
				return Ok(Some(avatar));
			}
		}
	}

	Ok(None)
}

fn avatar_of(steam_id: SteamId) -> Result<Option<RgbaImage>, PublishError> {
	let friend = steam!().client().friends().get_friend(steam_id);

	// These come back as raw RGBA buffers, at the sizes the Steam API documents
	friend
		.large_avatar()
		.map(|buffer| (buffer, 184))
		.or_else(|| friend.medium_avatar().map(|buffer| (buffer, 64)))
		.or_else(|| friend.small_avatar().map(|buffer| (buffer, 32)))
		.map(|(buffer, size)| {
			RgbaImage::from_raw(size, size, buffer).ok_or_else(|| {
				PublishError::io(
					"decode avatar",
					Path::new("Steam avatar"),
					std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected RGBA buffer size"),
				)
			})
		})
		.transpose()
}

fn fallback_avatar() -> Result<RgbaImage, PublishError> {
	image::load_from_memory_with_format(FALLBACK_AVATAR, image::ImageFormat::Jpeg)
		.map(|avatar| avatar.to_rgba8())
		.map_err(|error| PublishError::image("decode bundled avatar", Path::new("public/img/steam_anonymous.jpg"), error))
}

#[tauri::command]
pub fn default_workshop_icon() -> Result<String, PublishError> {
	let image = image::DynamicImage::ImageRgba8(compose()?);
	let mut bytes = Vec::new();
	image
		.write_to(&mut bytes, image::ImageFormat::Png)
		.map_err(|error| PublishError::image("encode preview", Path::new("default_workshop_icon.png"), error))?;
	Ok(base64::encode(bytes))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fills_the_square_with_the_avatar() {
		let color = image::Rgba([60, 100, 140, 255]);
		let icon = compose_with(Some(RgbaImage::from_pixel(184, 184, color))).unwrap();

		assert_eq!(icon.dimensions(), (ICON_SIZE, ICON_SIZE));
		assert!(icon.pixels().all(|pixel| *pixel == color));
	}
}
