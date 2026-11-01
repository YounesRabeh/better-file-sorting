//! Read only bounded CFB allocation/directory metadata; never extract streams.
use super::classifier::{Confidence, Detection, Format};
use std::{
    collections::BTreeSet,
    io::{Read, Seek, SeekFrom},
};
fn u16le(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(p..p.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32le(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(p..p.checked_add(4)?)?.try_into().ok()?,
    ))
}
pub fn detect<R: Read + Seek>(reader: &mut R) -> Option<Detection> {
    let length = reader.seek(SeekFrom::End(0)).ok()?;
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut header = [0; 512];
    reader.read_exact(&mut header).ok()?;
    if &header[..8] != b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"
        || u16le(&header, 28)? != 0xfffe
        || u16le(&header, 32)? != 6
        || header[34..40] != [0; 6]
    {
        return None;
    }
    let sector = match (u16le(&header, 26)?, u16le(&header, 30)?) {
        (3, 9) => 512u64,
        (4, 12) => 4096,
        _ => return None,
    };
    if u32le(&header, 72)? != 0 {
        return None;
    } // External DIFAT chains conservatively remain generic CFB.
    let count = u32le(&header, 44)? as usize;
    if count == 0 || count > 109 {
        return None;
    }
    let mut read_bytes = 512usize;
    let mut visited = BTreeSet::new();
    let mut read_sector = |id: u32| -> Option<Vec<u8>> {
        if id >= 0xfffffffa || !visited.insert(id) || visited.len() > 4096 {
            return None;
        }
        let offset = (id as u64 + 1).checked_mul(sector)?;
        if offset.checked_add(sector)? > length {
            return None;
        }
        read_bytes = read_bytes.checked_add(sector as usize)?;
        if read_bytes > 1024 * 1024 {
            return None;
        }
        reader.seek(SeekFrom::Start(offset)).ok()?;
        let mut data = vec![0; sector as usize];
        reader.read_exact(&mut data).ok()?;
        Some(data)
    };
    let mut fat = vec![];
    for i in 0..count {
        let data = read_sector(u32le(&header, 76 + i * 4)?)?;
        for chunk in data.as_chunks::<4>().0 {
            fat.push(u32::from_le_bytes(*chunk));
        }
    }
    let mut id = u32le(&header, 48)?;
    let mut names = BTreeSet::new();
    let mut msi = false;
    let mut entries = 0;
    while id != 0xfffffffe {
        let data = read_sector(id)?;
        for entry in data.as_chunks::<128>().0 {
            entries += 1;
            if entries > 4096 {
                return None;
            }
            if entry[66] == 0 {
                continue;
            }
            let n = u16le(entry, 64)? as usize;
            if !(2..=64).contains(&n)
                || !n.is_multiple_of(2)
                || entry[n - 2..n] != [0; 2]
                || !matches!(entry[66], 1 | 2 | 5)
            {
                return None;
            }
            let words: Vec<_> = entry[..n - 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            let name = String::from_utf16(&words).ok()?;
            if entry[66] == 2 {
                names.insert(name);
            }
            if entry[66] == 5
                && entry[80..96]
                    == [
                        0x84, 0x10, 0x0c, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46,
                    ]
            {
                msi = true;
            }
        }
        id = *fat.get(id as usize)?;
    }
    if names.contains("EncryptedPackage") || names.contains("EncryptionInfo") {
        return None;
    }
    let mut formats = vec![];
    if names.contains("WordDocument") && (names.contains("0Table") || names.contains("1Table")) {
        formats.push(Format::Doc);
    }
    if names.contains("Workbook") || names.contains("Book") {
        formats.push(Format::Xls);
    }
    if names.contains("PowerPoint Document") {
        formats.push(Format::Ppt);
    }
    if msi && !names.is_empty() {
        formats.push(Format::Msi);
    }
    if formats.is_empty() {
        return None;
    }
    let mut d = Detection::new(
        if formats.len() == 1 {
            formats[0]
        } else {
            Format::Ambiguous
        },
        Confidence::Medium,
        "bounded_cfb_directory",
    );
    if formats.len() > 1 {
        d.alternatives = formats;
    }
    Some(d)
}
