use std::io;
use std::path::Path;

#[cfg_attr(not(test), allow(dead_code))]
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    std::fs::write(path, encode_png(width, height, rgba))
}

/// The whole PNG as bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut file: Vec<u8> = Vec::new();
    file.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut file, b"IHDR", &ihdr);
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in 0..height as usize {
        raw.push(0);
        let start = row * width as usize * 4;
        raw.extend_from_slice(&rgba[start..start + width as usize * 4]);
    }
    chunk(&mut file, b"IDAT", &zlib_store(&raw));
    chunk(&mut file, b"IEND", &[]);
    file
}

fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend_from_slice(&(data.len() as u32).to_be_bytes());
    file.extend_from_slice(kind);
    file.extend_from_slice(data);
    let mut signed = kind.to_vec();
    signed.extend_from_slice(data);
    file.extend_from_slice(&crc32(&signed).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut rest = data;
    while !rest.is_empty() {
        let size = rest.len().min(65535);
        let last = size == rest.len();
        out.push(if last { 1 } else { 0 });
        out.extend((size as u16).to_le_bytes());
        out.extend((!(size as u16)).to_le_bytes());
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size..];
    }
    out.extend(adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
