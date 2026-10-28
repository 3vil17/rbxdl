struct Mesh {
    pos: Vec<[f32; 3]>,
    norm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    faces: Vec<[u32; 3]>,
}

pub fn to_obj(bytes: &[u8]) -> Option<String> {
    let nl = bytes.iter().position(|&b| b == b'\n')?;
    let header = std::str::from_utf8(&bytes[..nl]).ok()?.trim();
    let version = header.strip_prefix("version ")?;
    let body = &bytes[nl + 1..];
    let mesh = match version {
        "1.00" | "1.01" => text_mesh(body, version == "1.00")?,
        "2.00" => binary_mesh(body, 2)?,
        v if v.starts_with("3.") => binary_mesh(body, 3)?,
        v if v.starts_with("4.") || v.starts_with("5.") => binary_mesh(body, 4)?,
        _ => return None,
    };
    Some(write_obj(&mesh))
}

// Old text meshes: a face count, then 9 bracketed vectors per face
// (position, normal, uv for each corner). 1.00 positions are at half scale.
fn text_mesh(body: &[u8], half: bool) -> Option<Mesh> {
    let text = std::str::from_utf8(body).ok()?;
    let nums: Vec<f32> = text
        .split(|c: char| c == '[' || c == ']' || c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    let faces = *nums.first()? as usize;
    let data = &nums[1..];
    if data.len() < faces * 27 {
        return None;
    }
    let scale = if half { 0.5 } else { 1.0 };
    let mut m = Mesh { pos: vec![], norm: vec![], uv: vec![], faces: vec![] };
    for f in 0..faces {
        for c in 0..3 {
            let v = &data[f * 27 + c * 9..];
            m.pos.push([v[0] * scale, v[1] * scale, v[2] * scale]);
            m.norm.push([v[3], v[4], v[5]]);
            m.uv.push([v[6], 1.0 - v[7]]);
        }
        let b = (f * 3) as u32;
        m.faces.push([b, b + 1, b + 2]);
    }
    Some(m)
}

fn u16_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

// Binary meshes 2.00 to 5.xx. Only the vertex, face and LOD tables are read,
// skinning and FACS data is skipped. LOD 0 is the full detail face range.
fn binary_mesh(b: &[u8], kind: u8) -> Option<Mesh> {
    let header = u16_at(b, 0)?;
    let (vsize, fsize, nverts, nfaces, nlods, nbones) = match kind {
        2 => (*b.get(2)? as usize, *b.get(3)? as usize, u32_at(b, 4)?, u32_at(b, 8)?, 0, 0),
        3 => (*b.get(2)? as usize, *b.get(3)? as usize, u32_at(b, 8)?, u32_at(b, 12)?, u16_at(b, 6)?, 0),
        _ => (40, 12, u32_at(b, 4)?, u32_at(b, 8)?, u16_at(b, 12)?, u16_at(b, 14)?),
    };
    let mut o = header;
    let mut m = Mesh { pos: vec![], norm: vec![], uv: vec![], faces: vec![] };
    for i in 0..nverts {
        let v = o + i * vsize;
        m.pos.push([f32_at(b, v)?, f32_at(b, v + 4)?, f32_at(b, v + 8)?]);
        m.norm.push([f32_at(b, v + 12)?, f32_at(b, v + 16)?, f32_at(b, v + 20)?]);
        m.uv.push([f32_at(b, v + 24)?, 1.0 - f32_at(b, v + 28)?]);
    }
    o += nverts * vsize;
    if nbones > 0 {
        o += nverts * 8;
    }
    let faces_at = o;
    o += nfaces * fsize;
    let mut end = nfaces;
    if nlods >= 2 {
        end = u32_at(b, o + 4)?.min(nfaces);
    }
    for f in 0..end {
        let at = faces_at + f * fsize;
        let a = u32_at(b, at)? as u32;
        let c = u32_at(b, at + 4)? as u32;
        let d = u32_at(b, at + 8)? as u32;
        if a as usize >= nverts || c as usize >= nverts || d as usize >= nverts {
            return None;
        }
        m.faces.push([a, c, d]);
    }
    Some(m)
}

fn write_obj(m: &Mesh) -> String {
    use std::fmt::Write;
    let mut out = String::from("# converted by rbxyoinker\n");
    for p in &m.pos {
        let _ = writeln!(out, "v {} {} {}", p[0], p[1], p[2]);
    }
    for n in &m.norm {
        let _ = writeln!(out, "vn {} {} {}", n[0], n[1], n[2]);
    }
    for t in &m.uv {
        let _ = writeln!(out, "vt {} {}", t[0], t[1]);
    }
    for f in &m.faces {
        let [a, b, c] = [f[0] + 1, f[1] + 1, f[2] + 1];
        let _ = writeln!(out, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
    }
    out
}
