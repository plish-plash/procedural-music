// Built-in example patches (the acceptance patches from the design document).

const wire = (fromNode, fromPort, toNode, toPort) => ({ from: { node: fromNode, port: fromPort }, to: { node: toNode, port: toPort } });

export const EXAMPLES = [
  {
    name: 'Modal explorer',
    description: 'One scale, seven modes: turn the Degree slider while it plays.',
    doc: {
      version: 1,
      bpm: 112,
      nodes: [
        { id: 'n1', type: 'scale', x: 40, y: 40, params: { root: 'D', scale: 'Dorian' } },
        { id: 'n2', type: 'mode', x: 330, y: 40, params: { degree: 1 } },
        { id: 'n3', type: 'pattern', x: 330, y: 230, params: { pattern: 'x.xx.xx.x.xx.x.x', step: '1/16' } },
        { id: 'n4', type: 'arp', x: 620, y: 40, params: { pattern: 'Up-down', octave: 4, range: 2, gate: 80, velocity: 100 } },
        { id: 'n5', type: 'midi_out', x: 920, y: 40, params: { port: '', channel: 1, program: 12, mute: false } },
      ],
      wires: [wire('n1', 'scale', 'n2', 'scale'), wire('n2', 'scale', 'n4', 'scale'), wire('n3', 'rhythm', 'n4', 'rhythm'), wire('n4', 'notes', 'n5', 'notes')],
    },
  },
  {
    name: 'Progression to pads',
    description: 'I–vi–IV–V voiced in four parts and voice-led. Disconnect Voice lead to hear the difference.',
    doc: {
      version: 1,
      bpm: 84,
      nodes: [
        { id: 'n1', type: 'progression', x: 40, y: 40, params: { chords: 'I vi IV V', notation: 'Roman numerals', key: 'C major', bars: 1, octave: 4 } },
        { id: 'n2', type: 'voice', x: 360, y: 40, params: { notes: 4, low: 'C3', high: 'C5' } },
        { id: 'n3', type: 'voice_lead', x: 690, y: 40, params: { low: 'C3', high: 'C5' } },
        { id: 'n4', type: 'play_chords', x: 1020, y: 40, params: { gate: 98, velocity: 90 } },
        { id: 'n5', type: 'midi_out', x: 1350, y: 40, params: { port: '', channel: 1, program: 49, mute: false } },
      ],
      wires: [wire('n1', 'chords', 'n2', 'chords'), wire('n2', 'chords', 'n3', 'chords'), wire('n3', 'chords', 'n4', 'chords'), wire('n4', 'notes', 'n5', 'notes')],
    },
  },
  {
    name: 'Full band',
    description: 'Pads, bass, an arpeggiated lead and drums, all following one progression.',
    doc: {
      version: 1,
      bpm: 104,
      nodes: [
        { id: 'n1', type: 'progression', x: 40, y: 320, params: { chords: 'ii7 V7 Imaj7 vi7', notation: 'Roman numerals', key: 'Eb major', bars: 1, octave: 4 } },
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
        wire('n1', 'chords', 'n2', 'chords'), wire('n2', 'chords', 'n3', 'chords'), wire('n3', 'chords', 'n4', 'chords'), wire('n4', 'notes', 'n5', 'notes'),
        wire('n1', 'chords', 'n7', 'chords'), wire('n6', 'rhythm', 'n7', 'rhythm'), wire('n7', 'notes', 'n8', 'notes'),
        wire('n1', 'chords', 'n10', 'scale'), wire('n9', 'rhythm', 'n10', 'rhythm'), wire('n10', 'notes', 'n11', 'notes'),
        wire('n12', 'rhythm', 'n15', 'rhythm'), wire('n13', 'rhythm', 'n16', 'rhythm'), wire('n14', 'rhythm', 'n17', 'rhythm'),
        wire('n15', 'notes', 'n18', 'a'), wire('n16', 'notes', 'n18', 'b'), wire('n17', 'notes', 'n18', 'c'), wire('n18', 'notes', 'n19', 'notes'),
      ],
    },
  },
];
