// The node editor: renders a graph document as draggable nodes and wires, and edits it.
//
// Document shape (also the saved file format):
//   { version: 1, bpm, nodes: [{ id, type, x, y, params: {} }],
//     wires: [{ from: { node, port }, to: { node, port } }] }

const SVG = 'http://www.w3.org/2000/svg';
const WIDE = new Set(['progression', 'voice', 'voice_lead', 'drop', 'invert', 'play_chords', 'transpose', 'chord']);

function el(tag, attrs = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === 'class') e.className = v;
    else if (k === 'dataset') Object.assign(e.dataset, v);
    else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
    else if (v !== undefined && v !== null) e.setAttribute(k, v);
  }
  for (const c of children) if (c != null) e.append(c);
  return e;
}

function svg(tag, attrs = {}) {
  const e = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}

const TYPE_COLOR = { scale: '#40d2be', chord: '#f5b446', rhythm: '#ff7aa8', notes: '#6e96ff', audio: '#b8e06a' };

export class Editor {
  constructor({ canvas, world, nodesEl, wiresEl, addMenu, addSearch, addList, emptyHint, specs, onChange }) {
    Object.assign(this, { canvas, world, nodesEl, wiresEl, addMenu, addSearch, addList, emptyHint, onChange });
    this.specList = specs.nodes;
    this.specs = Object.fromEntries(specs.nodes.map((s) => [s.kind, s]));
    this.compat = new Set(specs.compat.map(([a, b]) => `${a}>${b}`));
    this.doc = { version: 1, nodes: [], wires: [] };
    this.view = { x: 0, y: 0, z: 1 };
    this.selection = new Set();
    this.selectedWire = null;
    this.midiPorts = [];
    this.drag = null;
    this.menu = null;
    this.bind();
    this.applyView();
  }

  // ---- Document -------------------------------------------------------------

  load(doc) {
    this.doc = {
      version: 1,
      bpm: doc.bpm ?? 110,
      nodes: (doc.nodes ?? []).map((n) => ({ id: String(n.id), type: n.type, x: n.x ?? 0, y: n.y ?? 0, params: { ...(n.params ?? {}) } })),
      wires: (doc.wires ?? []).map((w) => ({ from: { ...w.from }, to: { ...w.to } })),
    };
    this.selection.clear();
    this.selectedWire = null;
    this.renderAll();
    this.frame();
  }

  nextId() {
    let max = 0;
    for (const n of this.doc.nodes) {
      const m = /^n(\d+)$/.exec(n.id);
      if (m) max = Math.max(max, Number(m[1]));
    }
    return `n${max + 1}`;
  }

  node(id) {
    return this.doc.nodes.find((n) => n.id === id);
  }

  defaultParams(spec) {
    return Object.fromEntries(spec.params.map((p) => [p.name, p.default]));
  }

  addNode(type, x, y) {
    const spec = this.specs[type];
    const n = { id: this.nextId(), type, x: Math.round(x), y: Math.round(y), params: this.defaultParams(spec) };
    this.doc.nodes.push(n);
    this.nodesEl.append(this.renderNode(n));
    this.select([n.id]);
    this.changed(true);
    return n;
  }

  removeSelection() {
    if (this.selectedWire !== null) {
      this.doc.wires.splice(this.selectedWire, 1);
      this.selectedWire = null;
      this.updateWires();
      this.changed(true);
      return;
    }
    if (!this.selection.size) return;
    this.doc.nodes = this.doc.nodes.filter((n) => !this.selection.has(n.id));
    this.doc.wires = this.doc.wires.filter((w) => !this.selection.has(w.from.node) && !this.selection.has(w.to.node));
    for (const id of this.selection) this.nodeEl(id)?.remove();
    this.selection.clear();
    this.updateWires();
    this.changed(true);
  }

  duplicateSelection() {
    const ids = [...this.selection];
    if (!ids.length) return;
    const map = {};
    const fresh = [];
    for (const id of ids) {
      const src = this.node(id);
      const n = { ...structuredClone(src), id: this.nextId(), x: src.x + 30, y: src.y + 30 };
      map[id] = n.id;
      this.doc.nodes.push(n);
      this.nodesEl.append(this.renderNode(n));
      fresh.push(n.id);
    }
    // Copy wires between duplicated nodes.
    for (const w of [...this.doc.wires]) {
      if (map[w.from.node] && map[w.to.node]) {
        this.doc.wires.push({ from: { node: map[w.from.node], port: w.from.port }, to: { node: map[w.to.node], port: w.to.port } });
      }
    }
    this.select(fresh);
    this.updateWires();
    this.changed(true);
  }

  canConnect(fromType, toType) {
    return this.compat.has(`${fromType}>${toType}`);
  }

  /** Connects an output to an input, replacing whatever fed that input. */
  connect(from, to) {
    if (from.node === to.node) return false;
    this.doc.wires = this.doc.wires.filter((w) => !(w.to.node === to.node && w.to.port === to.port));
    this.doc.wires.push({ from: { node: from.node, port: from.port }, to: { node: to.node, port: to.port } });
    this.updateWires();
    this.changed(true);
    return true;
  }

  changed(graph) {
    this.emptyHint.hidden = this.doc.nodes.length > 0;
    this.onChange?.({ graph });
  }

  // ---- Rendering ----------------------------------------------------------------

  nodeEl(id) {
    return this.nodesEl.querySelector(`.node[data-id="${CSS.escape(id)}"]`);
  }

  renderAll() {
    this.nodesEl.replaceChildren(...this.doc.nodes.map((n) => this.renderNode(n)));
    this.emptyHint.hidden = this.doc.nodes.length > 0;
    // Ports need layout before wires can find them.
    requestAnimationFrame(() => this.updateWires());
  }

  renderNode(n) {
    const spec = this.specs[n.type];
    if (!spec) {
      return el('div', { class: 'node has-issue', dataset: { id: n.id, cat: 'Output' }, style: `left:${n.x}px;top:${n.y}px` },
        el('div', { class: 'node-head' }, el('span', { class: 'title' }, n.type)),
        el('div', { class: 'node-issue' }, `Unknown node type "${n.type}"`));
    }
    const ins = el('div', { class: 'col' }, ...spec.inputs.map((p) => this.renderPort(n, p, 'in')));
    const outs = el('div', { class: 'col' }, ...spec.outputs.map((p) => this.renderPort(n, p, 'out')));
    const params = el('div', { class: 'node-params' }, ...spec.params.map((p) => this.renderParam(n, p)));
    const node = el('div', {
      class: `node${WIDE.has(n.type) ? ' wide' : ''}${this.selection.has(n.id) ? ' selected' : ''}`,
      dataset: { id: n.id, cat: spec.category },
      style: `left:${n.x}px;top:${n.y}px`,
    },
    el('div', { class: 'node-head', title: spec.help }, el('span', { class: 'title' }, spec.title), el('span', { class: 'cat' }, spec.category)),
    (spec.inputs.length || spec.outputs.length) ? el('div', { class: 'node-ports' }, ins, outs) : null,
    spec.params.length ? params : null,
    el('pre', { class: 'node-preview' }),
    el('div', { class: 'node-issue' }));
    return node;
  }

  renderPort(n, p, dir) {
    return el('div', {
      class: `port ${dir}${p.optional ? ' optional' : ''}`,
      dataset: { port: p.name, type: p.type, dir },
      title: `${p.name} (${p.type})`,
    },
    dir === 'in' ? el('span', { class: 'dot' }) : null,
    el('span', { class: 'label' }, p.name),
    dir === 'out' ? el('span', { class: 'dot' }) : null);
  }

  renderParam(n, p) {
    const value = n.params[p.name] ?? p.default;
    const set = (v) => {
      n.params[p.name] = v;
      this.changed(true);
    };
    let control;
    switch (p.kind) {
      case 'number': {
        const steps = (p.max - p.min) / p.step;
        if (steps <= 400) {
          const out = el('output', {}, fmt(value));
          const range = el('input', { type: 'range', min: p.min, max: p.max, step: p.step, value });
          range.addEventListener('input', () => {
            out.textContent = fmt(Number(range.value));
            set(Number(range.value));
          });
          control = el('div', { class: 'slider' }, range, out);
        } else {
          control = el('input', { type: 'number', min: p.min, max: p.max, step: p.step, value });
          control.addEventListener('change', () => set(Number(control.value)));
        }
        break;
      }
      case 'log': {
        // Slider position 0..1000 maps exponentially onto min..max.
        const toPos = (v) => Math.round((1000 * Math.log(v / p.min)) / Math.log(p.max / p.min));
        const fromPos = (x) => round3(p.min * Math.pow(p.max / p.min, x / 1000));
        const out = el('output', {}, fmtLog(value));
        const range = el('input', { type: 'range', min: 0, max: 1000, step: 1, value: toPos(value) });
        range.addEventListener('input', () => {
          const v = fromPos(Number(range.value));
          out.textContent = fmtLog(v);
          set(v);
        });
        control = el('div', { class: 'slider' }, range, out);
        break;
      }
      case 'text': {
        control = el('input', { type: 'text', value, spellcheck: 'false' });
        let timer;
        control.addEventListener('input', () => {
          clearTimeout(timer);
          timer = setTimeout(() => set(control.value), 250);
        });
        control.addEventListener('keydown', (e) => { if (e.key === 'Enter') control.blur(); });
        control.addEventListener('change', () => { clearTimeout(timer); set(control.value); });
        break;
      }
      case 'choice': {
        control = el('select', {}, ...p.options.map((o) => el('option', { value: o }, o)));
        control.value = value;
        control.addEventListener('change', () => set(control.value));
        break;
      }
      case 'bool': {
        control = el('input', { type: 'checkbox' });
        control.checked = !!value;
        control.addEventListener('change', () => set(control.checked));
        break;
      }
      case 'midi_port': {
        control = el('select', { class: 'midi-port' });
        this.fillPortSelect(control, value);
        control.addEventListener('change', () => set(control.value));
        break;
      }
      default:
        control = el('span', {}, String(value));
    }
    return el('label', { class: 'param', title: p.label }, el('span', {}, p.label), control);
  }

  fillPortSelect(select, value) {
    const options = [el('option', { value: '' }, 'Default')];
    for (const name of this.midiPorts) options.push(el('option', { value: name }, name));
    if (value && !this.midiPorts.includes(value)) options.push(el('option', { value }, `${value} (not found)`));
    select.replaceChildren(...options);
    select.value = value ?? '';
  }

  setMidiPorts(ports) {
    this.midiPorts = ports;
    for (const n of this.doc.nodes) {
      const spec = this.specs[n.type];
      if (!spec) continue;
      for (const p of spec.params.filter((p) => p.kind === 'midi_port')) {
        const select = this.nodeEl(n.id)?.querySelector('select.midi-port');
        if (select) this.fillPortSelect(select, n.params[p.name]);
      }
    }
  }

  /** Shows the engine's previews and issues on the nodes. */
  setReport(report) {
    const issues = {};
    for (const i of report.issues ?? []) (issues[i.node] ??= []).push(i.message);
    for (const n of this.doc.nodes) {
      const node = this.nodeEl(n.id);
      if (!node) continue;
      const pre = node.querySelector('.node-preview');
      const issue = node.querySelector('.node-issue');
      if (pre) pre.textContent = report.previews?.[n.id] ?? '';
      if (issue) issue.textContent = (issues[n.id] ?? []).join('\n');
      node.classList.toggle('has-issue', !!issues[n.id]);
    }
  }

  // ---- Wires ---------------------------------------------------------------------

  toWorld(clientX, clientY) {
    const r = this.canvas.getBoundingClientRect();
    return { x: (clientX - r.left - this.view.x) / this.view.z, y: (clientY - r.top - this.view.y) / this.view.z };
  }

  dotPos(nodeId, dir, port) {
    const dot = this.nodeEl(nodeId)?.querySelector(`.port.${dir}[data-port="${CSS.escape(port)}"] .dot`);
    if (!dot) return null;
    const r = dot.getBoundingClientRect();
    return this.toWorld(r.left + r.width / 2, r.top + r.height / 2);
  }

  portType(nodeId, dir, port) {
    const spec = this.specs[this.node(nodeId)?.type];
    const list = dir === 'in' ? spec?.inputs : spec?.outputs;
    return list?.find((p) => p.name === port)?.type;
  }

  static curve(a, b) {
    const dx = Math.max(40, Math.abs(b.x - a.x) * 0.5);
    return `M${a.x},${a.y} C${a.x + dx},${a.y} ${b.x - dx},${b.y} ${b.x},${b.y}`;
  }

  updateWires() {
    const parts = [];
    const connected = new Set();
    this.doc.wires.forEach((w, i) => {
      const a = this.dotPos(w.from.node, 'out', w.from.port);
      const b = this.dotPos(w.to.node, 'in', w.to.port);
      if (!a || !b) return;
      connected.add(`${w.to.node}/${w.to.port}`);
      const fromType = this.portType(w.from.node, 'out', w.from.port);
      const toType = this.portType(w.to.node, 'in', w.to.port);
      const d = Editor.curve(a, b);
      const g = svg('g', { 'data-wire': i });
      g.append(svg('path', { class: 'hit', d }));
      g.append(svg('path', { class: `wire${this.selectedWire === i ? ' selected' : ''}`, d, stroke: TYPE_COLOR[fromType] ?? '#888' }));
      if (fromType && toType && fromType !== toType) {
        // Implicit conversion badge at the midpoint.
        const mx = (a.x + b.x) / 2;
        const my = (a.y + b.y) / 2;
        const label = `${fromType}→${toType}`;
        const badge = svg('g', { class: 'badge' });
        badge.append(svg('rect', { x: mx - 34, y: my - 9, width: 68, height: 18, rx: 9 }));
        const t = svg('text', { x: mx, y: my + 3.5, 'text-anchor': 'middle' });
        t.textContent = label;
        badge.append(t);
        g.append(badge);
      }
      parts.push(g);
    });
    if (this.drag?.kind === 'wire') {
      const d = this.drag;
      const a = d.fixed.dir === 'out' ? d.anchor : d.cursor;
      const b = d.fixed.dir === 'out' ? d.cursor : d.anchor;
      parts.push(svg('path', { class: 'wire temp', d: Editor.curve(a, b), stroke: TYPE_COLOR[d.fixed.type] ?? '#888' }));
    }
    this.wiresEl.replaceChildren(...parts);
    for (const p of this.nodesEl.querySelectorAll('.port.in')) {
      const id = p.closest('.node').dataset.id;
      p.classList.toggle('connected', connected.has(`${id}/${p.dataset.port}`));
    }
  }

  // ---- View ------------------------------------------------------------------------

  applyView() {
    const { x, y, z } = this.view;
    this.world.style.transform = `translate(${x}px, ${y}px) scale(${z})`;
    this.canvas.style.backgroundSize = `${24 * z}px ${24 * z}px`;
    this.canvas.style.backgroundPosition = `${x}px ${y}px`;
  }

  /** Fits all nodes in view. */
  frame() {
    if (!this.doc.nodes.length) {
      this.view = { x: 40, y: 40, z: 1 };
      this.applyView();
      return;
    }
    requestAnimationFrame(() => {
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (const n of this.doc.nodes) {
        const e = this.nodeEl(n.id);
        minX = Math.min(minX, n.x);
        minY = Math.min(minY, n.y);
        maxX = Math.max(maxX, n.x + (e?.offsetWidth ?? 240));
        maxY = Math.max(maxY, n.y + (e?.offsetHeight ?? 160));
      }
      const r = this.canvas.getBoundingClientRect();
      const pad = 40;
      const z = Math.min(1, (r.width - 2 * pad) / (maxX - minX), (r.height - 2 * pad) / (maxY - minY));
      this.view = { z, x: pad - minX * z + ((r.width - 2 * pad) - (maxX - minX) * z) / 2, y: pad - minY * z };
      this.applyView();
      this.updateWires();
    });
  }

  select(ids, add = false) {
    if (!add) this.selection.clear();
    for (const id of ids) this.selection.add(id);
    this.selectedWire = null;
    for (const e of this.nodesEl.querySelectorAll('.node')) e.classList.toggle('selected', this.selection.has(e.dataset.id));
    this.updateWires();
  }

  // ---- Interaction ---------------------------------------------------------------------

  bind() {
    const c = this.canvas;
    c.addEventListener('pointerdown', (e) => this.onDown(e));
    window.addEventListener('pointermove', (e) => this.onMove(e));
    window.addEventListener('pointerup', (e) => this.onUp(e));
    c.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    c.addEventListener('dblclick', (e) => {
      if (e.target.closest('.node')) return;
      this.openAddMenu(e.clientX, e.clientY, null);
    });
    c.addEventListener('keydown', (e) => {
      if (isTyping(e)) return;
      if (e.key === 'Delete' || e.key === 'Backspace') {
        e.preventDefault();
        this.removeSelection();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'd') {
        e.preventDefault();
        this.duplicateSelection();
      } else if (e.key === 'Tab') {
        e.preventDefault();
        const r = c.getBoundingClientRect();
        this.openAddMenu(r.left + r.width / 2 - 150, r.top + r.height / 3, null);
      } else if (e.key === 'f' || e.key === 'F') {
        this.frame();
      }
    });
    this.addSearch.addEventListener('input', () => this.fillAddList());
    this.addSearch.addEventListener('keydown', (e) => this.onMenuKey(e));
    document.addEventListener('pointerdown', (e) => {
      if (this.menu && !this.addMenu.contains(e.target)) this.closeAddMenu();
    }, true);
  }

  onDown(e) {
    if (e.button !== 0 && e.button !== 1) return;
    const dot = e.target.closest('.dot');
    const nodeE = e.target.closest('.node');
    const wireG = e.target.closest('[data-wire]');
    this.canvas.focus({ preventScroll: true });

    if (dot) {
      e.preventDefault();
      const portE = dot.closest('.port');
      const id = nodeE.dataset.id;
      const fixed = { node: id, port: portE.dataset.port, type: portE.dataset.type, dir: portE.dataset.dir };
      if (fixed.dir === 'in') {
        // Pick up an existing wire into this input and move its free end.
        const i = this.doc.wires.findIndex((w) => w.to.node === id && w.to.port === fixed.port);
        if (i >= 0) {
          const w = this.doc.wires.splice(i, 1)[0];
          this.changed(true);
          const type = this.portType(w.from.node, 'out', w.from.port);
          this.startWire({ node: w.from.node, port: w.from.port, type, dir: 'out' }, e);
          return;
        }
      }
      this.startWire(fixed, e);
      return;
    }
    if (nodeE) {
      const id = nodeE.dataset.id;
      if (!this.selection.has(id) || e.shiftKey) this.select([id], e.shiftKey);
      if (e.target.closest('.node-head')) {
        e.preventDefault();
        const start = this.toWorld(e.clientX, e.clientY);
        const nodes = [...this.selection].map((sid) => ({ n: this.node(sid), x: this.node(sid).x, y: this.node(sid).y }));
        this.drag = { kind: 'node', start, nodes, moved: false };
      }
      return;
    }
    if (wireG) {
      this.selection.clear();
      this.select([]);
      this.selectedWire = Number(wireG.dataset.wire);
      this.updateWires();
      return;
    }
    // Background: deselect and pan.
    e.preventDefault();
    this.select([]);
    this.drag = { kind: 'pan', sx: e.clientX, sy: e.clientY, vx: this.view.x, vy: this.view.y };
    this.canvas.classList.add('panning');
  }

  startWire(fixed, e) {
    const anchor = this.dotPos(fixed.node, fixed.dir, fixed.port);
    this.drag = { kind: 'wire', fixed, anchor, cursor: this.toWorld(e.clientX, e.clientY), target: null };
    // Dim ports that can't take this wire.
    for (const p of this.nodesEl.querySelectorAll('.port')) {
      const ok = this.wireFits(fixed, p);
      p.classList.toggle('dim', !ok && p.closest('.node').dataset.id !== fixed.node);
    }
    this.updateWires();
  }

  wireFits(fixed, portE) {
    if (!portE || portE.dataset.dir === fixed.dir) return false;
    if (portE.closest('.node').dataset.id === fixed.node) return false;
    return fixed.dir === 'out' ? this.canConnect(fixed.type, portE.dataset.type) : this.canConnect(portE.dataset.type, fixed.type);
  }

  onMove(e) {
    const d = this.drag;
    if (!d) return;
    if (d.kind === 'pan') {
      this.view.x = d.vx + (e.clientX - d.sx);
      this.view.y = d.vy + (e.clientY - d.sy);
      this.applyView();
    } else if (d.kind === 'node') {
      const p = this.toWorld(e.clientX, e.clientY);
      const dx = p.x - d.start.x;
      const dy = p.y - d.start.y;
      d.moved ||= Math.abs(dx) + Math.abs(dy) > 1;
      for (const s of d.nodes) {
        s.n.x = Math.round(s.x + dx);
        s.n.y = Math.round(s.y + dy);
        const ne = this.nodeEl(s.n.id);
        ne.style.left = `${s.n.x}px`;
        ne.style.top = `${s.n.y}px`;
      }
      this.updateWires();
    } else if (d.kind === 'wire') {
      d.cursor = this.toWorld(e.clientX, e.clientY);
      const under = document.elementFromPoint(e.clientX, e.clientY)?.closest('.port');
      const target = this.wireFits(d.fixed, under) ? under : null;
      if (target !== d.target) {
        d.target?.classList.remove('target');
        target?.classList.add('target');
        d.target = target;
      }
      if (target) d.cursor = this.dotPos(target.closest('.node').dataset.id, target.dataset.dir, target.dataset.port);
      this.updateWires();
    }
  }

  onUp(e) {
    const d = this.drag;
    if (!d) return;
    this.drag = null;
    this.canvas.classList.remove('panning');
    if (d.kind === 'node' && d.moved) this.changed(false);
    if (d.kind === 'wire') {
      for (const p of this.nodesEl.querySelectorAll('.port')) p.classList.remove('dim', 'target');
      if (d.target) {
        const other = { node: d.target.closest('.node').dataset.id, port: d.target.dataset.port };
        const fixed = { node: d.fixed.node, port: d.fixed.port };
        if (d.fixed.dir === 'out') this.connect(fixed, other);
        else this.connect(other, fixed);
      } else if (!document.elementFromPoint(e.clientX, e.clientY)?.closest('.node')) {
        // Dropped on empty canvas: offer nodes that can take this wire.
        this.openAddMenu(e.clientX, e.clientY, d.fixed);
      }
      this.updateWires();
    }
  }

  onWheel(e) {
    if (e.target.closest('.node-preview, select')) return;
    e.preventDefault();
    const r = this.canvas.getBoundingClientRect();
    const mx = e.clientX - r.left;
    const my = e.clientY - r.top;
    const z = Math.min(2, Math.max(0.25, this.view.z * Math.exp(-e.deltaY * 0.0015)));
    const k = z / this.view.z;
    this.view.x = mx - (mx - this.view.x) * k;
    this.view.y = my - (my - this.view.y) * k;
    this.view.z = z;
    this.applyView();
  }

  // ---- Add-node menu ---------------------------------------------------------------------

  openAddMenu(clientX, clientY, pending) {
    this.menu = { at: this.toWorld(clientX, clientY), pending, active: 0, items: [] };
    const w = 300;
    const h = 420;
    this.addMenu.style.left = `${Math.min(clientX, window.innerWidth - w - 8)}px`;
    this.addMenu.style.top = `${Math.min(clientY, window.innerHeight - h - 8)}px`;
    this.addMenu.hidden = false;
    this.addSearch.value = '';
    this.fillAddList();
    this.addSearch.focus();
  }

  closeAddMenu() {
    this.menu = null;
    this.addMenu.hidden = true;
    this.canvas.focus({ preventScroll: true });
  }

  /** The port on `spec` a pending wire should attach to, or null. */
  pendingPort(spec, pending) {
    if (!pending) return null;
    if (pending.dir === 'out') return spec.inputs.find((p) => this.canConnect(pending.type, p.type)) ?? null;
    return spec.outputs.find((p) => this.canConnect(p.type, pending.type)) ?? null;
  }

  fillAddList() {
    const q = this.addSearch.value.trim().toLowerCase();
    const pending = this.menu?.pending;
    const items = this.specList.filter((s) => {
      if (pending && !this.pendingPort(s, pending)) return false;
      if (!q) return true;
      return `${s.title} ${s.category} ${s.kind} ${s.help}`.toLowerCase().includes(q);
    });
    this.menu.items = items;
    this.menu.active = Math.min(this.menu.active, Math.max(0, items.length - 1));
    const rows = [];
    let cat = null;
    items.forEach((s, i) => {
      if (s.category !== cat) {
        cat = s.category;
        rows.push(el('div', { class: 'cat-label' }, cat));
      }
      rows.push(el('div', {
        class: `item${i === this.menu.active ? ' active' : ''}`,
        dataset: { i },
        title: s.help,
        onpointerdown: (e) => { e.preventDefault(); this.chooseAdd(i); },
      },
      el('span', { class: 'swatch', style: `background: var(--c-${s.category.toLowerCase()})` }),
      el('span', {}, s.title),
      el('small', {}, s.help)));
    });
    if (!items.length) rows.push(el('div', { class: 'cat-label' }, 'No matching nodes'));
    this.addList.replaceChildren(...rows);
    this.addList.querySelector('.item.active')?.scrollIntoView({ block: 'nearest' });
  }

  onMenuKey(e) {
    if (!this.menu) return;
    if (e.key === 'Escape') this.closeAddMenu();
    else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const n = this.menu.items.length;
      if (!n) return;
      this.menu.active = (this.menu.active + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
      this.fillAddList();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (this.menu.items.length) this.chooseAdd(this.menu.active);
    }
  }

  chooseAdd(i) {
    const { at, pending, items } = this.menu;
    const spec = items[i];
    this.closeAddMenu();
    // Place inputs-side nodes to the left of a pending output, and vice versa.
    const x = pending?.dir === 'in' ? at.x - 250 : at.x;
    const n = this.addNode(spec.kind, x, at.y - 20);
    const port = this.pendingPort(spec, pending);
    if (port) {
      requestAnimationFrame(() => {
        if (pending.dir === 'out') this.connect({ node: pending.node, port: pending.port }, { node: n.id, port: port.name });
        else this.connect({ node: n.id, port: port.name }, { node: pending.node, port: pending.port });
      });
    } else {
      requestAnimationFrame(() => this.updateWires());
    }
  }
}

/** Whether a key event is going to a text field (sliders and checkboxes don't count). */
export function isTyping(e) {
  const t = e.target;
  if (!t) return false;
  if (t.tagName === 'INPUT') return !['range', 'checkbox', 'button'].includes(t.type);
  return t.tagName === 'SELECT' || t.tagName === 'TEXTAREA' || t.isContentEditable;
}

function fmt(v) {
  return Number.isInteger(v) ? String(v) : String(Math.round(v * 100) / 100);
}

/** Three significant figures. */
function round3(v) {
  const m = Math.pow(10, Math.max(0, 2 - Math.floor(Math.log10(Math.abs(v) || 1))));
  return Math.round(v * m) / m;
}

function fmtLog(v) {
  if (v >= 1000) return `${round3(v / 1000)}k`;
  return String(round3(v));
}
