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
//
// Nodes are compact: their settings live in the inspector. A node's `pins: { preview,
// params: [name] }` choose what also shows on the node; see `previewPinned`. A pinned
// automatable parameter gets an input port named `param.<name>` that a Control can drive.

import { el, svg } from './dom.js';
import { renderCurve, renderKeys, renderPlain, renderRoll, renderStaff, renderSteps, renderTheory, showPlayhead } from './views.js';

export { el, svg };

const WIDE = new Set(['progression', 'voice', 'voice_lead', 'drop', 'invert', 'play_chords', 'transpose', 'chord',
  'section', 'tonnetz', 'interval_matrix',
  'chord_degree', 'extend', 'substitute', 'borrow', 'secondary_dominant', 'spread', 'close_open']);
const XWIDE = new Set(['counterpoint', 'piano_roll']);
const IN = '@in';
const OUT = '@out';
export const PARAM_PORT = 'param.';
// Marks clipboard text as nodes copied from this editor.
const CLIPBOARD_FORMAT = 'procedural-music/nodes@1';

const TYPE_COLOR = { scale: '#40d2be', chord: '#f5b446', rhythm: '#ff7aa8', notes: '#6e96ff', audio: '#b8e06a', control: '#b48cff', any: '#8a93a3' };
// Output types that can be auditioned with a click.
const AUDIBLE = new Set(['scale', 'chord', 'rhythm']);
// The pitch-set picker's keys, and how note names map back to pitch classes.
const PC_LABELS = ['C', 'Db', 'D', 'Eb', 'E', 'F', 'F#', 'G', 'Ab', 'A', 'Bb', 'B'];
const BLACK_PCS = new Set([1, 3, 6, 8, 10]);
function pcOfName(name) {
  const m = /^([A-Ga-g])([#b♯♭]*)$/.exec(name.trim());
  if (!m) return null;
  const natural = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 }[m[1].toLowerCase()];
  const shift = [...m[2]].reduce((s, c) => s + (c === '#' || c === '♯' ? 1 : -1), 0);
  return (((natural + shift) % 12) + 12) % 12;
}

/** An empty group graph. */
export function emptyGraph() {
  return { nodes: [], wires: [], inputs: [], outputs: [], exposed: [], labels: {}, io: { in: { x: 0, y: 0 }, out: { x: 700, y: 0 } } };
}

/** Fills in missing fields of a node list (recursively for groups). */
function normalizeNodes(nodes) {
  return (nodes ?? []).map((n) => {
    const out = { id: String(n.id), type: n.type, x: n.x ?? 0, y: n.y ?? 0, params: { ...(n.params ?? {}) } };
    if (n.pins) {
      out.pins = { params: [...(n.pins.params ?? [])] };
      if (typeof n.pins.preview === 'boolean') out.pins.preview = n.pins.preview;
    }
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

/** The document version this editor writes. */
export const DOC_VERSION = 2;

/**
 * Upgrades for documents saved by older versions: `MIGRATIONS[v]` changes a version-v
 * document (a private copy, edited in place) into version v + 1. Bump DOC_VERSION when
 * adding one. Nested group graphs can be reached with `eachGraph`.
 */
const MIGRATIONS = {};

/** Calls `f(graph, pathPrefix)` for a document's graph and every group graph inside it. */
function eachGraph(g, f, prefix = '') {
  if (!g?.nodes) return;
  f(g, prefix);
  for (const n of g.nodes) if (n.type === 'group') eachGraph(n.graph, f, `${prefix}${n.id}/`);
}

/** A copy of `doc` brought up to DOC_VERSION (a document without a version is taken as current). */
function migrateDoc(doc) {
  const out = structuredClone(doc ?? {});
  for (let v = out.version ?? DOC_VERSION; v < DOC_VERSION; v++) MIGRATIONS[v]?.(out, { eachGraph });
  return out;
}

export function normalizeDoc(doc) {
  doc = migrateDoc(doc);
  return {
    version: DOC_VERSION,
    bpm: doc.bpm ?? 110,
    ...(doc.clockPort ? { clockPort: doc.clockPort } : {}),
    ...(doc.clockInPort ? { clockInPort: doc.clockInPort } : {}),
    // A project-wide random lock: every playback uses this seed.
    ...(doc.random?.lock ? { random: { lock: true, seed: Number(doc.random.seed) || 0 } } : {}),
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
  constructor({ canvas, world, nodesEl, wiresEl, crumbsEl, addMenu, addSearch, addList, emptyHint, specs, onChange, onLibrarySave, onSectionMode, onAudition, onProbe, onSelect, onCaptureSeed }) {
    Object.assign(this, { canvas, world, nodesEl, wiresEl, crumbsEl, addMenu, addSearch, addList, emptyHint, onChange, onLibrarySave, onSectionMode, onAudition, onProbe, onSelect, onCaptureSeed });
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
    this.inspector = null; // set by the app (see inspector.js)
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
    this.onSelect?.([]);
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
      // Each new random node gets a seed of its own, so two of them don't play alike.
      if ('seed' in n.params) n.params.seed = randomSeed();
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

  // ---- Copy and paste ----------------------------------------------------------------
  // The clipboard holds the selected nodes and the wires between them, as JSON text, so
  // they paste into any graph or group, in this window or another.

  /** Copies (or cuts) the selected nodes to the clipboard. */
  copy(e, cut) {
    const ids = new Set([...this.selection].filter((id) => id !== IN && id !== OUT));
    if (!ids.size) return;
    const nodes = this.g.nodes.filter((n) => ids.has(n.id)).map((n) => structuredClone(n));
    const wires = this.g.wires.filter((w) => ids.has(w.from.node) && ids.has(w.to.node)).map((w) => structuredClone(w));
    const text = JSON.stringify({ format: CLIPBOARD_FORMAT, nodes, wires });
    e.preventDefault();
    e.clipboardData.setData('text/plain', text);
    this.clipboard = text;
    if (cut) this.removeSelection();
  }

  /** Pastes nodes from the clipboard under the mouse (or in the middle of the view). */
  paste(e) {
    const text = e.clipboardData?.getData('text/plain') || this.clipboard;
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      return;
    }
    if (data?.format !== CLIPBOARD_FORMAT || !data.nodes?.length) return;
    e.preventDefault();
    // Place the copied nodes' top-left corner at the mouse, if it's over the canvas.
    const r = this.canvas.getBoundingClientRect();
    const p = this.pointer;
    const over = p && p.x >= r.left && p.x <= r.right && p.y >= r.top && p.y <= r.bottom;
    const at = over ? this.toWorld(p.x, p.y) : this.toWorld(r.left + r.width / 3, r.top + r.height / 3);
    const minX = Math.min(...data.nodes.map((n) => n.x ?? 0));
    const minY = Math.min(...data.nodes.map((n) => n.y ?? 0));
    const map = {};
    const fresh = [];
    for (const src of normalizeNodes(data.nodes)) {
      const n = { ...src, id: this.nextId(), x: Math.round(src.x - minX + at.x), y: Math.round(src.y - minY + at.y) };
      map[src.id] = n.id;
      this.g.nodes.push(n);
      this.nodesEl.append(this.renderNode(n));
      fresh.push(n.id);
    }
    for (const w of data.wires ?? []) {
      if (map[w.from?.node] && map[w.to?.node]) {
        this.g.wires.push({ from: { node: map[w.from.node], port: w.from.port }, to: { node: map[w.to.node], port: w.to.port } });
      }
    }
    this.emptyHint.hidden = true;
    this.select(fresh);
    requestAnimationFrame(() => this.updateWires());
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

  // ---- Pins ------------------------------------------------------------------------

  /**
   * Whether a node shows its preview on its face: its own choice, or by default for
   * sources (no inputs: Scale, Progression, Pattern, LFO…), Counterpoint and analysis nodes.
   */
  previewPinned(n, spec = this.specFor(n)) {
    if (!spec || n.type === 'group' || n.type === IN || n.type === OUT) return false;
    if (typeof n.pins?.preview === 'boolean') return n.pins.preview;
    if (n.type === 'counterpoint' || n.type === 'progression' || spec.category === 'Analysis') return true;
    return spec.outputs.length > 0 && spec.inputs.length === 0;
  }

  /** The parameters pinned to a node's face (only automatable ones can be). */
  pinnedParams(n, spec = this.specFor(n)) {
    const names = n.pins?.params ?? [];
    return (spec?.params ?? []).filter((p) => p.automatable && names.includes(p.name));
  }

  togglePreviewPin(n) {
    n.pins = { params: [], ...n.pins, preview: !this.previewPinned(n) };
    this.rerenderNode(n.id);
    this.changed(false);
  }

  /** Pins or unpins a parameter; unpinning also removes the wire automating it. */
  toggleParamPin(n, name) {
    const pins = (n.pins ??= { params: [] });
    pins.params ??= [];
    const i = pins.params.indexOf(name);
    let graph = false;
    if (i >= 0) {
      pins.params.splice(i, 1);
      const before = this.g.wires.length;
      this.g.wires = this.g.wires.filter((w) => !(w.to.node === n.id && w.to.port === PARAM_PORT + name));
      graph = this.g.wires.length !== before;
    } else {
      pins.params.push(name);
    }
    this.rerenderNode(n.id);
    requestAnimationFrame(() => this.updateWires());
    this.changed(graph);
  }

  // ---- Randomness ------------------------------------------------------------------

  /** Whether a node draws random numbers (it has Lock and Seed settings). */
  isRandom(n, spec = this.specFor(n)) {
    const names = (spec?.params ?? []).map((p) => p.name);
    return n.type !== 'group' && names.includes('lock') && names.includes('seed');
  }

  /** A random node's Lock setting, as heard in the section in focus. */
  isLocked(n) {
    const spec = this.specFor(n).params.find((p) => p.name === 'lock');
    return !!this.paramValue(n, this.flat(n.id), 'lock', spec.default).value;
  }

  /**
   * Locks or unlocks a random node. Locking keeps what it's playing: the engine gives the
   * seed that reproduces the current material (what the previews show).
   */
  async toggleLock(n) {
    const flat = this.flat(n.id);
    if (this.isLocked(n)) {
      this.setParam(n, flat, 'lock', false);
    } else {
      const seed = await this.onCaptureSeed?.(flat);
      if (seed != null) this.setParam(n, flat, 'seed', seed);
      this.setParam(n, flat, 'lock', true);
    }
    this.rerenderNode(n.id);
  }

  /** A new seed for a locked random node: something else, which then stays. */
  reroll(n) {
    this.setParam(n, this.flat(n.id), 'seed', randomSeed());
    this.rerenderNode(n.id);
  }

  /** The 🔒/🔓 and 🎲 buttons of a random node. */
  randomButtons(n, cls = 'mini') {
    const locked = this.isLocked(n);
    return [
      el('button', {
        class: `${cls} rand-lock${locked ? ' on' : ''}`,
        title: locked
          ? 'Locked: plays the same every time. Click to make it fresh on every pass.'
          : 'Fresh on every pass. Click to lock it, keeping what it is playing now.',
        onclick: (e) => { e.stopPropagation(); this.toggleLock(n); },
      }, locked ? '🔒' : '🔓'),
      el('button', {
        class: `${cls} rand-roll`,
        disabled: locked ? null : '',
        title: locked ? 'Reroll: a new seed, for something else that then stays' : 'Unlocked nodes already play something new on every pass; lock one to reroll it',
        onclick: (e) => { e.stopPropagation(); this.reroll(n); },
      }, '🎲'),
    ];
  }

  /** The wire automating one of a node's parameters, if any. */
  paramWire(id, name) {
    return this.g.wires.find((w) => w.to.node === id && w.to.port === PARAM_PORT + name) ?? null;
  }

  renameGroup(n) {
    const answer = window.prompt('Group name (also the part name in MIDI exports)', n.title);
    if (answer == null || !answer.trim()) return;
    n.title = answer.trim();
    this.rerenderNode(n.id);
    this.changed(true);
  }

  /** Redraws the inspector (after the selection or a node's structure changes). */
  inspect() {
    this.inspector?.render();
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
    this.inspect();
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
    this.inspect();
    // Ports need layout before wires can find them.
    requestAnimationFrame(() => this.updateWires());
  }

  rerenderNode(id) {
    const n = this.node(id);
    const old = this.nodeEl(id);
    if (!n || !old) return;
    old.replaceWith(this.renderNode(n));
    if (this.report) this.setReport(this.report);
    if (this.selection.has(id)) this.inspect();
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
    const isGroup = n.type === 'group';
    const isIo = n.type === IN || n.type === OUT;
    // A node's face: its ports, pinned parameters and pinned preview. A group shows the
    // controls exposed from inside it.
    const showView = this.previewPinned(n, spec);
    const pinned = isGroup || isIo ? [] : this.pinnedParams(n, spec);
    const classes = ['node'];
    if (showView) classes.push('has-view');
    if (showView && WIDE.has(n.type)) classes.push('wide');
    if (showView && XWIDE.has(n.type)) classes.push('xwide');
    if (pinned.length) classes.push('has-pins');
    if (isGroup) classes.push('group-node');
    if (isIo) classes.push('io-node');
    if (this.selection.has(n.id)) classes.push('selected');
    let right;
    if (isGroup) {
      right = el('span', { class: 'head-buttons' },
        el('button', { class: 'mini', title: 'Open the group (or double-click its header)', onclick: () => this.enter(n.id) }, 'Open'),
        el('button', { class: 'mini', title: 'More', onclick: (e) => this.groupMenu(n, e) }, '⋯'));
    } else if (this.isRandom(n, spec)) {
      right = el('span', { class: 'head-buttons rand' }, ...this.randomButtons(n));
    } else {
      right = el('span', { class: 'cat' }, spec.category);
    }
    const head = el('div', { class: 'node-head', title: spec.help },
      el('span', { class: 'title' }, spec.title),
      el('span', { class: 'issue-mark' }, '!'),
      right);
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
    pinned.length ? el('div', { class: 'node-pins' }, ...pinned.map((p) => this.renderPinnedParam(n, p))) : null,
    isGroup && spec.params.length ? el('div', { class: 'node-params' }, ...spec.params.map((p) => this.renderParam(n, p))) : null,
    showView ? el('div', { class: 'node-view' }) : null,
    isGroup ? el('div', { class: 'group-info' }) : null);
    return node;
  }

  /** A pinned parameter on a node's face, with the input port that can automate it. */
  renderPinnedParam(n, p) {
    const port = PARAM_PORT + p.name;
    const wire = this.paramWire(n.id, p.name);
    const row = el('div', {
      class: 'port in param-port',
      dataset: { port, type: 'control', dir: 'in', wired: wire ? '1' : '' },
      title: wire ? `${p.label} follows ${this.sourceTitle(wire)}` : `${p.label} · wire a control here to automate it`,
    }, el('span', { class: 'dot' }));
    row.append(wire ? this.renderAutomated(p, wire) : this.renderParam(n, p, { face: true }));
    return row;
  }

  /** In place of a parameter's control: where its automation comes from. */
  renderAutomated(p, wire) {
    return el('div', { class: 'param automated' },
      el('span', { class: 'pname' }, p.label),
      el('span', { class: 'auto-src' }, `← ${this.sourceTitle(wire)}`));
  }

  /** The title of the node a wire comes from. */
  sourceTitle(w) {
    const n = this.node(w.from.node);
    return n?.type === 'group' ? n.title : this.specFor(n)?.title ?? w.from.node;
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

  /**
   * A parameter's control. The same parameter can show in several places (the inspector,
   * a node face, a group node); an edit in one updates the others in place. `face`: on a
   * node face, which leaves out the "show on group" toggle.
   */
  renderParam(n, p, { face = false } = {}) {
    const t = p.target ?? { owner: n, name: p.name, flat: this.flat(n.id) };
    const { value, over } = this.paramValue(t.owner, t.flat, t.name, p.default);
    const key = `${t.flat}|${t.name}`;
    let label;
    const set = (v) => {
      this.setParam(t.owner, t.flat, t.name, v);
      if (this.sectionEdit) label?.classList.add('overridden');
      for (const other of document.querySelectorAll(`label.param[data-key="${CSS.escape(key)}"]`)) {
        if (other !== label) other.setValue?.(v);
      }
    };
    let control;
    // Shows a value set elsewhere.
    let update = () => {};
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
          update = (v) => { range.value = v; out.textContent = fmt(v); };
        } else {
          const box = el('input', { type: 'number', min: p.min, max: p.max, step: p.step, value });
          box.addEventListener('change', () => set(Number(box.value)));
          control = box;
          update = (v) => { box.value = v; };
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
        update = (v) => { range.value = toPos(v); out.textContent = fmtLog(v); };
        break;
      }
      case 'text': {
        const box = el('input', { type: 'text', value, spellcheck: 'false' });
        let timer;
        box.addEventListener('input', () => {
          clearTimeout(timer);
          timer = setTimeout(() => set(box.value), 250);
        });
        box.addEventListener('keydown', (e) => { if (e.key === 'Enter') box.blur(); });
        box.addEventListener('change', () => { clearTimeout(timer); set(box.value); });
        control = box;
        update = (v) => { if (document.activeElement !== box) box.value = v; };
        break;
      }
      case 'choice': {
        const select = el('select', {}, ...p.options.map((o) => el('option', { value: o }, o)));
        select.value = value;
        // Other settings may show or hide with this one (a Custom scale's notes).
        const controlsOthers = this.specFor(t.owner)?.params.some((q) => q.when?.param === p.name
          || (p.name === 'root' && q.kind === 'pitchset'));
        select.addEventListener('change', () => {
          set(select.value);
          if (controlsOthers) this.inspector?.render();
        });
        control = select;
        update = (v) => { select.value = v; };
        break;
      }
      case 'bool': {
        const box = el('input', { type: 'checkbox' });
        box.checked = !!value;
        box.addEventListener('change', () => set(box.checked));
        control = box;
        update = (v) => { box.checked = !!v; };
        break;
      }
      case 'pitchset': {
        // One octave of toggles; the value is the chosen note names ("C D Eb G A"). The
        // node's root is always in the set, so its key stays on.
        const pcsOf = (text) => new Set(String(text).split(/[\s,]+/).map(pcOfName).filter((pc) => pc != null));
        let pcs = pcsOf(value);
        const rootSpec = this.specFor(t.owner)?.params.find((q) => q.name === 'root');
        const root = rootSpec ? pcOfName(String(this.paramValue(t.owner, t.flat, 'root', rootSpec.default).value)) : null;
        const keys = PC_LABELS.map((name, pc) => el('button', {
          class: `pc-key${BLACK_PCS.has(pc) ? ' black' : ''}${pc === root ? ' root' : ''}`,
          title: pc === root ? `${name} (the root, always included)` : name,
          disabled: pc === root ? '' : null,
          onclick: (e) => {
            e.preventDefault();
            if (pc === root) return;
            if (pcs.has(pc)) pcs.delete(pc);
            else pcs.add(pc);
            show();
            set([...pcs].sort((a, b) => a - b).map((x) => PC_LABELS[x]).join(' '));
          },
        }, name));
        const show = () => keys.forEach((k, pc) => k.classList.toggle('on', pcs.has(pc) || pc === root));
        show();
        control = el('div', { class: 'pc-set' }, ...keys);
        update = (v) => { pcs = pcsOf(v); show(); };
        break;
      }
      case 'midi_port': {
        const select = el('select', { class: 'midi-port' });
        this.fillPortSelect(select, value);
        select.addEventListener('change', () => set(select.value));
        control = select;
        update = (v) => this.fillPortSelect(select, v);
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
    label = el('label', { class: `param${over ? ' overridden' : ''}`, title: p.label, dataset: { key } }, name, control);
    label.setValue = update;
    if (over) {
      label.title = `${p.label}: set for ${this.section().name} only. Click the dot to use the song value.`;
      name.prepend(el('button', {
        class: 'over-dot', title: `Set for ${this.section().name} only; click to use the song value`,
        onclick: (e) => { e.preventDefault(); e.stopPropagation(); this.clearOverride(t.flat, t.name); },
      }));
    }
    // Inside a group, ◆ shows the parameter on the group node.
    if (!face && this.path.length && n.type !== IN && n.type !== OUT) {
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
    for (const select of document.querySelectorAll('select.midi-port')) this.fillPortSelect(select, select.value);
  }

  /** Shows the engine's previews and issues on the nodes. */
  setReport(report) {
    this.report = report;
    this.probe.cache.clear();
    for (const n of this.allNodes()) {
      const node = this.nodeEl(n.id);
      if (!node) continue;
      const mine = this.issuesFor(n);
      node.classList.toggle('has-issue', mine.length > 0);
      const mark = node.querySelector('.issue-mark');
      if (mark) mark.title = mine.join('\n');
      const info = node.querySelector('.group-info');
      if (info) info.textContent = this.groupInfo(n);
      // Unwired inputs that read the harmonic context.
      const fed = new Set(report.context?.[this.flat(n.id)] ?? []);
      for (const p of node.querySelectorAll('.port.in:not(.param-port)')) {
        const on = fed.has(p.dataset.port);
        if (on === p.classList.contains('from-context')) continue;
        p.classList.toggle('from-context', on);
        p.title = on
          ? `${p.dataset.port} (${p.dataset.type}) · from the Harmonic context (wire something in to use that instead)`
          : `${p.dataset.port} (${p.dataset.type})`;
      }
      const view = node.querySelector('.node-view');
      if (view) this.fillView(view, n, { text: true });
    }
    this.lastTick = null;
    this.inspector?.refresh();
  }

  /** Problems with a node; for a group, those inside it (with the first one named). */
  issuesFor(n) {
    const flat = this.flat(n.id);
    const all = this.report?.issues ?? [];
    if (n.type === 'group') {
      const inside = all.filter((i) => i.node.startsWith(`${flat}/`)).map((i) => i.message);
      return inside.length ? [`${inside.length} problem${inside.length === 1 ? '' : 's'} inside: ${inside[0]}`] : [];
    }
    return all.filter((i) => i.node === flat).map((i) => i.message);
  }

  /** "12 nodes inside · 3 set for B" */
  groupInfo(n) {
    const count = countNodes(n.graph);
    const over = this.overridesUnder(this.flat(n.id));
    return `${count} node${count === 1 ? '' : 's'} inside${over ? ` · ${over} set for ${this.section().name}` : ''}`;
  }

  /**
   * Draws a node's preview into `container`: its view, with ▶ for what can be heard.
   * With `text`, a node with nothing to draw shows its text preview instead.
   */
  fillView(container, n, { text = false } = {}) {
    const flat = this.flat(n.id);
    const data = this.report?.views?.[flat];
    const preview = this.report?.previews?.[flat] ?? '';
    const parts = data ? this.renderView(flat, data) : text && preview ? [el('pre', { class: 'view-text' }, preview)] : [];
    container.replaceChildren(...parts);
    container.title = data && data.kind !== 'roll' ? preview : '';
    if (data?.kind === 'keys' || data?.kind === 'steps') {
      container.append(el('button', {
        class: 'audition', title: 'Hear it (or click the output\'s name)',
        onclick: (e) => { e.stopPropagation(); this.audition(n.id, null); },
      }, '▶'));
    }
  }

  renderView(flat, data) {
    if (data.kind === 'keys') return [renderKeys(data)];
    if (data.kind === 'steps') return [renderSteps(data)];
    if (data.kind === 'curve') return [renderCurve(data)];
    if (data.kind !== 'roll') return renderTheory(data);
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
    showPlayhead(this.nodesEl, tick);
    showPlayhead(this.probe.el, tick);
    if (this.inspector) showPlayhead(this.inspector.root, tick);
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
    const view = renderPlain(result.view);
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
    if (dir === 'in' && port.startsWith(PARAM_PORT)) return 'control';
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
    // A pinned parameter shows its control, or where its automation comes from, so redraw
    // nodes whose parameter wiring changed.
    const stale = new Set();
    for (const p of this.nodesEl.querySelectorAll('.param-port')) {
      const id = p.closest('.node').dataset.id;
      if (!!p.dataset.wired !== !!this.paramWire(id, p.dataset.port.slice(PARAM_PORT.length))) stale.add(id);
    }
    if (stale.size && !this.drag) {
      for (const id of stale) this.rerenderNode(id);
      requestAnimationFrame(() => this.updateWires());
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
    this.inspect();
    this.onSelect?.([...this.selection].filter((id) => id !== IN && id !== OUT).map((id) => this.flat(id)));
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
    // Copy, cut and paste nodes (the clipboard events work without clipboard permissions).
    const onCanvas = (e) => !isTyping(e) && (document.activeElement === c || c.contains(document.activeElement));
    document.addEventListener('copy', (e) => { if (onCanvas(e)) this.copy(e, false); });
    document.addEventListener('cut', (e) => { if (onCanvas(e)) this.copy(e, true); });
    document.addEventListener('paste', (e) => { if (onCanvas(e)) this.paste(e); });
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
    this.pointer = { x: e.clientX, y: e.clientY };
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
    const w = 440;
    const h = 480;
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
    if (pending.dir === 'out' && pending.type === 'control') {
      // A control goes to the first parameter it can automate (pinning it).
      const p = (spec.params ?? []).find((x) => x.automatable);
      return p ? { name: PARAM_PORT + p.name, type: 'control', pin: p.name } : null;
    }
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
    if (port?.pin) {
      n.pins = { params: [port.pin] };
      this.rerenderNode(n.id);
    }
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

/** A seed for a random node (the range of its Seed parameter). */
function randomSeed() {
  return Math.floor(Math.random() * 10000);
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

