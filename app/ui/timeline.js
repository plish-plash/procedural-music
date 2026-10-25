// The arrangement timeline: sections along the song, an overview of every part's notes,
// the playhead, and the controls for editing sections.

import { el, svg } from './editor.js';

const RULER = 18;
const STRIP = 30;
const LANE = 26;
const LABELS = 120;

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
    this.root.replaceChildren(
      el('div', { class: 'tl-bar' }, el('strong', {}, 'Arrangement'), this.addBtn, this.selected,
        el('span', { class: 'tl-spacer' }), this.loopSel, this.info),
      el('div', { class: 'tl-body' }, this.labels, this.scroll),
      this.empty);
    this.svg.addEventListener('pointerdown', (e) => this.onDown(e));
    window.addEventListener('pointermove', (e) => this.onMove(e));
    window.addEventListener('pointerup', (e) => this.onUp(e));
    new ResizeObserver(() => this.render()).observe(this.scroll);
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
    const width = Math.max(200, this.scroll.clientWidth - 2);
    const pxBar = Math.max(10, Math.min(48, width / Math.max(1, bars)));
    this.pxBar = pxBar;
    const W = Math.max(width, bars * pxBar + 4);
    const H = RULER + STRIP + parts.length * LANE + 4;
    const x = (tick) => (tick / this.bar) * pxBar;
    this.svg.setAttribute('width', W);
    this.svg.setAttribute('height', H);
    this.svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
    const nodes = [];

    // Ruler: bar numbers, every few bars depending on zoom.
    const every = pxBar >= 28 ? 1 : pxBar >= 14 ? 2 : 4;
    nodes.push(svg('rect', { x: 0, y: 0, width: W, height: RULER, class: 'tl-ruler' }));
    for (let b = 0; b <= bars; b++) {
      const major = b % every === 0;
      nodes.push(svg('line', { x1: x(b * this.bar), x2: x(b * this.bar), y1: major ? 6 : 12, y2: H, class: major ? 'tl-tick major' : 'tl-tick' }));
      if (major && b < bars) {
        const t = svg('text', { x: x(b * this.bar) + 3, y: 13, class: 'tl-num' });
        t.textContent = b + 1;
        nodes.push(t);
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

    // Part lanes: a mini piano roll per part.
    const labelRows = [el('div', { class: 'tl-label', style: `height:${RULER}px` }), el('div', { class: 'tl-label sec', style: `height:${STRIP}px` }, 'Sections')];
    parts.forEach((part, li) => {
      const top = RULER + STRIP + li * LANE;
      nodes.push(svg('rect', { x: 0, y: top, width: W, height: LANE, class: `tl-lane${li % 2 ? ' odd' : ''}` }));
      const keys = part.notes.map((n) => n[2]);
      const lo = Math.min(...keys);
      const hi = Math.max(...keys);
      const span = Math.max(1, hi - lo);
      const color = `hsl(${(li * 67 + 200) % 360} 65% 62%)`;
      for (const [t, d, k] of part.notes) {
        const ny = top + 3 + (1 - (k - lo) / span) * (LANE - 9);
        nodes.push(svg('rect', { x: x(t), y: ny, width: Math.max(1.2, x(d) - 0.6), height: 2.6, fill: color, class: 'tl-note' }));
      }
      labelRows.push(el('div', { class: 'tl-label', style: `height:${LANE}px`, title: part.name }, part.name));
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
