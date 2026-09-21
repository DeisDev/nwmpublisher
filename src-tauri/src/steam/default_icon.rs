use image::{imageops, imageops::FilterType, Rgba, RgbaImage};
use steamworks::SteamId;

/// The preview size Steam recommends for Workshop items
const ICON_SIZE: u32 = 512;

/// Used when Steam can't give us the user's avatar
const FALLBACK_AVATAR: &[u8] = include_bytes!("../../../public/img/steam_anonymous.jpg");

/// Creates the default Workshop preview image from the user's square Steam avatar.
pub fn compose() -> RgbaImage {
	compose_with(avatar())
}

fn compose_with(avatar: Option<RgbaImage>) -> RgbaImage {
	let avatar = avatar.unwrap_or_else(fallback_avatar);

	imageops::resize(&avatar, ICON_SIZE, ICON_SIZE, FilterType::CatmullRom)
}

/// The current user's Steam avatar, preferring the largest one Steam offers
fn avatar() -> Option<RgbaImage> {
	if !steam!().connected() {
		return None;
	}

	let steam_id = steam!().client().steam_id;

	if let Some(avatar) = avatar_of(steam_id) {
		return Some(avatar);
	}

	// Steam may still be downloading the avatar, so request it and give it a moment
	if steam!().client().friends().request_user_information(steam_id, false) {
		for _ in 0..10 {
			sleep_ms!(100);
			if let Some(avatar) = avatar_of(steam_id) {
				return Some(avatar);
			}
		}
	}

	None
}

fn avatar_of(steam_id: SteamId) -> Option<RgbaImage> {
	let friend = steam!().client().friends().get_friend(steam_id);

	// These come back as raw RGBA buffers, at the sizes the Steam API documents
	friend
		.large_avatar()
		.map(|buffer| (buffer, 184))
		.or_else(|| friend.medium_avatar().map(|buffer| (buffer, 64)))
		.or_else(|| friend.small_avatar().map(|buffer| (buffer, 32)))
		.and_then(|(buffer, size)| RgbaImage::from_raw(size, size, buffer))
}

fn fallback_avatar() -> RgbaImage {
	image::load_from_memory_with_format(FALLBACK_AVATAR, image::ImageFormat::Jpeg)
		.map(|avatar| avatar.to_rgba8())
		.unwrap_or_else(|_| RgbaImage::from_pixel(64, 64, Rgba([45, 45, 45, 255])))
}

#[tauri::command]
pub fn default_workshop_icon() -> Option<crate::Base64Image> {
	Some(crate::Base64Image::new(compose().into_raw(), ICON_SIZE, ICON_SIZE))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fills_the_square_with_the_avatar() {
		let color = Rgba([60, 100, 140, 255]);
		let icon = compose_with(Some(RgbaImage::from_pixel(184, 184, color)));

		assert_eq!(icon.dimensions(), (ICON_SIZE, ICON_SIZE));
		assert!(icon.pixels().all(|pixel| *pixel == color));
	}
}
