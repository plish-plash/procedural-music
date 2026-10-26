// Built-in example patches (the acceptance patches from the design document).

import { wire, PLUCK, PAD, BASS, LEAD, fullSong } from './templates.js';

export const EXAMPLES = [
  {
    name: 'Full song',
    description: 'Drums, bass, pads and lead as groups, arranged Intro / A / B / A / Outro. B moves up a fourth with busier drums. Open a group to see inside.',
    doc: fullSong(),
  },
  {
    name: 'Modal explorer',
    description: 'One scale, seven modes: move the Degree slider while it plays.',
    doc: {
      version: 2,
      bpm: 112,
      nodes: [
        { id: 'n1', type: 'scale', x: 40, y: 40, params: { root: 'D', scale: 'Dorian' } },
        { id: 'n2', type: 'mode', x: 330, y: 40, params: { degree: 1 } },
        { id: 'n3', type: 'pattern', x: 330, y: 230, params: { pattern: 'x.xx.xx.x.xx.x.x', step: '1/16' } },
        { id: 'n4', type: 'sequencer', x: 620, y: 40, params: { degrees: '1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 14 13 12 11 10 9 8 7 6 5 4 3 2', octave: 4, gate: 80, velocity: 100 } },
        { id: 'n5', type: 'poly_synth', x: 910, y: 40, params: { ...PLUCK } },
        { id: 'n6', type: 'fx_delay', x: 1230, y: 40, params: { time: '1/8 dotted', feedback: 35, mix: 25, ping_pong: true, tone: 3500 } },
        { id: 'n7', type: 'fx_reverb', x: 1230, y: 300, params: { size: 70, damping: 45, mix: 18, width: 100 } },
        { id: 'n8', type: 'audio_out', x: 1540, y: 300, params: { level: 0 } },
        { id: 'n9', type: 'circle_of_fifths', x: 620, y: 300, params: {} },
      ],
      wires: [
        wire('n1', 'scale', 'n2', 'scale'), wire('n2', 'scale', 'n4', 'scale'), wire('n2', 'scale', 'n9', 'key'), wire('n3', 'rhythm', 'n4', 'rhythm'),
        wire('n4', 'notes', 'n5', 'notes'), wire('n5', 'audio', 'n6', 'audio'), wire('n6', 'audio', 'n7', 'audio'), wire('n7', 'audio', 'n8', 'audio'),
      ],
    },
  },
  {
    name: 'Progression to pads',
    description: 'I–vi–IV–V voiced in four parts and voice-led. Disconnect Voice lead to hear the difference.',
    doc: {
      version: 2,
      bpm: 84,
      nodes: [
        { id: 'n0', type: 'scale', x: 40, y: 40, params: { root: 'C', scale: 'Major' } },
        { id: 'n1', type: 'progression', x: 40, y: 260, params: { chords: 'I vi IV V', notation: 'Roman numerals', bars: 1, octave: 4 } },
        { id: 'n2', type: 'voice', x: 360, y: 40, params: { notes: 4, low: 'C3', high: 'C5' } },
        { id: 'n3', type: 'voice_lead', x: 690, y: 40, params: { low: 'C3', high: 'C5' } },
        { id: 'n4', type: 'play_chords', x: 1020, y: 40, params: { gate: 98, velocity: 90 } },
        { id: 'n5', type: 'poly_synth', x: 1020, y: 300, params: { ...PAD } },
        { id: 'n6', type: 'fx_chorus', x: 1340, y: 300, params: { rate: 0.4, depth: 60, mix: 45 } },
        { id: 'n7', type: 'fx_reverb', x: 1340, y: 520, params: { size: 88, damping: 35, mix: 32, width: 100 } },
        { id: 'n8', type: 'audio_out', x: 1650, y: 520, params: { level: 0 } },
      ],
      wires: [
        wire('n0', 'scale', 'n1', 'key'),
        wire('n1', 'chords', 'n2', 'chords'), wire('n2', 'chords', 'n3', 'chords'), wire('n3', 'chords', 'n4', 'chords'), wire('n4', 'notes', 'n5', 'notes'),
        wire('n5', 'audio', 'n6', 'audio'), wire('n6', 'audio', 'n7', 'audio'), wire('n7', 'audio', 'n8', 'audio'),
      ],
    },
  },
  {
    name: 'Full band',
    description: 'Pads, bass, an arpeggiated lead and drums on the built-in synths, all following one progression.',
    doc: {
      version: 2,
      bpm: 104,
      nodes: [
        { id: 'n0', type: 'scale', x: 40, y: 260, params: { root: 'Eb', scale: 'Major' } },
        { id: 'n1', type: 'progression', x: 40, y: 480, params: { chords: 'ii7 V7 Imaj7 vi7', notation: 'Roman numerals', bars: 1, octave: 4 } },
        // Pads
        { id: 'n2', type: 'voice', x: 380, y: 20, params: { notes: 4, low: 'G3', high: 'G5' } },
        { id: 'n3', type: 'voice_lead', x: 720, y: 20, params: { low: 'G3', high: 'G5' } },
        { id: 'n4', type: 'play_chords', x: 1060, y: 20, params: { gate: 98, velocity: 80 } },
        { id: 'n5', type: 'poly_synth', x: 1380, y: 20, params: { ...PAD } },
        { id: 'n6', type: 'fx_chorus', x: 1700, y: 20, params: { rate: 0.5, depth: 50, mix: 40 } },
        // Bass
        { id: 'n7', type: 'euclid', x: 380, y: 480, params: { hits: 5, steps: 8, rotate: 0, step: '1/8' } },
        { id: 'n8', type: 'chord_root', x: 720, y: 480, params: { tone: 'Root', octave: 2, gate: 80, velocity: 110 } },
        { id: 'n9', type: 'mono_synth', x: 1060, y: 480, params: { ...BASS } },
        // Lead
        { id: 'n10', type: 'euclid', x: 380, y: 960, params: { hits: 7, steps: 16, rotate: 2, step: '1/16' } },
        { id: 'n11', type: 'arp', x: 720, y: 960, params: { pattern: 'Up-down', octave: 5, range: 1, gate: 70, velocity: 85 } },
        { id: 'n12', type: 'poly_synth', x: 1060, y: 960, params: { ...LEAD } },
        { id: 'n13', type: 'fx_delay', x: 1380, y: 960, params: { time: '1/8 dotted', feedback: 40, mix: 30, ping_pong: true, tone: 3000 } },
        // Drums
        { id: 'n14', type: 'pattern', x: 40, y: 1360, params: { pattern: 'X...x...X..x.x..', step: '1/16' } },
        { id: 'n15', type: 'pattern', x: 40, y: 1580, params: { pattern: '....X.......X..x', step: '1/16' } },
        { id: 'n16', type: 'pattern', x: 40, y: 1800, params: { pattern: 'x.x.x.x.x.x.x.X.', step: '1/16' } },
        { id: 'n17', type: 'drum', x: 380, y: 1360, params: { sound: 'Kick', note: 0, velocity: 110 } },
        { id: 'n18', type: 'drum', x: 380, y: 1580, params: { sound: 'Snare', note: 0, velocity: 100 } },
        { id: 'n19', type: 'drum', x: 380, y: 1800, params: { sound: 'Closed hat', note: 0, velocity: 70 } },
        { id: 'n20', type: 'merge', x: 720, y: 1560, params: {} },
        { id: 'n21', type: 'drum_synth', x: 1060, y: 1560, params: { tune: 0, decay: 100, level: -4 } },
        // Mix
        { id: 'n22', type: 'mixer', x: 2020, y: 700, params: { level_a: -5, pan_a: 0, level_b: -1, pan_b: 0, level_c: -6, pan_c: 0, level_d: 0, pan_d: 0 } },
        { id: 'n23', type: 'fx_reverb', x: 2340, y: 700, params: { size: 72, damping: 45, mix: 14, width: 100 } },
        { id: 'n24', type: 'audio_out', x: 2650, y: 700, params: { level: 0 } },
      ],
      wires: [
        wire('n0', 'scale', 'n1', 'key'),
        wire('n1', 'chords', 'n2', 'chords'), wire('n2', 'chords', 'n3', 'chords'), wire('n3', 'chords', 'n4', 'chords'), wire('n4', 'notes', 'n5', 'notes'), wire('n5', 'audio', 'n6', 'audio'),
        wire('n1', 'chords', 'n8', 'chords'), wire('n7', 'rhythm', 'n8', 'rhythm'), wire('n8', 'notes', 'n9', 'notes'),
        wire('n1', 'chords', 'n11', 'chords'), wire('n10', 'rhythm', 'n11', 'rhythm'), wire('n11', 'notes', 'n12', 'notes'), wire('n12', 'audio', 'n13', 'audio'),
        wire('n14', 'rhythm', 'n17', 'rhythm'), wire('n15', 'rhythm', 'n18', 'rhythm'), wire('n16', 'rhythm', 'n19', 'rhythm'),
        wire('n17', 'notes', 'n20', 'a'), wire('n18', 'notes', 'n20', 'b'), wire('n19', 'notes', 'n20', 'c'), wire('n20', 'notes', 'n21', 'notes'),
        wire('n6', 'audio', 'n22', 'a'), wire('n9', 'audio', 'n22', 'b'), wire('n13', 'audio', 'n22', 'c'), wire('n21', 'audio', 'n22', 'd'),
        wire('n22', 'audio', 'n23', 'audio'), wire('n23', 'audio', 'n24', 'audio'),
      ],
    },
  },
  {
    name: 'First species',
    description: "Fux's cantus firmus in D Dorian in the bass; the solver writes the soprano. Try other species, or switch rule groups off.",
    doc: {
      version: 2,
      bpm: 150,
      nodes: [
        { id: 'n1', type: 'scale', x: 40, y: 40, params: { root: 'D', scale: 'Dorian' } },
        { id: 'n2', type: 'pattern', x: 40, y: 300, params: { pattern: 'x', step: '1/1' } },
        { id: 'n3', type: 'sequencer', x: 330, y: 160, params: { degrees: '1 3 2 1 4 3 5 4 3 2 1', octave: 3, gate: 96, velocity: 90 } },
        { id: 'n4', type: 'counterpoint', x: 640, y: 40, params: { species: '1st: note against note', soprano: 'Free', alto: 'Off', tenor: 'Off', bass: 'Fixed', bars: 11 } },
        { id: 'n5', type: 'split_voices', x: 1070, y: 40, params: {} },
        { id: 'n6', type: 'poly_synth', x: 1360, y: 40, params: { ...PLUCK, cutoff: 1400, decay: 900, sustain: 45, release: 500, level: -4 } },
        { id: 'n7', type: 'poly_synth', x: 1360, y: 560, params: { ...PAD, attack: 60, cutoff: 900, level: -6 } },
        { id: 'n8', type: 'mixer', x: 1680, y: 300, params: { level_a: 0, pan_a: -15, level_b: 0, pan_b: 15, level_c: 0, pan_c: 0, level_d: 0, pan_d: 0 } },
        { id: 'n9', type: 'fx_reverb', x: 2000, y: 300, params: { size: 80, damping: 40, mix: 24, width: 100 } },
        { id: 'n10', type: 'audio_out', x: 2310, y: 300, params: { level: 0 } },
      ],
      wires: [
        wire('n1', 'scale', 'n3', 'scale'), wire('n2', 'rhythm', 'n3', 'rhythm'), wire('n3', 'notes', 'n4', 'fixed'), wire('n1', 'scale', 'n4', 'key'),
        wire('n4', 'notes', 'n5', 'notes'), wire('n5', 'v1', 'n6', 'notes'), wire('n5', 'v4', 'n7', 'notes'),
        wire('n6', 'audio', 'n8', 'a'), wire('n7', 'audio', 'n8', 'b'), wire('n8', 'audio', 'n9', 'audio'), wire('n9', 'audio', 'n10', 'audio'),
      ],
    },
  },
  {
    name: 'Chorale',
    description: 'Four voices written over a progression in chorale style: chord tones, smooth lines, no parallels. Hover a note to see why it was chosen.',
    doc: {
      version: 2,
      bpm: 76,
      nodes: [
        { id: 'n1', type: 'scale', x: 40, y: 40, params: { root: 'G', scale: 'Major' } },
        { id: 'n2', type: 'progression', x: 40, y: 260, params: { chords: 'I IV vi V IV ii V I', notation: 'Roman numerals', bars: 1, octave: 4 } },
        { id: 'n3', type: 'counterpoint', x: 380, y: 40, params: { species: '1st: note against note', soprano: 'Free', alto: 'Free', tenor: 'Free', bass: 'Free', bars: 8 } },
        { id: 'n4', type: 'poly_synth', x: 810, y: 40, params: { ...PAD, attack: 120, release: 900, level: -6 } },
        { id: 'n5', type: 'fx_reverb', x: 1130, y: 40, params: { size: 85, damping: 35, mix: 30, width: 100 } },
        { id: 'n6', type: 'audio_out', x: 1440, y: 40, params: { level: 0 } },
      ],
      wires: [
        wire('n1', 'scale', 'n2', 'key'), wire('n1', 'scale', 'n3', 'key'), wire('n2', 'chords', 'n3', 'chords'),
        wire('n3', 'notes', 'n4', 'notes'), wire('n4', 'audio', 'n5', 'audio'), wire('n5', 'audio', 'n6', 'audio'),
      ],
    },
  },
  {
    name: 'Full band (MIDI)',
    description: 'The Full band patch sent to MIDI ports instead, for external synths (or the Windows GS synth).',
    doc: {
      version: 2,
      bpm: 104,
      nodes: [
        { id: 'n0', type: 'scale', x: 40, y: 100, params: { root: 'Eb', scale: 'Major' } },
        { id: 'n1', type: 'progression', x: 40, y: 320, params: { chords: 'ii7 V7 Imaj7 vi7', notation: 'Roman numerals', bars: 1, octave: 4 } },
        // Pads
        { id: 'n2', type: 'voice', x: 380, y: 20, params: { notes: 4, low: 'G3', high: 'G5' } },
        { id: 'n3', type: 'voice_lead', x: 720, y: 20, params: { low: 'G3', high: 'G5' } },
        { id: 'n4', type: 'play_chords', x: 1060, y: 20, params: { gate: 98, velocity: 70 } },
        { id: 'n5', type: 'midi_out', x: 1360, y: 20, params: { port: '', channel: 1, program: 90, mute: false } },
        // Bass
        { id: 'n6', type: 'euclid', x: 380, y: 320, params: { hits: 5, steps: 8, rotate: 0, step: '1/8' } },
        { id: 'n7', type: 'chord_root', x: 720, y: 320, params: { tone: 'Root', octave: 2, gate: 80, velocity: 110 } },
        { id: 'n8', type: 'midi_out', x: 1060, y: 320, params: { port: '', channel: 2, program: 34, mute: false } },
        // Lead
        { id: 'n9', type: 'euclid', x: 380, y: 660, params: { hits: 7, steps: 16, rotate: 2, step: '1/16' } },
        { id: 'n10', type: 'arp', x: 720, y: 660, params: { pattern: 'Up-down', octave: 5, range: 1, gate: 70, velocity: 85 } },
        { id: 'n11', type: 'midi_out', x: 1060, y: 660, params: { port: '', channel: 3, program: 82, mute: false } },
        // Drums
        { id: 'n12', type: 'pattern', x: 40, y: 1020, params: { pattern: 'X...x...X..x.x..', step: '1/16' } },
        { id: 'n13', type: 'pattern', x: 40, y: 1240, params: { pattern: '....X.......X..x', step: '1/16' } },
        { id: 'n14', type: 'pattern', x: 40, y: 1460, params: { pattern: 'x.x.x.x.x.x.x.X.', step: '1/16' } },
        { id: 'n15', type: 'drum', x: 380, y: 1020, params: { sound: 'Kick', note: 0, velocity: 110 } },
        { id: 'n16', type: 'drum', x: 380, y: 1240, params: { sound: 'Snare', note: 0, velocity: 100 } },
        { id: 'n17', type: 'drum', x: 380, y: 1460, params: { sound: 'Closed hat', note: 0, velocity: 70 } },
        { id: 'n18', type: 'merge', x: 720, y: 1200, params: {} },
        { id: 'n19', type: 'midi_out', x: 1060, y: 1200, params: { port: '', channel: 10, program: 0, mute: false } },
      ],
      wires: [
        wire('n0', 'scale', 'n1', 'key'),
        wire('n1', 'chords', 'n2', 'chords'), wire('n2', 'chords', 'n3', 'chords'), wire('n3', 'chords', 'n4', 'chords'), wire('n4', 'notes', 'n5', 'notes'),
        wire('n1', 'chords', 'n7', 'chords'), wire('n6', 'rhythm', 'n7', 'rhythm'), wire('n7', 'notes', 'n8', 'notes'),
        wire('n1', 'chords', 'n10', 'chords'), wire('n9', 'rhythm', 'n10', 'rhythm'), wire('n10', 'notes', 'n11', 'notes'),
        wire('n12', 'rhythm', 'n15', 'rhythm'), wire('n13', 'rhythm', 'n16', 'rhythm'), wire('n14', 'rhythm', 'n17', 'rhythm'),
        wire('n15', 'notes', 'n18', 'a'), wire('n16', 'notes', 'n18', 'b'), wire('n17', 'notes', 'n18', 'c'), wire('n18', 'notes', 'n19', 'notes'),
      ],
    },
  },
];
