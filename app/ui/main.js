import * as api from './api.js';
import { Editor, isTyping } from './editor.js';
import { EXAMPLES } from './examples.js';

const $ = (id) => document.getElementById(id);
const AUTOSAVE_KEY = 'pm.autosave';

let editor;
let filePath = null;
let dirty = false;
let playing = false;
let pushTimer = null;

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

function onChange({ graph }) {
  dirty = true;
  updateTitle();
  autosave();
  if (graph) {
    clearTimeout(pushTimer);
    pushTimer = setTimeout(pushGraph, 40);
  }
}

async function pushGraph() {
  try {
    const report = await api.call('set_graph', { doc: editor.doc });
    editor.setReport(report);
  } catch (e) {
    setStatus(`Engine error: ${e.message ?? e}`);
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
  await pushGraph();
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

function showPlaying(isPlaying) {
  playing = isPlaying;
  const b = $('btn-play');
  b.classList.toggle('playing', playing);
  b.querySelector('.icon').textContent = playing ? '■' : '▶';
  b.querySelector('.label').textContent = playing ? 'Stop' : 'Play';
}

let polling = false;
async function pollStatus() {
  if (polling) return;
  polling = true;
  try {
    const s = await api.call('status');
    showPlaying(s.playing);
    $('position').textContent = `${s.bar}.${s.beat}`;
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

async function exportWav() {
  const answer = window.prompt('How many bars to render?', '8');
  if (answer === null) return;
  const bars = Math.max(1, Math.min(256, Math.round(Number(answer)) || 8));
  try {
    setStatus(`Rendering ${bars} bars…`, true);
    clearTimeout(pushTimer);
    await pushGraph();
    const path = await api.exportWav(bars);
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

function buildExamplesMenu() {
  const menu = $('examples-menu');
  for (const ex of EXAMPLES) {
    const b = document.createElement('button');
    b.innerHTML = `${ex.name}<small></small>`;
    b.querySelector('small').textContent = ex.description;
    b.addEventListener('click', async () => {
      menu.hidden = true;
      if (!confirmDiscard()) return;
      await load(structuredClone(ex.doc));
      setStatus(`Loaded example: ${ex.name}`, true);
    });
    menu.append(b);
  }
  $('btn-examples').addEventListener('click', (e) => {
    e.stopPropagation();
    menu.hidden = !menu.hidden;
  });
  document.addEventListener('click', () => { menu.hidden = true; });
}

async function init() {
  const specs = await api.call('specs');
  editor = new Editor({
    canvas: $('canvas'),
    world: $('world'),
    nodesEl: $('nodes'),
    wiresEl: $('wires'),
    addMenu: $('add-menu'),
    addSearch: $('add-search'),
    addList: $('add-list'),
    emptyHint: $('empty-hint'),
    specs,
    onChange,
  });
  buildExamplesMenu();
  await refreshPorts();

  let saved = null;
  try {
    saved = JSON.parse(localStorage.getItem(AUTOSAVE_KEY) ?? 'null');
  } catch {
    saved = null;
  }
  if (saved?.doc) await load(saved.doc, saved.path ?? null, !!saved.dirty);
  else await load(structuredClone(EXAMPLES[1].doc));

  $('btn-play').addEventListener('click', togglePlay);
  $('btn-new').addEventListener('click', async () => {
    if (confirmDiscard()) await load({ version: 1, bpm: Number($('bpm').value) || 110, nodes: [], wires: [] });
  });
  $('btn-open').addEventListener('click', open);
  $('btn-save').addEventListener('click', (e) => save(e.shiftKey));
  $('btn-ports').addEventListener('click', refreshPorts);
  $('btn-export').addEventListener('click', exportWav);
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
    if (mod && e.key.toLowerCase() === 's') {
      e.preventDefault();
      save(e.shiftKey);
    } else if (mod && e.key.toLowerCase() === 'o') {
      e.preventDefault();
      open();
    } else if (e.key === ' ' && !isTyping(e) && $('add-menu').hidden) {
      e.preventDefault();
      togglePlay();
    }
  });

  setInterval(pollStatus, 60);
  $('canvas').focus();
}

init().catch((e) => {
  setStatus(`Startup failed: ${e.message ?? e}`);
  console.error(e);
});
