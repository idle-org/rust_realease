use crate::error::Error;
use std::{fs, path::Path};

const MH_MAGIC_64_LE: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
const LC_REEXPORT_DYLIB: u32 = 0x8000_001f;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x8000_0023;
const LC_BUILD_VERSION: u32 = 0x32;

pub fn verify_path(path: &Path, target: &str, minimum: &str) -> Result<(), Error> {
    let bytes = fs::read(path)
        .map_err(|e| Error(format!("cannot inspect Mach-O {}: {e}", path.display())))?;
    verify(&bytes, target, minimum).map_err(|e| Error(format!("{}: {e}", path.display())))
}

pub fn verify(bytes: &[u8], target: &str, minimum: &str) -> Result<(), Error> {
    if bytes.len() < 32 || bytes[..4] != MH_MAGIC_64_LE {
        return Err(Error(
            "expected a little-endian 64-bit Mach-O executable".into(),
        ));
    }
    let expected_cpu = match target {
        "aarch64-apple-darwin" => 0x0100_000c,
        "x86_64-apple-darwin" => 0x0100_0007,
        _ => return Err(Error(format!("unsupported Apple target {target}"))),
    };
    if word(bytes, 4)? != expected_cpu || word(bytes, 12)? != 2 {
        return Err(Error(format!(
            "Mach-O architecture or file type does not match {target}"
        )));
    }
    let commands = word(bytes, 16)? as usize;
    let command_bytes = word(bytes, 20)? as usize;
    let end = 32usize
        .checked_add(command_bytes)
        .ok_or_else(|| Error("Mach-O load command size overflow".into()))?;
    if end > bytes.len() {
        return Err(Error("truncated Mach-O load commands".into()));
    }
    let expected_minimum = parse_version(minimum)?;
    let mut offset = 32usize;
    let mut found_minimum = None;
    let mut dependencies = Vec::new();
    for _ in 0..commands {
        if offset.checked_add(8).is_none_or(|header| header > end) {
            return Err(Error("truncated Mach-O load command".into()));
        }
        let command = word(bytes, offset)?;
        let size = word(bytes, offset + 4)? as usize;
        let command_end = offset
            .checked_add(size)
            .ok_or_else(|| Error("Mach-O load command overflow".into()))?;
        if size < 8 || command_end > end {
            return Err(Error("invalid Mach-O load command size".into()));
        }
        if command == LC_BUILD_VERSION {
            if size < 24 || word(bytes, offset + 8)? != 1 {
                return Err(Error("invalid or non-macOS LC_BUILD_VERSION".into()));
            }
            found_minimum = Some(word(bytes, offset + 12)?);
        } else if [
            LC_LOAD_DYLIB,
            LC_LOAD_WEAK_DYLIB,
            LC_REEXPORT_DYLIB,
            LC_LOAD_UPWARD_DYLIB,
        ]
        .contains(&command)
        {
            if size < 24 {
                return Err(Error("invalid Mach-O dylib command".into()));
            }
            let name_offset = word(bytes, offset + 8)? as usize;
            if name_offset < 24 || name_offset >= size {
                return Err(Error("invalid Mach-O dylib name offset".into()));
            }
            let name = c_string(&bytes[offset + name_offset..command_end])?;
            if !name.starts_with("/usr/lib/") && !name.starts_with("/System/Library/") {
                return Err(Error(format!("non-system Mach-O dependency: {name}")));
            }
            dependencies.push(name.to_owned());
        }
        offset = command_end;
    }
    if offset != end {
        return Err(Error("Mach-O load command count/size mismatch".into()));
    }
    let found = found_minimum.ok_or_else(|| Error("Mach-O has no LC_BUILD_VERSION".into()))?;
    if found != expected_minimum {
        return Err(Error(format!(
            "Mach-O deployment minimum {} does not match configured {minimum}",
            display_version(found)
        )));
    }
    if dependencies.is_empty() {
        return Err(Error(
            "Mach-O declares no dynamic system dependencies".into(),
        ));
    }
    Ok(())
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| Error("truncated Mach-O field".into()))?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn c_string(bytes: &[u8]) -> Result<&str, Error> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| Error("unterminated Mach-O dylib name".into()))?;
    std::str::from_utf8(&bytes[..end]).map_err(|_| Error("non-UTF-8 Mach-O dylib name".into()))
}

fn parse_version(value: &str) -> Result<u32, Error> {
    let mut fields = value.split('.');
    let major = fields.next().and_then(|part| part.parse::<u32>().ok());
    let minor = fields.next().and_then(|part| part.parse::<u32>().ok());
    let patch = fields
        .next()
        .map_or(Some(0), |part| part.parse::<u32>().ok());
    match (major, minor, patch, fields.next()) {
        (Some(major), Some(minor), Some(patch), None)
            if major < 65536 && minor < 256 && patch < 256 =>
        {
            Ok((major << 16) | (minor << 8) | patch)
        }
        _ => Err(Error(format!("invalid macOS deployment version {value:?}"))),
    }
}

fn display_version(value: u32) -> String {
    format!("{}.{}.{}", value >> 16, (value >> 8) & 0xff, value & 0xff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(cpu: u32, minimum: u32, dependency: &str) -> Vec<u8> {
        let name_size = dependency.len() + 1;
        let dylib_size = (24 + name_size).div_ceil(8) * 8;
        let mut bytes = vec![0_u8; 32 + 24 + dylib_size];
        bytes[..4].copy_from_slice(&MH_MAGIC_64_LE);
        bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
        bytes[12..16].copy_from_slice(&2_u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&2_u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&(24_u32 + dylib_size as u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&LC_BUILD_VERSION.to_le_bytes());
        bytes[36..40].copy_from_slice(&24_u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&1_u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&minimum.to_le_bytes());
        let offset = 56;
        bytes[offset..offset + 4].copy_from_slice(&LC_LOAD_DYLIB.to_le_bytes());
        bytes[offset + 4..offset + 8].copy_from_slice(&(dylib_size as u32).to_le_bytes());
        bytes[offset + 8..offset + 12].copy_from_slice(&24_u32.to_le_bytes());
        bytes[offset + 24..offset + 24 + dependency.len()].copy_from_slice(dependency.as_bytes());
        bytes
    }

    #[test]
    fn validates_architecture_deployment_minimum_and_system_dependencies() {
        let arm = fixture(0x0100_000c, 13 << 16, "/usr/lib/libSystem.B.dylib");
        verify(&arm, "aarch64-apple-darwin", "13.0").unwrap();
        assert!(verify(&arm, "x86_64-apple-darwin", "13.0").is_err());
        assert!(verify(&arm, "aarch64-apple-darwin", "14.0").is_err());

        let intel = fixture(
            0x0100_0007,
            (13 << 16) | (2 << 8),
            "/System/Library/Frameworks/IOKit.framework/Versions/A/IOKit",
        );
        verify(&intel, "x86_64-apple-darwin", "13.2").unwrap();
        let unsafe_dependency = fixture(0x0100_0007, 13 << 16, "/tmp/libInjected.dylib");
        assert!(verify(&unsafe_dependency, "x86_64-apple-darwin", "13.0").is_err());
        assert!(verify(&intel[..40], "x86_64-apple-darwin", "13.2").is_err());
    }
}
