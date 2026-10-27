// The song template (New ▸ Song) and the parts it is made of. The Full song example is
// built from the same parts.

export const wire = (fromNode, fromPort, toNode, toPort) => ({ from: { node: fromNode, port: fromPort }, to: { node: toNode, port: toPort } });

// Instrument presets.
export const PLUCK = { wave: 'Saw', detune: 6, cutoff: 800, resonance: 25, env_amount: 3, filter_decay: 200, attack: 2, decay: 300, sustain: 20, release: 300, level: 2 };
export const PAD = { wave: 'Saw', detune: 14, cutoff: 1100, resonance: 10, env_amount: 1.2, filter_decay: 1500, attack: 450, decay: 1200, sustain: 80, release: 1400, level: -7 };
export const BASS = { wave: 'Saw', detune: 4, cutoff: 380, resonance: 35, env_amount: 2.6, filter_decay: 160, attack: 2, decay: 220, sustain: 55, release: 90, glide: 30, legato: true, level: -3 };
export const LEAD = { wave: 'Square', detune: 0, cutoff: 1800, resonance: 20, env_amount: 1.8, filter_decay: 140, attack: 2, decay: 180, sustain: 20, release: 160, level: -9 };

/** Scale node settings for a key written as text ("Eb major", "A harmonic minor"). */
export function keyParams(text) {
  const m = /^\s*([A-Ga-g])([#b]?)\s*(.*)$/.exec(String(text ?? ''));
  if (!m) return { root: 'C', scale: 'Major' };
  const name = m[3].trim().toLowerCase();
  return { root: m[1].toUpperCase() + m[2], scale: name ? name[0].toUpperCase() + name.slice(1) : 'Major' };
}

/** A group node. `inputs`/`outputs` are [name, type] pairs; `exposed` lists 'node.param'. */
export function group(id, title, x, y, { nodes, wires, inputs = [], outputs = [], exposed = [], labels = {}, io }) {
  const xs = nodes.map((n) => n.x);
  const ys = nodes.map((n) => n.y);
  return {
    id, type: 'group', title, x, y, params: {},
    graph: {
      nodes, wires, exposed, labels,
      inputs: inputs.map(([name, type]) => ({ name, type })),
      outputs: outputs.map(([name, type]) => ({ name, type })),
      io: io ?? { in: { x: Math.min(...xs) - 260, y: Math.min(...ys) }, out: { x: Math.max(...xs) + 340, y: Math.min(...ys) } },
    },
  };
}

/** Kick, snare and hats on the drum synth. */
export function drumsPart(id, x, y, { kick = 'X...x...X..x.x..', snare = '....X.......X..x', hats = 'x.x.x.x.x.x.x.X.' } = {}) {
  return group(id, 'Drums', x, y, {
    outputs: [['audio', 'audio']],
    exposed: ['k.pattern', 's.pattern', 'h.pattern', 'kit.level'],
    labels: { 'k.pattern': 'Kick', 's.pattern': 'Snare', 'h.pattern': 'Hats' },
    // Merge's second input (68px down) lines up with the snare.
    io: { in: { x: -280, y: 0 }, out: { x: 976, y: 88 } },
    nodes: [
      { id: 'k', type: 'pattern', x: 0, y: 0, params: { pattern: kick, step: '1/16' } },
      { id: 's', type: 'pattern', x: 0, y: 110, params: { pattern: snare, step: '1/16' } },
      { id: 'h', type: 'pattern', x: 0, y: 220, params: { pattern: hats, step: '1/16' } },
      { id: 'kd', type: 'drum', x: 286, y: 0, params: { sound: 'Kick', note: 0, velocity: 110 } },
      { id: 'sd', type: 'drum', x: 286, y: 110, params: { sound: 'Snare', note: 0, velocity: 100 } },
      { id: 'hd', type: 'drum', x: 286, y: 220, params: { sound: 'Closed hat', note: 0, velocity: 70 } },
      { id: 'm', type: 'merge', x: 516, y: 88, params: {} },
      { id: 'kit', type: 'drum_synth', x: 746, y: 88, params: { tune: 0, decay: 100, level: -4 } },
    ],
    wires: [
      wire('k', 'rhythm', 'kd', 'rhythm'), wire('s', 'rhythm', 'sd', 'rhythm'), wire('h', 'rhythm', 'hd', 'rhythm'),
      wire('kd', 'notes', 'm', 'a'), wire('sd', 'notes', 'm', 'b'), wire('hd', 'notes', 'm', 'c'),
      wire('m', 'notes', 'kit', 'notes'), wire('kit', 'audio', '@out', 'audio'),
    ],
  });
}

/** Chord roots on a Euclidean rhythm into the mono synth (chords from the harmonic context). */
export function bassPart(id, x, y) {
  return group(id, 'Bass', x, y, {
    outputs: [['audio', 'audio']],
    exposed: ['r.hits', 'syn.cutoff'],
    labels: { 'r.hits': 'Notes per bar' },
    // The rhythm sits 22px below Chord root, level with its second input.
    io: { in: { x: -280, y: 0 }, out: { x: 746, y: 0 } },
    nodes: [
      { id: 'r', type: 'euclid', x: 0, y: 22, params: { hits: 5, steps: 8, rotate: 0, step: '1/8' } },
      { id: 'root', type: 'chord_root', x: 286, y: 0, params: { tone: 'Root', octave: 2, gate: 80, velocity: 110 } },
      { id: 'syn', type: 'mono_synth', x: 516, y: 0, params: { ...BASS } },
    ],
    wires: [
      wire('r', 'rhythm', 'root', 'rhythm'),
      wire('root', 'notes', 'syn', 'notes'), wire('syn', 'audio', '@out', 'audio'),
    ],
  });
}

/** Four-note voicings, voice-led, held as pads (chords from the harmonic context). */
export function padsPart(id, x, y) {
  return group(id, 'Pads', x, y, {
    outputs: [['audio', 'audio']],
    exposed: ['syn.cutoff', 'fx.mix'],
    labels: { 'fx.mix': 'Chorus %' },
    io: { in: { x: -280, y: 0 }, out: { x: 1150, y: 0 } },
    nodes: [
      { id: 'v', type: 'voice', x: 0, y: 0, params: { notes: 4, low: 'G3', high: 'G5' } },
      { id: 'vl', type: 'voice_lead', x: 230, y: 0, params: { low: 'G3', high: 'G5' } },
      { id: 'pc', type: 'play_chords', x: 460, y: 0, params: { gate: 98, velocity: 80 } },
      { id: 'syn', type: 'poly_synth', x: 690, y: 0, params: { ...PAD } },
      { id: 'fx', type: 'fx_chorus', x: 920, y: 0, params: { rate: 0.5, depth: 50, mix: 40 } },
    ],
    wires: [
      wire('v', 'chords', 'vl', 'chords'), wire('vl', 'chords', 'pc', 'chords'),
      wire('pc', 'notes', 'syn', 'notes'), wire('syn', 'audio', 'fx', 'audio'), wire('fx', 'audio', '@out', 'audio'),
    ],
  });
}

/** An arpeggio over the chords (from the harmonic context), playing only in the sections
 * listed in its Section node (1 picks the Select's input, other sections get 0: none). */
export function leadPart(id, x, y, sections = 'A, B') {
  const values = sections.split(',').map((s) => s.trim()).filter(Boolean).map((s) => `${s} = 1`).join(', ');
  return group(id, 'Lead', x, y, {
    outputs: [['audio', 'audio']],
    exposed: ['sec.values', 'arp.pattern', 'r.hits'],
    labels: { 'sec.values': 'Plays in', 'r.hits': 'Notes per bar' },
    io: { in: { x: -280, y: 0 }, out: { x: 1262, y: 0 } },
    nodes: [
      { id: 'r', type: 'euclid', x: 0, y: 22, params: { hits: 7, steps: 16, rotate: 2, step: '1/16' } },
      { id: 'arp', type: 'arp', x: 286, y: 0, params: { pattern: 'Up-down', octave: 5, range: 1, gate: 70, velocity: 85 } },
      { id: 'sec', type: 'section', x: 0, y: 160, params: { values, default: 0 } },
      { id: 'sel', type: 'select_notes', x: 516, y: 0, params: { input: 1 }, pins: { params: ['input'] } },
      { id: 'syn', type: 'poly_synth', x: 802, y: 0, params: { ...LEAD } },
      { id: 'fx', type: 'fx_delay', x: 1032, y: 0, params: { time: '1/8 dotted', feedback: 40, mix: 30, ping_pong: true, tone: 3000 } },
    ],
    wires: [
      wire('r', 'rhythm', 'arp', 'rhythm'), wire('arp', 'notes', 'sel', '1'), wire('sec', 'control', 'sel', 'param.input'),
      wire('sel', 'out', 'syn', 'notes'), wire('syn', 'audio', 'fx', 'audio'), wire('fx', 'audio', '@out', 'audio'),
    ],
  });
}

/** Key and progression → harmonic context (which every part follows) and the parts →
 * mixer → reverb → out, positioned for the canvas. */
export function songGraph({ chords, key, leadSections = 'A, B' }) {
  return {
    nodes: [
      // Harmony along the top (the context's chords input is 22px lower than its key),
      // the parts in a column in mixer order, then the mix.
      { id: 'key', type: 'scale', x: 40, y: 40, params: keyParams(key) },
      { id: 'prog', type: 'progression', x: 326, y: 40, params: { chords, notation: 'Roman numerals', bars: 1, octave: 4 } },
      { id: 'ctx', type: 'context', x: 656, y: 18, params: {} },
      padsPart('pads', 920, 40),
      bassPart('bass', 920, 210),
      leadPart('lead', 920, 380),
      drumsPart('drums', 920, 588),
      { id: 'mix', type: 'mixer', x: 1206, y: 300, params: { level_a: -5, pan_a: 0, level_b: -1, pan_b: 0, level_c: -6, pan_c: 0, level_d: 0, pan_d: 0 } },
      { id: 'verb', type: 'fx_reverb', x: 1436, y: 300, params: { size: 72, damping: 45, mix: 14, width: 100 } },
      { id: 'out', type: 'audio_out', x: 1666, y: 300, params: { level: 0 } },
    ],
    wires: [
      wire('key', 'scale', 'prog', 'key'), wire('key', 'scale', 'ctx', 'key'), wire('prog', 'chords', 'ctx', 'chords'),
      wire('pads', 'audio', 'mix', 'a'), wire('bass', 'audio', 'mix', 'b'), wire('lead', 'audio', 'mix', 'c'), wire('drums', 'audio', 'mix', 'd'),
      wire('mix', 'audio', 'verb', 'audio'), wire('verb', 'audio', 'out', 'audio'),
    ],
  };
}

/** New ▸ Song: four parts as groups, and a short arrangement to build on. */
export function songTemplate(bpm = 110) {
  return {
    version: 2,
    bpm,
    ...songGraph({ chords: 'I vi IV V', key: 'C major' }),
    arrangement: {
      sections: [
        { id: 's1', name: 'Intro', bars: 4, bpm: null, overrides: {} },
        { id: 's2', name: 'A', bars: 8, bpm: null, overrides: {} },
        { id: 's3', name: 'B', bars: 8, bpm: null, overrides: {} },
        { id: 's4', name: 'A', bars: 8, bpm: null, overrides: {} },
        { id: 's5', name: 'Outro', bars: 4, bpm: null, overrides: {} },
      ],
    },
  };
}

/** Patch 5 from the design document: Intro 8 / A 16 / B 16 / A 16 / Outro 8, with B up a
 * fourth and busier drums; the lead plays in A and B; the intro and outro thin out. */
export function fullSong() {
  const quiet = { 'drums/k': { pattern: 'X.......X.......' }, 'drums/s': { pattern: '................' }, 'drums/h': { pattern: '....x.......x...' } };
  return {
    version: 2,
    bpm: 104,
    ...songGraph({ chords: 'ii7 V7 Imaj7 vi7', key: 'Eb major' }),
    arrangement: {
      sections: [
        { id: 's1', name: 'Intro', bars: 8, bpm: null, overrides: { ...quiet, mix: { level_b: -60 } } },
        { id: 's2', name: 'A', bars: 16, bpm: null, overrides: {} },
        {
          id: 's3', name: 'B', bars: 16, bpm: null,
          overrides: {
            key: keyParams('Ab major'),
            'drums/k': { pattern: 'X..xX...X..xX.x.' },
            'drums/s': { pattern: '....X..x....X.xx' },
            'drums/h': { pattern: 'xxxxxxxXxxxxxxxX' },
            'bass/r': { hits: 7 },
          },
        },
        { id: 's4', name: 'A', bars: 16, bpm: null, overrides: {} },
        { id: 's5', name: 'Outro', bars: 8, bpm: 96, overrides: { ...quiet, 'pads/syn': { cutoff: 700 } } },
      ],
    },
  };
}
