// The arrangement timeline: sections along the song, an overview of every part's notes,
// the playhead, and the controls for editing sections.
//
// Each part's lane is a thin strip of its notes; clicking its name opens it into a full
// piano roll or staff over the same bars. The view zooms (Ctrl+wheel, − Fit +) and scrolls
// (the wheel or the scrollbar).

import { el, svg } from './dom.js';
import { parseName, VOICE_COLOR } from './views.js';

const RULER = 18;
const STRIP = 30;
const LANE = 26;
// Zoom limits, in pixels per bar.
const MIN_ZOOM = 2;
const MAX_ZOOM = 240;
// An open lane's piano roll: row height range and the tallest it gets.
const ROLL_ROW = { min: 4, max: 9 };
const ROLL_MAX = 200;
// An open lane's staff: pixels per diatonic step.
const STAFF_STEP = 3.5;

/** A stable colour for a section name, so repeats of a section match. */
function hue(name) {
  let h = 0;
  for (const c of name.trim().toLowerCase()) h = (h * 31 + c.charCodeAt(0)) % 360;
  return (h * 47) % 360;
}

function nextName(sections) {
  const used = new Set(sections.map((s) => s.name));
  for (const c of 'ABCDEFGHIJKLMNOPQRSTUVWXYZ') if (!used.has(c)) return c;
  return `Section ${sections.length + 1}`;
}

/** Pitch rows of a lane opened as a piano roll: the notes' range plus a row each side. */
function rollLayout(notes) {
  const keys = notes.map((n) => n[2]);
  const lo = (keys.length ? Math.min(...keys) : 60) - 1;
  const hi = (keys.length ? Math.max(...keys) : 71) + 1;
  const rows = hi - lo + 1;
  const row = Math.max(ROLL_ROW.min, Math.min(ROLL_ROW.max, ROLL_MAX / rows));
  return { lo, hi, row, height: rows * row + 8 };
}

/**
 * Staff positions of a lane opened as a staff (diatonic steps, C4 = 28): the treble
 * staff, the bass staff or both, whichever the notes need, plus room for ledger lines.
 * Null when a note has no letter name (another tuning).
 */
function staffLayout(notes) {
  const parsed = notes.map((n) => parseName(n[5] ?? ''));
  if (parsed.some((p) => !p)) return null;
  const steps = parsed.map((p) => p.step);
  let treble = steps.some((s) => s >= 28);
  let bass = steps.some((s) => s < 28);
  if (!treble && !bass) treble = bass = true;
  // Outer staff lines: F5 (38) or A3 (26) on top, G2 (18) or E4 (30) at the bottom.
  const high = treble ? 38 : 26;
  const low = bass ? 18 : 30;
  const top = Math.max(high + 2, ...steps.map((s) => s + 2));
  const bottom = Math.min(low - 2, ...steps.map((s) => s - 2));
  return { parsed, treble, bass, high, low, top, bottom, height: (top - bottom) * STAFF_STEP + 12 };
}

function formatTime(seconds) {
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}

export class Timeline {
  /**
   * @param {object} o
   * @param {HTMLElement} o.root
   * @param {() => object} o.getDoc  the project document (sections live in doc.arrangement)
   * @param {(structural: boolean) => void} o.onChange  sections were edited
   * @param {(index: number|null) => void} o.onFocus  a section was selected
   * @param {(tick: number) => void} o.onSeek  the user clicked a position
   * @param {(mode: string) => void} o.onLoop
   * @param {(on: boolean) => void} o.onSectionEdit
   */
  constructor(o) {
    Object.assign(this, o);
    this.focus = null;
    this.song = null;
    this.status = null;
    this.loop = 'song';
    this.sectionEdit = false;
    this.bar = 3840;
    this.zoom = null; // pixels per bar, or null to fit the song to the width
    this.open = new Map(); // part node -> 'roll' | 'staff', for lanes opened up
    this.highlight = []; // flat ids of the selected nodes: their parts' lanes are marked
    this.build();
  }

  get sections() {
    return this.getDoc().arrangement.sections;
  }

  build() {
    this.addBtn = el('button', { title: 'Add a section after the selected one', onclick: () => this.addSection() }, '+ Section');
    this.nameIn = el('input', { type: 'text', class: 'tl-name', spellcheck: 'false', title: 'Section name: sections with the same name share a colour, and Section switch nodes match on it' });
    this.barsIn = el('input', { type: 'number', min: 1, max: 999, step: 1, class: 'tl-bars', title: 'Length in bars' });
    this.bpmIn = el('input', { type: 'number', min: 20, max: 400, step: 1, class: 'tl-bpm', placeholder: 'song', title: 'Tempo for this section (empty: the song tempo)' });
    this.editBtn = el('button', { class: 'tl-edit', title: 'While on, parameter changes apply to this section only' }, 'Edit this section only');
    this.dupBtn = el('button', { title: 'Duplicate the section, with its settings' }, 'Duplicate');
    this.delBtn = el('button', { title: 'Delete the section' }, 'Delete');
    this.leftBtn = el('button', { class: 'mini', title: 'Move earlier' }, '◀');
    this.rightBtn = el('button', { class: 'mini', title: 'Move later' }, '▶');
    this.loopSel = el('select', { title: 'What Play loops' },
      el('option', { value: 'off' }, 'Play once'),
      el('option', { value: 'song' }, 'Loop song'),
      el('option', { value: 'section' }, 'Loop section'));
    this.info = el('span', { class: 'tl-info' });
    this.selected = el('span', { class: 'tl-selected' },
      el('label', {}, 'Name ', this.nameIn), el('label', {}, 'Bars ', this.barsIn), el('label', {}, 'BPM ', this.bpmIn),
      this.leftBtn, this.rightBtn, this.dupBtn, this.delBtn, this.editBtn);

    this.nameIn.addEventListener('change', () => this.edit((s) => { s.name = this.nameIn.value.trim() || s.name; }));
    this.barsIn.addEventListener('change', () => this.edit((s) => { s.bars = Math.max(1, Math.min(999, Math.round(Number(this.barsIn.value)) || s.bars)); }));
    this.bpmIn.addEventListener('change', () => this.edit((s) => {
      const v = Number(this.bpmIn.value);
      s.bpm = this.bpmIn.value.trim() === '' || !v ? null : Math.max(20, Math.min(400, v));
    }));
    for (const input of [this.nameIn, this.barsIn, this.bpmIn]) {
      input.addEventListener('keydown', (e) => { if (e.key === 'Enter') input.blur(); });
    }
    this.editBtn.addEventListener('click', () => this.setSectionEdit(!this.sectionEdit));
    this.dupBtn.addEventListener('click', () => this.duplicate());
    this.delBtn.addEventListener('click', () => this.remove());
    this.leftBtn.addEventListener('click', () => this.move(-1));
    this.rightBtn.addEventListener('click', () => this.move(1));
    this.loopSel.addEventListener('change', () => this.setLoop(this.loopSel.value));

    this.labels = el('div', { class: 'tl-labels' });
    this.svg = svg('svg', { class: 'tl-svg' });
    this.scroll = el('div', { class: 'tl-scroll' }, this.svg);
    this.empty = el('div', { class: 'tl-empty' },
      'No sections yet: the graph plays continuously. Add sections to arrange a song; each can have its own length, tempo and settings.');
    const zoom = el('span', { class: 'tl-zoom' },
      el('button', { class: 'mini', title: 'Zoom out (Ctrl+wheel)', onclick: () => this.zoomBy(1 / 1.5) }, '−'),
      el('button', { class: 'mini', title: 'Fit the song to the width', onclick: () => this.fit() }, 'Fit'),
      el('button', { class: 'mini', title: 'Zoom in (Ctrl+wheel)', onclick: () => this.zoomBy(1.5) }, '+'));
    this.body = el('div', { class: 'tl-body' }, this.labels, this.scroll);
    this.root.replaceChildren(
      el('div', { class: 'tl-bar' }, el('strong', {}, 'Arrangement'), this.addBtn, this.selected,
        el('span', { class: 'tl-spacer' }), zoom, this.loopSel, this.info),
      this.body,
      this.empty);
    this.body.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    this.svg.addEventListener('pointerdown', (e) => this.onDown(e));
    window.addEventListener('pointermove', (e) => this.onMove(e));
    window.addEventListener('pointerup', (e) => this.onUp(e));
    // Redraw when the visible width changes (a scrollbar coming and going changes only
    // the height, which needs no redraw).
    let seenWidth = 0;
    new ResizeObserver(() => {
      if (this.scroll.clientWidth === seenWidth) return;
      seenWidth = this.scroll.clientWidth;
      this.render();
    }).observe(this.scroll);
  }

  edit(fn) {
    const s = this.sections[this.focus];
    if (!s) return;
    fn(s);
    this.onChange(true);
    this.render();
  }

  addSection() {
    const list = this.sections;
    const at = this.focus == null ? list.length : this.focus + 1;
    let id = 1;
    while (list.some((s) => s.id === `s${id}`)) id++;
    list.splice(at, 0, { id: `s${id}`, name: nextName(list), bars: 8, bpm: null, overrides: {} });
    this.onChange(true);
    this.select(at);
  }

  duplicate() {
    const list = this.sections;
    const s = list[this.focus];
    if (!s) return;
    let id = 1;
    while (list.some((x) => x.id === `s${id}`)) id++;
    list.splice(this.focus + 1, 0, { ...structuredClone(s), id: `s${id}` });
    this.onChange(true);
    this.select(this.focus + 1);
  }

  remove() {
    const list = this.sections;
    if (this.focus == null || !list[this.focus]) return;
    const s = list[this.focus];
    const overrides = Object.keys(s.overrides).length;
    if (overrides && !window.confirm(`Delete section ${s.name} and its ${overrides} changed node setting${overrides === 1 ? '' : 's'}?`)) return;
    list.splice(this.focus, 1);
    this.onChange(true);
    this.select(list.length ? Math.min(this.focus, list.length - 1) : null);
  }

  move(by) {
    const list = this.sections;
    const i = this.focus;
    const j = i + by;
    if (i == null || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    this.onChange(true);
    this.select(j);
  }

  select(index) {
    this.focus = index;
    if (index == null && this.sectionEdit) this.setSectionEdit(false);
    if (index == null && this.loop === 'section') this.setLoop('song');
    this.onFocus(index);
    if (this.loop === 'section' && index != null) this.onLoop('section');
    this.render();
  }

  setSectionEdit(on) {
    this.sectionEdit = on && this.focus != null;
    this.onSectionEdit(this.sectionEdit);
    this.render();
  }

  setLoop(mode) {
    this.loop = mode === 'section' && this.focus == null ? 'song' : mode;
    this.loopSel.value = this.loop;
    this.onLoop(this.loop);
  }

  setSong(song) {
    this.song = song;
    this.bar = song?.bar ?? this.bar;
    this.render();
  }

  setStatus(s) {
    this.status = s;
    this.drawPlayhead();
  }

  // ---- Zoom, scrolling and lanes -----------------------------------------------------

  /** Zooms keeping the bar at `px` (from the left of the visible area) in place. */
  zoomAt(px, factor) {
    const old = this.pxBar;
    const next = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, old * factor));
    if (next === old) return;
    const bar = (this.scroll.scrollLeft + px) / old;
    this.zoom = next;
    this.render();
    this.scroll.scrollLeft = bar * next - px;
  }

  /** Zooms around the playhead when it's in view, else the middle. */
  zoomBy(factor) {
    const w = this.scroll.clientWidth;
    const head = this.x ? this.x(this.status?.songTick ?? 0) - this.scroll.scrollLeft : -1;
    this.zoomAt(head >= 0 && head <= w ? head : w / 2, factor);
  }

  fit() {
    this.zoom = null;
    this.render();
    this.scroll.scrollLeft = 0;
  }

  onWheel(e) {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const r = this.scroll.getBoundingClientRect();
      this.zoomAt(e.clientX - r.left, Math.exp(-e.deltaY * 0.002));
      return;
    }
    // An up-down wheel moves along the song, unless there are lanes to scroll through.
    const vertical = Math.abs(e.deltaY) > Math.abs(e.deltaX);
    const tall = this.body.scrollHeight > this.body.clientHeight + 1;
    if (vertical && (e.shiftKey || !tall)) {
      e.preventDefault();
      this.scroll.scrollLeft += e.deltaY;
    }
  }

  /** Marks the lanes of the parts that the selected nodes (flat ids) belong to. */
  setHighlight(flats) {
    const lanes = () => (this.song?.parts ?? []).map((p) => this.highlighted(p)).join();
    const before = lanes();
    this.highlight = flats;
    if (lanes() !== before) this.render();
  }

  /** Whether a part's lane belongs to a selected node: the part's output itself, a
   * group around it, or anything in the same group. */
  highlighted(part) {
    const inside = part.node.slice(0, part.node.lastIndexOf('/'));
    return this.highlight.some((sel) => sel === part.node
      || part.node.startsWith(`${sel}/`)
      || (inside && sel.startsWith(`${inside}/`)));
  }

  /** Opens a part's lane into a piano roll, or closes it. */
  toggleLane(node) {
    if (this.open.has(node)) this.open.delete(node);
    else this.open.set(node, 'roll');
    this.render();
  }

  setLaneMode(node, mode) {
    this.open.set(node, mode);
    this.render();
  }

  /** Song layout in ticks: [{start, length}] per section. */
  layout() {
    let t = 0;
    return this.sections.map((s) => {
      const r = { start: t, length: s.bars * this.bar };
      t += r.length;
      return r;
    });
  }

  render() {
    const list = this.sections;
    const has = list.length > 0;
    const s = list[this.focus];
    this.selected.hidden = !s;
    this.empty.hidden = has;
    this.root.classList.toggle('no-sections', !has);
    if (s) {
      if (document.activeElement !== this.nameIn) this.nameIn.value = s.name;
      if (document.activeElement !== this.barsIn) this.barsIn.value = s.bars;
      if (document.activeElement !== this.bpmIn) this.bpmIn.value = s.bpm ?? '';
      this.editBtn.classList.toggle('on', this.sectionEdit);
      const n = Object.values(s.overrides).reduce((sum, o) => sum + Object.keys(o).length, 0);
      this.editBtn.textContent = this.sectionEdit ? `Editing ${s.name} only${n ? ` (${n})` : ''}` : `Edit ${s.name} only${n ? ` (${n})` : ''}`;
    }
    this.loopSel.querySelector('option[value=section]').disabled = this.focus == null;
    const bars = has ? list.reduce((a, x) => a + x.bars, 0) : (this.song ? Math.round(this.song.length / this.bar) : 8);
    const songBpm = this.getDoc().bpm ?? 110;
    const seconds = has
      ? list.reduce((a, x) => a + (x.bars * 4 * 60) / (x.bpm ?? songBpm), 0)
      : (bars * 4 * 60) / songBpm;
    this.info.textContent = `${bars} bars · ${formatTime(seconds)}`;

    const parts = this.song?.parts ?? [];
    // Fitted, the whole song fills the visible width (leaving room for the last bar
    // line); zoomed, it scrolls.
    const width = Math.max(200, this.scroll.clientWidth - 2);
    const fit = Math.max(MIN_ZOOM, Math.min(48, (width - 4) / Math.max(1, bars)));
    const pxBar = this.zoom ?? fit;
    this.pxBar = pxBar;
    const W = Math.max(width, bars * pxBar + 4);
    // Each lane is a strip of notes, or opened up into a piano roll or a staff.
    const lanes = parts.map((part) => {
      const staff = this.open.get(part.node) === 'staff' ? staffLayout(part.notes) : null;
      if (staff) return { part, mode: 'staff', staff, height: staff.height };
      if (this.open.has(part.node)) {
        const roll = rollLayout(part.notes);
        return { part, mode: 'roll', roll, height: roll.height };
      }
      return { part, mode: null, height: LANE };
    });
    const H = RULER + STRIP + lanes.reduce((sum, l) => sum + l.height, 0) + 4;
    const x = (tick) => (tick / this.bar) * pxBar;
    this.svg.setAttribute('width', W);
    this.svg.setAttribute('height', H);
    this.svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
    const nodes = [];

    // Ruler: bar numbers, every few bars depending on zoom; beats when zoomed in far.
    const every = [1, 2, 4, 8, 16, 32, 64].find((n) => n * pxBar >= 28) ?? 128;
    nodes.push(svg('rect', { x: 0, y: 0, width: W, height: RULER, class: 'tl-ruler' }));
    const beats = Math.max(1, Math.round(this.bar / 960));
    for (let b = 0; b <= bars; b++) {
      const major = b % every === 0;
      nodes.push(svg('line', { x1: x(b * this.bar), x2: x(b * this.bar), y1: major ? 6 : 12, y2: H, class: major ? 'tl-tick major' : 'tl-tick' }));
      if (major && b < bars) {
        const t = svg('text', { x: x(b * this.bar) + 3, y: 13, class: 'tl-num' });
        t.textContent = b + 1;
        nodes.push(t);
      }
      if (pxBar >= 16 * beats && b < bars) {
        for (let k = 1; k < beats; k++) {
          const bx = x(b * this.bar + (k * this.bar) / beats);
          nodes.push(svg('line', { x1: bx, x2: bx, y1: 14, y2: H, class: 'tl-beat' }));
        }
      }
    }

    // Sections.
    const layout = this.layout();
    list.forEach((sec, i) => {
      const { start, length } = layout[i];
      const g = svg('g', { class: `tl-section${i === this.focus ? ' selected' : ''}`, 'data-i': i, style: `--h:${hue(sec.name)}` });
      g.append(svg('rect', { x: x(start) + 1, y: RULER + 2, width: Math.max(2, x(length) - 2), height: STRIP - 4, rx: 4 }));
      const label = svg('text', { x: x(start) + 6, y: RULER + STRIP / 2 + 4 });
      const over = Object.values(sec.overrides).reduce((sum, o) => sum + Object.keys(o).length, 0);
      label.textContent = `${sec.name} · ${sec.bars}${sec.bpm ? ` · ${sec.bpm} bpm` : ''}${over ? ' ✎' : ''}`;
      g.append(label);
      const title = svg('title');
      title.textContent = `${sec.name}: ${sec.bars} bars${sec.bpm ? `, ${sec.bpm} bpm` : ''}${over ? `, ${over} changed setting${over === 1 ? '' : 's'}` : ''}\nClick to select (previews show this section); drag to reorder.`;
      g.append(title);
      nodes.push(g);
    });

    // Part lanes.
    const labelRows = [el('div', { class: 'tl-label', style: `height:${RULER}px` }), el('div', { class: 'tl-label sec', style: `height:${STRIP}px` }, 'Sections')];
    let top = RULER + STRIP;
    lanes.forEach((lane, li) => {
      const { part, mode, height } = lane;
      const hl = this.highlighted(part);
      nodes.push(svg('rect', { x: 0, y: top, width: W, height, class: `tl-lane${li % 2 ? ' odd' : ''}${hl ? ' hl' : ''}` }));
      const color = `hsl(${(li * 67 + 200) % 360} 65% 62%)`;
      // Marks in the label column (octaves, clefs), at heights within the lane.
      const marks = [];
      if (mode === 'roll') this.drawRoll(nodes, marks, part, lane.roll, top, W, x, color);
      else if (mode === 'staff') this.drawStaff(nodes, marks, part, lane.staff, top, W, x);
      else this.drawStrip(nodes, part, top, x, color);
      labelRows.push(this.laneLabel(lane, hl, marks));
      top += height;
    });
    if (!parts.length) labelRows.push(el('div', { class: 'tl-label dim', style: `height:${LANE}px` }, 'No parts playing'));

    this.playhead = svg('line', { x1: 0, x2: 0, y1: 0, y2: H, class: 'tl-playhead' });
    this.cursorMark = svg('path', { class: 'tl-cursor', d: 'M-5,0 L5,0 L0,7 Z' });
    nodes.push(this.cursorMark, this.playhead);
    this.svg.replaceChildren(...nodes);
    this.labels.replaceChildren(...labelRows);
    this.x = x;
    this.drawPlayhead();
  }

  /** A closed lane: every note as a thin dash, scaled to the part's range. */
  drawStrip(nodes, part, top, x, color) {
    const keys = part.notes.map((n) => n[2]);
    const lo = Math.min(...keys);
    const span = Math.max(1, Math.max(...keys) - lo);
    for (const [t, d, k] of part.notes) {
      const ny = top + 3 + (1 - (k - lo) / span) * (LANE - 9);
      nodes.push(svg('rect', { x: x(t), y: ny, width: Math.max(1.2, x(d) - 0.6), height: 2.6, fill: color, class: 'tl-note' }));
    }
  }

  /** A lane opened as a piano roll: one row per semitone, octave lines at each C, notes
   * coloured by voice when the part has several (counterpoint, chords). */
  drawRoll(nodes, marks, part, L, top, W, x, color) {
    const y = (k) => top + 4 + (L.hi - k) * L.row;
    for (let k = L.lo; k <= L.hi; k++) {
      const pc = ((k % 12) + 12) % 12;
      if ([1, 3, 6, 8, 10].includes(pc)) nodes.push(svg('rect', { x: 0, y: y(k), width: W, height: L.row, class: 'tl-black' }));
      if (pc === 0) {
        nodes.push(svg('line', { x1: 0, x2: W, y1: y(k) + L.row, y2: y(k) + L.row, class: 'tl-oct-line' }));
        marks.push([y(k) + L.row - top, `C${Math.floor(k / 12) - 1}`, 'oct']);
      }
    }
    const voices = new Set(part.notes.map((n) => n[4])).size > 1;
    for (const [t, d, k, vel, v] of part.notes) {
      nodes.push(svg('rect', {
        x: x(t) + 0.5, y: y(k) + 0.5, width: Math.max(1.5, x(d) - 1), height: Math.max(2, L.row - 1), rx: 1,
        fill: voices ? VOICE_COLOR[(v ?? 0) % VOICE_COLOR.length] : color,
        'fill-opacity': (0.55 + 0.45 * (vel / 127)).toFixed(2),
        class: 'tl-note',
      }));
    }
  }

  /** A lane opened as a staff: noteheads at their spelled pitches, with a faint line for
   * each note's length, ledger lines and accidentals. */
  drawStaff(nodes, marks, part, S, top, W, x) {
    const y = (s) => top + 6 + (S.top - s) * STAFF_STEP;
    // Treble lines E4 G4 B4 D5 F5 and bass lines G2 B2 D3 F3 A3.
    const lines = [...(S.treble ? [30, 32, 34, 36, 38] : []), ...(S.bass ? [18, 20, 22, 24, 26] : [])];
    for (const s of lines) nodes.push(svg('line', { x1: 0, x2: W, y1: y(s), y2: y(s), class: 'tl-staff-line' }));
    if (S.treble) marks.push([y(31.2) - top, '𝄞', 'clef treble']);
    if (S.bass) marks.push([y(24.6) - top, '𝄢', 'clef bass']);
    const voices = new Set(part.notes.map((n) => n[4])).size > 1;
    part.notes.forEach(([t, d, , , v], i) => {
      const { step, acc } = S.parsed[i];
      const cx = x(t) + 4;
      const cy = y(step);
      const fill = voices ? VOICE_COLOR[(v ?? 0) % VOICE_COLOR.length] : '#d9dee7';
      nodes.push(svg('line', { x1: cx, x2: Math.max(cx, x(t + d)), y1: cy, y2: cy, stroke: fill, class: 'tl-staff-len' }));
      // Ledger lines beyond the staves shown, and middle C between them.
      const ledgers = [];
      for (let s = S.high + 2; s <= step; s += 2) ledgers.push(s);
      for (let s = S.low - 2; s >= step; s -= 2) ledgers.push(s);
      if (step === 28 && S.treble && S.bass) ledgers.push(28);
      for (const s of ledgers) nodes.push(svg('line', { x1: cx - 5.5, x2: cx + 5.5, y1: y(s), y2: y(s), class: 'tl-staff-line' }));
      if (acc) {
        const a = svg('text', { x: cx - 10, y: cy + 3.5, class: 'tl-staff-acc' });
        a.textContent = { 1: '♯', 2: '𝄪', '-1': '♭', '-2': '𝄫' }[acc];
        nodes.push(a);
      }
      nodes.push(svg('ellipse', { cx, cy, rx: 3.8, ry: 2.7, fill, transform: `rotate(-20 ${cx} ${cy})` }));
    });
  }

  /** A lane's name (click to open or close it), its Roll/Staff choice and marks when open. */
  laneLabel(lane, hl, marks) {
    const { part, mode, height } = lane;
    const label = el('div', {
      class: `tl-label lane${mode ? ' open' : ''}${hl ? ' hl' : ''}`,
      style: `height:${height}px`,
      title: `${part.name} · click to ${mode ? 'close' : 'open'} its notes`,
    });
    label.append(el('button', { class: 'tl-lane-name', onclick: () => this.toggleLane(part.node) }, `${mode ? '▾' : '▸'} ${part.name}`));
    if (mode) {
      const staffable = !!staffLayout(part.notes);
      label.append(el('span', { class: 'view-toggle' },
        el('button', { class: mode === 'roll' ? 'on' : '', onclick: () => this.setLaneMode(part.node, 'roll') }, 'Roll'),
        el('button', {
          class: mode === 'staff' ? 'on' : '', disabled: staffable ? null : '',
          title: staffable ? 'Show on a grand staff' : 'Only notes with letter names can go on a staff',
          onclick: () => this.setLaneMode(part.node, 'staff'),
        }, 'Staff')));
      for (const [at, text, cls] of marks) label.append(el('span', { class: `tl-mark ${cls}`, style: `top:${at}px` }, text));
    }
    return label;
  }

  drawPlayhead() {
    if (!this.playhead || !this.x) return;
    const s = this.status;
    const tick = s?.songTick ?? 0;
    const px = this.x(tick);
    this.playhead.setAttribute('x1', px);
    this.playhead.setAttribute('x2', px);
    this.playhead.classList.toggle('playing', !!s?.playing);
    if (!s?.playing) this.cursorMark.setAttribute('transform', `translate(${px},${RULER - 7})`);
    this.cursorMark.setAttribute('visibility', s?.playing ? 'hidden' : 'visible');
    for (const g of this.svg.querySelectorAll('.tl-section')) {
      const i = Number(g.dataset.i);
      g.classList.toggle('current', s?.playing && s.section === i);
      g.classList.toggle('cued', s?.cued === i);
    }
    // Keep the playhead in view while playing.
    if (s?.playing) {
      const left = this.scroll.scrollLeft;
      const w = this.scroll.clientWidth;
      if (px < left || px > left + w - 20) this.scroll.scrollLeft = Math.max(0, px - 40);
    }
  }

  tickAt(e) {
    const r = this.svg.getBoundingClientRect();
    const px = e.clientX - r.left;
    return Math.max(0, Math.floor(px / this.pxBar)) * this.bar;
  }

  onDown(e) {
    const g = e.target.closest('.tl-section');
    if (g) {
      this.drag = { i: Number(g.dataset.i), x: e.clientX, moved: false };
      return;
    }
    const r = this.svg.getBoundingClientRect();
    const y = e.clientY - r.top;
    if (y >= RULER && y < RULER + STRIP) {
      this.select(null);
      return;
    }
    this.onSeek(this.tickAt(e));
  }

  onMove(e) {
    const d = this.drag;
    if (!d) return;
    if (Math.abs(e.clientX - d.x) > 4) d.moved = true;
    if (!d.moved) return;
    // Where would it drop?
    const tick = this.tickAt(e) + this.bar / 2;
    const layout = this.layout();
    let to = layout.findIndex((l) => tick < l.start + l.length / 2);
    if (to < 0) to = layout.length;
    d.to = to;
    this.svg.querySelector('.tl-drop')?.remove();
    const px = this.x(to < layout.length ? layout[to].start : layout.at(-1).start + layout.at(-1).length);
    this.svg.append(svg('line', { x1: px, x2: px, y1: RULER, y2: RULER + STRIP, class: 'tl-drop' }));
  }

  onUp() {
    const d = this.drag;
    if (!d) return;
    this.drag = null;
    if (!d.moved) {
      this.select(this.focus === d.i ? null : d.i);
      return;
    }
    const list = this.sections;
    let to = d.to ?? d.i;
    if (to === d.i || to === d.i + 1) {
      this.render();
      return;
    }
    const [s] = list.splice(d.i, 1);
    if (to > d.i) to -= 1;
    list.splice(to, 0, s);
    this.onChange(true);
    this.select(to);
  }
}
