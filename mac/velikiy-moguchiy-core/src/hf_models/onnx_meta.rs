//! The `metadata_props` of an ONNX model, read from the end of the file.
//!
//! ONNX writers serialize `ModelProto` in field order, so the metadata
//! (field 14) comes after the graph (field 7) and sits at the end of the file.
//! A ranged request for the last bytes reads it without the weights.

use std::collections::BTreeMap;

/// How much of the end of a model file is fetched. SenseVoice and Whisper
/// exports carry several kilobytes of metadata.
pub const TAIL_BYTES: u64 = 64 * 1024;

/// `metadata_props` entry: field 14, length-delimited.
const ENTRY_TAG: u8 = 0x72;
/// `StringStringEntryProto.key` and `.value`: fields 1 and 2, length-delimited.
const KEY_TAG: u8 = 0x0a;
const VALUE_TAG: u8 = 0x12;

/// Metadata found in `tail`. Entries are only taken from runs of two or more
/// back-to-back entries: the tail also holds weights and graph strings, where
/// a lone well-formed entry is a coincidence.
pub fn metadata_from_tail(tail: &[u8]) -> BTreeMap<String, String> {
    let mut entries: Vec<(usize, usize, String, String)> = Vec::new();
    for start in 0..tail.len() {
        if tail[start] != ENTRY_TAG {
            continue;
        }
        let Some((len, body)) = varint(tail, start + 1) else {
            continue;
        };
        let Some(end) = body.checked_add(len as usize).filter(|&e| e <= tail.len()) else {
            continue;
        };
        if let Some((key, value)) = entry(&tail[body..end]) {
            entries.push((start, end, key, value));
        }
    }

    let mut metadata = BTreeMap::new();
    let mut i = 0;
    while i < entries.len() {
        let mut run = vec![i];
        let mut end = entries[i].1;
        for (j, candidate) in entries.iter().enumerate().skip(i + 1) {
            if candidate.0 == end {
                run.push(j);
                end = candidate.1;
            }
        }
        if run.len() >= 2 {
            for &k in &run {
                metadata.insert(entries[k].2.clone(), entries[k].3.clone());
            }
            i = *run.last().unwrap() + 1;
        } else {
            i += 1;
        }
    }
    metadata
}

fn varint(bytes: &[u8], mut at: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(at)?;
        at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Some((value, at));
        }
    }
    None
}

fn entry(body: &[u8]) -> Option<(String, String)> {
    let mut key = None;
    let mut value = String::new();
    let mut at = 0;
    while at < body.len() {
        let tag = body[at];
        let (len, start) = varint(body, at + 1)?;
        let end = start
            .checked_add(len as usize)
            .filter(|&e| e <= body.len())?;
        let text = std::str::from_utf8(&body[start..end]).ok()?.to_string();
        match tag {
            KEY_TAG => key = Some(text),
            VALUE_TAG => value = text,
            _ => return None,
        }
        at = end;
    }
    let key = key?;
    let plain = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    plain.then_some((key, value))
}

#[cfg(test)]
pub(crate) fn encode_entries(entries: &[(&str, &str)]) -> Vec<u8> {
    fn push_len(out: &mut Vec<u8>, mut len: usize) {
        loop {
            let byte = (len & 0x7f) as u8;
            len >>= 7;
            if len == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }
    let mut out = Vec::new();
    for (key, value) in entries {
        let mut body = vec![KEY_TAG];
        push_len(&mut body, key.len());
        body.extend_from_slice(key.as_bytes());
        body.push(VALUE_TAG);
        push_len(&mut body, value.len());
        body.extend_from_slice(value.as_bytes());
        out.push(ENTRY_TAG);
        push_len(&mut out, body.len());
        out.extend(body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (state >> 33) as u8
            })
            .collect()
    }

    #[test]
    fn reads_metadata_after_weights() {
        let long = "0.28683996200561523,".repeat(400);
        let mut tail = noise(30_000, 7);
        tail.extend(encode_entries(&[
            ("model_type", "zipformer2"),
            ("comment", "streaming zipformer2"),
            ("decode_chunk_len", "64"),
            ("T", "77"),
            ("neg_mean", &long),
        ]));
        let meta = metadata_from_tail(&tail);
        assert_eq!(meta["model_type"], "zipformer2");
        assert_eq!(meta["decode_chunk_len"], "64");
        assert_eq!(meta["T"], "77");
        assert_eq!(meta["neg_mean"].len(), long.len());
    }

    #[test]
    fn a_cut_first_entry_does_not_hide_the_rest() {
        let full = encode_entries(&[
            ("vocab_size", "1024"),
            ("model_type", "EncDecRNNTBPEModel"),
            ("is_giga_am", "1"),
        ]);
        let meta = metadata_from_tail(&full[5..]);
        assert_eq!(
            meta.get("model_type").map(String::as_str),
            Some("EncDecRNNTBPEModel")
        );
        assert!(!meta.contains_key("vocab_size"));
    }

    #[test]
    fn lone_entries_and_noise_are_not_metadata() {
        let mut tail = noise(65_536, 42);
        tail.extend(encode_entries(&[("onnx.infer", "onnxruntime.quant")]));
        assert!(metadata_from_tail(&tail).is_empty());
        for seed in 0..20 {
            assert!(
                metadata_from_tail(&noise(65_536, seed)).is_empty(),
                "seed {seed}"
            );
        }
    }
}
