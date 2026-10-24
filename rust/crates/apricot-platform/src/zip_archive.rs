//! Minimal ZIP reader with Python `zipfile` semantics for the `AudioVault`
//! TV show packages: the central directory (with ZIP64 and prepended data),
//! stored and deflated members, and the CRC-32 check at the end of a member.

use std::io::{self, Read, Seek, SeekFrom};

use flate2::{Crc, read::DeflateDecoder};

const END_SIGNATURE: u32 = 0x0605_4b50;
const END_SIZE: u64 = 22;
const ZIP64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
const ZIP64_LOCATOR_SIZE: u64 = 20;
const ZIP64_END_SIGNATURE: u32 = 0x0606_4b50;
const ZIP64_END_SIZE: u64 = 56;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const CENTRAL_SIZE: usize = 46;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const LOCAL_SIZE: usize = 30;
const MAX_COMMENT: u64 = 65_535;

/// Python `zipfile.ZipInfo`: the central directory record of one member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZipEntry {
    pub name: String,
    pub flag_bits: u16,
    pub compress_type: u16,
    pub crc: u32,
    pub compress_size: u64,
    pub file_size: u64,
    pub header_offset: u64,
    pub external_attr: u32,
}

impl ZipEntry {
    /// Python `ZipInfo.is_dir`.
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

fn bad_zip(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

fn read_exact_at<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    length: usize,
) -> io::Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0_u8; length];
    reader.read_exact(&mut data)?;
    Ok(data)
}

struct EndRecord {
    offset: u64,
    directory_size: u64,
    directory_offset: u64,
    /// Python `sizeEndCentDir64 + sizeEndCentDir64Locator` when present.
    zip64_size: u64,
}

/// Python `_EndRecData`: the plain end record first, then a search through
/// the largest possible comment.
fn find_end_record<R: Read + Seek>(reader: &mut R) -> io::Result<Option<EndRecord>> {
    let size = reader.seek(SeekFrom::End(0))?;
    if size < END_SIZE {
        return Ok(None);
    }
    let tail = read_exact_at(reader, size - END_SIZE, 22)?;
    let offset = if u32_at(&tail, 0) == END_SIGNATURE && u16_at(&tail, 20) == 0 {
        size - END_SIZE
    } else {
        let start = size.saturating_sub(MAX_COMMENT + END_SIZE);
        let data = read_exact_at(
            reader,
            start,
            usize::try_from(size - start).map_err(|_| bad_zip("File is not a zip file"))?,
        )?;
        let Some(position) = data
            .windows(4)
            .rposition(|window| window == END_SIGNATURE.to_le_bytes())
        else {
            return Ok(None);
        };
        if data.len() - position < 22 {
            return Ok(None);
        }
        start + u64::try_from(position).unwrap_or_default()
    };
    let record = read_exact_at(reader, offset, 22)?;
    let mut end = EndRecord {
        offset,
        directory_size: u64::from(u32_at(&record, 12)),
        directory_offset: u64::from(u32_at(&record, 16)),
        zip64_size: 0,
    };
    if offset >= ZIP64_LOCATOR_SIZE {
        let locator = read_exact_at(reader, offset - ZIP64_LOCATOR_SIZE, 20)?;
        if u32_at(&locator, 0) == ZIP64_LOCATOR_SIGNATURE {
            if u32_at(&locator, 16) > 1 {
                return Err(bad_zip(
                    "zipfiles that span multiple disks are not supported",
                ));
            }
            let record_offset = offset
                .checked_sub(ZIP64_LOCATOR_SIZE + ZIP64_END_SIZE)
                .ok_or_else(|| bad_zip("Corrupt zip64 end of central directory locator"))?;
            let record = read_exact_at(reader, record_offset, 56)?;
            if u32_at(&record, 0) != ZIP64_END_SIGNATURE {
                return Err(bad_zip("Corrupt zip64 end of central directory locator"));
            }
            end.directory_size = u64_at(&record, 40);
            end.directory_offset = u64_at(&record, 48);
            end.zip64_size = ZIP64_LOCATOR_SIZE + ZIP64_END_SIZE;
        }
    }
    Ok(Some(end))
}

/// Python `zipfile.is_zipfile` for a seekable source.
pub fn is_zip<R: Read + Seek>(reader: &mut R) -> bool {
    matches!(find_end_record(reader), Ok(Some(_)))
}

/// Python `ZipFile(...).infolist()`.
///
/// # Errors
///
/// Returns the read error or an `InvalidData` error for a damaged archive.
pub fn read_entries<R: Read + Seek>(reader: &mut R) -> io::Result<Vec<ZipEntry>> {
    let Some(end) = find_end_record(reader)? else {
        return Err(bad_zip("File is not a zip file"));
    };
    let directory_end = end.offset - end.zip64_size;
    let concat = directory_end
        .checked_sub(end.directory_size)
        .and_then(|value| value.checked_sub(end.directory_offset))
        .ok_or_else(|| bad_zip("Bad offset for central directory"))?;
    let directory = read_exact_at(
        reader,
        end.directory_offset + concat,
        usize::try_from(end.directory_size).map_err(|_| bad_zip("Truncated central directory"))?,
    )?;
    let mut entries = Vec::new();
    let mut position = 0_usize;
    while position < directory.len() {
        if directory.len() - position < CENTRAL_SIZE {
            return Err(bad_zip("Truncated central directory"));
        }
        let record = &directory[position..position + CENTRAL_SIZE];
        if u32_at(record, 0) != CENTRAL_SIGNATURE {
            return Err(bad_zip("Bad magic number for central directory"));
        }
        let flag_bits = u16_at(record, 8);
        let name_length = usize::from(u16_at(record, 28));
        let extra_length = usize::from(u16_at(record, 30));
        let comment_length = usize::from(u16_at(record, 32));
        let name_start = position + CENTRAL_SIZE;
        let extra_start = name_start + name_length;
        let next = extra_start + extra_length + comment_length;
        if next > directory.len() {
            return Err(bad_zip("Truncated central directory"));
        }
        let raw_name = &directory[name_start..extra_start];
        let name = if flag_bits & 0x800 == 0 {
            decode_cp437(raw_name)
        } else {
            String::from_utf8_lossy(raw_name).into_owned()
        };
        let mut entry = ZipEntry {
            name,
            flag_bits,
            compress_type: u16_at(record, 10),
            crc: u32_at(record, 16),
            compress_size: u64::from(u32_at(record, 20)),
            file_size: u64::from(u32_at(record, 24)),
            header_offset: u64::from(u32_at(record, 42)),
            external_attr: u32_at(record, 38),
        };
        apply_zip64_extra(
            &mut entry,
            &directory[extra_start..extra_start + extra_length],
        )?;
        entry.header_offset += concat;
        entries.push(entry);
        position = next;
    }
    Ok(entries)
}

/// Python `ZipInfo._decodeExtra`: ZIP64 sizes and offset replace the
/// saturated 32-bit values, in that order.
fn apply_zip64_extra(entry: &mut ZipEntry, mut extra: &[u8]) -> io::Result<()> {
    while extra.len() >= 4 {
        let kind = u16_at(extra, 0);
        let length = usize::from(u16_at(extra, 2));
        if length + 4 > extra.len() {
            return Err(bad_zip(format!(
                "Corrupt extra field {kind:04x} (size={length})"
            )));
        }
        if kind == 0x0001 {
            let mut data = &extra[4..4 + length];
            for field in [
                &mut entry.file_size,
                &mut entry.compress_size,
                &mut entry.header_offset,
            ] {
                if *field == u64::from(u32::MAX) {
                    if data.len() < 8 {
                        return Err(bad_zip("Corrupt zip64 extra field"));
                    }
                    *field = u64_at(data, 0);
                    data = &data[8..];
                }
            }
        }
        extra = &extra[4 + length..];
    }
    Ok(())
}

/// Python decodes names without the UTF-8 flag as code page 437.
fn decode_cp437(bytes: &[u8]) -> String {
    const HIGH: [char; 128] = [
        'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ',
        'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú',
        'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡',
        '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟',
        '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘',
        '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ',
        '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²',
        '■', '\u{a0}',
    ];
    bytes
        .iter()
        .map(|byte| {
            if *byte < 0x80 {
                char::from(*byte)
            } else {
                HIGH[usize::from(*byte - 0x80)]
            }
        })
        .collect()
}

/// Python `ZipFile.open(member)`: the member's data, decompressed, with the
/// CRC-32 checked once all of it has been read.
///
/// # Errors
///
/// Returns an error for a damaged local header, a name that differs from the
/// central directory, an encrypted member or an unsupported compression.
pub fn open_entry<'a, R: Read + Seek>(
    reader: &'a mut R,
    entry: &ZipEntry,
) -> io::Result<EntryReader<'a, R>> {
    let header = read_exact_at(reader, entry.header_offset, LOCAL_SIZE)?;
    if u32_at(&header, 0) != LOCAL_SIGNATURE {
        return Err(bad_zip("Bad magic number for file header"));
    }
    let name_length = usize::from(u16_at(&header, 26));
    let extra_length = usize::from(u16_at(&header, 28));
    let mut raw_name = vec![0_u8; name_length];
    reader.read_exact(&mut raw_name)?;
    let local_flags = u16_at(&header, 6);
    let local_name = if local_flags & 0x800 == 0 {
        decode_cp437(&raw_name)
    } else {
        String::from_utf8_lossy(&raw_name).into_owned()
    };
    if local_name != entry.name {
        return Err(bad_zip(format!(
            "File name in directory {:?} and header {:?} differ.",
            entry.name, local_name
        )));
    }
    if entry.flag_bits & 0x1 != 0 {
        return Err(bad_zip(format!(
            "File {:?} is encrypted, password required for extraction",
            entry.name
        )));
    }
    let data_start = entry.header_offset
        + u64::try_from(LOCAL_SIZE + name_length + extra_length).unwrap_or_default();
    reader.seek(SeekFrom::Start(data_start))?;
    let raw = RawTake {
        inner: reader,
        remaining: entry.compress_size,
    };
    let source = match entry.compress_type {
        0 => EntrySource::Stored(raw),
        8 => EntrySource::Deflated(Box::new(DeflateDecoder::new(raw))),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("compression type {other} is not supported"),
            ));
        }
    };
    Ok(EntryReader {
        source,
        crc: Crc::new(),
        expected_crc: entry.crc,
        expected_size: entry.file_size,
        produced: 0,
        name: entry.name.clone(),
        done: false,
    })
}

struct RawTake<'a, R> {
    inner: &'a mut R,
    remaining: u64,
}

impl<R: Read> Read for RawTake<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let limit = usize::try_from(
            self.remaining
                .min(u64::try_from(buffer.len()).unwrap_or(u64::MAX)),
        )
        .unwrap_or(buffer.len());
        let read = self.inner.read(&mut buffer[..limit])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "File-like object ended before the member did",
            ));
        }
        self.remaining -= u64::try_from(read).unwrap_or_default();
        Ok(read)
    }
}

enum EntrySource<'a, R> {
    Stored(RawTake<'a, R>),
    Deflated(Box<DeflateDecoder<RawTake<'a, R>>>),
}

/// Decompressed data of one member.
pub struct EntryReader<'a, R> {
    source: EntrySource<'a, R>,
    crc: Crc,
    expected_crc: u32,
    expected_size: u64,
    produced: u64,
    name: String,
    done: bool,
}

impl<R: Read> Read for EntryReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.done || buffer.is_empty() {
            return Ok(0);
        }
        let remaining = self.expected_size.saturating_sub(self.produced);
        let limit = usize::try_from(remaining.min(u64::try_from(buffer.len()).unwrap_or(u64::MAX)))
            .unwrap_or(buffer.len());
        let read = if limit == 0 {
            0
        } else {
            match &mut self.source {
                EntrySource::Stored(raw) => raw.read(&mut buffer[..limit])?,
                EntrySource::Deflated(decoder) => decoder.read(&mut buffer[..limit])?,
            }
        };
        self.crc.update(&buffer[..read]);
        self.produced += u64::try_from(read).unwrap_or_default();
        if read == 0 || self.produced >= self.expected_size {
            self.done = true;
            if self.produced < self.expected_size {
                return Err(bad_zip(format!("Truncated file {:?}", self.name)));
            }
            if self.crc.sum() != self.expected_crc {
                return Err(bad_zip(format!("Bad CRC-32 for file {:?}", self.name)));
            }
        }
        Ok(read)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::{Cursor, Read, Write};

    use flate2::{Compression, Crc, write::DeflateEncoder};

    use super::{is_zip, open_entry, read_entries};

    /// Builds a ZIP archive with one stored and one deflated member.
    pub(crate) fn sample_zip(members: &[(&str, &[u8], bool)], prefix: &[u8]) -> Vec<u8> {
        let mut archive = prefix.to_vec();
        let mut directory = Vec::new();
        for (name, data, deflate) in members {
            let mut crc = Crc::new();
            crc.update(data);
            let compressed = if *deflate {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(data).expect("deflate");
                encoder.finish().expect("deflate")
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = u32::try_from(archive.len() - prefix.len()).expect("offset");
            let name_bytes = name.as_bytes();
            let name_length = u16::try_from(name_bytes.len()).expect("name");
            let compressed_size = u32::try_from(compressed.len()).expect("size");
            let file_size = u32::try_from(data.len()).expect("size");
            archive.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
            archive.extend_from_slice(&20_u16.to_le_bytes());
            archive.extend_from_slice(&0x800_u16.to_le_bytes());
            archive.extend_from_slice(&method.to_le_bytes());
            archive.extend_from_slice(&[0; 4]);
            archive.extend_from_slice(&crc.sum().to_le_bytes());
            archive.extend_from_slice(&compressed_size.to_le_bytes());
            archive.extend_from_slice(&file_size.to_le_bytes());
            archive.extend_from_slice(&name_length.to_le_bytes());
            archive.extend_from_slice(&0_u16.to_le_bytes());
            archive.extend_from_slice(name_bytes);
            archive.extend_from_slice(&compressed);
            directory.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
            directory.extend_from_slice(&20_u16.to_le_bytes());
            directory.extend_from_slice(&20_u16.to_le_bytes());
            directory.extend_from_slice(&0x800_u16.to_le_bytes());
            directory.extend_from_slice(&method.to_le_bytes());
            directory.extend_from_slice(&[0; 4]);
            directory.extend_from_slice(&crc.sum().to_le_bytes());
            directory.extend_from_slice(&compressed_size.to_le_bytes());
            directory.extend_from_slice(&file_size.to_le_bytes());
            directory.extend_from_slice(&name_length.to_le_bytes());
            directory.extend_from_slice(&[0; 8]);
            directory.extend_from_slice(&0_u32.to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name_bytes);
        }
        let directory_offset = u32::try_from(archive.len() - prefix.len()).expect("offset");
        let count = u16::try_from(members.len()).expect("count");
        archive.extend_from_slice(&directory);
        archive.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&u32::try_from(directory.len()).expect("size").to_le_bytes());
        archive.extend_from_slice(&directory_offset.to_le_bytes());
        archive.extend_from_slice(&0_u16.to_le_bytes());
        archive
    }

    #[test]
    fn reads_stored_and_deflated_members_with_prepended_data() {
        let deflated = b"episode two ".repeat(500);
        let archive = sample_zip(
            &[
                ("Show/01 Pilot.mp3", b"episode one", false),
                ("Show/02 Next.mp3", &deflated, true),
                ("Show/", b"", false),
            ],
            b"prefix bytes",
        );
        let mut reader = Cursor::new(archive);
        assert!(is_zip(&mut reader));
        let entries = read_entries(&mut reader).expect("entries");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "Show/01 Pilot.mp3");
        assert!(entries[2].is_dir());
        let mut first = Vec::new();
        open_entry(&mut reader, &entries[0])
            .expect("open")
            .read_to_end(&mut first)
            .expect("read");
        assert_eq!(first, b"episode one");
        let mut second = Vec::new();
        open_entry(&mut reader, &entries[1])
            .expect("open")
            .read_to_end(&mut second)
            .expect("read");
        assert_eq!(second, deflated);
    }

    #[test]
    fn a_wrong_crc_fails_like_python() {
        let mut archive = sample_zip(&[("a.mp3", b"abc", false)], b"");
        // The first member's data follows its 30-byte header and name.
        archive[35] = b'x';
        let mut reader = Cursor::new(archive);
        let entries = read_entries(&mut reader).expect("entries");
        let mut data = Vec::new();
        let error = open_entry(&mut reader, &entries[0])
            .expect("open")
            .read_to_end(&mut data)
            .expect_err("bad crc");
        assert!(error.to_string().contains("Bad CRC-32"));
    }

    #[test]
    fn rejects_non_archives() {
        let mut reader = Cursor::new(b"<html>login</html>".repeat(10));
        assert!(!is_zip(&mut reader));
        assert!(read_entries(&mut reader).is_err());
    }
}
