//! Bounded structural checks for additional formats; no payload decompression.
use super::classifier::Format;
fn le(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(p..p.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn be(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(p..p.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn short(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        b.get(p..p.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn java(b: &[u8]) -> Option<()> {
    if b.get(..4)? != b"\xca\xfe\xba\xbe" || !(45..=70).contains(&short(b, 6)?) {
        return None;
    }
    let count = short(b, 8)? as usize;
    if !(2..=4096).contains(&count) {
        return None;
    }
    let mut tags = vec![0; count];
    let mut class_refs = vec![];
    let mut pos = 10usize;
    let mut i = 1;
    while i < count {
        let tag = *b.get(pos)?;
        tags[i] = tag;
        pos += 1;
        let size = match tag {
            1 => {
                let n = short(b, pos)? as usize;
                2 + n
            }
            3 | 4 | 9..=12 | 17 | 18 => 4,
            5 | 6 => {
                i += 1;
                if i >= count {
                    return None;
                }
                8
            }
            7 => {
                class_refs.push(short(b, pos)? as usize);
                2
            }
            8 | 16 | 19 | 20 => 2,
            15 => 3,
            _ => return None,
        };
        pos = pos.checked_add(size)?;
        b.get(..pos)?;
        i += 1;
    }
    if class_refs.into_iter().any(|n| tags.get(n) != Some(&1)) {
        return None;
    }
    let this = short(b, pos + 2)? as usize;
    let parent = short(b, pos + 4)? as usize;
    if tags.get(this) != Some(&7) || (parent != 0 && tags.get(parent) != Some(&7)) {
        return None;
    }
    Some(())
}
fn fat_macho(b: &[u8], length: Option<u64>) -> Option<()> {
    let magic = b.get(..4)?;
    let wide = magic == b"\xca\xfe\xba\xbf";
    if magic != b"\xca\xfe\xba\xbe" && !wide {
        return None;
    }
    let count = be(b, 4)? as usize;
    if !(1..=64).contains(&count) {
        return None;
    }
    let size = if wide { 32 } else { 20 };
    let end = 8 + size * count;
    b.get(..end)?;
    for i in 0..count {
        let p = 8 + size * i;
        let (offset, bytes, align) = if wide {
            (
                u64::from_be_bytes(b.get(p + 8..p + 16)?.try_into().ok()?),
                u64::from_be_bytes(b.get(p + 16..p + 24)?.try_into().ok()?),
                be(b, p + 24)?,
            )
        } else {
            (be(b, p + 8)? as u64, be(b, p + 12)? as u64, be(b, p + 16)?)
        };
        if be(b, p)? == 0
            || offset < end as u64
            || bytes < 28
            || align > 31
            || offset % (1u64 << align) != 0
            || length.is_some_and(|n| offset.checked_add(bytes).is_none_or(|end| end > n))
        {
            return None;
        }
    }
    Some(())
}
// LZ4 frame descriptors are at most 14 bytes: the short-input xxHash32 path suffices.
fn xxh32_short(b: &[u8]) -> u32 {
    let mut h = 0x165667b1u32.wrapping_add(b.len() as u32);
    let mut p = 0;
    while p + 4 <= b.len() {
        h = h
            .wrapping_add(le(b, p).unwrap().wrapping_mul(0xc2b2ae3d))
            .rotate_left(17)
            .wrapping_mul(0x27d4eb2f);
        p += 4;
    }
    for x in &b[p..] {
        h = h
            .wrapping_add((*x as u32).wrapping_mul(0x165667b1))
            .rotate_left(11)
            .wrapping_mul(0x9e3779b1);
    }
    h ^= h >> 15;
    h = h.wrapping_mul(0x85ebca77);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae3d);
    h ^ (h >> 16)
}
fn cabinet(b: &[u8], length: Option<u64>) -> Option<()> {
    if b.get(..4)? != b"MSCF"
        || b.len() < 36
        || [4, 12, 20].into_iter().any(|p| le(b, p) != Some(0))
        || b[24] != 3
        || b[25] != 1
    {
        return None;
    }
    let size = le(b, 8)? as usize;
    let files = le(b, 16)? as usize;
    let folders = u16::from_le_bytes([b[26], b[27]]) as usize;
    let file_count = u16::from_le_bytes([b[28], b[29]]) as usize;
    let flags = u16::from_le_bytes([b[30], b[31]]);
    if folders == 0 || flags & !7 != 0 || length.is_some_and(|n| n != size as u64) {
        return None;
    }
    let mut p = 36usize;
    let mut folder_reserved = 0usize;
    if flags & 4 != 0 {
        let raw = b.get(p..p + 4)?;
        let n = u16::from_le_bytes([raw[0], raw[1]]) as usize;
        folder_reserved = raw[2] as usize;
        p = p.checked_add(4 + n)?;
        b.get(..p)?;
    }
    for flag in [1, 2] {
        if flags & flag != 0 {
            for _ in 0..2 {
                let n = b.get(p..)?.iter().position(|x| *x == 0)?;
                p = p.checked_add(n + 1)?;
            }
        }
    }
    let folder_end = p.checked_add(folders.checked_mul(8 + folder_reserved)?)?;
    if folder_end > files || files > size {
        return None;
    }
    b.get(..folder_end)?;
    for i in 0..folders {
        let start = p + i * (8 + folder_reserved);
        let data = le(b, start)? as usize;
        let blocks = u16::from_le_bytes(b.get(start + 4..start + 6)?.try_into().ok()?);
        if blocks > 0 && (data < folder_end || data >= size) {
            return None;
        }
    }
    p = files;
    for _ in 0..file_count {
        let header = b.get(p..p.checked_add(16)?)?;
        let folder = u16::from_le_bytes([header[8], header[9]]) as usize;
        if folder < 0xfffd && folder >= folders {
            return None;
        }
        p += 16;
        let name = b.get(p..)?.iter().position(|x| *x == 0)?;
        p = p.checked_add(name + 1)?;
        if p > size {
            return None;
        }
    }
    Some(())
}
pub fn candidates(b: &[u8], length: Option<u64>) -> Vec<Format> {
    use Format::*;
    let mut found = vec![];
    if b.starts_with(b"dex\n")
        && b.len() >= 112
        && matches!(
            &b[4..8],
            b"035\0" | b"037\0" | b"038\0" | b"039\0" | b"040\0"
        )
        && le(b, 36) == Some(112)
        && le(b, 40) == Some(0x12345678)
    {
        let size = le(b, 32).unwrap() as u64;
        let mut valid = size >= 112 && length.is_none_or(|n| n == size);
        for (p, width) in [
            (56, 4u64),
            (64, 4),
            (72, 12),
            (80, 8),
            (88, 8),
            (96, 32),
            (104, 1),
        ] {
            let count = le(b, p).unwrap() as u64;
            let off = le(b, p + 4).unwrap() as u64;
            valid &= if count == 0 {
                off == 0
            } else {
                off >= 112
                    && off
                        .checked_add(count * width)
                        .is_some_and(|end| end <= size)
            };
        }
        if valid {
            found.push(Dex);
        }
    }
    if java(b).is_some() {
        found.push(JavaClass);
    }
    if fat_macho(b, length).is_some() {
        found.push(Macho);
    }
    if b.len() >= 76
        && le(b, 0) == Some(76)
        && b[4..20] == [1, 0x14, 2, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]
        && b[66..76] == [0; 10]
        && le(b, 60).is_some_and(|n| matches!(n, 1 | 3 | 7))
    {
        found.push(Lnk);
    }
    if cabinet(b, length).is_some() {
        found.push(Cab);
    }
    if b.starts_with(b"\x28\xb5\x2f\xfd") && b.len() >= 6 {
        let f = b[4];
        let single = f & 32 != 0;
        let dict = [0, 1, 2, 4][(f & 3) as usize];
        let content = match f >> 6 {
            0 => usize::from(single),
            1 => 2,
            2 => 4,
            _ => 8,
        };
        let end = 5 + usize::from(!single) + dict + content;
        if f & 0x18 == 0 && b.len() >= end + 3 {
            let header = (b[end] as u32) | ((b[end + 1] as u32) << 8) | ((b[end + 2] as u32) << 16);
            if (header >> 1) & 3 != 3 {
                found.push(Zstd);
            }
        }
    }
    if b.starts_with(b"\x04\x22\x4d\x18") && b.len() >= 7 {
        let f = b[4];
        let end = 6 + if f & 8 != 0 { 8 } else { 0 } + if f & 1 != 0 { 4 } else { 0 };
        if f >> 6 == 1
            && f & 2 == 0
            && b[5] & 0x8f == 0
            && (4..=7).contains(&(b[5] >> 4))
            && b.get(end)
                .is_some_and(|x| *x == (xxh32_short(&b[4..end]) >> 8) as u8)
        {
            found.push(Lz4);
        }
    }
    if b.starts_with(b"!<arch>\n") && b.len() >= 68 && b.get(66..68) == Some(b"`\n") {
        let numeric = |p, q, radix| {
            std::str::from_utf8(&b[p..q])
                .ok()
                .and_then(|s| u64::from_str_radix(s.trim(), radix).ok())
        };
        if numeric(24, 36, 10).is_some()
            && numeric(36, 42, 10).is_some()
            && numeric(42, 48, 10).is_some()
            && numeric(48, 56, 8).is_some()
            && numeric(56, 66, 10)
                .is_some_and(|n| length.is_none_or(|size| n <= size.saturating_sub(68)))
        {
            found.push(Ar);
        }
    }
    if b.len() >= 8 && matches!(&b[..3], b"FWS" | b"CWS" | b"ZWS") && (1..=50).contains(&b[3]) {
        let size = le(b, 4).unwrap() as u64;
        let valid = match &b[..3] {
            b"FWS" => size >= 8 && length.is_none_or(|n| n == size),
            b"CWS" => {
                b[3] >= 6
                    && size >= 8
                    && b.len() >= 10
                    && b[8] & 15 == 8
                    && ((b[8] as u16) * 256 + b[9] as u16).is_multiple_of(31)
            }
            _ => {
                b[3] >= 13
                    && size >= 8
                    && b.len() >= 17
                    && le(b, 8).is_some_and(|n| n >= 5)
                    && b[12] < 225
            }
        };
        if valid {
            found.push(Swf);
        }
    }
    found
}
