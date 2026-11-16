use std::fs;
use std::io;
use std::path::Path;

pub fn load_rom(path: &Path) -> io::Result<Vec<u8>> {
    let bytes = fs::read(path)?;
    if looks_like_srec(&bytes) {
        parse_srec(&bytes)
    } else {
        Ok(bytes)
    }
}

pub fn normalize_rom(mut rom: Vec<u8>, size: usize) -> Vec<u8> {
    if rom.len() < size {
        rom.resize(size, 0xff);
    } else if rom.len() > size {
        rom.truncate(size);
    }
    rom
}

fn looks_like_srec(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(b'S'))
}

/// A contiguous run of bytes from an S-record file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub address: u32,
    pub data: Vec<u8>,
}

/// Read an S-record file as address/data segments (one per data record),
/// for loading a program into RAM.
pub fn load_srec_segments(path: &Path) -> io::Result<Vec<Segment>> {
    parse_srec_segments(&fs::read(path)?)
}

fn parse_srec(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut image = Vec::new();
    for segment in parse_srec_segments(bytes)? {
        let address = segment.address as usize;
        let needed_len = address + segment.data.len();
        if image.len() < needed_len {
            image.resize(needed_len, 0xff);
        }
        image[address..needed_len].copy_from_slice(&segment.data);
    }
    Ok(image)
}

pub fn parse_srec_segments(bytes: &[u8]) -> io::Result<Vec<Segment>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut segments = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with('S') || line.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid S-record line: {line}"),
            ));
        }

        let record_type = line.as_bytes()[1];
        let address_bytes = match record_type {
            b'0' | b'1' | b'5' | b'9' => 2usize,
            b'2' | b'8' => 3usize,
            b'3' | b'7' => 4usize,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported S-record type: {}", record_type as char),
                ))
            }
        };

        let count = hex_byte(&line[2..4])? as usize;
        let expected_hex_len = 4 + (count * 2);
        if line.len() != expected_hex_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("malformed S-record length: {line}"),
            ));
        }

        let mut fields = Vec::with_capacity(count);
        for index in 0..count {
            let start = 4 + (index * 2);
            fields.push(hex_byte(&line[start..start + 2])?);
        }

        let checksum_sum = fields
            .iter()
            .fold(count as u8, |acc, value| acc.wrapping_add(*value));
        if checksum_sum != 0xff {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad S-record checksum: {line}"),
            ));
        }

        if matches!(record_type, b'0' | b'5' | b'7' | b'8' | b'9') {
            continue;
        }

        let mut address = 0u32;
        for byte in fields.iter().take(address_bytes) {
            address = (address << 8) | u32::from(*byte);
        }
        segments.push(Segment {
            address,
            data: fields[address_bytes..fields.len() - 1].to_vec(),
        });
    }

    Ok(segments)
}

fn hex_byte(text: &str) -> io::Result<u8> {
    u8::from_str_radix(text, 16).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::{load_rom, normalize_rom, parse_srec_segments, Segment};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalizes_binary_rom_size() {
        let rom = normalize_rom(vec![0xaa, 0xbb], 4);
        assert_eq!(rom, vec![0xaa, 0xbb, 0xff, 0xff]);
    }

    #[test]
    fn parses_s2_records_as_segments() {
        let segments =
            parse_srec_segments(b"S00600004844521B\nS2060100004E7535\nS804010000FA\n").unwrap();
        assert_eq!(
            segments,
            vec![Segment {
                address: 0x10000,
                data: vec![0x4e, 0x75],
            }]
        );
    }

    #[test]
    fn rejects_bad_checksums() {
        assert!(parse_srec_segments(b"S2060100004E7500\n").is_err());
    }

    #[test]
    fn parses_simple_srec_image() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mini68k-test-{unique}.srec"));
        fs::write(&path, "S10512340102B1\nS9030000FC\n").unwrap();

        let rom = load_rom(&path).unwrap();
        assert_eq!(rom[0x1234], 0x01);
        assert_eq!(rom[0x1235], 0x02);

        let _ = fs::remove_file(path);
    }
}
