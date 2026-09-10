//! 内置图标清单 + 自定义图标裁剪/压缩。

use crate::error::{AppError, Result};
use crate::store;
use crate::vault::Vault;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinIconInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub glyph: String,
}

pub fn list_builtin() -> Vec<BuiltinIconInfo> {
    const ITEMS: &[(&str, &str, &str, &str)] = &[
        ("github", "GitHub", "#24292e", "GH"),
        ("gitlab", "GitLab", "#fc6d26", "🦊"),
        ("gitee", "Gitee", "#c71d23", "码"),
        ("google", "Google", "#4285f4", "G"),
        ("microsoft", "Microsoft", "#00a4ef", "MS"),
        ("apple", "Apple", "#111111", "🍎"),
        ("aws", "AWS", "#ff9900", "AWS"),
        ("cloudflare", "Cloudflare", "#f38020", "CF"),
        ("openai", "OpenAI", "#10a37f", "AI"),
        ("vercel", "Vercel", "#000000", "▲"),
        ("docker", "Docker", "#2496ed", "🐳"),
        ("linux", "Linux", "#fcc624", "🐧"),
        ("npm", "NPM", "#cb3837", "npm"),
        ("telegram", "Telegram", "#229ed9", "TG"),
        ("discord", "Discord", "#5865f2", "DC"),
        ("twitter", "Twitter/X", "#111111", "𝕏"),
        ("aliyun", "阿里云", "#ff6a00", "阿里"),
        ("tencentcloud", "腾讯云", "#0052d9", "腾讯"),
        ("slack", "Slack", "#4a154b", "#"),
        ("notion", "Notion", "#111111", "N"),
        ("bitbucket", "Bitbucket", "#0052cc", "BB"),
        ("azure", "Azure", "#0078d4", "Az"),
        ("digitalocean", "DigitalOcean", "#0080ff", "DO"),
        ("cloudflare-r2", "R2", "#f38020", "R2"),
        ("figma", "Figma", "#a259ff", "Fg"),
        ("dropbox", "Dropbox", "#0061ff", "Db"),
        ("proton", "Proton", "#6d4aff", "P"),
        ("1password", "1Password", "#0094f5", "1P"),
        ("bitwarden", "Bitwarden", "#175ddc", "BW"),
        ("ssh", "SSH", "#0f172a", "SSH"),
        ("email", "邮箱", "#0ea5e9", "@"),
        ("generic", "通用", "#6366f1", "•"),
    ];
    ITEMS
        .iter()
        .map(|(id, name, color, glyph)| BuiltinIconInfo {
            id: (*id).into(),
            name: (*name).into(),
            color: (*color).into(),
            glyph: (*glyph).into(),
        })
        .collect()
}

pub fn suggest_builtin(name: &str) -> Option<String> {
    let q = name.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    list_builtin()
        .into_iter()
        .find(|i| q.contains(&i.id) || i.name.to_ascii_lowercase().contains(&q) || q.contains(&i.name.to_ascii_lowercase()))
        .map(|i| format!("builtin:{}", i.id))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomIconInfo {
    pub icon_ref: String,
    pub data_url: String,
    pub bytes: usize,
}

pub fn process_and_store(vault: &Vault, bytes: &[u8]) -> Result<CustomIconInfo> {
    if bytes.len() > 12 * 1024 * 1024 {
        return Err(AppError::Invalid("图片超过 12MB".into()));
    }
    let img = image::load_from_memory(bytes)
        .or_else(|_| detect_and_load(bytes))
        .map_err(|_| AppError::Invalid("无法解码图片，请改用 PNG / JPG / WEBP / ICO".into()))?;
    let webp = crop_square_and_encode(&img)?;
    let hash = hex_sha256(&webp);
    store::save_icon(vault, &hash, &webp)?;
    Ok(CustomIconInfo {
        icon_ref: format!("custom:{hash}"),
        data_url: data_url(&webp),
        bytes: webp.len(),
    })
}

fn detect_and_load(bytes: &[u8]) -> image::ImageResult<DynamicImage> {
    let format = image::guess_format(bytes)?;
    let cursor = std::io::Cursor::new(bytes);
    let reader = image::ImageReader::with_format(cursor, format);
    reader.decode()
}

fn crop_square_and_encode(img: &DynamicImage) -> Result<Vec<u8>> {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(AppError::Invalid("图片尺寸无效".into()));
    }
    let side = w.min(h);
    let x = (w - side) / 2;
    let y = (h - side) / 2;
    let cropped = img.crop_imm(x, y, side, side);
    let resized = cropped.resize_exact(128, 128, FilterType::Lanczos3);
    let rgba = resized.to_rgba8();
    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
    encoder
        .encode(rgba.as_raw(), 128, 128, ExtendedColorType::Rgba8)
        .map_err(|_| AppError::Invalid("WebP 编码失败".into()))?;
    Ok(out)
}

pub fn data_url(webp: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:image/webp;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(webp)
    )
}

fn hex_sha256(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

pub fn parse_custom_hash(icon_ref: &str) -> Option<&str> {
    icon_ref.strip_prefix("custom:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_square_from_rect() {
        let img = DynamicImage::new_rgba8(200, 100);
        let webp = crop_square_and_encode(&img).unwrap();
        assert!(webp.len() > 10);
        assert!(webp.len() < 20_000);
    }

    #[test]
    fn suggest_github() {
        assert_eq!(suggest_builtin("GitHub").as_deref(), Some("builtin:github"));
    }
}
