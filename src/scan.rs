const PATTERNS: [&[u8]; 3] = [b"rbxassetid://", b"asset/?id=", b"assetid="];

pub fn asset_ids(data: &[u8]) -> Vec<i64> {
    let text = if data.starts_with(b"<roblox!") { binary_strings(data) } else { data.to_vec() };
    find_ids(&text.to_ascii_lowercase())
}

// Binary places are a 32 byte header followed by chunks, each LZ4 or zstd
// compressed. Content ids live in the PROP chunks as plain strings.
fn binary_strings(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 32;
    while pos + 16 <= data.len() {
        let name = &data[pos..pos + 4];
        let clen = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        let ulen = u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]) as usize;
        pos += 16;
        let take = if clen == 0 { ulen } else { clen };
        if pos + take > data.len() || name == b"END\0" {
            break;
        }
        let raw = &data[pos..pos + take];
        pos += take;
        if name != b"PROP" {
            continue;
        }
        let chunk = if clen == 0 {
            raw.to_vec()
        } else if raw.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
            zstd::stream::decode_all(raw).unwrap_or_default()
        } else {
            lz4_flex::block::decompress(raw, ulen).unwrap_or_default()
        };
        out.extend_from_slice(&chunk);
        out.push(b'\n');
    }
    out
}

fn find_ids(text: &[u8]) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for pat in PATTERNS {
        let mut i = 0;
        while i + pat.len() <= text.len() {
            if &text[i..i + pat.len()] != pat {
                i += 1;
                continue;
            }
            i += pat.len();
            let start = i;
            while i < text.len() && text[i].is_ascii_digit() {
                i += 1;
            }
            let id = std::str::from_utf8(&text[start..i]).ok().and_then(|s| s.parse::<i64>().ok());
            if let Some(id) = id.filter(|n| *n > 0) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    out
}
