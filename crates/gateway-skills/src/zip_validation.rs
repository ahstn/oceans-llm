//! Structural ZIP checks that must run before zip-rs, which merges duplicate names.

use std::collections::BTreeSet;

use crate::{BundleError, BundleLimits};

pub(crate) fn preflight(bytes: &[u8], limits: &BundleLimits) -> Result<(), BundleError> {
    let end = (bytes.len().saturating_sub(65557)..bytes.len().saturating_sub(21))
        .rev()
        .find(|&offset| {
            bytes.get(offset..offset + 4) == Some(b"PK\x05\x06")
                && u16_at(bytes, offset + 20)
                    .is_ok_and(|length| offset + 22 + usize::from(length) == bytes.len())
        })
        .ok_or_else(|| invalid("missing ZIP end record"))?;
    let count = u16_at(bytes, end + 10)?;
    if u16_at(bytes, end + 4)? != 0
        || u16_at(bytes, end + 6)? != 0
        || u16_at(bytes, end + 8)? != count
        || count == u16::MAX
    {
        return Err(invalid("multi-disk and ZIP64 archives are not supported"));
    }
    if u32::from(count) > limits.max_files {
        return Err(BundleError::Limit("archive entry count"));
    }
    let mut offset = u32_at(bytes, end + 16)? as usize;
    let size = u32_at(bytes, end + 12)? as usize;
    if offset.checked_add(size) != Some(end) {
        return Err(invalid("invalid central directory bounds"));
    }
    let mut names = BTreeSet::new();
    for _ in 0..count {
        let header = range(bytes, offset, 46)?;
        if &header[..4] != b"PK\x01\x02" {
            return Err(invalid("invalid central directory entry"));
        }
        let name_len = usize::from(u16_at(header, 28)?);
        let extra_len = usize::from(u16_at(header, 30)?);
        let comment_len = usize::from(u16_at(header, 32)?);
        let name = range(bytes, offset + 46, name_len)?;
        if !names.insert(name) {
            return Err(BundleError::DuplicatePath(
                String::from_utf8_lossy(name).into_owned(),
            ));
        }
        reject_link_extras(range(bytes, offset + 46 + name_len, extra_len)?)?;
        check_local_header(bytes, u32_at(header, 42)? as usize, name)?;
        offset = offset
            .checked_add(46 + name_len + extra_len + comment_len)
            .ok_or_else(|| invalid("invalid central directory length"))?;
        if offset > end {
            return Err(invalid("central directory entry exceeds its bounds"));
        }
    }
    if offset != end {
        return Err(invalid(
            "central directory count does not match its entries",
        ));
    }
    Ok(())
}

fn check_local_header(bytes: &[u8], offset: usize, name: &[u8]) -> Result<(), BundleError> {
    let header = range(bytes, offset, 30)?;
    if &header[..4] != b"PK\x03\x04" {
        return Err(invalid("invalid local file header"));
    }
    let name_len = usize::from(u16_at(header, 26)?);
    let extra_len = usize::from(u16_at(header, 28)?);
    if range(bytes, offset + 30, name_len)? != name {
        return Err(invalid("local and central file names differ"));
    }
    reject_link_extras(range(bytes, offset + 30 + name_len, extra_len)?)
}

fn reject_link_extras(mut bytes: &[u8]) -> Result<(), BundleError> {
    while !bytes.is_empty() {
        let tag = u16_at(bytes, 0)?;
        let size = usize::from(u16_at(bytes, 2)?);
        // PKWARE Unix and ASi Unix may encode symbolic or hard-link targets.
        // The canonical format does not need these platform-specific fields.
        if matches!(tag, 0x000d | 0x756e) {
            return Err(invalid("Unix link extra fields are not supported"));
        }
        bytes = bytes
            .get(4 + size..)
            .ok_or_else(|| invalid("invalid ZIP extra field"))?;
    }
    Ok(())
}

fn range(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8], BundleError> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("ZIP range overflow"))?;
    bytes
        .get(offset..end)
        .ok_or_else(|| invalid("truncated ZIP header"))
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, BundleError> {
    let value = range(bytes, offset, 2)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, BundleError> {
    let value = range(bytes, offset, 4)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn invalid(message: &str) -> BundleError {
    BundleError::Archive(message.into())
}
