import * as api from './api.js';
import { Editor, isTyping, el } from './editor.js';
import { EXAMPLES } from './examples.js';
import { songTemplate } from './templates.js';
import { Timeline } from './timeline.js';

const $ = (id) => document.getElementById(id);
const AUTOSAVE_KEY = 'pm.autosave';
const MODE_KEY = 'pm.mode';

let editor;
let timeline;
let filePath = null;
let dirty = false;
let playing = false;
let pushTimer = null;
let songTimer = null;
let mode = 'compose';
let status = null;

function setStatus(text, ok = false) {
  const s = $('status-msg');
  s.textContent = text ?? '';
  s.classList.toggle('ok', ok);
}

function baseName(path) {
  return path ? path.split(/[\\/]/).pop() : 'Untitled';
}

function updateTitle() {
  const name = baseName(filePath) + (dirty ? ' •' : '');
  $('file-name').textContent = name;
  document.title = `${name} — Procedural Music`;
}

function autosave() {
  try {
    localStorage.setItem(AUTOSAVE_KEY, JSON.stringify({ path: filePath, dirty, doc: editor.doc }));
  } catch {
    /* storage unavailable: nothing to do */
  }
}

// ---- Undo history ---------------------------------------------------------------
// Snapshots of the whole project. A burst of edits (dragging a slider) is one step.

const history = { undo: [], redo: [], current: null, last: 0 };

function resetHistory() {
  history.undo = [];
  history.redo = [];
  history.current = JSON.stringify(editor.doc);
  updateUndoButtons();
}

function recordHistory() {
  const now = performance.now();
  const snapshot = JSON.stringify(editor.doc);
  if (snapshot === history.current) return;
  if (now - history.last > 600 || !history.undo.length) {
    history.undo.push(history.current);
    if (history.undo.length > 200) history.undo.shift();
  }
  history.last = now;
  history.current = snapshot;
  history.redo = [];
  updateUndoButtons();
}

function undoRedo(redo) {
  const from = redo ? history.redo : history.undo;
  const to = redo ? history.undo : history.redo;
  if (!from.length) return;
  to.push(history.current);
  history.current = from.pop();
  history.last = 0;
  editor.restore(JSON.parse(history.current));
  timeline.select(editor.focus);
  dirty = true;
  updateTitle();
  autosave();
  schedulePush();
  updateUndoButtons();
  setStatus(redo ? 'Redone' : 'Undone', true);
}

function updateUndoButtons() {
  $('btn-undo').disabled = !history.undo.length;
  $('btn-redo').disabled = !history.redo.length;
}

// ---- Engine sync ---------------------------------------------------------------

function onChange({ graph }) {
  dirty = true;
  updateTitle();
  autosave();
  recordHistory();
  if (graph) schedulePush();
  else timeline.render();
}

function schedulePush() {
  clearTimeout(pushTimer);
  pushTimer = setTimeout(pushGraph, 40);
}

async function pushGraph() {
  pushTimer = null;
  try {
    const report = await api.call('set_graph', { doc: editor.doc, focus: editor.focus, quantize: mode === 'perform' });
    editor.setReport(report);
    timeline.render();
    buildCueBar();
    scheduleSong();
  } catch (e) {
    setStatus(`Engine error: ${e.message ?? e}`);
  }
}

/** Refreshes the timeline's overview of every part (a little after edits settle). */
function scheduleSong() {
  clearTimeout(songTimer);
  songTimer = setTimeout(async () => {
    try {
      timeline.setSong(await api.call('song', { bars: 8 }));
    } catch {
      /* the overview is optional */
    }
  }, 300);
}

async function setFocus(index) {
  editor.setFocus(index);
  // Selecting a section also puts the play cursor at its start.
  if (index != null && !playing) {
    const start = editor.doc.arrangement.sections.slice(0, index).reduce((t, s) => t + s.bars, 0) * timeline.bar;
    api.call('set_cursor', { tick: start }).catch(() => {});
  }
  try {
    editor.setReport(await api.call('set_focus', { focus: index }));
  } catch {
    /* ignore */
  }
}

async function load(doc, path = null, isDirty = false) {
  editor.load(doc);
  filePath = path;
  dirty = isDirty;
  updateTitle();
  const bpm = doc.bpm ?? 110;
  $('bpm').value = bpm;
  await api.call('set_bpm', { bpm });
  $('clock-port').value = doc.clockPort ?? '';
  await api.call('set_clock_ports', { ports: doc.clockPort ? [doc.clockPort] : [] });
  timeline.focus = null;
  timeline.setSectionEdit(false);
  timeline.setLoop(mode === 'perform' ? 'section' : 'song');
  await api.call('set_cursor', { tick: 0 });
  await pushGraph();
  resetHistory();
  autosave();
}

function confirmDiscard() {
  return !dirty || window.confirm('Discard unsaved changes to this patch?');
}

async function refreshPorts() {
  try {
    const ports = await api.midiPorts();
    editor.setMidiPorts(ports);
    const clock = $('clock-port');
    const current = editor.doc.clockPort ?? '';
    clock.replaceChildren(new Option('Off', ''), ...ports.map((p) => new Option(p, p)));
    clock.value = ports.includes(current) ? current : '';
    setStatus(ports.length ? `MIDI ports: ${ports.join(', ')}` : 'No MIDI output ports found', ports.length > 0);
  } catch (e) {
    setStatus(`Couldn't list MIDI ports: ${e.message ?? e}`);
  }
}

async function refreshLibrary() {
  try {
    const entries = await api.libraryList();
    editor.library = entries.flatMap(([name, text]) => {
      try {
        return [{ name, group: JSON.parse(text) }];
      } catch {
        return [];
      }
    });
  } catch (e) {
    setStatus(`Couldn't read the group library: ${e.message ?? e}`);
  }
}

async function saveToLibrary(n) {
  const name = window.prompt('Save this group to the library as:', n.title);
  if (!name?.trim()) return;
  try {
    const entry = { ...structuredClone(n), title: n.title };
    delete entry.x;
    delete entry.y;
    await api.librarySave(name.trim(), JSON.stringify(entry, null, 2));
    await refreshLibrary();
    setStatus(`Saved "${name.trim()}" to the library: add it from the node menu (Library)`, true);
  } catch (e) {
    setStatus(`Couldn't save to the library: ${e.message ?? e}`);
  }
}

// ---- Transport ---------------------------------------------------------------

async function togglePlay() {
  if (playing) {
    await api.call('stop');
  } else {
    clearTimeout(pushTimer);
    await pushGraph();
    await api.call('play');
  }
  await pollStatus();
}

/** Plays a node's output once through the preview instrument; resolves to its length in seconds. */
async function audition(node, port) {
  try {
    // Make sure the engine has the latest edits first.
    if (pushTimer) {
      clearTimeout(pushTimer);
      await pushGraph();
    }
    const { seconds } = await api.call('audition', { node, port });
    return seconds;
  } catch (e) {
    setStatus(`Can't play that: ${e.message ?? e}`);
    return null;
  }
}

/** What is on one of a node's outputs: { text, view } or { error }. */
async function probe(node, port) {
  try {
    return await api.call('probe', { node, port });
  } catch (e) {
    return { error: String(e.message ?? e) };
  }
}

async function seek(tick) {
  const wasPlaying = playing;
  if (wasPlaying) await api.call('stop');
  await api.call('set_cursor', { tick });
  if (wasPlaying) await api.call('play');
  await pollStatus();
}

async function setLoop(m) {
  await api.call('set_loop', { mode: m, section: editor.focus ?? 0 });
}

function showPlaying(isPlaying) {
  playing = isPlaying;
  const b = $('btn-play');
  b.classList.toggle('playing', playing);
  b.querySelector('.icon').textContent = playing ? '■' : '▶';
  b.querySelector('.label').textContent = playing ? 'Stop' : 'Play';
}

let polling = false;
let solveGeneration = null;
async function pollStatus() {
  if (polling) return;
  polling = true;
  try {
    const s = await api.call('status');
    status = s;
    showPlaying(s.playing);
    const sections = editor.doc.arrangement.sections;
    const sec = s.section != null ? sections[s.section] : null;
    $('position').textContent = `${s.bar}.${s.beat}`;
    $('section-name').textContent = sec ? sec.name : '';
    $('section-name').hidden = !sec;
    $('position').title = s.tempo !== s.bpm ? `Tempo now ${s.tempo} bpm` : '';
    // Rolls show the section in focus; their playhead moves only while it plays.
    const showHead = s.playing && (editor.focus == null || editor.focus === s.section);
    editor.setPlayhead(showHead ? s.tick : null);
    timeline.setStatus(s);
    updateCueBar(s);
    if (s.solveGeneration !== undefined && s.solveGeneration !== solveGeneration) {
      // A background counterpoint solve finished: refresh the piano rolls.
      const first = solveGeneration === null;
      solveGeneration = s.solveGeneration;
      if (!first) {
        editor.setReport(await api.call('report'));
        scheduleSong();
      }
    }
    if (s.midiError) setStatus(s.midiError);
    const audio = $('audio-status');
    if (s.audioError) {
      audio.textContent = `Audio: ${s.audioError}`;
      audio.classList.add('error');
    } else if (s.audio) {
      audio.textContent = `Audio: ${s.audio}`;
      audio.classList.remove('error');
    }
  } catch {
    /* ignore a missed poll */
  } finally {
    polling = false;
  }
}

// ---- Compose and Perform -------------------------------------------------------

async function setMode(m) {
  mode = m;
  document.body.classList.toggle('perform', m === 'perform');
  for (const b of document.querySelectorAll('#mode button')) b.classList.toggle('on', b.dataset.mode === m);
  try {
    localStorage.setItem(MODE_KEY, m);
  } catch {
    /* ignore */
  }
  if (m === 'perform') {
    // Perform loops one section at a time; cue the next one with the buttons or 1-9.
    const start = editor.focus ?? (status?.section ?? 0);
    if (editor.doc.arrangement.sections.length) await api.call('cue', { section: start });
  } else {
    await setLoop(timeline.loop);
  }
  buildCueBar();
  await pushGraph();
  requestAnimationFrame(() => editor.updateWires());
}

function buildCueBar() {
  const bar = $('cue-bar');
  const sections = editor.doc.arrangement.sections;
  if (!sections.length) {
    bar.replaceChildren(el('span', { class: 'cue-empty' }, 'Add sections in Compose mode to cue them here. Edits wait for the next bar line while performing.'));
    return;
  }
  bar.replaceChildren(el('span', { class: 'cue-label' }, 'Cue'), ...sections.map((s, i) => el('button', {
    class: 'cue',
    dataset: { i },
    title: `${s.name}: ${s.bars} bars${i < 9 ? ` (key ${i + 1})` : ''}. Plays from the next bar line and loops.`,
    onclick: () => cue(i),
  }, el('small', {}, i < 9 ? String(i + 1) : ''), s.name)));
  if (status) updateCueBar(status);
}

function updateCueBar(s) {
  for (const b of document.querySelectorAll('#cue-bar .cue')) {
    const i = Number(b.dataset.i);
    b.classList.toggle('current', s.section === i && (s.playing || s.loop === i));
    b.classList.toggle('cued', s.cued === i);
  }
}

async function cue(i) {
  await api.call('cue', { section: i });
  await pollStatus();
}

// ---- Files and export -----------------------------------------------------------

async function save(saveAs) {
  try {
    const contents = JSON.stringify(editor.doc, null, 2);
    const path = await api.saveProject(contents, saveAs || !api.inApp ? null : filePath);
    if (!path) return;
    filePath = path;
    dirty = false;
    updateTitle();
    autosave();
    setStatus(`Saved ${baseName(path)}`, true);
  } catch (e) {
    setStatus(String(e));
  }
}

/** Bars to render: the whole arrangement, or (without one) ask. */
function exportBars() {
  if (editor.doc.arrangement.sections.length) return 8;
  const answer = window.prompt('No arrangement yet. How many bars to render?', '8');
  if (answer === null) return null;
  return Math.max(1, Math.min(256, Math.round(Number(answer)) || 8));
}

async function exportFile(kind) {
  const bars = exportBars();
  if (bars === null) return;
  const what = editor.doc.arrangement.sections.length ? 'the song' : `${bars} bars`;
  try {
    setStatus(`Rendering ${what}…`, true);
    clearTimeout(pushTimer);
    await pushGraph();
    const path = kind === 'midi' ? await api.exportMidi(bars) : await api.exportWav(bars);
    setStatus(path ? `Exported ${baseName(path)}` : 'Export cancelled', true);
  } catch (e) {
    setStatus(`Export failed: ${e.message ?? e}`);
  }
}

async function open() {
  if (!confirmDiscard()) return;
  try {
    const result = await api.openProject();
    if (!result) return;
    const [path, contents] = result;
    await load(JSON.parse(contents), path);
    setStatus(`Opened ${baseName(path)}`, true);
  } catch (e) {
    setStatus(`Couldn't open: ${e.message ?? e}`);
  }
}

/** A toolbar button that opens a dropdown of [title, description, action]. */
function dropdown(buttonId, menuId, items) {
  const menu = $(menuId);
  menu.replaceChildren(...items.map(([title, description, action]) => {
    const b = el('button', {}, title, el('small', {}, description));
    b.addEventListener('click', () => {
      menu.hidden = true;
      action();
    });
    return b;
  }));
  $(buttonId).addEventListener('click', (e) => {
    e.stopPropagation();
    for (const m of document.querySelectorAll('.dropdown')) if (m !== menu) m.hidden = true;
    menu.hidden = !menu.hidden;
  });
}

async function newProject(template) {
  if (!confirmDiscard()) return;
  const bpm = Number($('bpm').value) || 110;
  await load(template ? songTemplate(bpm) : { version: 2, bpm, nodes: [], wires: [] });
  setStatus(template ? 'New song from the template: four parts, each a group you can open' : 'New empty patch', true);
}

async function init() {
  const specs = await api.call('specs');
  editor = new Editor({
    canvas: $('canvas'),
    world: $('world'),
    nodesEl: $('nodes'),
    wiresEl: $('wires'),
    crumbsEl: $('crumbs'),
    addMenu: $('add-menu'),
    addSearch: $('add-search'),
    addList: $('add-list'),
    emptyHint: $('empty-hint'),
    specs,
    onChange,
    onLibrarySave: saveToLibrary,
    onAudition: audition,
    onProbe: probe,
  });
  timeline = new Timeline({
    root: $('timeline'),
    getDoc: () => editor.doc,
    onChange: () => onChange({ graph: true }),
    onFocus: (i) => setFocus(i),
    onSeek: seek,
    onLoop: setLoop,
    onSectionEdit: (on) => editor.setSectionEdit(on),
  });

  dropdown('btn-examples', 'examples-menu', EXAMPLES.map((ex) => [ex.name, ex.description, async () => {
    if (!confirmDiscard()) return;
    await load(structuredClone(ex.doc));
    setStatus(`Loaded example: ${ex.name}`, true);
  }]));
  dropdown('btn-new', 'new-menu', [
    ['Song', 'Drums, bass, pads and lead as groups, with an arrangement', () => newProject(true)],
    ['Empty patch', 'A blank canvas', () => newProject(false)],
  ]);
  dropdown('btn-export', 'export-menu', [
    ['WAV audio…', 'The built-in instruments, rendered faster than real time', () => exportFile('wav')],
    ['MIDI file…', 'One track per part, with tempo changes', () => exportFile('midi')],
  ]);
  document.addEventListener('click', () => {
    for (const m of document.querySelectorAll('.dropdown')) m.hidden = true;
  });

  await refreshPorts();
  await refreshLibrary();

  let saved = null;
  try {
    saved = JSON.parse(localStorage.getItem(AUTOSAVE_KEY) ?? 'null');
  } catch {
    saved = null;
  }
  if (saved?.doc) await load(saved.doc, saved.path ?? null, !!saved.dirty);
  else await load(structuredClone(EXAMPLES.find((e) => e.name === 'Full song')?.doc ?? EXAMPLES[1].doc));

  $('btn-play').addEventListener('click', togglePlay);
  $('btn-open').addEventListener('click', open);
  $('btn-save').addEventListener('click', (e) => save(e.shiftKey));
  $('btn-ports').addEventListener('click', refreshPorts);
  $('btn-undo').addEventListener('click', () => undoRedo(false));
  $('btn-redo').addEventListener('click', () => undoRedo(true));
  for (const b of document.querySelectorAll('#mode button')) b.addEventListener('click', () => setMode(b.dataset.mode));
  $('bpm').addEventListener('change', async () => {
    const bpm = Math.min(400, Math.max(20, Number($('bpm').value) || 110));
    $('bpm').value = bpm;
    editor.doc.bpm = bpm;
    onChange({ graph: false });
    await api.call('set_bpm', { bpm });
  });
  $('clock-port').addEventListener('change', async () => {
    const port = $('clock-port').value;
    editor.doc.clockPort = port || undefined;
    onChange({ graph: false });
    await api.call('set_clock_ports', { ports: port ? [port] : [] });
  });

  document.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (mod && key === 's') {
      e.preventDefault();
      save(e.shiftKey);
    } else if (mod && key === 'o') {
      e.preventDefault();
      open();
    } else if (mod && (key === 'z' || key === 'y') && !isTyping(e)) {
      e.preventDefault();
      undoRedo(key === 'y' || e.shiftKey);
    } else if (e.key === ' ' && !isTyping(e) && $('add-menu').hidden) {
      e.preventDefault();
      togglePlay();
    } else if (mode === 'perform' && !mod && !isTyping(e) && /^[1-9]$/.test(e.key)) {
      const i = Number(e.key) - 1;
      if (i < editor.doc.arrangement.sections.length) cue(i);
    }
  });

  let savedMode = 'compose';
  try {
    savedMode = localStorage.getItem(MODE_KEY) ?? 'compose';
  } catch {
    /* ignore */
  }
  await setMode(savedMode === 'perform' ? 'perform' : 'compose');
  setInterval(pollStatus, 60);
  $('canvas').focus();
}

init().catch((e) => {
  setStatus(`Startup failed: ${e.message ?? e}`);
  console.error(e);
});
