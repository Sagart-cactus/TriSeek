use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type Trigram = u32;

pub fn normalize_for_index(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().map(u8::to_ascii_lowercase).collect()
}

pub fn encode_trigram(bytes: &[u8]) -> Option<Trigram> {
    if bytes.len() != 3 {
        return None;
    }
    Some(((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32)
}

pub fn decode_trigram(trigram: Trigram) -> [u8; 3] {
    [
        ((trigram >> 16) & 0xff) as u8,
        ((trigram >> 8) & 0xff) as u8,
        (trigram & 0xff) as u8,
    ]
}

pub fn trigrams_from_bytes(bytes: &[u8]) -> Vec<Trigram> {
    if bytes.len() < 3 {
        return Vec::new();
    }

    let normalized = normalize_for_index(bytes);
    let mut grams = BTreeSet::new();
    for window in normalized.windows(3) {
        if let Some(gram) = encode_trigram(window) {
            grams.insert(gram);
        }
    }
    grams.into_iter().collect()
}

pub fn trigrams_from_text(text: &str) -> Vec<Trigram> {
    trigrams_from_bytes(text.as_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrigramDebug(pub Trigram);

impl TrigramDebug {
    pub fn as_string(self) -> String {
        let decoded = decode_trigram(self.0);
        String::from_utf8_lossy(&decoded).into_owned()
    }
}
