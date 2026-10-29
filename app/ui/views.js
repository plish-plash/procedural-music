// Drawn views of values, from the engine's report: piano roll and staff (notes), keyboards
// (scales and chords), step grids (rhythms) and curves (controls). Node faces, the
// inspector and the wire probe all draw with these.
//
// Views that follow playback are marked `data-preview` and have a `showAt(tick)` method;
// rolls and staves have a `.roll-playhead` line. `showPlayhead` moves both.

import { el, svg } from './dom.js';

// Voice colours for piano rolls: soprano, alto, tenor, bass, then extra voices.
export const VOICE_COLOR = ['#ff8a65', '#ffd54f', '#81c784', '#64b5f6', '#ba68c8', '#4dd0e1', '#f06292', '#aed581'];

/** Lights up the views inside `container` at `tick` (null: stopped). */
export function showPlayhead(container, tick) {
  for (const preview of container.querySelectorAll('[data-preview]')) preview.showAt?.(tick);
  for (const line of container.querySelectorAll('.roll-playhead')) {
    const length = Number(line.dataset.length);
    const left = Number(line.dataset.left ?? ROLL.left);
    if (tick == null || !length) {
      line.setAttribute('visibility', 'hidden');
      continue;
    }
    const x = left + ((tick % length) / length) * (ROLL.width - left);
    line.setAttribute('x1', x);
    line.setAttribute('x2', x);
    line.setAttribute('visibility', 'visible');
  }
}

/** A view without editor state (no Roll/Staff choice): for the probe. */
export function renderPlain(v) {
  if (v?.kind === 'keys') return [renderKeys(v)];
  if (v?.kind === 'steps') return [renderSteps(v)];
  if (v?.kind === 'curve') return [renderCurve(v)];
  if (v?.kind === 'roll') return renderRoll(v);
  return renderTheory(v);
}

/** The theory views (circle of fifths, Tonnetz, interval matrix), or nothing. */
export function renderTheory(v) {
  if (v?.kind === 'circle') return [renderCircle(v)];
  if (v?.kind === 'tonnetz') return [renderTonnetz(v)];
  if (v?.kind === 'intervals') return [renderIntervals(v)];
  return [];
}

// ---- Piano roll and staff -----------------------------------------------------------

const ROLL = { width: 360, left: 26, row: 8, min: 110, max: 230 };
const ROLL_COMPACT = { ...ROLL, row: 5, min: 54, max: 96 };

function summary(minor, broken) {
  const parts = [];
  if (broken) parts.push(`${broken} rule${broken === 1 ? '' : 's'} broken`);
  if (minor) parts.push(`${minor} minor issue${minor === 1 ? '' : 's'}`);
  return parts.length ? parts.join(', ') : 'Follows every rule';
}

/** The summary line above a roll or staff. */
function viewHead(v) {
  const head = el('div', { class: 'roll-head' });
  const issues = v.marks ?? [];
  if (v.cost !== undefined) {
    const hard = issues.filter((m) => m.hard).length;
    head.append(
      el('span', {}, summary(issues.length - hard, hard)),
      el('span', { class: 'roll-cost', title: 'Total cost: lower is better' }, `cost ${v.cost}`),
    );
    if (v.pending) head.append(el('span', { class: 'roll-pending' }, 'refining…'));
    head.classList.toggle('bad', issues.some((m) => m.hard));
  } else {
    head.append(el('span', { class: 'roll-cost' }, ''));
  }
  return head;
}

/** Per-note marks and tooltip text, shared by the roll and the staff. */
function noteInfo(v) {
  const notes = v.notes ?? [];
  const issues = v.marks ?? [];
  const marksFor = new Map();
  for (const m of issues) {
    for (const i of [m.n, m.o]) if (i != null) (marksFor.get(i) ?? marksFor.set(i, []).get(i)).push(m);
  }
  const alts = new Map((v.alts ?? []).map((a) => [a.n, a]));
  return notes.map((n, i) => {
    const marks = marksFor.get(i) ?? [];
    const voice = v.voices?.[n.v] ?? `Voice ${n.v + 1}`;
    const lines = [`${voice}: ${n.name}${n.fixed ? ' (fixed)' : ''}`];
    for (const m of marks) {
      const other = m.o != null && notes[m.o === i ? m.n : m.o];
      const with_ = other && other !== n ? ` (with ${v.voices?.[other.v] ?? 'another voice'} ${other.name})` : '';
      lines.push(`${m.hard ? '✖' : '⚠'} ${m.rule}${with_}, cost ${m.cost}`);
    }
    const alt = alts.get(i);
    if (alt) lines.push(`Next best: ${alt.name}, costs ${alt.delta} more`);
    else if (!n.fixed && v.cost !== undefined) lines.push('No other note fits here');
    const cls = `${n.fixed ? ' fixed' : ''}${marks.length ? ' marked' : ''}${marks.some((m) => m.hard) ? ' hard' : ''}`;
    return { cls, tip: lines.join('\n') };
  });
}

function legend(v, notes) {
  const out = el('div', { class: 'roll-legend' });
  for (const vi of [...new Set(notes.map((n) => n.v))].sort((a, b) => a - b)) {
    out.append(el('span', { style: `--c:${VOICE_COLOR[vi % VOICE_COLOR.length]}` }, v.voices?.[vi] ?? `Voice ${vi + 1}`));
  }
  return out;
}

function issueLinks(v, root, pos) {
  for (const m of v.marks ?? []) {
    if (m.o == null || m.o === m.n) continue;
    const a = v.notes[m.n];
    const b = v.notes[m.o];
    if (!a || !b) continue;
    const pa = pos(a);
    const pb = pos(b);
    root.append(svg('line', { x1: pa.x, y1: pa.y, x2: pb.x, y2: pb.y, class: `roll-link${m.hard ? ' hard' : ''}` }));
  }
}

/**
 * Builds a piano-roll SVG (plus a summary line) from an engine view. A compact roll
 * (the preview of any notes output) is shorter and has no summary or legend.
 */
export function renderRoll(v) {
  const notes = v.notes ?? [];
  const head = v.compact ? null : viewHead(v);
  if (!notes.length) return [head, el('div', { class: 'roll-empty' }, 'No notes yet')].filter(Boolean);

  const size = v.compact ? ROLL_COMPACT : ROLL;
  const lo = Math.min(...notes.map((n) => n.p)) - 2;
  const hi = Math.max(...notes.map((n) => n.p)) + 2;
  const rows = hi - lo + 1;
  const row = Math.max(2, Math.min(size.row, (size.max - 8) / rows));
  const height = Math.max(size.min, rows * row + 8);
  const top = (height - rows * row) / 2;
  const W = ROLL.width;
  const x = (t) => ROLL.left + (t / v.length) * (W - ROLL.left);
  const y = (p) => top + (hi - p) * row;
  const root = svg('svg', { class: 'roll', viewBox: `0 0 ${W} ${height}`, width: W, height });

  // Octave lines and labels at each C; bar lines.
  for (let p = lo; p <= hi; p++) {
    if (((p % 12) + 12) % 12 !== 0) continue;
    root.append(svg('line', { x1: ROLL.left, x2: W, y1: y(p) + row, y2: y(p) + row, class: 'roll-oct' }));
    const label = svg('text', { x: 2, y: y(p) + row, class: 'roll-label' });
    label.textContent = `C${Math.floor(p / 12) - 1}`;
    root.append(label);
  }
  for (let t = 0; t <= v.length; t += v.bar) {
    root.append(svg('line', { x1: x(t), x2: x(t), y1: 0, y2: height, class: 'roll-bar' }));
  }

  const info = noteInfo(v);
  notes.forEach((n, i) => {
    const w = Math.max(2, x(n.t + n.d) - x(n.t) - 1);
    const rect = svg('rect', {
      x: x(n.t) + 0.5, y: y(n.p) + 0.5, width: w, height: Math.max(2, row - 1), rx: 1.5,
      class: `roll-note${info[i].cls}`,
      style: `--c:${VOICE_COLOR[n.v % VOICE_COLOR.length]}`,
    });
    const title = svg('title');
    title.textContent = info[i].tip;
    rect.append(title);
    root.append(rect);
  });
  // Lines joining the two notes of each two-voice issue (parallels, crossing, ...).
  issueLinks(v, root, (n) => ({ x: x(n.t) + 3, y: y(n.p) + row / 2 }));
  root.append(svg('line', { x1: 0, x2: 0, y1: 0, y2: height, class: 'roll-playhead', visibility: 'hidden', 'data-length': v.length, 'data-left': ROLL.left }));
  return v.compact ? [root] : [head, root, legend(v, notes)];
}

// ---- Keyboard and step-grid previews -------------------------------------------------

const WHITE_PCS = [0, 2, 4, 5, 7, 9, 11];
const KEYS = { width: 210, height: 34, black: 0.62 };

/** The segment of a looping timeline view sounding at `tick` (null: the first). */
function segmentAt(v, tick) {
  if (tick == null || !v.length) return 0;
  const t = tick % v.length;
  const i = v.segs.findIndex((s) => t >= s.t && t < s.t + s.d);
  return i < 0 ? v.segs.length - 1 : i;
}

/**
 * A keyboard for a scale (one octave, its pitch classes lit) or a chord timeline (the
 * octaves its voicings span, its notes lit). A timeline also gets a strip of its steps;
 * during playback the sounding step is lit, and hovering a step shows it.
 */
export function renderKeys(v) {
  const segs = v.segs ?? [];
  const isScale = v.of === 'scale';
  let lo = 0;
  let hi = 11;
  if (!isScale) {
    const all = segs.flatMap((s) => s.keys);
    lo = all.length ? Math.floor(Math.min(...all) / 12) * 12 : 60;
    hi = all.length ? Math.floor(Math.max(...all) / 12) * 12 + 11 : 71;
  }
  const whites = [];
  for (let k = lo; k <= hi; k++) if (WHITE_PCS.includes(k % 12)) whites.push(k);
  const ww = KEYS.width / whites.length;
  const root = svg('svg', { class: 'keys', viewBox: `0 0 ${KEYS.width} ${KEYS.height}`, width: KEYS.width, height: KEYS.height });
  const keyEls = new Map();
  whites.forEach((k, i) => {
    const r = svg('rect', { x: i * ww + 0.5, y: 0.5, width: ww - 1, height: KEYS.height - 1, rx: 2, class: 'key white' });
    keyEls.set(k, r);
    root.append(r);
  });
  whites.forEach((k, i) => {
    // A black key sits on the boundary after every white key except E and B.
    const b = k + 1;
    if (b > hi || WHITE_PCS.includes(b % 12)) return;
    const bw = ww * KEYS.black;
    const r = svg('rect', { x: (i + 1) * ww - bw / 2, y: 0, width: bw, height: KEYS.height * 0.6, rx: 1.5, class: 'key black' });
    keyEls.set(b, r);
    root.append(r);
  });
  // C labels show where octaves start on a chord keyboard.
  if (!isScale) {
    whites.forEach((k, i) => {
      if (k % 12 !== 0) return;
      const t = svg('text', { x: i * ww + ww / 2, y: KEYS.height - 4, class: 'key-label', 'text-anchor': 'middle' });
      t.textContent = `C${k / 12 - 1}`;
      root.append(t);
    });
  }

  const label = el('div', { class: 'keys-label' });
  const strip = segs.length > 1 ? el('div', { class: 'keys-strip' }) : null;
  const cells = [];
  const wrap = el('div', { class: `keys-view of-${isScale ? 'scale' : 'chord'}`, dataset: { preview: 'keys' } }, label, root, strip);
  let current = 0;
  let hover = null;
  const show = (i) => {
    const seg = segs[i];
    if (!seg) return;
    const lit = new Set(seg.keys.map((k) => (isScale ? ((k % 12) + 12) % 12 : k)));
    for (const [k, r] of keyEls) {
      r.classList.toggle('on', lit.has(k));
      r.classList.toggle('root', lit.has(k) && seg.root != null && k % 12 === seg.root);
    }
    label.textContent = seg.label;
    cells.forEach((c, j) => c.classList.toggle('now', j === i));
  };
  if (strip) {
    segs.forEach((s, i) => {
      const c = el('span', { class: 'keys-seg', style: `flex: ${s.d} 1 0`, title: s.label }, s.label);
      c.addEventListener('pointerenter', () => { hover = i; show(i); });
      c.addEventListener('pointerleave', () => { hover = null; show(current); });
      cells.push(c);
      strip.append(c);
    });
  }
  wrap.showAt = (tick) => {
    current = segmentAt(v, tick);
    if (hover == null) show(current);
  };
  wrap.showAt(null);
  return wrap;
}

/** A rhythm as a row of grid steps: hits filled (brighter when louder, dashed when they
 * might not play, outlined when dropped), bar lines, and the step under the playhead lit. */
export function renderSteps(v) {
  const length = Math.max(1, v.length);
  let grid = v.grid > 0 ? v.grid : length / 16;
  let n = Math.max(1, Math.round(length / grid));
  if (n > 64) {
    n = 64;
    grid = length / n;
  }
  const W = KEYS.width;
  const H = 18;
  const cw = W / n;
  const root = svg('svg', { class: 'steps', viewBox: `0 0 ${W} ${H}`, width: W, height: H });
  const cellEls = [];
  for (let i = 0; i < n; i++) {
    const r = svg('rect', { x: i * cw + 0.5, y: 0.5, width: Math.max(1, cw - 1), height: H - 1, rx: Math.min(2, cw / 4), class: 'step' });
    cellEls.push(r);
    root.append(r);
  }
  for (const o of v.onsets ?? []) {
    const r = cellEls[Math.min(n - 1, Math.floor(o.t / grid))];
    r.classList.add('hit');
    if (o.prob < 100) r.classList.add(o.prob === 0 ? 'dropped' : 'maybe');
    r.style.opacity = String((0.45 + 0.55 * (o.vel / 127)) * (o.prob < 100 ? 0.4 + 0.6 * (o.prob / 100) : 1));
  }
  if (v.bar && length > v.bar) {
    for (let t = v.bar; t < length; t += v.bar) {
      const x = (t / length) * W;
      root.append(svg('line', { x1: x, x2: x, y1: 0, y2: H, class: 'step-bar' }));
    }
  }
  const wrap = el('div', { class: 'steps-view', dataset: { preview: 'steps' } }, root);
  let now = null;
  wrap.showAt = (tick) => {
    now?.classList.remove('now');
    now = tick == null ? null : cellEls[Math.min(n - 1, Math.floor((tick % length) / grid))];
    now?.classList.add('now');
  };
  return wrap;
}

const LETTERS = 'CDEFGAB';

/** Diatonic step (C4 = 28) and accidental of a spelled note name such as "F#4" or "Bb3". */
export function parseName(name) {
  const m = /^([A-G])(bb|b|##|#|x)?(-?\d+)$/.exec(name);
  if (!m) return null;
  const acc = { b: -1, bb: -2, '#': 1, '##': 2, x: 2 }[m[2]] ?? 0;
  return { step: Number(m[3]) * 7 + LETTERS.indexOf(m[1]), acc };
}

/** A simple grand staff: noteheads placed in time, coloured by voice. */
export function renderStaff(v) {
  const notes = v.notes ?? [];
  const head = viewHead(v);
  if (!notes.length) return [head, el('div', { class: 'roll-empty' }, 'No notes yet')];
  const parsed = notes.map((n) => parseName(n.name));
  const steps = parsed.map((p) => p.step);
  const half = 3.2; // one diatonic step, in px
  const topStep = Math.max(40, Math.max(...steps) + 2);
  const bottomStep = Math.min(16, Math.min(...steps) - 2);
  const height = (topStep - bottomStep) * half + 12;
  const y = (s) => 6 + (topStep - s) * half;
  const W = ROLL.width;
  const left = 30;
  const x = (t) => left + 6 + (t / v.length) * (W - left - 10);
  const root = svg('svg', { class: 'roll staff', viewBox: `0 0 ${W} ${height}`, width: W, height });
  // Treble lines E4 G4 B4 D5 F5 and bass lines G2 B2 D3 F3 A3.
  for (const s of [30, 32, 34, 36, 38, 18, 20, 22, 24, 26]) {
    root.append(svg('line', { x1: 4, x2: W - 2, y1: y(s), y2: y(s), class: 'staff-line' }));
  }
  const clef = (text, s, size) => {
    const t = svg('text', { x: 6, y: y(s), class: 'staff-clef', 'font-size': size });
    t.textContent = text;
    root.append(t);
  };
  clef('𝄞', 31.2, 25);
  clef('𝄢', 24.6, 21);
  for (let t = v.bar; t < v.length; t += v.bar) {
    root.append(svg('line', { x1: x(t) - 4, x2: x(t) - 4, y1: y(38), y2: y(18), class: 'staff-bar' }));
  }
  root.append(svg('line', { x1: W - 2, x2: W - 2, y1: y(38), y2: y(18), class: 'staff-bar' }));
  const info = noteInfo(v);
  notes.forEach((n, i) => {
    const { step, acc } = parsed[i];
    const cx = x(n.t);
    const cy = y(step);
    // Ledger lines above, below and for middle C.
    const ledgers = [];
    for (let s = 40; s <= step; s += 2) ledgers.push(s);
    for (let s = 16; s >= step; s -= 2) ledgers.push(s);
    if (step === 28) ledgers.push(28);
    for (const s of ledgers) root.append(svg('line', { x1: cx - 6, x2: cx + 6, y1: y(s), y2: y(s), class: 'staff-line' }));
    if (acc) {
      const a = svg('text', { x: cx - 11, y: cy + 3.5, class: 'staff-acc' });
      a.textContent = { 1: '♯', 2: '𝄪', '-1': '♭', '-2': '𝄫' }[acc];
      root.append(a);
    }
    const g = svg('g', { class: `staff-note${info[i].cls}`, style: `--c:${VOICE_COLOR[n.v % VOICE_COLOR.length]}` });
    g.append(svg('ellipse', { cx, cy, rx: 4.2, ry: 3, transform: `rotate(-20 ${cx} ${cy})` }));
    const title = svg('title');
    title.textContent = info[i].tip;
    g.append(title);
    root.append(g);
  });
  issueLinks(v, root, (n) => ({ x: x(n.t), y: y(parseName(n.name).step) }));
  root.append(svg('line', { x1: 0, x2: 0, y1: 0, y2: height, class: 'roll-playhead', visibility: 'hidden', 'data-length': v.length, 'data-left': left + 6 }));
  return [head, root, legend(v, notes)];
}

// ---- Theory views ------------------------------------------------------------------
// Each gets {chords, key}: keyboard views (timelines of segments with `keys`: MIDI notes
// for chords, pitch classes for a key), either of which may be null.

const PC_NAMES = ['C', 'D♭', 'D', 'E♭', 'E', 'F', 'F♯', 'G', 'A♭', 'A', 'B♭', 'B'];
const pcOf = (k) => ((k % 12) + 12) % 12;

function theoryEmpty() {
  return el('div', { class: 'theory-empty' }, 'Wire in chords (or add a Harmonic context).');
}

/** Calls `draw(chord segment index, key segment index)` (-1: none) whenever the playhead
 * moves into another chord or key, and once to start. */
function followSegments(wrap, v, draw) {
  let last = null;
  wrap.showAt = (tick) => {
    const ci = v.chords ? segmentAt(v.chords, tick) : -1;
    const ki = v.key ? segmentAt(v.key, tick) : -1;
    if (`${ci}/${ki}` === last) return;
    last = `${ci}/${ki}`;
    draw(ci, ki);
  };
  wrap.showAt(null);
}

/**
 * The circle of fifths: the key's notes around the rim (its tonic stronger), the sounding
 * chord's notes joined as a shape inside, and the path its roots take through the
 * progression.
 */
export function renderCircle(v) {
  if (!v.chords && !v.key) return theoryEmpty();
  const S = 210;
  const c = S / 2;
  const R = 86;
  const r = 60;
  const at = (pc, rad) => {
    const a = (((pc * 7) % 12) * 30 - 90) * (Math.PI / 180);
    return [c + rad * Math.cos(a), c + rad * Math.sin(a)];
  };
  const root = svg('svg', { class: 'theory circle', viewBox: `0 0 ${S} ${S}`, width: S, height: S });
  root.append(svg('circle', { cx: c, cy: c, r: R, class: 'cf-ring' }), svg('circle', { cx: c, cy: c, r, class: 'cf-ring inner' }));
  // The chord roots' path through the progression.
  if (v.chords?.segs.length > 1) {
    const roots = v.chords.segs.map((s) => s.root ?? pcOf(Math.min(...s.keys)));
    root.append(svg('polyline', { class: 'cf-path', points: roots.map((pc) => at(pc, r * 0.55).join(',')).join(' ') }));
  }
  const shape = svg('polygon', { class: 'cf-chord' });
  root.append(shape);
  const rim = new Map();
  const inner = new Map();
  for (let pc = 0; pc < 12; pc++) {
    const [x, y] = at(pc, R);
    const dot = svg('circle', { cx: x, cy: y, r: 12, class: 'cf-key' });
    const t = svg('text', { x, y: y + 4, class: 'cf-name', 'text-anchor': 'middle' });
    t.textContent = PC_NAMES[pc];
    const [ix, iy] = at(pc, r);
    const note = svg('circle', { cx: ix, cy: iy, r: 4, class: 'cf-note' });
    rim.set(pc, dot);
    inner.set(pc, note);
    root.append(dot, t, note);
  }
  const label = el('div', { class: 'theory-label' });
  const wrap = el('div', { class: 'theory-view', dataset: { preview: 'circle' } }, label, root);
  followSegments(wrap, v, (ci, ki) => {
    const key = ki >= 0 ? v.key.segs[ki] : null;
    const keyPcs = new Set((key?.keys ?? []).map(pcOf));
    for (const [pc, dot] of rim) {
      dot.classList.toggle('on', keyPcs.has(pc));
      dot.classList.toggle('tonic', key?.root === pc);
    }
    const seg = ci >= 0 ? v.chords.segs[ci] : null;
    const pcs = [...new Set((seg?.keys ?? []).map(pcOf))].sort((a, b) => ((a * 7) % 12) - ((b * 7) % 12));
    for (const [pc, note] of inner) {
      note.classList.toggle('on', pcs.includes(pc));
      note.classList.toggle('root', seg?.root === pc);
    }
    shape.setAttribute('points', pcs.map((pc) => at(pc, r).join(',')).join(' '));
    label.textContent = [key?.label, seg?.label].filter(Boolean).join(' · ');
  });
  return wrap;
}

/**
 * The Tonnetz: notes on a lattice with fifths running across and major thirds up, so
 * each upward triangle is a major triad and each downward one a minor triad. The sounding
 * chord's notes and triads light up; the previous chord's triads stay faintly lit.
 */
export function renderTonnetz(v) {
  if (!v.chords) return theoryEmpty();
  const cols = 9;
  const rows = 4;
  const w = 28;
  const h = 26;
  const pad = 13;
  // Centred on C: column 4 of the second row.
  const pcAt = (x, y) => pcOf(7 * (x - 4) + 4 * (y - 1));
  const px = (x, y) => pad + x * w + y * (w / 2);
  const py = (y) => pad + (rows - 1 - y) * h;
  const W = 2 * pad + (cols - 1) * w + (rows - 1) * (w / 2);
  const H = 2 * pad + (rows - 1) * h;
  const root = svg('svg', { class: 'theory tonnetz', viewBox: `0 0 ${W} ${H}`, width: W, height: H });
  const triangles = [];
  for (let y = 0; y < rows - 1; y++) {
    for (let x = 0; x < cols - 1; x++) {
      const up = [[x, y], [x + 1, y], [x, y + 1]];
      const down = [[x + 1, y], [x, y + 1], [x + 1, y + 1]];
      for (const tri of [up, down]) {
        const poly = svg('polygon', { class: 'tz-tri', points: tri.map(([a, b]) => `${px(a, b)},${py(b)}`).join(' ') });
        triangles.push({ el: poly, pcs: tri.map(([a, b]) => pcAt(a, b)) });
        root.append(poly);
      }
    }
  }
  const nodes = [];
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const pc = pcAt(x, y);
      const dot = svg('circle', { cx: px(x, y), cy: py(y), r: 9, class: 'tz-node' });
      const t = svg('text', { x: px(x, y), y: py(y) + 3.5, class: 'tz-name', 'text-anchor': 'middle' });
      t.textContent = PC_NAMES[pc];
      nodes.push({ el: dot, pc });
      root.append(dot, t);
    }
  }
  const label = el('div', { class: 'theory-label' });
  const wrap = el('div', { class: 'theory-view', dataset: { preview: 'tonnetz' } }, label, root);
  const pcsOf = (i) => new Set((v.chords.segs[i]?.keys ?? []).map(pcOf));
  followSegments(wrap, v, (ci) => {
    const now = pcsOf(ci);
    const n = v.chords.segs.length;
    const before = n > 1 ? pcsOf((ci - 1 + n) % n) : new Set();
    for (const t of triangles) {
      t.el.classList.toggle('on', t.pcs.every((pc) => now.has(pc)));
      t.el.classList.toggle('prev', n > 1 && t.pcs.every((pc) => before.has(pc)));
    }
    for (const d of nodes) d.el.classList.toggle('on', now.has(d.pc));
    label.textContent = v.chords.segs[ci]?.label ?? '';
  });
  return wrap;
}

const INTERVAL_NAMES = ['P1', 'm2', 'M2', 'm3', 'M3', 'P4', 'TT', 'P5', 'm6', 'M6', 'm7', 'M7'];

/** How an interval of `st` semitones sounds, for colouring. */
function intervalQuality(st) {
  const c = pcOf(st);
  if (c === 0 || c === 7) return 'perfect';
  if (c === 5) return 'fourth';
  if ([3, 4, 8, 9].includes(c)) return 'consonant';
  return 'dissonant';
}

/**
 * The interval matrix of the sounding chord: the interval between every two of its notes,
 * low to high, coloured by how it sounds, and the chord's interval-class vector (how many
 * semitones, whole tones, minor thirds, major thirds, fourths and tritones it holds).
 */
export function renderIntervals(v) {
  if (!v.chords) return theoryEmpty();
  const label = el('div', { class: 'theory-label' });
  const body = el('div', { class: 'iv-body' });
  const wrap = el('div', { class: 'theory-view', dataset: { preview: 'intervals' } }, label, body);
  followSegments(wrap, v, (ci) => {
    const seg = v.chords.segs[ci];
    const keys = [...new Set(seg?.keys ?? [])].sort((a, b) => a - b);
    label.textContent = seg?.label ?? '';
    const name = (k) => `${PC_NAMES[pcOf(k)]}${Math.floor(k / 12) - 1}`;
    const head = el('tr', {}, el('th', {}), ...keys.map((k) => el('th', {}, name(k))));
    const rowsEls = keys.map((a, i) => el('tr', {}, el('th', {}, name(a)), ...keys.map((b, j) => {
      if (j <= i) return el('td', { class: 'iv-blank' });
      const st = b - a;
      // Beyond an octave, the simple interval with "+8" (a compound interval).
      const text = st === 12 ? 'P8' : `${INTERVAL_NAMES[st % 12]}${st > 12 ? '+8' : ''}`;
      return el('td', { class: `iv-${intervalQuality(st)}`, title: `${st} semitone${st === 1 ? '' : 's'}` }, text);
    })));
    // Interval-class vector: each pair's distance around the octave, folded to 1..6.
    const vec = [0, 0, 0, 0, 0, 0];
    const pcs = [...new Set(keys.map(pcOf))];
    for (let i = 0; i < pcs.length; i++) {
      for (let j = i + 1; j < pcs.length; j++) {
        const d = pcOf(pcs[j] - pcs[i]);
        vec[Math.min(d, 12 - d) - 1] += 1;
      }
    }
    const icNames = ['m2', 'M2', 'm3', 'M3', 'P4', 'TT'];
    const vector = el('div', { class: 'iv-vector', title: 'Interval-class vector: how many of each interval the chord holds (inversions count as the same, so a P5 counts as a P4)' },
      el('span', { class: 'iv-vec-label' }, `⟨${vec.join('')}⟩`),
      ...vec.map((n, i) => el('span', { class: `iv-ic iv-${intervalQuality(i + 1)}` }, el('b', {}, String(n)), icNames[i])));
    body.replaceChildren(el('table', { class: 'iv-table' }, head, ...rowsEls), vector);
  });
  return wrap;
}

// ---- Control curves ----------------------------------------------------------------

function num(v) {
  return String(Math.round(v * 100) / 100);
}

/** A control signal as a line over the preview window, with its range, the value at the
 * playhead, and a dot that follows playback. */
export function renderCurve(v) {
  const W = KEYS.width;
  const H = 40;
  const pad = 4;
  const points = v.points ?? [];
  const span = v.max - v.min || 1;
  const x = (t) => (t / Math.max(1, v.length)) * W;
  const y = (val) => (v.max === v.min ? H / 2 : pad + (1 - (val - v.min) / span) * (H - 2 * pad));
  const root = svg('svg', { class: 'curve', viewBox: `0 0 ${W} ${H}`, width: W, height: H });
  for (let t = v.bar; t < v.length; t += v.bar) {
    root.append(svg('line', { x1: x(t), x2: x(t), y1: 0, y2: H, class: 'curve-bar' }));
  }
  root.append(svg('polyline', { class: 'curve-line', points: points.map(([t, val]) => `${x(t)},${y(val)}`).join(' ') }));
  const dot = svg('circle', { r: 3, class: 'curve-dot', visibility: 'hidden' });
  root.append(dot);
  const now = el('span', { class: 'curve-now' });
  const label = el('div', { class: 'curve-label' }, el('span', {}, `${num(v.min)} – ${num(v.max)}`), now);
  const wrap = el('div', { class: 'curve-view', dataset: { preview: 'curve' } }, label, root);
  // The value at a tick, between the sampled points.
  const valueAt = (tick) => {
    const t = tick % Math.max(1, v.length);
    const i = points.findIndex((p) => p[0] > t);
    if (i <= 0) return points[Math.max(0, i === 0 ? 0 : points.length - 1)]?.[1] ?? 0;
    const [t0, a] = points[i - 1];
    const [t1, b] = points[i];
    return a + ((b - a) * (t - t0)) / (t1 - t0);
  };
  wrap.showAt = (tick) => {
    if (tick == null) {
      dot.setAttribute('visibility', 'hidden');
      now.textContent = '';
      return;
    }
    const val = valueAt(tick);
    dot.setAttribute('cx', x(tick % Math.max(1, v.length)));
    dot.setAttribute('cy', y(val));
    dot.setAttribute('visibility', 'visible');
    now.textContent = `now ${num(val)}`;
  };
  return wrap;
}
