//! A minimal Standard MIDI File (format 1) writer.

use pm_theory::{Note, Ticks, PPQ};

/// One track of timed MIDI messages.
#[derive(Default)]
pub struct Track {
    name: String,
    events: Vec<(Ticks, u8, Vec<u8>)>, // (time, order within the same tick, bytes)
}

impl Track {
    pub fn new(name: &str) -> Track {
        Track {
            name: name.to_string(),
            events: Vec::new(),
        }
    }

    /// Adds a note on `channel` (0–15). Note-offs sort before note-ons at the same tick.
    pub fn note(&mut self, channel: u8, note: &Note) {
        let (key, _) = note.pitch.tuning().to_midi(note.pitch.step());
        let key = key.clamp(0, 127) as u8;
        let ch = channel & 0x0f;
        self.events.push((
            note.start,
            1,
            vec![0x90 | ch, key, note.velocity.clamp(1, 127)],
        ));
        self.events.push((note.end(), 0, vec![0x80 | ch, key, 0]));
    }

    /// Adds a tempo change in beats per minute.
    pub fn tempo(&mut self, at: Ticks, bpm: f64) {
        let us = (60_000_000.0 / bpm).round() as u32;
        self.events.push((
            at,
            0,
            vec![0xff, 0x51, 3, (us >> 16) as u8, (us >> 8) as u8, us as u8],
        ));
    }

    fn bytes(&self) -> Vec<u8> {
        let mut events = self.events.clone();
        events.sort_by_key(|(t, order, _)| (*t, *order));
        let mut data = Vec::new();
        // Track name.
        write_vlq(&mut data, 0);
        data.extend([0xff, 0x03]);
        write_vlq(&mut data, self.name.len() as u32);
        data.extend(self.name.as_bytes());
        let mut last = 0;
        for (t, _, bytes) in &events {
            write_vlq(&mut data, t - last);
            data.extend(bytes);
            last = *t;
        }
        write_vlq(&mut data, 0);
        data.extend([0xff, 0x2f, 0x00]); // end of track
        let mut out = b"MTrk".to_vec();
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        out
    }
}

/// Encodes a whole file.
pub fn file(tracks: &[Track]) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes()); // format 1
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend((PPQ as u16).to_be_bytes());
    for t in tracks {
        out.extend(t.bytes());
    }
    out
}

fn write_vlq(out: &mut Vec<u8>, mut value: u32) {
    let mut buf = [0u8; 5];
    let mut i = buf.len() - 1;
    buf[i] = (value & 0x7f) as u8;
    value >>= 7;
    while value > 0 {
        i -= 1;
        buf[i] = (value & 0x7f) as u8 | 0x80;
        value >>= 7;
    }
    out.extend(&buf[i..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq() {
        let enc = |v| {
            let mut o = Vec::new();
            write_vlq(&mut o, v);
            o
        };
        assert_eq!(enc(0), [0x00]);
        assert_eq!(enc(0x7f), [0x7f]);
        assert_eq!(enc(0x80), [0x81, 0x00]);
        assert_eq!(enc(0x0fffffff), [0xff, 0xff, 0xff, 0x7f]);
    }

    #[test]
    fn header() {
        let bytes = file(&[Track::new("t")]);
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(&bytes[12..14], &960u16.to_be_bytes());
        assert_eq!(&bytes[14..18], b"MTrk");
    }
}
