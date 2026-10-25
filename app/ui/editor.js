// The node editor: renders a graph document as draggable nodes and wires, and edits it.
//
// Document shape (also the saved file format):
//   { version: 2, bpm, nodes: [{ id, type, x, y, params: {} }],
//     wires: [{ from: { node, port }, to: { node, port } }],
//     arrangement: { sections: [{ id, name, bars, bpm, overrides: { "<node path>": { param: value } } }] } }
//
// A group node ({ type: 'group', title, graph }) holds its own graph: { nodes, wires,
// inputs: [{ name, type }], outputs: [...], exposed: ['<node>.<param>'], io: { in: {x, y}, out: {x, y} } }.
// Inside it, the pseudo-nodes '@in' and '@out' carry the group's ports. Nodes inside groups
// are addressed by path ('g1/n3'), which is also how the engine reports on them.

const SVG = 'http://www.w3.org/2000/svg';
const WIDE = new Set(['progression', 'voice', 'voice_lead', 'drop', 'invert', 'play_chords', 'transpose', 'chord',
  'switch_notes', 'switch_rhythm', 'switch_chord', 'switch_scale']);
const XWIDE = new Set(['counterpoint', 'piano_roll']);
// Voice colours for piano rolls: soprano, alto, tenor, bass, then extra voices.
const VOICE_COLOR = ['#ff8a65', '#ffd54f', '#81c784', '#64b5f6', '#ba68c8', '#4dd0e1', '#f06292', '#aed581'];
const IN = '@in';
const OUT = '@out';

export function el(tag, attrs = {}, ...children) {
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

export function svg(tag, attrs = {}) {
  const e = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}

const TYPE_COLOR = { scale: '#40d2be', chord: '#f5b446', rhythm: '#ff7aa8', notes: '#6e96ff', audio: '#b8e06a', any: '#8a93a3' };
// Output types that can be auditioned with a click.
const AUDIBLE = new Set(['scale', 'chord', 'rhythm']);

/** An empty group graph. */
export function emptyGraph() {
  return { nodes: [], wires: [], inputs: [], outputs: [], exposed: [], labels: {}, io: { in: { x: 0, y: 0 }, out: { x: 700, y: 0 } } };
}

/** Fills in missing fields of a node list (recursively for groups). */
function normalizeNodes(nodes) {
  return (nodes ?? []).map((n) => {
    const out = { id: String(n.id), type: n.type, x: n.x ?? 0, y: n.y ?? 0, params: { ...(n.params ?? {}) } };
    if (n.type === 'group') {
      out.title = n.title ?? 'Group';
      out.graph = normalizeGraph(n.graph);
    }
    return out;
  });
}

function normalizeWires(wires) {
  return (wires ?? []).map((w) => ({ from: { ...w.from }, to: { ...w.to } }));
}

function normalizeGraph(g) {
  const base = emptyGraph();
  return {
    nodes: normalizeNodes(g?.nodes),
    wires: normalizeWires(g?.wires),
    inputs: (g?.inputs ?? []).map((p) => ({ name: p.name, type: p.type })),
    outputs: (g?.outputs ?? []).map((p) => ({ name: p.name, type: p.type })),
    exposed: [...(g?.exposed ?? [])],
    labels: { ...(g?.labels ?? {}) },
    io: { in: { x: g?.io?.in?.x ?? base.io.in.x, y: g?.io?.in?.y ?? 0 }, out: { x: g?.io?.out?.x ?? base.io.out.x, y: g?.io?.out?.y ?? 0 } },
  };
}

export function normalizeDoc(doc) {
  return {
    version: 2,
    bpm: doc.bpm ?? 110,
    ...(doc.clockPort ? { clockPort: doc.clockPort } : {}),
    nodes: normalizeNodes(doc.nodes),
    wires: normalizeWires(doc.wires),
    arrangement: {
      sections: (doc.arrangement?.sections ?? []).map((s, i) => ({
        id: s.id ?? `s${i + 1}`,
        name: s.name ?? `Section ${i + 1}`,
        bars: Math.max(1, Math.round(s.bars ?? 8)),
        bpm: s.bpm ?? null,
        overrides: structuredClone(s.overrides ?? {}),
      })),
    },
  };
}

function uniqueName(list, base) {
  const names = new Set(list.map((p) => p.name));
  const clean = (base || 'port').replace(/[^A-Za-z0-9_ -]/g, '') || 'port';
  if (!names.has(clean)) return clean;
  for (let i = 2; ; i++) if (!names.has(`${clean}${i}`)) return `${clean}${i}`;
}

export class Editor {
  constructor({ canvas, world, nodesEl, wiresEl, crumbsEl, addMenu, addSearch, addList, emptyHint, specs, onChange, onLibrarySave, onSectionMode, onAudition, onProbe }) {
    Object.assign(this, { canvas, world, nodesEl, wiresEl, crumbsEl, addMenu, addSearch, addList, emptyHint, onChange, onLibrarySave, onSectionMode, onAudition, onProbe });
    // The card shown while hovering a wire: what the wire carries.
    this.probe = { el: el('div', { class: 'probe', hidden: '' }), wire: null, timer: null, cache: new Map(), x: 0, y: 0 };
    document.body.append(this.probe.el);
    this.specList = specs.nodes;
    this.specs = Object.fromEntries(specs.nodes.map((s) => [s.kind, s]));
    this.compat = new Set(specs.compat.map(([a, b]) => `${a}>${b}`));
    this.doc = normalizeDoc({});
    this.path = [];
    this.views = {}; // per path: saved pan/zoom
    this.view = { x: 0, y: 0, z: 1 };
    this.selection = new Set();
    this.selectedWire = null;
    this.midiPorts = [];
    this.library = [];
    this.focus = null; // section index whose overrides are shown
    this.sectionEdit = false; // edits go to the focused section
    this.report = null;
    this.rollMode = {}; // node path -> 'roll' | 'staff'
    this.drag = null;
    this.menu = null;
    this.popup = null;
    this.bind();
    this.applyView();
  }

  // ---- Document and navigation ------------------------------------------------

  /** The graph being edited (the root, or a group's graph). */
  get g() {
    let g = this.doc;
    for (const id of this.path) g = g.nodes.find((n) => n.id === id)?.graph ?? g;
    return g;
  }

  /** The group node being edited, or null at the top level. */
  get group() {
    if (!this.path.length) return null;
    return this.parentGraph().nodes.find((n) => n.id === this.path.at(-1)) ?? null;
  }

  parentGraph() {
    let g = this.doc;
    for (const id of this.path.slice(0, -1)) g = g.nodes.find((n) => n.id === id).graph;
    return g;
  }

  get prefix() {
    return this.path.map((p) => `${p}/`).join('');
  }

  flat(id) {
    return this.prefix + id;
  }

  load(doc) {
    this.doc = normalizeDoc(doc);
    this.path = [];
    this.views = {};
    this.focus = null;
    this.sectionEdit = false;
    this.selection.clear();
    this.selectedWire = null;
    this.renderAll();
    this.frame();
  }

  /** Replaces the document without moving the view (undo and redo). */
  restore(doc) {
    this.doc = normalizeDoc(doc);
    // Keep as much of the group path as still exists.
    const path = [];
    let g = this.doc;
    for (const id of this.path) {
      const n = g.nodes.find((x) => x.id === id && x.type === 'group');
      if (!n) break;
      path.push(id);
      g = n.graph;
    }
    this.path = path;
    if (this.focus != null && this.focus >= this.doc.arrangement.sections.length) this.focus = null;
    this.selection = new Set([...this.selection].filter((id) => this.node(id)));
    this.selectedWire = null;
    this.renderAll();
  }

  enter(id) {
    const n = this.g.nodes.find((x) => x.id === id);
    if (n?.type !== 'group') return;
    this.views[this.path.join('/')] = { ...this.view };
    this.path.push(id);
    this.afterNavigate();
  }

  exitTo(depth) {
    if (depth >= this.path.length) return;
    this.views[this.path.join('/')] = { ...this.view };
    this.path = this.path.slice(0, depth);
    this.afterNavigate();
  }

  afterNavigate() {
    this.selection.clear();
    this.selectedWire = null;
    this.renderAll();
    const saved = this.views[this.path.join('/')];
    if (saved) {
      this.view = { ...saved };
      this.applyView();
    } else {
      this.frame();
    }
    this.canvas.focus({ preventScroll: true });
  }

  renderCrumbs() {
    if (!this.crumbsEl) return;
    this.crumbsEl.hidden = !this.path.length;
    if (!this.path.length) return;
    const parts = [el('button', { onclick: () => this.exitTo(0) }, 'Song')];
    let g = this.doc;
    this.path.forEach((id, i) => {
      const n = g.nodes.find((x) => x.id === id);
      g = n.graph;
      parts.push(el('span', { class: 'sep' }, '›'));
      if (i === this.path.length - 1) parts.push(el('span', { class: 'here' }, n.title));
      else parts.push(el('button', { onclick: () => this.exitTo(i + 1) }, n.title));
    });
    parts.push(el('span', { class: 'crumb-hint' }, 'Esc: up a level'));
    this.crumbsEl.replaceChildren(...parts);
  }

  /** Nodes on the canvas: the graph's nodes plus, inside a group, its port nodes. */
  allNodes() {
    const list = [...this.g.nodes];
    if (this.path.length) {
      const io = this.g.io;
      list.unshift(Object.assign(io.in, { id: IN, type: IN }), Object.assign(io.out, { id: OUT, type: OUT }));
    }
    return list;
  }

  nextId() {
    let max = 0;
    for (const n of this.g.nodes) {
      const m = /^n(\d+)$/.exec(n.id);
      if (m) max = Math.max(max, Number(m[1]));
    }
    return `n${max + 1}`;
  }

  node(id) {
    return this.allNodes().find((n) => n.id === id);
  }

  defaultParams(spec) {
    return Object.fromEntries(spec.params.map((p) => [p.name, p.default]));
  }

  /** The spec of any canvas node, including groups and group port nodes. */
  specFor(n) {
    if (!n) return null;
    if (n.type === 'group') {
      return {
        kind: 'group',
        title: n.title,
        category: 'Group',
        help: 'A group of nodes. Open it to edit what is inside; its exposed controls appear here.',
        inputs: n.graph.inputs.map((p) => ({ ...p, optional: true })),
        outputs: n.graph.outputs.map((p) => ({ ...p, optional: false })),
        params: this.exposedParams(n, this.flat(n.id)),
      };
    }
    if (n.type === IN) {
      return {
        kind: IN, title: 'Group inputs', category: 'Group', help: 'The inputs of this group. Drag a wire from a node input to "+" to add one.',
        inputs: [], params: [],
        outputs: [...this.g.inputs.map((p) => ({ ...p, io: true })), { name: '+', type: 'any', add: true }],
      };
    }
    if (n.type === OUT) {
      return {
        kind: OUT, title: 'Group outputs', category: 'Group', help: 'The outputs of this group. Drag a wire from a node output to "+" to add one.',
        outputs: [], params: [],
        inputs: [...this.g.outputs.map((p) => ({ ...p, io: true })), { name: '+', type: 'any', add: true }],
      };
    }
    return this.specs[n.type] ?? null;
  }

  /** Parameter specs a group shows: its exposed parameters, bound to the inner nodes. */
  exposedParams(group, groupFlat) {
    const out = [];
    for (const key of group.graph.exposed ?? []) {
      const r = this.resolveExposed(group, groupFlat, key);
      if (r) out.push({ ...r.spec, name: key, label: r.label, target: r });
    }
    // Tell apart repeated labels that weren't named (double-click a label to name it).
    const counts = {};
    for (const p of out) counts[p.label] = (counts[p.label] ?? 0) + 1;
    for (const p of out) if (counts[p.label] > 1 && !p.target.named) p.label = `${p.label} (${p.target.id ?? p.target.title})`;
    return out;
  }

  resolveExposed(group, groupFlat, key) {
    const dot = key.indexOf('.');
    if (dot < 0) return null;
    const id = key.slice(0, dot);
    const rest = key.slice(dot + 1);
    const inner = group.graph.nodes.find((n) => n.id === id);
    if (!inner) return null;
    const flat = `${groupFlat}/${id}`;
    const named = group.graph.labels?.[key];
    if (inner.type === 'group') {
      const r = this.resolveExposed(inner, flat, rest);
      return r && named ? { ...r, label: named, named: true } : r;
    }
    const spec = this.specs[inner.type];
    const ps = spec?.params.find((p) => p.name === rest);
    if (!ps) return null;
    return { owner: inner, name: rest, flat, spec: ps, label: named ?? ps.label, named: !!named, title: spec.title, id };
  }

  // ---- Editing ---------------------------------------------------------------

  addNode(type, x, y) {
    const n = { id: this.nextId(), type, x: Math.round(x), y: Math.round(y), params: {} };
    if (type === 'group') {
      n.title = 'Group';
      n.graph = emptyGraph();
    } else {
      n.params = this.defaultParams(this.specs[type]);
    }
    return this.insertNode(n);
  }

  insertNode(n) {
    this.g.nodes.push(n);
    this.nodesEl.append(this.renderNode(n));
    this.select([n.id]);
    this.changed(true);
    return n;
  }

  /** Adds a saved group from the library. */
  addLibraryGroup(entry, x, y) {
    const src = normalizeNodes([entry.group])[0];
    const n = { ...src, id: this.nextId(), x: Math.round(x), y: Math.round(y) };
    return this.insertNode(n);
  }

  removeSelection() {
    if (this.selectedWire !== null) {
      this.g.wires.splice(this.selectedWire, 1);
      this.selectedWire = null;
      this.updateWires();
      this.changed(true);
      return;
    }
    const ids = [...this.selection].filter((id) => id !== IN && id !== OUT);
    if (!ids.length) return;
    const gone = new Set(ids);
    this.g.nodes = this.g.nodes.filter((n) => !gone.has(n.id));
    this.g.wires = this.g.wires.filter((w) => !gone.has(w.from.node) && !gone.has(w.to.node));
    if (this.g.exposed) this.g.exposed = this.g.exposed.filter((k) => !gone.has(k.split('.')[0]));
    for (const id of ids) {
      this.nodeEl(id)?.remove();
      this.dropOverrides(this.flat(id));
    }
    this.selection.clear();
    this.updateWires();
    this.changed(true);
  }

  duplicateSelection() {
    const ids = [...this.selection].filter((id) => id !== IN && id !== OUT);
    if (!ids.length) return;
    const map = {};
    const fresh = [];
    for (const id of ids) {
      const src = this.node(id);
      const n = { ...structuredClone(src), id: this.nextId(), x: src.x + 30, y: src.y + 30 };
      map[id] = n.id;
      this.g.nodes.push(n);
      this.nodesEl.append(this.renderNode(n));
      fresh.push(n.id);
    }
    // Copy wires between duplicated nodes.
    for (const w of [...this.g.wires]) {
      if (map[w.from.node] && map[w.to.node]) {
        this.g.wires.push({ from: { node: map[w.from.node], port: w.from.port }, to: { node: map[w.to.node], port: w.to.port } });
      }
    }
    this.select(fresh);
    this.updateWires();
    this.changed(true);
  }

  canConnect(fromType, toType) {
    if (fromType === 'any' || toType === 'any') return true;
    return this.compat.has(`${fromType}>${toType}`);
  }

  /** Connects an output to an input, replacing whatever fed that input. */
  connect(from, to) {
    if (from.node === to.node) return false;
    // Dropping on a group's "+" port makes a new port of the matching type.
    if (from.node === IN && from.port === '+') {
      const type = this.portType(to.node, 'in', to.port);
      if (!type || type === 'any') return false;
      const name = uniqueName(this.g.inputs, to.port);
      this.g.inputs.push({ name, type });
      from = { node: IN, port: name };
      this.rerenderNode(IN);
    }
    if (to.node === OUT && to.port === '+') {
      const type = this.portType(from.node, 'out', from.port);
      if (!type || type === 'any') return false;
      const name = uniqueName(this.g.outputs, from.port);
      this.g.outputs.push({ name, type });
      to = { node: OUT, port: name };
      this.rerenderNode(OUT);
    }
    this.g.wires = this.g.wires.filter((w) => !(w.to.node === to.node && w.to.port === to.port));
    this.g.wires.push({ from: { node: from.node, port: from.port }, to: { node: to.node, port: to.port } });
    requestAnimationFrame(() => this.updateWires());
    this.changed(true);
    return true;
  }

  /** Removes one of the current group's ports, and the wires to it inside and outside. */
  removeGroupPort(dir, name) {
    const outer = this.parentGraph();
    const gid = this.path.at(-1);
    if (dir === 'in') {
      this.g.inputs = this.g.inputs.filter((p) => p.name !== name);
      this.g.wires = this.g.wires.filter((w) => !(w.from.node === IN && w.from.port === name));
      outer.wires = outer.wires.filter((w) => !(w.to.node === gid && w.to.port === name));
      this.rerenderNode(IN);
    } else {
      this.g.outputs = this.g.outputs.filter((p) => p.name !== name);
      this.g.wires = this.g.wires.filter((w) => !(w.to.node === OUT && w.to.port === name));
      outer.wires = outer.wires.filter((w) => !(w.from.node === gid && w.from.port === name));
      this.rerenderNode(OUT);
    }
    requestAnimationFrame(() => this.updateWires());
    this.changed(true);
  }

  renameGroupPort(dir, name) {
    const list = dir === 'in' ? this.g.inputs : this.g.outputs;
    const port = list.find((p) => p.name === name);
    const answer = window.prompt('Port name', name);
    if (!port || answer == null) return;
    const fresh = uniqueName(list.filter((p) => p !== port), answer.trim());
    if (fresh === name) return;
    port.name = fresh;
    const outer = this.parentGraph();
    const gid = this.path.at(-1);
    for (const w of this.g.wires) {
      if (dir === 'in' && w.from.node === IN && w.from.port === name) w.from.port = fresh;
      if (dir === 'out' && w.to.node === OUT && w.to.port === name) w.to.port = fresh;
    }
    for (const w of outer.wires) {
      if (dir === 'in' && w.to.node === gid && w.to.port === name) w.to.port = fresh;
      if (dir === 'out' && w.from.node === gid && w.from.port === name) w.from.port = fresh;
    }
    this.rerenderNode(dir === 'in' ? IN : OUT);
    requestAnimationFrame(() => this.updateWires());
    this.changed(true);
  }

  /** Collapses the selected nodes into a group, with ports for the wires that cross its edge. */
  groupSelection() {
    const ids = new Set([...this.selection].filter((id) => id !== IN && id !== OUT));
    if (!ids.size) return;
    const g = this.g;
    const inner = g.nodes.filter((n) => ids.has(n.id));
    const graph = emptyGraph();
    graph.nodes = inner;
    const gid = this.nextId();
    const inputs = new Map(); // "node/port" of the outer source -> input name
    const outputs = new Map(); // "node/port" of the inner source -> output name
    const outerWires = [];
    for (const w of g.wires) {
      const fromIn = ids.has(w.from.node);
      const toIn = ids.has(w.to.node);
      if (fromIn && toIn) graph.wires.push(w);
      else if (toIn) {
        const key = `${w.from.node}/${w.from.port}`;
        let name = inputs.get(key);
        if (!name) {
          name = uniqueName(graph.inputs, w.to.port);
          graph.inputs.push({ name, type: this.portType(w.to.node, 'in', w.to.port) ?? 'notes' });
          inputs.set(key, name);
          outerWires.push({ from: w.from, to: { node: gid, port: name } });
        }
        graph.wires.push({ from: { node: IN, port: name }, to: w.to });
      } else if (fromIn) {
        const key = `${w.from.node}/${w.from.port}`;
        let name = outputs.get(key);
        if (!name) {
          name = uniqueName(graph.outputs, w.from.port);
          graph.outputs.push({ name, type: this.portType(w.from.node, 'out', w.from.port) ?? 'notes' });
          outputs.set(key, name);
          graph.wires.push({ from: w.from, to: { node: OUT, port: name } });
        }
        outerWires.push({ from: { node: gid, port: name }, to: w.to });
      } else outerWires.push(w);
    }
    const xs = inner.map((n) => n.x);
    const ys = inner.map((n) => n.y);
    const minX = Math.min(...xs);
    const minY = Math.min(...ys);
    const maxX = Math.max(...inner.map((n) => n.x + (this.nodeEl(n.id)?.offsetWidth ?? 240)));
    graph.io = { in: { x: minX - 260, y: minY }, out: { x: maxX + 60, y: minY } };
    // Exposed parameters move with their nodes.
    if (g.exposed) {
      graph.exposed = g.exposed.filter((k) => ids.has(k.split('.')[0]));
      g.exposed = g.exposed.filter((k) => !ids.has(k.split('.')[0]));
    }
    const group = { id: gid, type: 'group', title: 'Group', x: minX, y: minY, params: {}, graph };
    g.nodes = [...g.nodes.filter((n) => !ids.has(n.id)), group];
    g.wires = outerWires;
    for (const id of ids) this.renameOverrides(this.flat(id), this.flat(`${gid}/${id}`));
    this.renderAll();
    this.select([gid]);
    this.changed(true);
  }

  /** Replaces the selected group with its contents. */
  ungroupSelection() {
    const ids = [...this.selection];
    const group = this.g.nodes.find((n) => n.id === ids[0] && n.type === 'group');
    if (ids.length !== 1 || !group) return;
    const g = this.g;
    const inner = group.graph;
    const rename = {};
    const taken = new Set(g.nodes.map((n) => n.id));
    let next = Number(this.nextId().slice(1));
    const lifted = inner.nodes.map((n) => {
      let id = n.id;
      if (taken.has(id) || id === group.id) {
        while (taken.has(`n${next}`)) next++;
        id = `n${next++}`;
      }
      taken.add(id);
      rename[n.id] = id;
      return { ...n, id };
    });
    const sourcesOf = (port) => g.wires.filter((w) => w.to.node === group.id && w.to.port === port).map((w) => w.from);
    const wires = g.wires.filter((w) => w.to.node !== group.id && w.from.node !== group.id);
    for (const w of inner.wires) {
      const fromIo = w.from.node === IN;
      const toIo = w.to.node === OUT;
      const froms = fromIo ? sourcesOf(w.from.port) : [{ node: rename[w.from.node], port: w.from.port }];
      const tos = toIo
        ? g.wires.filter((x) => x.from.node === group.id && x.from.port === w.to.port).map((x) => x.to)
        : [{ node: rename[w.to.node], port: w.to.port }];
      for (const f of froms) for (const t of tos) wires.push({ from: { ...f }, to: { ...t } });
    }
    g.nodes = [...g.nodes.filter((n) => n !== group), ...lifted];
    g.wires = wires;
    if (g.exposed) {
      g.exposed = g.exposed.filter((k) => k.split('.')[0] !== group.id);
    }
    for (const [old, fresh] of Object.entries(rename)) this.renameOverrides(this.flat(`${group.id}/${old}`), this.flat(fresh));
    this.renderAll();
    this.select(lifted.map((n) => n.id));
    this.changed(true);
  }

  /** Shows or hides a parameter of a node inside this group on the group node. */
  toggleExposed(key) {
    const list = this.g.exposed ?? (this.g.exposed = []);
    const i = list.indexOf(key);
    if (i >= 0) list.splice(i, 1);
    else list.push(key);
    this.changed(true);
  }

  renameGroup(n) {
    const answer = window.prompt('Group name (also the part name in MIDI exports)', n.title);
    if (answer == null || !answer.trim()) return;
    n.title = answer.trim();
    this.rerenderNode(n.id);
    this.changed(true);
  }

  changed(graph) {
    this.emptyHint.hidden = this.allNodes().length > 0;
    this.onChange?.({ graph });
  }

  // ---- Sections and overrides ---------------------------------------------------

  section() {
    return this.focus != null ? this.doc.arrangement.sections[this.focus] ?? null : null;
  }

  setFocus(index) {
    this.focus = index;
    if (index == null) this.sectionEdit = false;
    this.renderAll();
  }

  setSectionEdit(on) {
    this.sectionEdit = !!on && this.section() != null;
    this.canvas.classList.toggle('section-edit', this.sectionEdit);
    this.onSectionMode?.(this.sectionEdit);
  }

  paramValue(owner, flat, name, def) {
    const over = this.section()?.overrides?.[flat];
    if (over && name in over) return { value: over[name], over: true };
    return { value: owner.params[name] ?? def, over: false };
  }

  setParam(owner, flat, name, v) {
    const s = this.section();
    if (this.sectionEdit && s) {
      (s.overrides[flat] ??= {})[name] = v;
    } else {
      owner.params[name] = v;
    }
    this.changed(true);
  }

  clearOverride(flat, name) {
    const s = this.section();
    if (!s?.overrides[flat]) return;
    delete s.overrides[flat][name];
    if (!Object.keys(s.overrides[flat]).length) delete s.overrides[flat];
    this.renderAll();
    this.changed(true);
  }

  /** Moves overrides when a node's path changes (grouping). */
  renameOverrides(from, to) {
    for (const s of this.doc.arrangement.sections) {
      for (const key of Object.keys(s.overrides)) {
        if (key === from || key.startsWith(`${from}/`)) {
          s.overrides[to + key.slice(from.length)] = s.overrides[key];
          delete s.overrides[key];
        }
      }
    }
  }

  dropOverrides(flat) {
    for (const s of this.doc.arrangement.sections) {
      for (const key of Object.keys(s.overrides)) {
        if (key === flat || key.startsWith(`${flat}/`)) delete s.overrides[key];
      }
    }
  }

  /** How many parameters a section overrides under a path prefix (for group badges). */
  overridesUnder(flat) {
    const s = this.section();
    if (!s) return 0;
    return Object.entries(s.overrides)
      .filter(([k]) => k.startsWith(`${flat}/`))
      .reduce((sum, [, v]) => sum + Object.keys(v).length, 0);
  }

  // ---- Rendering ----------------------------------------------------------------

  nodeEl(id) {
    return this.nodesEl.querySelector(`.node[data-id="${CSS.escape(id)}"]`);
  }

  renderAll() {
    this.hideProbe();
    this.nodesEl.replaceChildren(...this.allNodes().map((n) => this.renderNode(n)));
    this.emptyHint.hidden = this.allNodes().length > 0;
    this.renderCrumbs();
    this.canvas.classList.toggle('in-group', this.path.length > 0);
    this.canvas.classList.toggle('section-edit', this.sectionEdit);
    if (this.report) this.setReport(this.report);
    // Ports need layout before wires can find them.
    requestAnimationFrame(() => this.updateWires());
  }

  rerenderNode(id) {
    const n = this.node(id);
    const old = this.nodeEl(id);
    if (!n || !old) return;
    old.replaceWith(this.renderNode(n));
    if (this.report) this.setReport(this.report);
  }

  renderNode(n) {
    const spec = this.specFor(n);
    if (!spec) {
      return el('div', { class: 'node has-issue', dataset: { id: n.id, cat: 'Output' }, style: `left:${n.x}px;top:${n.y}px` },
        el('div', { class: 'node-head' }, el('span', { class: 'title' }, n.type)),
        el('div', { class: 'node-issue' }, `Unknown node type "${n.type}"`));
    }
    const ins = el('div', { class: 'col' }, ...spec.inputs.map((p) => this.renderPort(n, p, 'in')));
    const outs = el('div', { class: 'col' }, ...spec.outputs.map((p) => this.renderPort(n, p, 'out')));
    const params = el('div', { class: 'node-params' }, ...spec.params.map((p) => this.renderParam(n, p)));
    const isGroup = n.type === 'group';
    const isIo = n.type === IN || n.type === OUT;
    const classes = ['node'];
    if (WIDE.has(n.type)) classes.push('wide');
    if (XWIDE.has(n.type)) classes.push('xwide');
    if (isGroup) classes.push('group-node');
    if (isIo) classes.push('io-node');
    if (this.selection.has(n.id)) classes.push('selected');
    const head = el('div', { class: 'node-head', title: spec.help },
      el('span', { class: 'title' }, spec.title),
      isGroup
        ? el('span', { class: 'head-buttons' },
          el('button', { class: 'mini', title: 'Open the group (or double-click its header)', onclick: () => this.enter(n.id) }, 'Open'),
          el('button', { class: 'mini', title: 'More', onclick: (e) => this.groupMenu(n, e) }, '⋯'))
        : el('span', { class: 'cat' }, spec.category));
    if (isGroup) {
      head.addEventListener('dblclick', (e) => {
        if (e.target.closest('button')) return;
        this.enter(n.id);
      });
    }
    const node = el('div', {
      class: classes.join(' '),
      dataset: { id: n.id, cat: spec.category },
      style: `left:${n.x}px;top:${n.y}px`,
    },
    head,
    (spec.inputs.length || spec.outputs.length) ? el('div', { class: 'node-ports' }, ins, outs) : null,
    el('div', { class: 'node-view' }),
    spec.params.length ? params : null,
    isGroup ? el('div', { class: 'group-info' }) : null,
    el('pre', { class: 'node-preview' }),
    el('div', { class: 'node-issue' }));
    return node;
  }

  groupMenu(n, e) {
    e.stopPropagation();
    this.showPopup(e.clientX, e.clientY, [
      ['Rename…', () => this.renameGroup(n)],
      ['Open', () => this.enter(n.id)],
      ['Save to library…', () => this.onLibrarySave?.(n)],
      ['Ungroup', () => { this.select([n.id]); this.ungroupSelection(); }],
    ]);
  }

  /** A small menu of [label, action] at a screen position. */
  showPopup(x, y, items) {
    this.closePopup();
    const menu = el('div', { class: 'popup' }, ...items.map(([label, action]) => el('button', {
      onclick: () => { this.closePopup(); action(); },
    }, label)));
    menu.style.left = `${Math.min(x, window.innerWidth - 200)}px`;
    menu.style.top = `${Math.min(y, window.innerHeight - 40 * items.length)}px`;
    document.body.append(menu);
    this.popup = menu;
  }

  closePopup() {
    this.popup?.remove();
    this.popup = null;
  }

  renderPort(n, p, dir) {
    const port = el('div', {
      class: `port ${dir}${p.optional ? ' optional' : ''}${p.add ? ' add' : ''}`,
      dataset: { port: p.name, type: p.type, dir, ...(p.add ? { add: '1' } : {}) },
      title: p.add ? 'Drop a wire here to add a port' : `${p.name} (${p.type})`,
    },
    dir === 'in' ? el('span', { class: 'dot' }) : null,
    el('span', { class: 'label' }, p.add ? '+ add' : p.name),
    dir === 'out' ? el('span', { class: 'dot' }) : null);
    if (dir === 'out' && AUDIBLE.has(p.type) && !p.io && n.type !== 'group' && n.type !== IN) {
      // Clicking a theory output's name plays it.
      port.classList.add('audible');
      port.title = `${p.name} (${p.type}) · click to hear`;
      port.querySelector('.label').addEventListener('click', () => this.audition(n.id, p.name));
    }
    if (p.io) {
      // Group port nodes: rename on double-click, remove with ×.
      const ioDir = n.type === IN ? 'in' : 'out';
      port.querySelector('.label').addEventListener('dblclick', () => this.renameGroupPort(ioDir, p.name));
      port.append(el('button', { class: 'port-x', title: 'Remove this port', onclick: (e) => { e.stopPropagation(); this.removeGroupPort(ioDir, p.name); } }, '×'));
      port.title = `${p.name} (${p.type}) · double-click to rename`;
    }
    return port;
  }

  renderParam(n, p) {
    const t = p.target ?? { owner: n, name: p.name, flat: this.flat(n.id) };
    const { value, over } = this.paramValue(t.owner, t.flat, t.name, p.default);
    let label;
    const set = (v) => {
      this.setParam(t.owner, t.flat, t.name, v);
      if (this.sectionEdit) label?.classList.add('overridden');
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
    const name = el('span', { class: 'pname' }, p.label);
    if (p.target && n.type === 'group') {
      // Exposed controls on a group node: double-click the label to rename it.
      name.addEventListener('dblclick', (e) => {
        e.preventDefault();
        const answer = window.prompt('Label for this control', p.label);
        if (answer == null) return;
        const labels = n.graph.labels ?? (n.graph.labels = {});
        if (answer.trim()) labels[p.name] = answer.trim();
        else delete labels[p.name];
        this.rerenderNode(n.id);
        this.changed(false);
      });
      name.title = `${p.label} · double-click to rename`;
    }
    label = el('label', { class: `param${over ? ' overridden' : ''}`, title: p.label }, name, control);
    if (over) {
      label.title = `${p.label}: set for ${this.section().name} only. Click the dot to use the song value.`;
      name.prepend(el('button', {
        class: 'over-dot', title: `Set for ${this.section().name} only; click to use the song value`,
        onclick: (e) => { e.preventDefault(); e.stopPropagation(); this.clearOverride(t.flat, t.name); },
      }));
    }
    // Inside a group, a pin shows the parameter on the group node.
    if (this.path.length && n.type !== IN && n.type !== OUT) {
      const key = `${n.id}.${p.name}`;
      const pinned = (this.g.exposed ?? []).includes(key);
      name.prepend(el('button', {
        class: `pin${pinned ? ' on' : ''}`,
        title: pinned ? 'Shown on the group node; click to hide' : 'Show this control on the group node',
        onclick: (e) => { e.preventDefault(); e.stopPropagation(); this.toggleExposed(key); e.currentTarget.classList.toggle('on'); },
      }, '◆'));
    }
    return label;
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
    for (const select of this.nodesEl.querySelectorAll('select.midi-port')) this.fillPortSelect(select, select.value);
  }

  /** Shows the engine's previews and issues on the nodes. */
  setReport(report) {
    this.report = report;
    this.probe.cache.clear();
    const issues = {};
    for (const i of report.issues ?? []) (issues[i.node] ??= []).push(i.message);
    for (const n of this.allNodes()) {
      const node = this.nodeEl(n.id);
      if (!node) continue;
      const flat = this.flat(n.id);
      const pre = node.querySelector('.node-preview');
      const issue = node.querySelector('.node-issue');
      const data = report.views?.[flat];
      let mine = issues[flat] ?? [];
      if (n.type === 'group') {
        // Problems inside the group show on it, with the inner node named.
        const inside = Object.entries(issues).filter(([k]) => k.startsWith(`${flat}/`)).flatMap(([, v]) => v);
        if (inside.length) mine = [`${inside.length} problem${inside.length === 1 ? '' : 's'} inside: ${inside[0]}`];
        const info = node.querySelector('.group-info');
        if (info) {
          const count = countNodes(n.graph);
          const over = this.overridesUnder(flat);
          info.textContent = `${count} node${count === 1 ? '' : 's'} inside${over ? ` · ${over} set for ${this.section().name}` : ''}`;
        }
      }
      // A node with a drawn view shows its text preview on hover instead.
      const text = report.previews?.[flat] ?? '';
      if (pre) pre.textContent = data ? '' : text;
      if (issue) issue.textContent = mine.join('\n');
      node.classList.toggle('has-issue', mine.length > 0);
      const view = node.querySelector('.node-view');
      if (view) {
        view.replaceChildren(...(data ? this.renderView(flat, data) : []));
        view.title = data && data.kind !== 'roll' ? text : '';
        if (data?.kind === 'keys' || data?.kind === 'steps') {
          view.append(el('button', {
            class: 'audition', title: 'Hear it (or click the output\'s name)',
            onclick: (e) => { e.stopPropagation(); this.audition(n.id, null); },
          }, '▶'));
        }
      }
    }
    this.lastTick = null;
  }

  renderView(flat, data) {
    if (data.kind === 'keys') return [renderKeys(data)];
    if (data.kind === 'steps') return [renderSteps(data)];
    if (data.kind !== 'roll') return [];
    if (data.compact) return renderRoll(data);
    const mode = this.rollMode[flat] ?? 'roll';
    const staffable = (data.notes ?? []).every((n) => /^[A-G]/.test(n.name));
    const parts = mode === 'staff' && staffable ? renderStaff(data) : renderRoll(data);
    if (staffable && (data.notes ?? []).length) {
      const toggle = el('span', { class: 'view-toggle' },
        el('button', { class: mode !== 'staff' ? 'on' : '', onclick: () => { this.rollMode[flat] = 'roll'; this.setReport(this.report); } }, 'Roll'),
        el('button', { class: mode === 'staff' ? 'on' : '', onclick: () => { this.rollMode[flat] = 'staff'; this.setReport(this.report); } }, 'Staff'));
      parts[0].append(toggle);
    }
    return parts;
  }

  /** Plays one of a node's outputs (null: the first) once, and marks the node meanwhile. */
  async audition(id, port) {
    const seconds = await this.onAudition?.(this.flat(id), port);
    const node = this.nodeEl(id);
    if (seconds == null || !node) return;
    clearTimeout(this.auditionTimer);
    for (const e of this.nodesEl.querySelectorAll('.node.auditioning')) e.classList.remove('auditioning');
    node.classList.add('auditioning');
    this.auditionTimer = setTimeout(() => this.nodeEl(id)?.classList.remove('auditioning'), seconds * 1000);
  }

  /** Moves the playhead on piano-roll views and lights up previews (tick = null: stopped). */
  setPlayhead(tick) {
    if (tick === this.lastTick) return;
    this.lastTick = tick;
    const all = (sel) => [...this.nodesEl.querySelectorAll(sel), ...this.probe.el.querySelectorAll(sel)];
    for (const preview of all('[data-preview]')) preview.showAt?.(tick);
    for (const line of all('.roll-playhead')) {
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

  // ---- Wire probe ------------------------------------------------------------------

  /**
   * The flat node path and port that produce the value on an output, looking through
   * groups: a group's output leads to what feeds it inside, a group input to what feeds
   * the group outside. Null when nothing does.
   */
  resolveSource(path, node, port) {
    const graphAt = (p) => p.reduce((g, id) => g.nodes.find((n) => n.id === id).graph, this.doc);
    for (let depth = 0; depth < 64; depth++) {
      if (node === IN) {
        if (!path.length) return null;
        const w = graphAt(path.slice(0, -1)).wires.find((x) => x.to.node === path.at(-1) && x.to.port === port);
        if (!w) return null;
        path = path.slice(0, -1);
        ({ node, port } = w.from);
        continue;
      }
      const n = graphAt(path).nodes.find((x) => x.id === node);
      if (n?.type === 'group') {
        const w = n.graph.wires.find((x) => x.to.node === OUT && x.to.port === port);
        if (!w) return null;
        path = [...path, node];
        ({ node, port } = w.from);
        continue;
      }
      return { node: [...path, node].join('/'), port };
    }
    return null;
  }

  /** Starts showing what a hovered wire carries, after a short pause. */
  hoverWire(e) {
    const g = e.target.closest?.('[data-wire]');
    if (!g || this.drag || this.menu) return;
    const i = Number(g.dataset.wire);
    if (this.probe.wire === i) return;
    this.hideProbe();
    this.probe.wire = i;
    g.querySelector('path.wire')?.classList.add('probed');
    this.probe.timer = setTimeout(() => this.showProbe(i), 150);
  }

  async showProbe(i) {
    const w = this.g.wires[i];
    if (!w || this.probe.wire !== i) return;
    const src = this.resolveSource(this.path, w.from.node, w.from.port);
    const key = src && `${src.node}|${src.port}`;
    let result = key ? this.probe.cache.get(key) : { error: 'Nothing feeds this group port yet' };
    if (!result) {
      result = (await this.onProbe?.(src.node, src.port)) ?? { error: 'Nothing to show' };
      this.probe.cache.set(key, result);
    }
    if (this.probe.wire !== i) return; // moved on while waiting
    const name = (id, port) => `${this.specFor(this.node(id))?.title ?? id} · ${port}`;
    const type = this.portType(w.from.node, 'out', w.from.port);
    const head = el('div', { class: 'probe-head' },
      el('span', { class: 'swatch', style: `background:${TYPE_COLOR[type] ?? '#888'}` }),
      `${name(w.from.node, w.from.port)} → ${name(w.to.node, w.to.port)}`);
    const v = result.view;
    const view = v?.kind === 'keys' ? [renderKeys(v)] : v?.kind === 'steps' ? [renderSteps(v)] : v?.kind === 'roll' ? renderRoll(v) : [];
    const text = result.error ? el('div', { class: 'probe-error' }, result.error) : el('pre', { class: 'probe-text' }, result.text);
    this.probe.el.replaceChildren(head, ...view, text);
    this.probe.el.hidden = false;
    this.placeProbe();
    // Light up the new views at the current playhead.
    const tick = this.lastTick;
    this.lastTick = undefined;
    this.setPlayhead(tick);
  }

  /** Keeps the probe card next to the pointer, inside the window. */
  placeProbe(x = this.probe.x, y = this.probe.y) {
    this.probe.x = x;
    this.probe.y = y;
    const p = this.probe.el;
    if (p.hidden) return;
    const left = x + 16 + p.offsetWidth > window.innerWidth - 8 ? x - 16 - p.offsetWidth : x + 16;
    const top = Math.min(y + 16, window.innerHeight - p.offsetHeight - 8);
    p.style.left = `${Math.max(8, left)}px`;
    p.style.top = `${Math.max(8, top)}px`;
  }

  hideProbe() {
    clearTimeout(this.probe.timer);
    this.probe.wire = null;
    this.probe.el.hidden = true;
    for (const p of this.wiresEl.querySelectorAll('path.probed')) p.classList.remove('probed');
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
    const spec = this.specFor(this.node(nodeId));
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
    this.g.wires.forEach((w, i) => {
      const a = this.dotPos(w.from.node, 'out', w.from.port);
      const b = this.dotPos(w.to.node, 'in', w.to.port);
      if (!a || !b) return;
      connected.add(`${w.to.node}/${w.to.port}`);
      const fromType = this.portType(w.from.node, 'out', w.from.port);
      const toType = this.portType(w.to.node, 'in', w.to.port);
      const d = Editor.curve(a, b);
      const g = svg('g', { 'data-wire': i });
      g.append(svg('path', { class: 'hit', d }));
      const cls = `wire${this.selectedWire === i ? ' selected' : ''}${this.probe.wire === i ? ' probed' : ''}`;
      g.append(svg('path', { class: cls, d, stroke: TYPE_COLOR[fromType] ?? '#888' }));
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
    const nodes = this.allNodes();
    if (!nodes.length) {
      this.view = { x: 40, y: 40, z: 1 };
      this.applyView();
      return;
    }
    requestAnimationFrame(() => {
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (const n of nodes) {
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
    // Hovering a wire shows what it carries.
    this.wiresEl.addEventListener('pointerover', (e) => this.hoverWire(e));
    this.wiresEl.addEventListener('pointermove', (e) => this.placeProbe(e.clientX, e.clientY));
    this.wiresEl.addEventListener('pointerout', (e) => {
      if (!e.relatedTarget?.closest?.('#wires [data-wire]')) this.hideProbe();
    });
    c.addEventListener('dblclick', (e) => {
      if (e.target.closest('.node')) return;
      this.openAddMenu(e.clientX, e.clientY, null);
    });
    c.addEventListener('keydown', (e) => {
      if (isTyping(e)) return;
      const mod = e.ctrlKey || e.metaKey;
      if (e.key === 'Delete' || e.key === 'Backspace') {
        e.preventDefault();
        this.removeSelection();
      } else if (mod && e.key.toLowerCase() === 'd') {
        e.preventDefault();
        this.duplicateSelection();
      } else if (mod && e.key.toLowerCase() === 'g') {
        e.preventDefault();
        if (e.shiftKey) this.ungroupSelection();
        else this.groupSelection();
      } else if (e.key === 'Tab') {
        e.preventDefault();
        const r = c.getBoundingClientRect();
        this.openAddMenu(r.left + r.width / 2 - 150, r.top + r.height / 3, null);
      } else if ((e.key === 'f' || e.key === 'F') && !mod) {
        this.frame();
      } else if (e.key === 'Escape' && this.path.length) {
        this.exitTo(this.path.length - 1);
      } else if (e.key === 'Enter' && this.selection.size === 1) {
        this.enter([...this.selection][0]);
      }
    });
    this.addSearch.addEventListener('input', () => this.fillAddList());
    this.addSearch.addEventListener('keydown', (e) => this.onMenuKey(e));
    document.addEventListener('pointerdown', (e) => {
      if (this.menu && !this.addMenu.contains(e.target)) this.closeAddMenu();
      if (this.popup && !this.popup.contains(e.target)) this.closePopup();
    }, true);
  }

  onDown(e) {
    this.hideProbe();
    if (e.button !== 0 && e.button !== 1) return;
    const dot = e.target.closest('.dot');
    const nodeE = e.target.closest('.node');
    const wireG = e.target.closest('[data-wire]');
    this.canvas.focus({ preventScroll: true });

    if (dot) {
      e.preventDefault();
      const portE = dot.closest('.port');
      if (portE.dataset.add) return; // "+" ports only take drops
      const id = nodeE.dataset.id;
      const fixed = { node: id, port: portE.dataset.port, type: portE.dataset.type, dir: portE.dataset.dir };
      if (fixed.dir === 'in') {
        // Pick up an existing wire into this input and move its free end.
        const i = this.g.wires.findIndex((w) => w.to.node === id && w.to.port === fixed.port);
        if (i >= 0) {
          const w = this.g.wires.splice(i, 1)[0];
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
      if (e.target.closest('.node-head') && !e.target.closest('button')) {
        e.preventDefault();
        const start = this.toWorld(e.clientX, e.clientY);
        const nodes = [...this.selection].map((sid) => this.node(sid)).filter(Boolean).map((n) => ({ n, x: n.x, y: n.y }));
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
    // Background: shift-drag selects a box; otherwise deselect and pan.
    e.preventDefault();
    if (e.shiftKey) {
      const start = this.toWorld(e.clientX, e.clientY);
      const box = el('div', { class: 'select-box' });
      this.world.append(box);
      this.drag = { kind: 'box', start, box };
      return;
    }
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
    const other = portE.closest('.node').dataset.id;
    if (other === fixed.node) return false;
    if (portE.dataset.add) return fixed.node !== IN && fixed.node !== OUT && fixed.type !== 'any';
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
        if (!ne) continue;
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
    } else if (d.kind === 'box') {
      const p = this.toWorld(e.clientX, e.clientY);
      const x = Math.min(p.x, d.start.x);
      const y = Math.min(p.y, d.start.y);
      Object.assign(d.box.style, { left: `${x}px`, top: `${y}px`, width: `${Math.abs(p.x - d.start.x)}px`, height: `${Math.abs(p.y - d.start.y)}px` });
      d.rect = { x, y, w: Math.abs(p.x - d.start.x), h: Math.abs(p.y - d.start.y) };
    }
  }

  onUp(e) {
    const d = this.drag;
    if (!d) return;
    this.drag = null;
    this.canvas.classList.remove('panning');
    if (d.kind === 'node' && d.moved) this.changed(false);
    if (d.kind === 'box') {
      d.box.remove();
      if (d.rect) {
        const r = d.rect;
        const hit = this.allNodes().filter((n) => {
          const ne = this.nodeEl(n.id);
          const w = ne?.offsetWidth ?? 240;
          const h = ne?.offsetHeight ?? 100;
          return n.x < r.x + r.w && n.x + w > r.x && n.y < r.y + r.h && n.y + h > r.y;
        });
        this.select(hit.map((n) => n.id), true);
      }
    }
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
    this.hideProbe();
    e.preventDefault();
    const r = this.canvas.getBoundingClientRect();
    const mx = e.clientX - r.left;
    const my = e.clientY - r.top;
    const z = Math.min(2, Math.max(0.2, this.view.z * Math.exp(-e.deltaY * 0.0015)));
    const k = z / this.view.z;
    this.view.x = mx - (mx - this.view.x) * k;
    this.view.y = my - (my - this.view.y) * k;
    this.view.z = z;
    this.applyView();
  }

  // ---- Add-node menu ---------------------------------------------------------------------

  openAddMenu(clientX, clientY, pending) {
    if (pending?.type === 'any') return;
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

  /** Everything the add menu offers: node kinds, an empty group, and saved groups. */
  menuEntries() {
    const groupEntry = {
      kind: 'group', title: 'Group', category: 'Structure', inputs: [], outputs: [],
      help: 'An empty group. Select nodes and press Ctrl+G to group them instead.',
    };
    const lib = this.library.map((entry) => ({
      kind: 'library', title: entry.name, category: 'Library', entry,
      help: `Saved group · ${countNodes(entry.group.graph)} nodes`,
      inputs: entry.group.graph?.inputs ?? [], outputs: entry.group.graph?.outputs ?? [],
    }));
    const list = [...this.specList];
    const at = list.findIndex((s) => s.category === 'Structure');
    list.splice(at < 0 ? list.length : at, 0, groupEntry);
    return [...list, ...lib];
  }

  fillAddList() {
    const q = this.addSearch.value.trim().toLowerCase();
    const pending = this.menu?.pending;
    const items = this.menuEntries().filter((s) => {
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
    const n = spec.kind === 'library' ? this.addLibraryGroup(spec.entry, x, at.y - 20) : this.addNode(spec.kind, x, at.y - 20);
    const port = this.pendingPort(this.specFor(n), pending);
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

function countNodes(graph) {
  return (graph?.nodes ?? []).reduce((sum, n) => sum + (n.type === 'group' ? countNodes(n.graph) : 1), 0);
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
function renderRoll(v) {
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
function renderKeys(v) {
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
 * might not play), bar lines, and the step under the playhead lit. */
function renderSteps(v) {
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
    if (o.prob < 100) r.classList.add('maybe');
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
function parseName(name) {
  const m = /^([A-G])(bb|b|##|#|x)?(-?\d+)$/.exec(name);
  if (!m) return null;
  const acc = { b: -1, bb: -2, '#': 1, '##': 2, x: 2 }[m[2]] ?? 0;
  return { step: Number(m[3]) * 7 + LETTERS.indexOf(m[1]), acc };
}

/** A simple grand staff: noteheads placed in time, coloured by voice. */
function renderStaff(v) {
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
