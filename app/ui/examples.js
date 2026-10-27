// Built-in example patches (the acceptance patches from the design document).
//
// Layout: signal flows left to right with 50px between nodes. Most nodes' first port sits
// 46px below their top and the second 68px, so nodes joined on their first ports share a
// top edge, and a rhythm feeding a second input sits 22px lower than the node it feeds;
// those wires run straight.

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
        // The circle sits above the chain, so its wire from Mode rotate rises clear of the
        // rhythm's.
        { id: 'n1', type: 'scale', x: 40, y: 410, params: { root: 'D', scale: 'Dorian' } },
        { id: 'n2', type: 'mode', x: 326, y: 410, params: { degree: 1 } },
        { id: 'n3', type: 'pattern', x: 40, y: 585, params: { pattern: 'x.xx.xx.x.xx.x.x', step: '1/16' } },
        { id: 'n4', type: 'sequencer', x: 606, y: 410, params: { degrees: '1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 14 13 12 11 10 9 8 7 6 5 4 3 2', octave: 4, gate: 80, velocity: 100 } },
        { id: 'n5', type: 'poly_synth', x: 836, y: 410, params: { ...PLUCK } },
        { id: 'n6', type: 'fx_delay', x: 1066, y: 410, params: { time: '1/8 dotted', feedback: 35, mix: 25, ping_pong: true, tone: 3500 } },
        { id: 'n7', type: 'fx_reverb', x: 1296, y: 410, params: { size: 70, damping: 45, mix: 18, width: 100 } },
        { id: 'n8', type: 'audio_out', x: 1526, y: 410, params: { level: 0 } },
        { id: 'n9', type: 'circle_of_fifths', x: 606, y: 40, params: {} },
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
        { id: 'n1', type: 'progression', x: 326, y: 40, params: { chords: 'I vi IV V', notation: 'Roman numerals', bars: 1, octave: 4 } },
        { id: 'n2', type: 'voice', x: 656, y: 40, params: { notes: 4, low: 'C3', high: 'C5' } },
        { id: 'n3', type: 'voice_lead', x: 886, y: 40, params: { low: 'C3', high: 'C5' } },
        { id: 'n4', type: 'play_chords', x: 1116, y: 40, params: { gate: 98, velocity: 90 } },
        { id: 'n5', type: 'poly_synth', x: 1346, y: 40, params: { ...PAD } },
        { id: 'n6', type: 'fx_chorus', x: 1576, y: 40, params: { rate: 0.4, depth: 60, mix: 45 } },
        { id: 'n7', type: 'fx_reverb', x: 1806, y: 40, params: { size: 88, damping: 35, mix: 32, width: 100 } },
        { id: 'n8', type: 'audio_out', x: 2036, y: 40, params: { level: 0 } },
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
        // Lanes, one per part. The nodes that read the chords stand in one column 100px
        // after the progression, and their rhythms in the column beneath it, so the chord
        // wires fan out through clear space.
        { id: 'n0', type: 'scale', x: 40, y: 40, params: { root: 'Eb', scale: 'Major' } },
        { id: 'n1', type: 'progression', x: 326, y: 40, params: { chords: 'ii7 V7 Imaj7 vi7', notation: 'Roman numerals', bars: 1, octave: 4 } },
        // Pads
        { id: 'n2', type: 'voice', x: 706, y: 40, params: { notes: 4, low: 'G3', high: 'G5' } },
        { id: 'n3', type: 'voice_lead', x: 936, y: 40, params: { low: 'G3', high: 'G5' } },
        { id: 'n4', type: 'play_chords', x: 1166, y: 40, params: { gate: 98, velocity: 80 } },
        { id: 'n5', type: 'poly_synth', x: 1396, y: 40, params: { ...PAD } },
        { id: 'n6', type: 'fx_chorus', x: 1626, y: 40, params: { rate: 0.5, depth: 50, mix: 40 } },
        // Bass
        { id: 'n7', type: 'euclid', x: 370, y: 252, params: { hits: 5, steps: 8, rotate: 0, step: '1/8' } },
        { id: 'n8', type: 'chord_root', x: 706, y: 230, params: { tone: 'Root', octave: 2, gate: 80, velocity: 110 } },
        { id: 'n9', type: 'mono_synth', x: 936, y: 230, params: { ...BASS } },
        // Lead
        { id: 'n10', type: 'euclid', x: 370, y: 412, params: { hits: 7, steps: 16, rotate: 2, step: '1/16' } },
        { id: 'n11', type: 'arp', x: 706, y: 390, params: { pattern: 'Up-down', octave: 5, range: 1, gate: 70, velocity: 85 } },
        { id: 'n12', type: 'poly_synth', x: 936, y: 390, params: { ...LEAD } },
        { id: 'n13', type: 'fx_delay', x: 1166, y: 390, params: { time: '1/8 dotted', feedback: 40, mix: 30, ping_pong: true, tone: 3000 } },
        // Drums
        { id: 'n14', type: 'pattern', x: 370, y: 560, params: { pattern: 'X...x...X..x.x..', step: '1/16' } },
        { id: 'n15', type: 'pattern', x: 370, y: 670, params: { pattern: '....X.......X..x', step: '1/16' } },
        { id: 'n16', type: 'pattern', x: 370, y: 780, params: { pattern: 'x.x.x.x.x.x.x.X.', step: '1/16' } },
        { id: 'n17', type: 'drum', x: 706, y: 560, params: { sound: 'Kick', note: 0, velocity: 110 } },
        { id: 'n18', type: 'drum', x: 706, y: 670, params: { sound: 'Snare', note: 0, velocity: 100 } },
        { id: 'n19', type: 'drum', x: 706, y: 780, params: { sound: 'Closed hat', note: 0, velocity: 70 } },
        { id: 'n20', type: 'merge', x: 936, y: 648, params: {} },
        { id: 'n21', type: 'drum_synth', x: 1166, y: 648, params: { tune: 0, decay: 100, level: -4 } },
        // Mix
        { id: 'n22', type: 'mixer', x: 1876, y: 230, params: { level_a: -5, pan_a: 0, level_b: -1, pan_b: 0, level_c: -6, pan_c: 0, level_d: 0, pan_d: 0 } },
        { id: 'n23', type: 'fx_reverb', x: 2106, y: 230, params: { size: 72, damping: 45, mix: 14, width: 100 } },
        { id: 'n24', type: 'audio_out', x: 2336, y: 230, params: { level: 0 } },
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
        { id: 'n2', type: 'pattern', x: 40, y: 215, params: { pattern: 'x', step: '1/1' } },
        { id: 'n3', type: 'sequencer', x: 326, y: 40, params: { degrees: '1 3 2 1 4 3 5 4 3 2 1', octave: 3, gate: 96, velocity: 90 } },
        { id: 'n4', type: 'counterpoint', x: 556, y: 40, params: { species: '1st: note against note', soprano: 'Free', alto: 'Off', tenor: 'Off', bass: 'Fixed', bars: 11 } },
        { id: 'n5', type: 'split_voices', x: 990, y: 40, params: {} },
        { id: 'n6', type: 'poly_synth', x: 1220, y: 40, params: { ...PLUCK, cutoff: 1400, decay: 900, sustain: 45, release: 500, level: -4 } },
        { id: 'n7', type: 'poly_synth', x: 1220, y: 150, params: { ...PAD, attack: 60, cutoff: 900, level: -6 } },
        { id: 'n8', type: 'mixer', x: 1450, y: 40, params: { level_a: 0, pan_a: -15, level_b: 0, pan_b: 15, level_c: 0, pan_c: 0, level_d: 0, pan_d: 0 } },
        { id: 'n9', type: 'fx_reverb', x: 1680, y: 40, params: { size: 80, damping: 40, mix: 24, width: 100 } },
        { id: 'n10', type: 'audio_out', x: 1910, y: 40, params: { level: 0 } },
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
        // The progression's chords meet Counterpoint's third input (90px down), so the
        // key and progression sit 44px lower than it.
        { id: 'n1', type: 'scale', x: 40, y: 84, params: { root: 'G', scale: 'Major' } },
        { id: 'n2', type: 'progression', x: 326, y: 84, params: { chords: 'I IV vi V IV ii V I', notation: 'Roman numerals', bars: 1, octave: 4 } },
        { id: 'n3', type: 'counterpoint', x: 656, y: 40, params: { species: '1st: note against note', soprano: 'Free', alto: 'Free', tenor: 'Free', bass: 'Free', bars: 8 } },
        { id: 'n4', type: 'poly_synth', x: 1090, y: 40, params: { ...PAD, attack: 120, release: 900, level: -6 } },
        { id: 'n5', type: 'fx_reverb', x: 1320, y: 40, params: { size: 85, damping: 35, mix: 30, width: 100 } },
        { id: 'n6', type: 'audio_out', x: 1550, y: 40, params: { level: 0 } },
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
        // Laid out like Full band.
        { id: 'n0', type: 'scale', x: 40, y: 40, params: { root: 'Eb', scale: 'Major' } },
        { id: 'n1', type: 'progression', x: 326, y: 40, params: { chords: 'ii7 V7 Imaj7 vi7', notation: 'Roman numerals', bars: 1, octave: 4 } },
        // Pads
        { id: 'n2', type: 'voice', x: 706, y: 40, params: { notes: 4, low: 'G3', high: 'G5' } },
        { id: 'n3', type: 'voice_lead', x: 936, y: 40, params: { low: 'G3', high: 'G5' } },
        { id: 'n4', type: 'play_chords', x: 1166, y: 40, params: { gate: 98, velocity: 70 } },
        { id: 'n5', type: 'midi_out', x: 1396, y: 40, params: { port: '', channel: 1, program: 90, mute: false } },
        // Bass
        { id: 'n6', type: 'euclid', x: 370, y: 252, params: { hits: 5, steps: 8, rotate: 0, step: '1/8' } },
        { id: 'n7', type: 'chord_root', x: 706, y: 230, params: { tone: 'Root', octave: 2, gate: 80, velocity: 110 } },
        { id: 'n8', type: 'midi_out', x: 936, y: 230, params: { port: '', channel: 2, program: 34, mute: false } },
        // Lead
        { id: 'n9', type: 'euclid', x: 370, y: 412, params: { hits: 7, steps: 16, rotate: 2, step: '1/16' } },
        { id: 'n10', type: 'arp', x: 706, y: 390, params: { pattern: 'Up-down', octave: 5, range: 1, gate: 70, velocity: 85 } },
        { id: 'n11', type: 'midi_out', x: 936, y: 390, params: { port: '', channel: 3, program: 82, mute: false } },
        // Drums
        { id: 'n12', type: 'pattern', x: 370, y: 560, params: { pattern: 'X...x...X..x.x..', step: '1/16' } },
        { id: 'n13', type: 'pattern', x: 370, y: 670, params: { pattern: '....X.......X..x', step: '1/16' } },
        { id: 'n14', type: 'pattern', x: 370, y: 780, params: { pattern: 'x.x.x.x.x.x.x.X.', step: '1/16' } },
        { id: 'n15', type: 'drum', x: 706, y: 560, params: { sound: 'Kick', note: 0, velocity: 110 } },
        { id: 'n16', type: 'drum', x: 706, y: 670, params: { sound: 'Snare', note: 0, velocity: 100 } },
        { id: 'n17', type: 'drum', x: 706, y: 780, params: { sound: 'Closed hat', note: 0, velocity: 70 } },
        { id: 'n18', type: 'merge', x: 936, y: 648, params: {} },
        { id: 'n19', type: 'midi_out', x: 1166, y: 648, params: { port: '', channel: 10, program: 0, mute: false } },
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
