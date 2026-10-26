// Calls into the engine. In the desktop app this goes through Tauri; in a plain
// browser (the dev server) it goes over HTTP to the same command handler.

const tauri = window.__TAURI__ && window.__TAURI__.core;

export const inApp = !!tauri;

async function post(path, body) {
  const res = await fetch(path, { method: 'POST', body: JSON.stringify(body ?? null) });
  const json = await res.json();
  if (!res.ok) throw new Error(json);
  return json;
}

/** Runs an engine command (see pm_engine::api::handle for the list). */
export async function call(command, args = null) {
  if (tauri) {
    const text = await tauri.invoke('engine', { command, args: JSON.stringify(args) });
    return JSON.parse(text);
  }
  return post(`/api/${command}`, args);
}

export async function midiPorts() {
  if (tauri) return tauri.invoke('midi_ports');
  return post('/api/midi_ports');
}

/** MIDI input ports, for following an external clock (none outside the app). */
export async function midiInPorts() {
  if (tauri) return tauri.invoke('midi_in_ports');
  return [];
}

/** Follows MIDI clock from an input port, or stops with `null`. */
export async function setClockIn(port) {
  if (tauri) return tauri.invoke('set_clock_in', { port: port || null });
  if (port) throw new Error('MIDI input needs the desktop app');
  return null;
}

/** Saves text to `path`, or asks where if `path` is null. Resolves to the path, or null if cancelled. */
export async function saveProject(contents, path) {
  if (tauri) return tauri.invoke('save_project', { contents, path: path ?? null });
  // Browser fallback: download a file.
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([contents], { type: 'application/json' }));
  a.download = 'untitled.pmpatch';
  a.click();
  return 'untitled.pmpatch';
}

/** Renders the song (or `bars` bars without one) through the built-in instruments and asks where to save the WAV. */
export async function exportWav(bars) {
  if (tauri) return tauri.invoke('export_wav', { bars });
  return post('/api/export_wav', { bars });
}

/** Writes the song (or `bars` bars) as a standard MIDI file, one track per part. */
export async function exportMidi(bars) {
  if (tauri) return tauri.invoke('export_midi', { bars });
  return post('/api/export_midi', { bars });
}

// The group library: saved groups, shared between projects. In the app they are files in
// the app's data folder; in a browser they live in local storage.
const LIBRARY_KEY = 'pm.library';

function browserLibrary() {
  try {
    return JSON.parse(localStorage.getItem(LIBRARY_KEY) ?? '{}');
  } catch {
    return {};
  }
}

/** Saved groups as [name, contents] pairs. */
export async function libraryList() {
  if (tauri) return tauri.invoke('library_list');
  return Object.entries(browserLibrary()).sort((a, b) => a[0].localeCompare(b[0]));
}

export async function librarySave(name, contents) {
  if (tauri) return tauri.invoke('library_save', { name, contents });
  const lib = browserLibrary();
  lib[name] = contents;
  localStorage.setItem(LIBRARY_KEY, JSON.stringify(lib));
  return null;
}

export async function libraryDelete(name) {
  if (tauri) return tauri.invoke('library_delete', { name });
  const lib = browserLibrary();
  delete lib[name];
  localStorage.setItem(LIBRARY_KEY, JSON.stringify(lib));
  return null;
}

/** Asks for a file. Resolves to [path, contents], or null if cancelled. */
export async function openProject() {
  if (tauri) return tauri.invoke('open_project');
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.pmpatch,.json';
    input.onchange = async () => {
      const f = input.files[0];
      resolve(f ? [f.name, await f.text()] : null);
    };
    input.click();
  });
}
