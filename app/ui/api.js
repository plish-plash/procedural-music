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

/** Runs an engine command (specs, set_graph, play, stop, set_bpm, set_clock_ports, status). */
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
