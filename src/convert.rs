pub fn extension_for(bytes: &[u8], asset_type_id: i64) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "png";
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return "jpg";
    }
    if bytes.starts_with(b"OggS") {
        return "ogg";
    }
    if bytes.starts_with(b"ID3") || bytes.starts_with(b"\xff\xfb") || bytes.starts_with(b"\xff\xf3") {
        return "mp3";
    }
    if bytes.starts_with(b"RIFF") {
        return "wav";
    }
    if bytes.starts_with(b"<roblox!") {
        return "rbxm";
    }
    if bytes.starts_with(b"version ") {
        return "mesh";
    }
    if bytes.starts_with(b"<roblox") || bytes.starts_with(b"<?xml") {
        match asset_type_id {
            1 | 13 | 18 => "png",
            _ => "rbxmx",
        }
    } else {
        match asset_type_id {
            1 | 13 | 18 => "png",
            3 => "ogg",
            4 | 40 => "mesh",
            5 => "lua",
            _ => "bin",
        }
    }
}

/// Decals and images are often an XML descriptor that points at the real image
/// asset id. Returns that inner id so the resolver can follow it.
pub fn inner_asset_id(bytes: &[u8]) -> Option<i64> {
    if !(bytes.starts_with(b"<roblox") || bytes.starts_with(b"<?xml")) {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    for marker in ["rbxassetid://", "asset/?id=", "asset/?ID=", "id="] {
        if let Some(pos) = text.find(marker) {
            let rest = &text[pos + marker.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<i64>() {
                if n > 0 {
                    return Some(n);
                }
            }
        }
    }
    None
}

pub fn type_label(asset_type_id: i64) -> &'static str {
    match asset_type_id {
        1 => "Image",
        3 => "Audio",
        4 => "Mesh",
        5 => "Lua",
        8 => "Hat",
        10 => "Model",
        13 => "Decal",
        18 => "Face",
        19 => "Gear",
        24 => "Animation",
        32 => "Package",
        38 => "Plugin",
        40 => "MeshPart",
        11 | 12 => "Clothing",
        _ => "Asset",
    }
}
