//! Fast binary index format designed for zero-copy mmap access.
//!
//! Layout:
//! ```text
//! [Header: 64 bytes]
//!   magic: [u8; 8]  "TRISEEK\0"
//!   version: u32
//!   num_docs: u32
//!   num_content_trigrams: u32
//!   num_path_trigrams: u32
//!   content_table_offset: u64
//!   content_postings_offset: u64
//!   path_table_offset: u64
//!   path_postings_offset: u64
//!   docs_offset: u64
//!   strings_offset: u64
//!   strings_size: u64
//!
//! [Content Trigram Table: num_content_trigrams * 12 bytes]
//!   For each entry: trigram: u32, offset: u32 (into postings), count: u32
//!
//! [Content Postings: packed u32 doc_ids]
//!
//! [Path Trigram Table: num_path_trigrams * 12 bytes]
//!   For each entry: trigram: u32, offset: u32 (into postings), count: u32
//!
//! [Path Postings: packed u32 doc_ids]
//!
//! [Doc Table: num_docs * DocEntry bytes]
//!   For each doc: doc_id: u32, path_offset: u32, path_len: u16,
//!                 name_offset: u32, name_len: u16,
//!                 ext_offset: u32, ext_len: u8,
//!                 fingerprint: 20 bytes
//!
//! [String Pool: concatenated UTF-8 strings]
//! ```

use memmap2::Mmap;
use search_core::{FileFingerprint, Trigram};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::error::SearchIndexError;
use crate::model::{DeltaSnapshot, DocumentRecord, PersistedIndex};

const MAGIC: &[u8; 8] = b"TRISEEK\0";
const VERSION: u32 = 2;
const HEADER_SIZE: usize = 96;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct TrigramTableEntry {
    trigram: u32,
    offset: u32, // offset in posting array (count of u32s, not bytes)
    count: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DocEntry {
    doc_id: u32,
    path_offset: u32,
    path_len: u16,
    name_offset: u32,
    name_len: u16,
    ext_offset: u32,
    ext_len: u8,
    _pad: u8,
    fp_size: u64,
    fp_mtime: i64,
    fp_hash: u64,
}

const DOC_ENTRY_SIZE: usize = std::mem::size_of::<DocEntry>();

/// A fast, mmap-backed read-only index.
pub struct FastIndex {
    mmap: Mmap,
    num_docs: u32,
    // Pre-built lookup tables for O(1) trigram access
    content_table: HashMap<u32, (u32, u32)>, // trigram -> (offset, count)
    path_table: HashMap<u32, (u32, u32)>,
    // Offsets into mmap
    content_postings_offset: usize,
    path_postings_offset: usize,
    docs_offset: usize,
    strings_offset: usize,
    // Doc lookup maps (built once on open)
    path_to_doc: HashMap<String, u32>,
    filename_map: HashMap<String, Vec<u32>>,
    extension_map: HashMap<String, Vec<u32>>,
}

impl FastIndex {
    pub fn open(path: &Path) -> Result<Self, SearchIndexError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        if mmap.len() < HEADER_SIZE {
            return Err(SearchIndexError::InvalidQuery(
                "index file too small".into(),
            ));
        }
        if &mmap[0..8] != MAGIC {
            return Err(SearchIndexError::InvalidQuery("invalid index magic".into()));
        }

        let version = read_u32(&mmap, 8);
        if version != VERSION {
            return Err(SearchIndexError::InvalidQuery(format!(
                "unsupported index version {version}"
            )));
        }

        let num_docs = read_u32(&mmap, 12);
        let num_content_trigrams = read_u32(&mmap, 16);
        let num_path_trigrams = read_u32(&mmap, 20);
        let content_table_offset = read_u64(&mmap, 24) as usize;
        let content_postings_offset = read_u64(&mmap, 32) as usize;
        let path_table_offset = read_u64(&mmap, 40) as usize;
        let path_postings_offset = read_u64(&mmap, 48) as usize;
        let docs_offset = read_u64(&mmap, 56) as usize;
        let strings_offset = read_u64(&mmap, 64) as usize;

        // Build trigram lookup tables
        let content_table =
            build_trigram_lookup(&mmap, content_table_offset, num_content_trigrams as usize);
        let path_table = build_trigram_lookup(&mmap, path_table_offset, num_path_trigrams as usize);

        // Build doc metadata maps
        let mut path_to_doc = HashMap::with_capacity(num_docs as usize);
        let mut filename_map: HashMap<String, Vec<u32>> = HashMap::new();
        let mut extension_map: HashMap<String, Vec<u32>> = HashMap::new();

        for i in 0..num_docs as usize {
            let entry = read_doc_entry(&mmap, docs_offset, i);
            let doc_id = entry.doc_id;
            let path = read_string(
                &mmap,
                strings_offset,
                entry.path_offset,
                entry.path_len as usize,
            );
            let name = read_string(
                &mmap,
                strings_offset,
                entry.name_offset,
                entry.name_len as usize,
            );
            let ext = if entry.ext_len > 0 {
                Some(read_string(
                    &mmap,
                    strings_offset,
                    entry.ext_offset,
                    entry.ext_len as usize,
                ))
            } else {
                None
            };

            path_to_doc.insert(path, doc_id);
            filename_map
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push(doc_id);
            if let Some(ext) = ext {
                extension_map
                    .entry(ext.to_ascii_lowercase())
                    .or_default()
                    .push(doc_id);
            }
        }

        Ok(Self {
            mmap,
            num_docs,
            content_table,
            path_table,
            content_postings_offset,
            path_postings_offset,
            docs_offset,
            strings_offset,
            path_to_doc,
            filename_map,
            extension_map,
        })
    }

    /// Get content posting list for a trigram — returns a slice of sorted doc_ids.
    pub fn content_postings(&self, trigram: Trigram) -> Option<Vec<u32>> {
        let (offset, count) = self.content_table.get(&trigram)?;
        Some(read_posting_list(
            &self.mmap,
            self.content_postings_offset,
            *offset as usize,
            *count as usize,
        ))
    }

    /// Get path posting list for a trigram.
    pub fn path_postings(&self, trigram: Trigram) -> Option<Vec<u32>> {
        let (offset, count) = self.path_table.get(&trigram)?;
        Some(read_posting_list(
            &self.mmap,
            self.path_postings_offset,
            *offset as usize,
            *count as usize,
        ))
    }

    pub fn num_docs(&self) -> u32 {
        self.num_docs
    }

    pub fn path_to_doc_id(&self, path: &str) -> Option<u32> {
        self.path_to_doc.get(path).copied()
    }

    pub fn docs_by_filename(&self, name: &str) -> Option<&[u32]> {
        self.filename_map.get(name).map(Vec::as_slice)
    }

    pub fn docs_by_extension(&self, ext: &str) -> Option<&[u32]> {
        self.extension_map.get(ext).map(Vec::as_slice)
    }

    pub fn doc_path(&self, doc_id: u32) -> Option<String> {
        if doc_id >= self.num_docs {
            return None;
        }
        let entry = read_doc_entry(&self.mmap, self.docs_offset, doc_id as usize);
        Some(read_string(
            &self.mmap,
            self.strings_offset,
            entry.path_offset,
            entry.path_len as usize,
        ))
    }

    pub fn doc_record(&self, doc_id: u32) -> Option<DocumentRecord> {
        if doc_id >= self.num_docs {
            return None;
        }
        let entry = read_doc_entry(&self.mmap, self.docs_offset, doc_id as usize);
        let path = read_string(
            &self.mmap,
            self.strings_offset,
            entry.path_offset,
            entry.path_len as usize,
        );
        let name = read_string(
            &self.mmap,
            self.strings_offset,
            entry.name_offset,
            entry.name_len as usize,
        );
        let ext = if entry.ext_len > 0 {
            Some(read_string(
                &self.mmap,
                self.strings_offset,
                entry.ext_offset,
                entry.ext_len as usize,
            ))
        } else {
            None
        };
        Some(DocumentRecord {
            doc_id,
            relative_path: path,
            file_name: name,
            extension: ext,
            fingerprint: FileFingerprint {
                size: entry.fp_size,
                modified_unix_secs: entry.fp_mtime,
                hash: entry.fp_hash,
            },
        })
    }

    /// Get all doc_ids (sorted).
    pub fn all_doc_ids(&self) -> Vec<u32> {
        (0..self.num_docs).collect()
    }

    /// Iterate all doc paths for path filtering.
    pub fn all_docs(&self) -> Vec<(u32, String)> {
        (0..self.num_docs)
            .map(|i| {
                let entry = read_doc_entry(&self.mmap, self.docs_offset, i as usize);
                let path = read_string(
                    &self.mmap,
                    self.strings_offset,
                    entry.path_offset,
                    entry.path_len as usize,
                );
                (i, path)
            })
            .collect()
    }
}

/// Write a FastIndex to disk from a PersistedIndex.
pub fn write_fast_index(
    path: &Path,
    index: &PersistedIndex,
    delta: Option<&DeltaSnapshot>,
) -> Result<u64, SearchIndexError> {
    // Merge base + delta to get final docs and postings
    let (docs, content_postings, path_postings) = if let Some(delta) = delta {
        merge_for_write(index, delta)
    } else {
        (
            index.docs.clone(),
            index
                .content_postings
                .iter()
                .map(|e| (e.trigram, e.docs.clone()))
                .collect(),
            index
                .path_postings
                .iter()
                .map(|e| (e.trigram, e.docs.clone()))
                .collect(),
        )
    };

    // Assign sequential doc_ids (0..n)
    let mut doc_id_remap: HashMap<u32, u32> = HashMap::new();
    for (new_id, doc) in docs.iter().enumerate() {
        doc_id_remap.insert(doc.doc_id, new_id as u32);
    }

    // Build string pool
    let mut string_pool = Vec::new();
    let mut string_offsets: Vec<(u32, u16, u32, u16, u32, u8)> = Vec::new(); // path_off, path_len, name_off, name_len, ext_off, ext_len

    for doc in &docs {
        let path_offset = string_pool.len() as u32;
        let path_len = doc.relative_path.len() as u16;
        string_pool.extend_from_slice(doc.relative_path.as_bytes());

        let name_offset = string_pool.len() as u32;
        let name_len = doc.file_name.len() as u16;
        string_pool.extend_from_slice(doc.file_name.as_bytes());

        let (ext_offset, ext_len) = if let Some(ext) = &doc.extension {
            let off = string_pool.len() as u32;
            let len = ext.len() as u8;
            string_pool.extend_from_slice(ext.as_bytes());
            (off, len)
        } else {
            (0, 0)
        };

        string_offsets.push((
            path_offset,
            path_len,
            name_offset,
            name_len,
            ext_offset,
            ext_len,
        ));
    }

    // Sort trigram tables and build posting arrays
    let mut content_entries: Vec<(u32, Vec<u32>)> = content_postings;
    content_entries.sort_by_key(|(t, _)| *t);
    let mut content_table_data = Vec::new();
    let mut content_posting_data = Vec::new();
    for (trigram, post_docs) in &content_entries {
        // Remap doc_ids
        let remapped: Vec<u32> = post_docs
            .iter()
            .filter_map(|id| doc_id_remap.get(id).copied())
            .collect();
        let offset = content_posting_data.len() as u32;
        let count = remapped.len() as u32;
        content_table_data.push(TrigramTableEntry {
            trigram: *trigram,
            offset,
            count,
        });
        content_posting_data.extend_from_slice(&remapped);
    }

    let mut path_entries: Vec<(u32, Vec<u32>)> = path_postings;
    path_entries.sort_by_key(|(t, _)| *t);
    let mut path_table_data = Vec::new();
    let mut path_posting_data = Vec::new();
    for (trigram, post_docs) in &path_entries {
        let remapped: Vec<u32> = post_docs
            .iter()
            .filter_map(|id| doc_id_remap.get(id).copied())
            .collect();
        let offset = path_posting_data.len() as u32;
        let count = remapped.len() as u32;
        path_table_data.push(TrigramTableEntry {
            trigram: *trigram,
            offset,
            count,
        });
        path_posting_data.extend_from_slice(&remapped);
    }

    // Build doc entries
    let mut doc_entries = Vec::with_capacity(docs.len());
    for (i, doc) in docs.iter().enumerate() {
        let (path_offset, path_len, name_offset, name_len, ext_offset, ext_len) = string_offsets[i];
        doc_entries.push(DocEntry {
            doc_id: i as u32,
            path_offset,
            path_len,
            name_offset,
            name_len,
            ext_offset,
            ext_len,
            _pad: 0,
            fp_size: doc.fingerprint.size,
            fp_mtime: doc.fingerprint.modified_unix_secs,
            fp_hash: doc.fingerprint.hash,
        });
    }

    // Calculate offsets
    let content_table_offset = HEADER_SIZE;
    let content_table_size = content_table_data.len() * std::mem::size_of::<TrigramTableEntry>();
    let content_postings_offset = content_table_offset + content_table_size;
    let content_postings_size = content_posting_data.len() * 4;
    let path_table_offset = content_postings_offset + content_postings_size;
    let path_table_size = path_table_data.len() * std::mem::size_of::<TrigramTableEntry>();
    let path_postings_offset = path_table_offset + path_table_size;
    let path_postings_size = path_posting_data.len() * 4;
    let docs_offset = path_postings_offset + path_postings_size;
    let docs_size = doc_entries.len() * DOC_ENTRY_SIZE;
    let strings_offset = docs_offset + docs_size;

    // Write file
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut file = std::io::BufWriter::with_capacity(1 << 20, file);

    // Header
    let mut header = [0u8; HEADER_SIZE];
    header[0..8].copy_from_slice(MAGIC);
    write_u32(&mut header, 8, VERSION);
    write_u32(&mut header, 12, docs.len() as u32);
    write_u32(&mut header, 16, content_table_data.len() as u32);
    write_u32(&mut header, 20, path_table_data.len() as u32);
    write_u64(&mut header, 24, content_table_offset as u64);
    write_u64(&mut header, 32, content_postings_offset as u64);
    write_u64(&mut header, 40, path_table_offset as u64);
    write_u64(&mut header, 48, path_postings_offset as u64);
    write_u64(&mut header, 56, docs_offset as u64);
    write_u64(&mut header, 64, strings_offset as u64);
    write_u64(&mut header, 72, string_pool.len() as u64);
    file.write_all(&header)?;

    // Content trigram table
    for entry in &content_table_data {
        file.write_all(&entry.trigram.to_le_bytes())?;
        file.write_all(&entry.offset.to_le_bytes())?;
        file.write_all(&entry.count.to_le_bytes())?;
    }

    // Content postings — write as bulk u32 slice
    let content_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            content_posting_data.as_ptr() as *const u8,
            content_posting_data.len() * 4,
        )
    };
    file.write_all(content_bytes)?;

    // Path trigram table
    for entry in &path_table_data {
        file.write_all(&entry.trigram.to_le_bytes())?;
        file.write_all(&entry.offset.to_le_bytes())?;
        file.write_all(&entry.count.to_le_bytes())?;
    }

    // Path postings — write as bulk u32 slice
    let path_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            path_posting_data.as_ptr() as *const u8,
            path_posting_data.len() * 4,
        )
    };
    file.write_all(path_bytes)?;

    // Doc entries
    for entry in &doc_entries {
        file.write_all(&entry.doc_id.to_le_bytes())?;
        file.write_all(&entry.path_offset.to_le_bytes())?;
        file.write_all(&entry.path_len.to_le_bytes())?;
        file.write_all(&entry.name_offset.to_le_bytes())?;
        file.write_all(&entry.name_len.to_le_bytes())?;
        file.write_all(&entry.ext_offset.to_le_bytes())?;
        file.write_all(&entry.ext_len.to_le_bytes())?;
        file.write_all(&[entry._pad])?;
        file.write_all(&entry.fp_size.to_le_bytes())?;
        file.write_all(&entry.fp_mtime.to_le_bytes())?;
        file.write_all(&entry.fp_hash.to_le_bytes())?;
    }

    // String pool
    file.write_all(&string_pool)?;
    file.flush()?;

    let total_size = strings_offset + string_pool.len();
    Ok(total_size as u64)
}

fn merge_for_write(
    base: &PersistedIndex,
    delta: &DeltaSnapshot,
) -> (
    Vec<DocumentRecord>,
    Vec<(u32, Vec<u32>)>,
    Vec<(u32, Vec<u32>)>,
) {
    use std::collections::HashSet;

    let removed: HashSet<&str> = delta.removed_paths.iter().map(String::as_str).collect();
    let mut docs: Vec<DocumentRecord> = base
        .docs
        .iter()
        .filter(|d| !removed.contains(d.relative_path.as_str()))
        .cloned()
        .collect();
    // Add delta docs, replacing any with same path
    let existing_paths: HashSet<String> = docs.iter().map(|d| d.relative_path.clone()).collect();
    for doc in &delta.docs {
        if !existing_paths.contains(&doc.relative_path) {
            docs.push(doc.clone());
        }
    }
    docs.sort_by_key(|d| d.doc_id);

    let active_ids: HashSet<u32> = docs.iter().map(|d| d.doc_id).collect();

    // Merge content postings
    let mut content_map: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in &base.content_postings {
        let filtered: Vec<u32> = entry
            .docs
            .iter()
            .copied()
            .filter(|id| active_ids.contains(id))
            .collect();
        if !filtered.is_empty() {
            content_map.insert(entry.trigram, filtered);
        }
    }
    for entry in &delta.content_postings {
        content_map
            .entry(entry.trigram)
            .or_default()
            .extend(&entry.docs);
        if let Some(list) = content_map.get_mut(&entry.trigram) {
            list.sort_unstable();
            list.dedup();
        }
    }

    // Merge path postings
    let mut path_map: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in &base.path_postings {
        let filtered: Vec<u32> = entry
            .docs
            .iter()
            .copied()
            .filter(|id| active_ids.contains(id))
            .collect();
        if !filtered.is_empty() {
            path_map.insert(entry.trigram, filtered);
        }
    }
    for entry in &delta.path_postings {
        path_map
            .entry(entry.trigram)
            .or_default()
            .extend(&entry.docs);
        if let Some(list) = path_map.get_mut(&entry.trigram) {
            list.sort_unstable();
            list.dedup();
        }
    }

    (
        docs,
        content_map.into_iter().collect(),
        path_map.into_iter().collect(),
    )
}

// Helper functions for reading/writing binary data

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn read_i64(data: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn write_u32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
}

fn write_u64(buf: &mut [u8], offset: usize, val: u64) {
    buf[offset..offset + 8].copy_from_slice(&val.to_le_bytes());
}

fn build_trigram_lookup(
    mmap: &[u8],
    table_offset: usize,
    count: usize,
) -> HashMap<u32, (u32, u32)> {
    let mut map = HashMap::with_capacity(count);
    for i in 0..count {
        let base = table_offset + i * 12;
        let trigram = read_u32(mmap, base);
        let offset = read_u32(mmap, base + 4);
        let cnt = read_u32(mmap, base + 8);
        map.insert(trigram, (offset, cnt));
    }
    map
}

fn read_posting_list(mmap: &[u8], postings_base: usize, offset: usize, count: usize) -> Vec<u32> {
    let byte_offset = postings_base + offset * 4;
    (0..count)
        .map(|i| read_u32(mmap, byte_offset + i * 4))
        .collect()
}

fn read_doc_entry(mmap: &[u8], docs_offset: usize, index: usize) -> DocEntry {
    let base = docs_offset + index * DOC_ENTRY_SIZE;
    DocEntry {
        doc_id: read_u32(mmap, base),
        path_offset: read_u32(mmap, base + 4),
        path_len: read_u16(mmap, base + 8),
        name_offset: read_u32(mmap, base + 10),
        name_len: read_u16(mmap, base + 14),
        ext_offset: read_u32(mmap, base + 16),
        ext_len: mmap[base + 20],
        _pad: mmap[base + 21],
        fp_size: read_u64(mmap, base + 22),
        fp_mtime: read_i64(mmap, base + 30),
        fp_hash: read_u64(mmap, base + 38),
    }
}

fn read_string(mmap: &[u8], strings_base: usize, offset: u32, len: usize) -> String {
    let start = strings_base + offset as usize;
    String::from_utf8_lossy(&mmap[start..start + len]).into_owned()
}
