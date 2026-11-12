//! Live MIDI input: the keys held, recent notes and controller values from a keyboard
//! or controller, read by the live input nodes (in Perform mode), and the captures and
//! recordings that turn what is played into node settings.
//!
//! Live input is the one exception to the graph being a pure function of time: the
//! live nodes read this state, so they only play while performing and never export.

use std::collections::{HashMap, VecDeque};

use pm_theory::Ticks;

/// How many recent notes are kept for the live nodes (older ones are dropped).
const KEEP_NOTES: usize = 256;

/// A channel message from a MIDI input. Channels are zero-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    NoteOn { channel: u8, key: u8, velocity: u8 },
    NoteOff { channel: u8, key: u8 },
    Cc { channel: u8, cc: u8, value: u8 },
}

impl Message {
    /// Reads one message; note-on with velocity 0 is a note-off. Other messages
    /// (clock, pitch bend, system) are `None`.
    pub fn parse(bytes: &[u8]) -> Option<Message> {
        let (&status, data) = bytes.split_first()?;
        let channel = status & 0x0f;
        let byte = |i: usize| data.get(i).map(|b| b & 0x7f);
        Some(match status & 0xf0 {
            0x90 if byte(1)? > 0 => Message::NoteOn { channel, key: byte(0)?, velocity: byte(1)? },
            0x80 | 0x90 => Message::NoteOff { channel, key: byte(0)? },
            0xb0 => Message::Cc { channel, cc: byte(0)?, value: byte(1)? },
            _ => return None,
        })
    }
}

/// A note played on the live input, placed in play ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveNote {
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    /// Play tick where it sounds: the input delay after the key went down.
    pub on: Ticks,
    /// Play tick of the release (with the same delay), once it has happened.
    pub off: Option<Ticks>,
    /// Ties the notes made from it to the key's release (see `Note::held`).
    pub tag: u16,
}

/// What the live input is doing.
#[derive(Default)]
pub struct LiveInput {
    /// Recent notes, oldest first (only while playing in Perform mode).
    notes: VecDeque<LiveNote>,
    /// Keys down now: (channel, key, velocity), in the order pressed.
    held: Vec<(u8, u8, u8)>,
    /// The keys of the last chord played, kept after they are released.
    latched: Vec<(u8, u8)>,
    /// Controller values by (channel, controller), and by controller on any channel.
    cc: HashMap<(u8, u8), u8>,
    cc_any: HashMap<u8, u8>,
    next_tag: u16,
    /// Counts changes, so previews know when to refresh.
    pub generation: u64,
}

impl LiveInput {
    /// A key went down.
    pub fn press(&mut self, channel: u8, key: u8, velocity: u8) {
        self.held.retain(|&(c, k, _)| (c, k) != (channel, key));
        if self.held.is_empty() {
            // A new chord starts.
            self.latched.clear();
        }
        self.held.push((channel, key, velocity));
        self.latched.push((channel, key));
        self.generation += 1;
    }

    /// A key came up.
    pub fn release(&mut self, channel: u8, key: u8) {
        self.held.retain(|&(c, k, _)| (c, k) != (channel, key));
        self.generation += 1;
    }

    pub fn set_cc(&mut self, channel: u8, cc: u8, value: u8) {
        self.cc.insert((channel, cc), value);
        self.cc_any.insert(cc, value);
        self.generation += 1;
    }

    /// Whether no keys are down.
    pub fn idle(&self) -> bool {
        self.held.is_empty()
    }

    /// Records a note that sounds from play tick `on`; returns its tag.
    pub fn place(&mut self, channel: u8, key: u8, velocity: u8, on: Ticks) -> u16 {
        self.next_tag = self.next_tag.wrapping_add(1).max(1);
        let tag = self.next_tag;
        self.notes.push_back(LiveNote { channel, key, velocity, on, off: None, tag });
        if self.notes.len() > KEEP_NOTES {
            self.notes.pop_front();
        }
        tag
    }

    /// Ends the sounding note on this key at play tick `off`; returns its tag and end.
    pub fn place_off(&mut self, channel: u8, key: u8, off: Ticks) -> Option<(u16, Ticks)> {
        let n = self
            .notes
            .iter_mut()
            .rev()
            .find(|n| n.channel == channel && n.key == key && n.off.is_none())?;
        let off = off.max(n.on + 1);
        n.off = Some(off);
        Some((n.tag, off))
    }

    /// Forgets the placed notes (play ticks start again on every Play).
    pub fn clear_notes(&mut self) {
        self.notes.clear();
    }

    /// Releases every key (when the input port changes).
    pub fn release_all(&mut self) {
        self.held.clear();
        self.generation += 1;
    }

    /// Recent notes, oldest first.
    pub fn notes(&self) -> impl Iterator<Item = &LiveNote> {
        self.notes.iter()
    }

    /// The keys held on a channel (`None`: any), low to high; with `latch`, the last
    /// chord played when none are.
    pub fn held_keys(&self, channel: Option<u8>, latch: bool) -> Vec<u8> {
        let on = |c: u8| channel.is_none_or(|ch| ch == c);
        let mut keys: Vec<u8> = self.held.iter().filter(|h| on(h.0)).map(|h| h.1).collect();
        if keys.is_empty() && latch {
            keys = self.latched.iter().filter(|h| on(h.0)).map(|h| h.1).collect();
        }
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// A controller's last value on a channel (`None`: any).
    pub fn cc(&self, channel: Option<u8>, cc: u8) -> Option<u8> {
        match channel {
            Some(c) => self.cc.get(&(c, cc)).copied(),
            None => self.cc_any.get(&cc).copied(),
        }
    }
}

/// Live input as one evaluation sees it.
#[derive(Clone, Copy)]
pub struct LiveCtx<'a> {
    pub input: &'a LiveInput,
    /// The play tick at the evaluation's tick 0 (play ticks minus section-local ticks).
    pub offset: i64,
}

impl std::fmt::Debug for LiveCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LiveCtx {{ offset: {} }}", self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_messages() {
        assert_eq!(Message::parse(&[0x91, 60, 100]), Some(Message::NoteOn { channel: 1, key: 60, velocity: 100 }));
        assert_eq!(Message::parse(&[0x90, 60, 0]), Some(Message::NoteOff { channel: 0, key: 60 }));
        assert_eq!(Message::parse(&[0x80, 60, 64]), Some(Message::NoteOff { channel: 0, key: 60 }));
        assert_eq!(Message::parse(&[0xb2, 1, 127]), Some(Message::Cc { channel: 2, cc: 1, value: 127 }));
        assert_eq!(Message::parse(&[0xf8]), None);
        assert_eq!(Message::parse(&[0x90, 60]), None);
    }

    #[test]
    fn held_keys_and_latch() {
        let mut live = LiveInput::default();
        live.press(0, 64, 90);
        live.press(0, 60, 90);
        live.press(1, 67, 90);
        assert_eq!(live.held_keys(None, false), [60, 64, 67]);
        assert_eq!(live.held_keys(Some(0), false), [60, 64]);
        live.release(0, 60);
        live.release(0, 64);
        live.release(1, 67);
        assert!(live.held_keys(None, false).is_empty());
        assert_eq!(live.held_keys(None, true), [60, 64, 67], "the last chord stays latched");
        live.press(0, 62, 90);
        assert_eq!(live.held_keys(None, true), [62], "a new chord replaces it");
    }

    #[test]
    fn notes_end_with_their_key() {
        let mut live = LiveInput::default();
        let a = live.place(0, 60, 100, 480);
        let b = live.place(0, 60, 100, 960);
        assert_ne!(a, b);
        assert_eq!(live.place_off(0, 60, 1000), Some((b, 1000)), "the latest note on the key");
        assert_eq!(live.place_off(0, 60, 1100), Some((a, 1100)));
        assert_eq!(live.place_off(0, 60, 1200), None);
    }
}
