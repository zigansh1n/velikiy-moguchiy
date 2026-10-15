//! Decoding Ogg Opus files (Telegram and WhatsApp voice messages, `.ogg`,
//! `.oga`, `.opus`) to 16 kHz mono PCM, without ffmpeg.

use audiopus::coder::Decoder;
use audiopus::packet::Packet;
use audiopus::{Channels, MutSignals, SampleRate};

/// Opus granule positions always count 48 kHz samples (RFC 7845 §4).
const GRANULE_RATE: u64 = 48_000;
const OUTPUT_RATE: u64 = 16_000;
/// The longest Opus frame is 120 ms.
const MAX_FRAME_SAMPLES: usize = (OUTPUT_RATE as usize) * 120 / 1000;

/// Whether `bytes` start like an Ogg stream.
pub fn is_ogg(bytes: &[u8]) -> bool {
    bytes.starts_with(b"OggS")
}

/// Decodes the first Opus stream of an Ogg file to 16 kHz mono 16-bit PCM.
pub fn decode_ogg_opus(bytes: &[u8]) -> Result<Vec<i16>, String> {
    let mut packets = OggPackets::new(bytes);
    let head = packets
        .next()
        .ok_or("not an Ogg file")?
        .map_err(|e| format!("Ogg: {e}"))?;
    if !head.data.starts_with(b"OpusHead") || head.data.len() < 19 {
        return Err("Ogg file holds no Opus audio (only Ogg Opus is supported)".into());
    }
    let channel_mapping = head.data[18];
    if channel_mapping != 0 {
        return Err(format!(
            "Opus channel mapping family {channel_mapping} is not supported"
        ));
    }
    let pre_skip = u16::from_le_bytes([head.data[10], head.data[11]]) as usize;
    let pre_skip_out = pre_skip * OUTPUT_RATE as usize / GRANULE_RATE as usize;

    // libopus downmixes a stereo stream when the decoder is mono.
    let mut decoder = Decoder::new(SampleRate::Hz16000, Channels::Mono)
        .map_err(|e| format!("Opus decoder: {e}"))?;
    let mut pcm: Vec<i16> = Vec::new();
    let mut frame = vec![0i16; MAX_FRAME_SAMPLES];
    let mut last_granule = None;
    let mut audio_packets = 0usize;

    for packet in packets {
        let packet = packet.map_err(|e| format!("Ogg: {e}"))?;
        if packet.serial != head.serial {
            continue;
        }
        if packet.data.starts_with(b"OpusTags") {
            continue;
        }
        if packet.data.is_empty() {
            continue;
        }
        let input =
            Packet::try_from(packet.data.as_slice()).map_err(|e| format!("Opus packet: {e}"))?;
        let output =
            MutSignals::try_from(frame.as_mut_slice()).map_err(|e| format!("Opus buffer: {e}"))?;
        let n = decoder
            .decode(Some(input), output, false)
            .map_err(|e| format!("Opus decode: {e}"))?;
        pcm.extend_from_slice(&frame[..n]);
        audio_packets += 1;
        if let Some(granule) = packet.granule {
            last_granule = Some(granule);
        }
    }
    if audio_packets == 0 {
        return Err("Ogg Opus file has no audio".into());
    }

    // The last page's granule position marks where the audio really ends;
    // the final frame is padded past it.
    if let Some(granule) = last_granule {
        let total = (granule.saturating_sub(pre_skip as u64) * OUTPUT_RATE / GRANULE_RATE) as usize;
        pcm.truncate(pre_skip_out + total);
    }
    pcm.drain(..pre_skip_out.min(pcm.len()));
    Ok(pcm)
}

struct OggPacket {
    serial: u32,
    data: Vec<u8>,
    /// Granule position of the page this packet ends on, when it is the
    /// last packet completed on that page.
    granule: Option<u64>,
}

/// Iterates over the packets of an Ogg bitstream (RFC 3533), joining packets
/// that continue across pages. Pages of interleaved streams are kept apart
/// by serial number.
struct OggPackets<'a> {
    bytes: &'a [u8],
    pos: usize,
    ready: std::collections::VecDeque<OggPacket>,
    partial: std::collections::HashMap<u32, Vec<u8>>,
    failed: bool,
}

impl<'a> OggPackets<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            ready: Default::default(),
            partial: Default::default(),
            failed: false,
        }
    }

    fn read_page(&mut self) -> Result<bool, String> {
        let rest = &self.bytes[self.pos..];
        if rest.is_empty() {
            return Ok(false);
        }
        if rest.len() < 27 || !is_ogg(rest) {
            return Err(format!("no page at byte {}", self.pos));
        }
        let granule = u64::from_le_bytes(rest[6..14].try_into().unwrap());
        let serial = u32::from_le_bytes(rest[14..18].try_into().unwrap());
        let segment_count = rest[26] as usize;
        let table_end = 27 + segment_count;
        if rest.len() < table_end {
            return Err("truncated page header".into());
        }
        let lacing = &rest[27..table_end];
        let body_len: usize = lacing.iter().map(|&l| l as usize).sum();
        if rest.len() < table_end + body_len {
            return Err("truncated page".into());
        }
        let body = &rest[table_end..table_end + body_len];
        let continued = rest[5] & 0x01 != 0;

        let mut packet = self.partial.remove(&serial).unwrap_or_default();
        if !continued {
            packet.clear();
        }
        let mut offset = 0;
        let mut completed = Vec::new();
        for &len in lacing {
            packet.extend_from_slice(&body[offset..offset + len as usize]);
            offset += len as usize;
            if len < 255 {
                completed.push(std::mem::take(&mut packet));
            }
        }
        if !packet.is_empty() {
            self.partial.insert(serial, packet);
        }
        let last = completed.len().saturating_sub(1);
        // A granule of -1 means no packet finishes on this page.
        let page_granule = (granule != u64::MAX).then_some(granule);
        for (i, data) in completed.into_iter().enumerate() {
            self.ready.push_back(OggPacket {
                serial,
                data,
                granule: if i == last { page_granule } else { None },
            });
        }
        self.pos += table_end + body_len;
        Ok(true)
    }
}

impl Iterator for OggPackets<'_> {
    type Item = Result<OggPacket, String>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.ready.is_empty() {
            if self.failed {
                return None;
            }
            match self.read_page() {
                Ok(true) => {}
                Ok(false) => return None,
                Err(e) => {
                    self.failed = true;
                    return Some(Err(e));
                }
            }
        }
        self.ready.pop_front().map(Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiopus::coder::Encoder;
    use audiopus::Application;

    /// Writes `packets` as one Ogg stream with at most `max_segments` lacing
    /// values per page, so long packets continue on the next page.
    fn ogg_stream(packets: &[(Vec<u8>, u64)], max_segments: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut seq = 0u32;
        for (index, (packet, granule)) in packets.iter().enumerate() {
            let mut lacing = vec![255u8; packet.len() / 255];
            lacing.push((packet.len() % 255) as u8);
            let pages: Vec<&[u8]> = lacing.chunks(max_segments).collect();
            let mut offset = 0;
            for (p, page_lacing) in pages.iter().enumerate() {
                let len: usize = page_lacing.iter().map(|&l| l as usize).sum();
                let last_page = p == pages.len() - 1;
                let mut header_type = 0u8;
                if p > 0 {
                    header_type |= 0x01;
                }
                if seq == 0 {
                    header_type |= 0x02;
                }
                if index == packets.len() - 1 && last_page {
                    header_type |= 0x04;
                }
                let page_granule = if last_page { *granule } else { u64::MAX };
                out.extend_from_slice(b"OggS");
                out.push(0);
                out.push(header_type);
                out.extend_from_slice(&page_granule.to_le_bytes());
                out.extend_from_slice(&7u32.to_le_bytes());
                out.extend_from_slice(&seq.to_le_bytes());
                out.extend_from_slice(&[0; 4]); // CRC is not checked
                out.push(page_lacing.len() as u8);
                out.extend_from_slice(page_lacing);
                out.extend_from_slice(&packet[offset..offset + len]);
                offset += len;
                seq += 1;
            }
        }
        out
    }

    fn opus_head(pre_skip: u16) -> Vec<u8> {
        let mut head = b"OpusHead".to_vec();
        head.push(1);
        head.push(1);
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&48_000u32.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes());
        head.push(0);
        head
    }

    fn voice_message(seconds: usize, trim_last_ms: u64) -> Vec<u8> {
        let encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip).unwrap();
        let frame = 960; // 20 ms at 48 kHz
        let tone: Vec<i16> = (0..48_000 * seconds)
            .map(|i| ((i as f32 * 0.03).sin() * 8000.0) as i16)
            .collect();
        let mut packets = vec![
            (opus_head(312), 0),
            (b"OpusTags\0\0\0\0\0\0\0\0".to_vec(), 0),
        ];
        let mut buf = vec![0u8; 4000];
        for (i, chunk) in tone.chunks(frame).enumerate() {
            let n = encoder.encode(chunk, &mut buf).unwrap();
            packets.push((buf[..n].to_vec(), ((i + 1) * frame) as u64));
        }
        packets.last_mut().unwrap().1 -= trim_last_ms * 48;
        ogg_stream(&packets, 255)
    }

    #[test]
    fn decodes_a_voice_message_to_16khz() {
        let pcm = decode_ogg_opus(&voice_message(2, 0)).unwrap();
        // 2 s of 16 kHz minus the 312-sample (48 kHz) encoder delay.
        assert_eq!(pcm.len(), 32_000 - 104);
        let loud = pcm.iter().filter(|s| s.unsigned_abs() > 1000).count();
        assert!(loud > pcm.len() / 2, "decoded audio is mostly silent");
    }

    #[test]
    fn trims_padding_after_the_last_granule() {
        let pcm = decode_ogg_opus(&voice_message(1, 10)).unwrap();
        assert_eq!(pcm.len(), 16_000 - 104 - 160);
    }

    #[test]
    fn joins_packets_that_span_pages() {
        let big = vec![7u8; 700];
        let stream = ogg_stream(&[(big.clone(), 5)], 1);
        let packets: Vec<_> = OggPackets::new(&stream).collect::<Result<_, _>>().unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].data, big);
        assert_eq!(packets[0].granule, Some(5));
    }

    #[test]
    fn rejects_other_files() {
        assert!(decode_ogg_opus(b"RIFF....WAVE").is_err());
        let vorbis = ogg_stream(&[(b"\x01vorbis".to_vec(), 0)], 255);
        assert!(decode_ogg_opus(&vorbis)
            .unwrap_err()
            .contains("only Ogg Opus"));
    }
}
