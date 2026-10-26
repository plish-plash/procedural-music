// The inspector: everything about the selected node, in a panel beside the canvas. Its
// preview and its automatable parameters can be pinned (●), which also shows them on the
// node; a pinned parameter gets an input that a Control (an LFO, a Ramp) can drive.

import { el } from './dom.js';

const COLLAPSED_KEY = 'pm.inspector.collapsed';

export class Inspector {
  /**
   * @param {object} o
   * @param {HTMLElement} o.root
   * @param {import('./editor.js').Editor} o.editor
   * @param {(n: object) => void} o.onLibrarySave
   */
  constructor({ root, editor, onLibrarySave }) {
    Object.assign(this, { root, editor, onLibrarySave });
    this.body = el('div', { class: 'insp-body' });
    this.toggleBtn = el('button', { class: 'mini insp-toggle', onclick: () => this.setCollapsed(!this.collapsed, true) });
    root.replaceChildren(el('div', { class: 'insp-bar' }, el('span', { class: 'insp-name' }, 'Inspector'), this.toggleBtn), this.body);
    this.nodeId = null;
    this.setCollapsed(this.savedCollapsed(), false);
  }

  savedCollapsed() {
    try {
      return localStorage.getItem(COLLAPSED_KEY) === '1';
    } catch {
      return false;
    }
  }

  /** Hides or shows the panel; `remember` keeps the choice for next time. */
  setCollapsed(on, remember) {
    this.collapsed = on;
    this.root.classList.toggle('collapsed', on);
    this.toggleBtn.textContent = on ? '⟨' : '⟩';
    this.toggleBtn.title = on ? 'Show the inspector (I)' : 'Hide the inspector (I)';
    if (remember) {
      try {
        localStorage.setItem(COLLAPSED_KEY, on ? '1' : '0');
      } catch {
        /* storage unavailable */
      }
    }
    if (!on) this.render();
  }

  /** Builds the panel for the current selection. */
  render() {
    if (this.collapsed) return;
    const ed = this.editor;
    const ids = [...ed.selection];
    const n = ids.length === 1 ? ed.node(ids[0]) : null;
    this.nodeId = n?.id ?? null;
    this.viewEl = null;
    this.textEl = null;
    this.infoEl = null;
    if (!n) {
      this.body.replaceChildren(this.empty(ids.length));
      return;
    }
    const spec = ed.specFor(n);
    if (!spec) {
      this.body.replaceChildren(el('h2', {}, n.type), el('p', { class: 'insp-problem' }, `Unknown node type "${n.type}"`));
      return;
    }
    const isGroup = n.type === 'group';
    const isIo = spec.kind === '@in' || spec.kind === '@out';
    const parts = [
      el('div', { class: 'insp-head', style: `--cat: var(--c-${spec.category.toLowerCase()})` },
        el('h2', {}, spec.title), el('span', { class: 'insp-cat' }, spec.category)),
      el('p', { class: 'insp-help' }, spec.help),
    ];
    const section = ed.section();
    if (ed.sectionEdit && section) {
      parts.push(el('div', { class: 'insp-banner' }, `Changes apply to ${section.name} only`));
    }
    this.problemsEl = el('div', { class: 'insp-problems' });
    parts.push(this.problemsEl);
    if (isGroup) parts.push(...this.groupParts(n, spec));
    else if (!isIo) parts.push(...this.nodeParts(n, spec));
    this.body.replaceChildren(...parts);
    this.refresh();
  }

  /** The preview (with its pin) and every parameter (pinnable ones with theirs). */
  nodeParts(n, spec) {
    const ed = this.editor;
    const parts = [];
    if (spec.outputs.length || ed.report?.views?.[ed.flat(n.id)]) {
      const pinned = ed.previewPinned(n, spec);
      this.viewEl = el('div', { class: 'insp-view node-view' });
      this.textEl = el('pre', { class: 'insp-text' });
      parts.push(el('section', {},
        el('div', { class: 'insp-title' }, 'Preview',
          this.pinButton(pinned, 'the preview', () => ed.togglePreviewPin(n))),
        this.viewEl, this.textEl));
    }
    if (ed.isRandom(n, spec)) {
      // Lock, seed and reroll in one row (instead of the Lock and Seed settings).
      const seed = spec.params.find((p) => p.name === 'seed');
      const [lock, roll] = ed.randomButtons(n, 'rand-btn');
      lock.textContent = ed.isLocked(n) ? '🔒 Locked' : '🔓 Fresh';
      parts.push(el('section', {},
        el('div', { class: 'insp-title' }, 'Randomness'),
        el('div', { class: 'insp-random' }, lock, ed.renderParam(n, seed), roll),
        el('p', { class: 'insp-hint' }, ed.isLocked(n)
          ? 'Plays the same on every playback. 🎲 picks another seed.'
          : 'Plays something new on every pass. Lock it to keep what it is playing now.')));
    }
    const shown = (p) => {
      if (!p.when) return true;
      const other = spec.params.find((q) => q.name === p.when.param);
      return p.when.values.includes(ed.paramValue(n, ed.flat(n.id), p.when.param, other?.default).value);
    };
    const settings = spec.params.filter((p) => shown(p) && !(ed.isRandom(n, spec) && (p.name === 'lock' || p.name === 'seed')));
    if (settings.length) {
      const pinned = new Set(ed.pinnedParams(n, spec).map((p) => p.name));
      const rows = settings.map((p) => {
        const wire = p.automatable ? ed.paramWire(n.id, p.name) : null;
        return el('div', { class: 'insp-param' },
          p.automatable
            ? this.pinButton(pinned.has(p.name), p.label, () => ed.toggleParamPin(n, p.name))
            : el('span', { class: 'pin-spacer' }),
          wire ? ed.renderAutomated(p, wire) : ed.renderParam(n, p));
      });
      parts.push(el('section', {}, el('div', { class: 'insp-title' }, 'Settings'), ...rows));
    }
    return parts;
  }

  groupParts(n, spec) {
    const ed = this.editor;
    this.infoEl = el('p', { class: 'insp-info' });
    const parts = [
      el('div', { class: 'insp-actions' },
        el('button', { onclick: () => ed.enter(n.id) }, 'Open'),
        el('button', { onclick: () => ed.renameGroup(n) }, 'Rename…'),
        el('button', { onclick: () => { ed.select([n.id]); ed.ungroupSelection(); } }, 'Ungroup'),
        el('button', { onclick: () => this.onLibrarySave?.(n) }, 'Save to library…')),
      this.infoEl,
    ];
    if (spec.params.length) {
      parts.push(el('section', {},
        el('div', { class: 'insp-title' }, 'Controls shown on the group'),
        ...spec.params.map((p) => ed.renderParam(n, p))));
    } else {
      parts.push(el('p', { class: 'insp-hint' }, 'Open the group and click ◆ next to a setting to show it here and on the group node.'));
    }
    return parts;
  }

  pinButton(on, what, toggle) {
    return el('button', {
      class: `pin-btn${on ? ' on' : ''}`,
      title: on ? `Pinned: ${what} shows on the node. Click to unpin.` : `Pin ${what} to the node`,
      onclick: (e) => { e.preventDefault(); toggle(); },
    }, '●');
  }

  empty(count) {
    if (count > 1) {
      return el('div', { class: 'insp-empty' },
        el('h2', {}, `${count} nodes selected`),
        el('p', {}, 'Ctrl+G groups them, Ctrl+D duplicates them, Del removes them.'));
    }
    return el('div', { class: 'insp-empty' },
      el('p', {}, 'Select a node to see its settings and preview here.'),
      el('p', {}, 'Pin (●) the preview or a setting to show it on the node. A pinned setting gets an input that a Control node (LFO, Ramp, Steps) can drive.'));
  }

  /** Updates what the engine reports (preview, text, problems) without rebuilding controls. */
  refresh() {
    if (this.collapsed || !this.nodeId) return;
    const ed = this.editor;
    const n = ed.node(this.nodeId);
    if (!n) {
      this.render();
      return;
    }
    const problems = ed.issuesFor(n);
    this.problemsEl?.replaceChildren(...problems.map((p) => el('p', { class: 'insp-problem' }, p)));
    if (this.viewEl) ed.fillView(this.viewEl, n);
    if (this.textEl) this.textEl.textContent = ed.report?.previews?.[ed.flat(n.id)] ?? '';
    if (this.infoEl) this.infoEl.textContent = ed.groupInfo(n);
  }
}
