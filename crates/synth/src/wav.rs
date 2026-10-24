//! 16-bit PCM WAV encoding.

/// Encodes interleaved stereo samples (−1..1) as a 16-bit WAV file.
pub fn encode_stereo(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(2u16.to_le_bytes()); // channels
    out.extend(sample_rate.to_le_bytes());
    out.extend((sample_rate * 4).to_le_bytes()); // byte rate
    out.extend(4u16.to_le_bytes()); // block align
    out.extend(16u16.to_le_bytes()); // bits
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend(v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn header_and_length() {
        let w = super::encode_stereo(&[0.0, 1.0, -1.0, 0.5], 48_000);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 44 + 8);
        assert_eq!(&w[44 + 2..44 + 4], &32767i16.to_le_bytes());
    }
}
