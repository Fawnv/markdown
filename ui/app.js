'use strict';
/* Markdown — UI logic. Talks to the Rust host through window.ipc / window.__recv. */

const $ = (s) => document.querySelector(s);
const send = (cmd, data = {}) => window.ipc.postMessage(JSON.stringify({ cmd, ...data }));

/* ---------------------------------------------------------------- i18n -- */
const I18N = {
  en: {
    preview: 'Preview', raw: 'Raw', split: 'Split', untitled: 'Untitled',
    openTip: 'Open… (Ctrl+O)', saveTip: 'Save (Ctrl+S)', themeTip: 'Theme', menuTip: 'Menu',
    new: 'New', newWindow: 'New window', open: 'Open…', save: 'Save', saveAs: 'Save as…',
    find: 'Find', findPh: 'Find', findPrev: 'Previous match (Shift+Enter)', findNext: 'Next match (Enter)',
    findClose: 'Close (Esc)', noResults: 'No results', wrap: 'Word wrap', view: 'View',
    themeGroup: 'Theme', themeAuto: 'System', themeLight: 'Light', themeDark: 'Dark',
    zoomIn: 'Zoom in', zoomOut: 'Zoom out', zoomReset: 'Reset zoom', print: 'Print…',
    reveal: 'Show in folder', install: 'Register in Windows…', uninstall: 'Remove from Windows…',
    timeDate: 'Time/Date', ln: 'Ln', col: 'Col', saved: 'Saved', reloaded: 'Reloaded — file changed on disk',
    changedOnDisk: 'File changed on disk (you have unsaved edits)', dropHere: 'Drop a file to open it',
    emptyPreview: 'Nothing to preview yet. Press Ctrl+E to switch to Raw and start writing.',
    eolTip: 'Line endings (click to switch)', words: (n) => `${n.toLocaleString()} ${n === 1 ? 'word' : 'words'}`,
    chars: (n) => `${n.toLocaleString()} chars`,
  },
  ru: {
    preview: 'Просмотр', raw: 'Raw', split: 'Рядом', untitled: 'Без имени',
    openTip: 'Открыть… (Ctrl+O)', saveTip: 'Сохранить (Ctrl+S)', themeTip: 'Тема', menuTip: 'Меню',
    new: 'Новый', newWindow: 'Новое окно', open: 'Открыть…', save: 'Сохранить', saveAs: 'Сохранить как…',
    find: 'Найти', findPh: 'Найти', findPrev: 'Предыдущее (Shift+Enter)', findNext: 'Следующее (Enter)',
    findClose: 'Закрыть (Esc)', noResults: 'Нет совпадений', wrap: 'Перенос по словам', view: 'Вид',
    themeGroup: 'Тема', themeAuto: 'Как в системе', themeLight: 'Светлая', themeDark: 'Тёмная',
    zoomIn: 'Увеличить', zoomOut: 'Уменьшить', zoomReset: 'Сбросить масштаб', print: 'Печать…',
    reveal: 'Показать в папке', install: 'Зарегистрировать в Windows…', uninstall: 'Удалить из Windows…',
    timeDate: 'Время и дата', ln: 'Стр', col: 'стлб', saved: 'Сохранено', reloaded: 'Файл изменён на диске — перезагружен',
    changedOnDisk: 'Файл изменён на диске (есть несохранённые правки)', dropHere: 'Отпустите файл, чтобы открыть',
    emptyPreview: 'Пока нечего показать. Нажмите Ctrl+E, чтобы перейти в Raw и начать писать.',
    eolTip: 'Окончания строк (нажмите, чтобы сменить)',
    words: (n) => `${n.toLocaleString('ru')} ${plural(n, 'слово', 'слова', 'слов')}`,
    chars: (n) => `${n.toLocaleString('ru')} ${plural(n, 'символ', 'символа', 'символов')}`,
  },
};
function plural(n, one, few, many) {
  const m10 = n % 10, m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return few;
  return many;
}
let T = I18N.en;

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach((el) => { el.textContent = T[el.dataset.i18n]; });
  document.querySelectorAll('[data-i18n-title]').forEach((el) => {
    el.title = T[el.dataset.i18nTitle];
    el.setAttribute('aria-label', T[el.dataset.i18nTitle]);
  });
  $('#find-input').placeholder = T.findPh;
}

/* --------------------------------------------------------------- state -- */
const editor = $('#editor');
const preview = $('#preview');
const previewPane = $('#preview-pane');
const main = $('#main');

const state = {
  mode: 'preview',
  path: null,
  name: '',
  isMarkdown: true,
  savedText: '',
  dirty: false,
  eol: 'crlf',
  encoding: 'UTF-8',
  installed: false,
  renderSeq: 0,
  renderedText: null,
  inflightText: null,
  renderedSeq: 0,
  renderSentAt: 0,
  renderCost: 30,
  afterRender: [],
  shown: false,
  settings: { theme: 'auto', wrap: true, zoom: 1, split: 0.5 },
};

/* ------------------------------------------------------- host messages -- */
window.__recv = (msg) => {
  switch (msg.type) {
    case 'init': onInit(msg); break;
    case 'doc': onDoc(msg); break;
    case 'rendered': onRendered(msg); break;
    case 'saved': onSaved(msg); break;
    case 'status': flash(T[msg.key] || msg.text || ''); break;
    case 'installed': state.installed = msg.value; break;
    case 'close-request': send('close', dirtyPayload()); break;
    case 'drag': $('#drop-overlay').hidden = !msg.over; break;
    case 'dropOpen': send('openPath', { path: msg.path, ...dirtyPayload() }); break;
  }
};

function onInit(msg) {
  T = I18N[msg.lang] || I18N.en;
  document.documentElement.lang = msg.lang;
  state.installed = !!msg.installed;
  Object.assign(state.settings, msg.settings || {});
  applyI18n();
  applyTheme();
  applyWrap();
  applyZoom();
  main.style.setProperty('--split', `${state.settings.split * 100}%`);
}

function onDoc(msg) {
  const reload = !!msg.reload;
  const anchor = reload ? captureAnchor() : null;
  const caret = editor.selectionStart;

  state.path = msg.path || null;
  state.name = msg.name || T.untitled;
  state.isMarkdown = msg.isMarkdown !== false;
  state.eol = msg.eol || 'crlf';
  state.encoding = msg.encoding || 'UTF-8';
  state.savedText = msg.text;
  state.savedEol = state.eol;
  editor.value = msg.text;
  state.renderedText = null;
  state.inflightText = null;
  lineCache = null;
  setDirty(false);
  updateTitle();
  updateStatus();

  let mode = state.mode;
  if (!reload) {
    if (!state.isMarkdown || (!msg.path && !msg.text)) mode = 'raw';
    else if (mode === 'raw') mode = 'preview';
    editor.setSelectionRange(0, 0);
    editor.scrollTop = 0;
    previewPane.scrollTop = 0;
  } else {
    const c = Math.min(caret, editor.value.length);
    editor.setSelectionRange(c, c);
  }
  setMode(mode, { keepPosition: false });

  const after = () => {
    if (reload && anchor) restoreAnchor(anchor);
    else if (msg.fragment) scrollToFragment(msg.fragment);
    if (reload) flash(T.reloaded);
    reveal();
  };
  if (mode === 'raw') after(); else whenRendered(after);
  if (findOpen) runFind(false);
}

function reveal() {
  if (state.shown) return;
  state.shown = true;
  send('shown');
  requestAnimationFrame(() => (state.mode === 'preview' ? previewPane : editor).focus({ preventScroll: true }));
}

/* ----------------------------------------------------------- rendering -- */
let renderTimer = 0;
function requestRender(immediate = false) {
  clearTimeout(renderTimer);
  const go = () => {
    const text = editor.value;
    if (state.renderedText === text || state.inflightText === text) return;
    state.renderSeq += 1;
    state.inflightText = text;
    state.renderSentAt = performance.now();
    send('render', { text, seq: state.renderSeq });
  };
  if (immediate) go();
  else renderTimer = setTimeout(go, Math.min(400, 60 + state.renderCost * 2));
}

function whenRendered(fn) {
  if (state.renderedText === editor.value) fn();
  else { state.afterRender.push(fn); requestRender(true); }
}

function onRendered(msg) {
  if (msg.seq < state.renderedSeq) return;
  state.renderedSeq = msg.seq;
  state.renderCost = performance.now() - state.renderSentAt;
  const scrollTop = previewPane.scrollTop;
  preview.replaceChildren(sanitize(msg.html));
  if (!preview.firstChild) {
    const p = document.createElement('p');
    p.className = 'empty-hint';
    p.textContent = T.emptyPreview;
    preview.append(p);
  }
  previewPane.scrollTop = scrollTop;
  blockCache = null;
  if (msg.seq === state.renderSeq) {
    state.renderedText = state.inflightText;
    state.inflightText = null;
    const queue = state.afterRender.splice(0);
    queue.forEach((fn) => fn());
  }
  if (findOpen && findSurface() === 'preview') runFind(false);
}

/* Keep GitHub's rules: no scripts, no styles, no event handlers, no js: URLs. */
const DROP_TAGS = new Set(['SCRIPT', 'STYLE', 'IFRAME', 'FRAME', 'FRAMESET', 'OBJECT', 'EMBED', 'APPLET',
  'LINK', 'META', 'BASE', 'FORM', 'NOSCRIPT', 'TEMPLATE', 'TITLE', 'TEXTAREA', 'XMP', 'PLAINTEXT',
  'NOEMBED', 'NOFRAMES', 'DIALOG', 'PORTAL']);
const URL_ATTRS = ['href', 'src', 'xlink:href', 'action', 'formaction', 'poster', 'background', 'cite', 'longdesc'];

function sanitize(html) {
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  const frag = tpl.content;
  frag.querySelectorAll('*').forEach((el) => {
    if (DROP_TAGS.has(el.tagName.toUpperCase())) { el.remove(); return; }
    for (const attr of [...el.attributes]) {
      const n = attr.name.toLowerCase();
      if (n.startsWith('on') || n === 'style' || n === 'srcdoc' || n === 'autofocus') el.removeAttribute(attr.name);
      else if (URL_ATTRS.includes(n)) {
        const v = attr.value.trim().replace(/[\u0000-\u001f\s]+/g, '').toLowerCase();
        if (v.startsWith('javascript:') || v.startsWith('vbscript:') ||
            (v.startsWith('data:') && !(n === 'src' && v.startsWith('data:image/')))) el.removeAttribute(attr.name);
      }
    }
    if (el.tagName === 'INPUT' && el.type === 'checkbox' && el.closest('.task-list-item')) {
      el.removeAttribute('disabled');
      el.tabIndex = -1;
    } else if (el.tagName === 'INPUT') {
      el.setAttribute('disabled', '');
    }
    for (const a of ['src', 'poster']) {
      const v = el.getAttribute(a);
      if (v && isLocalUrl(v)) el.setAttribute(a, fileUrl(v));
    }
    if (el.hasAttribute('srcset')) {
      el.setAttribute('srcset', el.getAttribute('srcset').split(',').map((part) => {
        const [u, ...rest] = part.trim().split(/\s+/);
        return [isLocalUrl(u) ? fileUrl(u) : u, ...rest].join(' ');
      }).join(', '));
    }
  });
  return frag;
}

function isLocalUrl(u) {
  if (!u || u.startsWith('#') || u.startsWith('//')) return false;
  if (/^[a-z]:[\\/]/i.test(u) || /^file:/i.test(u)) return true;
  if (u.startsWith('/__file?')) return false;
  return !/^[a-z][a-z0-9+.-]*:/i.test(u);
}
const fileUrl = (u) => `/__file?p=${encodeURIComponent(u)}`;

/* --------------------------------------------------------------- modes -- */
function setMode(mode, { keepPosition = true } = {}) {
  const prev = state.mode;
  const line = keepPosition && prev !== mode ? currentTopLine(prev) : null;
  state.mode = mode;
  main.dataset.mode = mode;
  document.querySelectorAll('#modes [data-mode]').forEach((b) => b.setAttribute('aria-selected', String(b.dataset.mode === mode)));
  lineCache = null;
  blockCache = null; // pane widths changed
  if (mode !== 'raw') requestRender(true);
  if (line != null) {
    if (mode === 'raw') scrollEditorToLine(line);
    else whenRendered(() => {
      scrollPreviewToLine(line);
      if (mode === 'split') scrollEditorToLine(line);
    });
  }
  updateStatus();
  if (findOpen) runFind(false);
  if (prev !== mode) {
    if (mode === 'preview') previewPane.focus({ preventScroll: true });
    else editor.focus({ preventScroll: true });
  }
}

function currentTopLine(mode) {
  if (mode === 'preview') return previewTopLine();
  return editorTopLine();
}

/* ------------------------------------------- source line <-> pixel maps -- */
let lineCache = null;      // { text, width, tops: number[] } — y of each source line in the editor
let blockCache = null;     // [{ line, end, top, height }] — rendered blocks with data-sourcepos

function editorLineTops() {
  const width = editor.clientWidth;
  if (lineCache && lineCache.text === editor.value && lineCache.width === width) return lineCache.tops;
  const mirror = $('#mirror');
  mirror.style.width = `${width}px`;
  mirror.style.whiteSpace = main.classList.contains('nowrap') ? 'pre' : 'pre-wrap';
  const frag = document.createDocumentFragment();
  for (const l of editor.value.split('\n')) {
    const d = document.createElement('div');
    d.textContent = l || '​';
    frag.append(d);
  }
  mirror.replaceChildren(frag);
  const tops = Array.from(mirror.children, (d) => d.offsetTop);
  mirror.replaceChildren();
  lineCache = { text: editor.value, width, tops };
  return tops;
}

function editorTopLine() {
  const tops = editorLineTops();
  const y = editor.scrollTop + parseFloat(getComputedStyle(editor).paddingTop);
  let lo = 0, hi = tops.length - 1;
  while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (tops[mid] <= y) lo = mid; else hi = mid - 1; }
  const next = tops[lo + 1] ?? tops[lo] + 1;
  return lo + 1 + Math.max(0, Math.min(1, (y - tops[lo]) / Math.max(1, next - tops[lo])));
}

function scrollEditorToLine(line) {
  const tops = editorLineTops();
  const i = Math.max(0, Math.min(tops.length - 1, Math.floor(line) - 1));
  const next = tops[i + 1] ?? tops[i];
  const y = tops[i] + (line - Math.floor(line)) * (next - tops[i]);
  setScrollTop(editor, y - parseFloat(getComputedStyle(editor).paddingTop));
  if (document.activeElement !== editor || state.mode === 'raw') {
    const offset = lineOffset(i);
    editor.setSelectionRange(offset, offset);
  }
}

function lineOffset(index) {
  let pos = 0;
  for (let i = 0; i < index; i++) {
    const n = editor.value.indexOf('\n', pos);
    if (n < 0) return editor.value.length;
    pos = n + 1;
  }
  return pos;
}

const BLOCK_TAGS = new Set(['P', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'LI', 'PRE', 'BLOCKQUOTE', 'TABLE', 'TR',
  'HR', 'DIV', 'DL', 'DT', 'DD', 'SECTION', 'DETAILS', 'FIGURE']);

function previewBlocks() {
  if (blockCache) return blockCache;
  const paneTop = previewPane.getBoundingClientRect().top - previewPane.scrollTop;
  const out = [];
  preview.querySelectorAll('[data-sourcepos]').forEach((el) => {
    if (!BLOCK_TAGS.has(el.tagName)) return;
    const m = /^(\d+):\d+-(\d+):\d+/.exec(el.dataset.sourcepos);
    if (!m) return;
    const r = el.getBoundingClientRect();
    if (!r.height && !r.width) return;
    out.push({ line: +m[1], end: +m[2], top: r.top - paneTop, height: r.height });
  });
  out.sort((a, b) => a.line - b.line || a.top - b.top);
  blockCache = out;
  return out;
}

function previewTopLine() {
  const blocks = previewBlocks();
  if (!blocks.length) return 1;
  const y = previewPane.scrollTop + 8;
  let prev = null, next = null;
  for (const b of blocks) {
    if (b.top <= y) { if (!prev || b.top >= prev.top) prev = b; } else if (!next || b.top < next.top) next = b;
  }
  if (!prev) return 1;
  if (y < prev.top + prev.height) {
    return prev.line + ((y - prev.top) / Math.max(1, prev.height)) * (prev.end - prev.line + 1);
  }
  if (next) {
    const gapTop = prev.top + prev.height;
    return prev.end + 1 + ((y - gapTop) / Math.max(1, next.top - gapTop)) * Math.max(0, next.line - prev.end - 1);
  }
  return prev.end;
}

function scrollPreviewToLine(line) {
  const blocks = previewBlocks();
  if (!blocks.length) return;
  let prev = null, next = null;
  for (const b of blocks) {
    if (b.line <= line) { if (!prev || b.line > prev.line || (b.line === prev.line && b.end < prev.end)) prev = b; }
    else if (!next || b.line < next.line) next = b;
  }
  let y;
  if (!prev) y = 0;
  else if (line <= prev.end + 1) y = prev.top + ((line - prev.line) / (prev.end - prev.line + 1)) * prev.height;
  else if (next) {
    const gapTop = prev.top + prev.height;
    y = gapTop + ((line - prev.end - 1) / Math.max(1, next.line - prev.end - 1)) * (next.top - gapTop);
  } else y = prev.top + prev.height;
  setScrollTop(previewPane, y - 8);
}

function captureAnchor() {
  return state.mode === 'raw' ? { line: editorTopLine() } : { line: previewTopLine() };
}
function restoreAnchor(a) {
  if (state.mode !== 'raw') scrollPreviewToLine(a.line);
  if (state.mode !== 'preview') scrollEditorToLine(a.line);
}

/* split view scroll sync: remember where we scrolled programmatically, so the
   resulting scroll event is not mirrored back (no time windows, no feedback loops) */
const programmatic = new WeakMap();
function setScrollTop(el, y) {
  const target = Math.max(0, Math.min(el.scrollHeight - el.clientHeight, y));
  programmatic.set(el, target);
  el.scrollTop = target;
}
function ignoring(el) {
  const t = programmatic.get(el);
  if (t === undefined) return false;
  programmatic.delete(el);
  return Math.abs(el.scrollTop - t) < 1.5;
}

let syncFrame = 0;
editor.addEventListener('scroll', () => {
  const bd = $('#backdrop');
  bd.scrollTop = editor.scrollTop;
  bd.scrollLeft = editor.scrollLeft;
  if (state.mode !== 'split' || ignoring(editor)) return;
  cancelAnimationFrame(syncFrame);
  syncFrame = requestAnimationFrame(() => scrollPreviewToLine(editorTopLine()));
});
previewPane.addEventListener('scroll', () => {
  if (state.mode !== 'split' || ignoring(previewPane)) return;
  cancelAnimationFrame(syncFrame);
  syncFrame = requestAnimationFrame(() => scrollEditorToLine(previewTopLine()));
});
new ResizeObserver(() => { blockCache = null; lineCache = null; }).observe(main);
preview.addEventListener('load', () => { blockCache = null; }, true); // images change layout

/* ---------------------------------------------------------- editing ---- */
const isDirty = () => editor.value !== state.savedText || state.eol !== state.savedEol;

editor.addEventListener('input', () => {
  setDirty(isDirty());
  lineCache = null;
  if (state.mode === 'split') requestRender();
  scheduleStatus();
  if (findOpen) runFind(false, true);
});
['keyup', 'mouseup', 'select', 'focus'].forEach((e) => editor.addEventListener(e, scheduleStatus));

editor.addEventListener('keydown', (e) => {
  if (e.key === 'Tab' && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    indent(e.shiftKey);
  } else if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.altKey && state.isMarkdown) {
    if (continueList()) e.preventDefault();
  }
});

function insertText(text) {
  // execCommand keeps the native undo stack intact.
  if (!document.execCommand('insertText', false, text)) {
    editor.setRangeText(text, editor.selectionStart, editor.selectionEnd, 'end');
    editor.dispatchEvent(new Event('input'));
  }
}

function indent(outdent) {
  const v = editor.value;
  let { selectionStart: s, selectionEnd: e } = editor;
  const multi = v.slice(s, e).includes('\n');
  if (!multi && !outdent) { insertText('\t'); return; }
  const lineStart = v.lastIndexOf('\n', s - 1) + 1;
  if (e > s && v[e - 1] === '\n') e -= 1;
  const block = v.slice(lineStart, e);
  const lines = block.split('\n');
  const changed = lines.map((l) => {
    if (!outdent) return '\t' + l;
    if (l.startsWith('\t')) return l.slice(1);
    return l.replace(/^ {1,4}/, '');
  }).join('\n');
  editor.setSelectionRange(lineStart, e);
  insertText(changed);
  editor.setSelectionRange(lineStart, lineStart + changed.length);
}

function continueList() {
  const v = editor.value;
  const pos = editor.selectionStart;
  if (pos !== editor.selectionEnd) return false;
  const lineStart = v.lastIndexOf('\n', pos - 1) + 1;
  const line = v.slice(lineStart, pos);
  const m = /^(\s*)([-+*]|(\d+)([.)]))(\s+)(\[[ xX]\]\s+)?/.exec(line);
  if (!m) return false;
  if (line.length === m[0].length && v.slice(pos, v.indexOf('\n', pos) < 0 ? v.length : v.indexOf('\n', pos)).trim() === '') {
    // Empty item: end the list.
    editor.setSelectionRange(lineStart, pos);
    insertText('');
    return true;
  }
  const bullet = m[3] ? `${+m[3] + 1}${m[4]}` : m[2];
  insertText(`\n${m[1]}${bullet}${m[5]}${m[6] ? '[ ] ' : ''}`);
  return true;
}

function insertTimeDate() {
  if (state.mode === 'preview') setMode('raw');
  const d = new Date();
  const lang = document.documentElement.lang;
  insertText(`${d.toLocaleTimeString(lang, { hour: '2-digit', minute: '2-digit' })} ${d.toLocaleDateString(lang)}`);
}

/* task list checkboxes in preview write back to the source */
preview.addEventListener('change', (e) => {
  const box = e.target;
  if (!(box instanceof HTMLInputElement) || box.type !== 'checkbox') return;
  const li = box.closest('[data-sourcepos]');
  const m = li && /^(\d+):/.exec(li.dataset.sourcepos);
  if (!m) return;
  const start = lineOffset(+m[1] - 1);
  const end = editor.value.indexOf('\n', start);
  const line = editor.value.slice(start, end < 0 ? undefined : end);
  const mm = /^(\s*(?:>\s*)*(?:[-+*]|\d+[.)])\s+\[)([ xX])(\])/.exec(line);
  if (!mm) return;
  const at = start + mm[1].length;
  editor.setRangeText(box.checked ? 'x' : ' ', at, at + 1, 'preserve');
  state.renderedText = editor.value; // DOM already shows the new state
  setDirty(isDirty());
  lineCache = null;
});

/* ------------------------------------------------------------- links --- */
preview.addEventListener('click', (e) => {
  const a = e.target.closest('a[href]');
  if (!a) return;
  e.preventDefault();
  const href = a.getAttribute('href');
  if (href.startsWith('#')) { scrollToFragment(href.slice(1)); return; }
  if (/^(https?|mailto|ftp):/i.test(href)) { send('openExternal', { url: href }); return; }
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) && !/^[a-z]:[\\/]/i.test(href) && !/^file:/i.test(href)) return;
  send('openLink', { href, newWindow: e.ctrlKey || e.button === 1, ...dirtyPayload() });
});
preview.addEventListener('auxclick', (e) => {
  if (e.button !== 1) return;
  const a = e.target.closest('a[href]');
  if (a) { e.preventDefault(); a.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true })); }
});

function scrollToFragment(frag) {
  let id = frag;
  try { id = decodeURIComponent(frag); } catch { /* keep raw */ }
  const el = document.getElementById(id) || document.getElementById(id.toLowerCase()) ||
             preview.querySelector(`[name="${CSS.escape(id)}"]`);
  if (!el) return;
  if (state.mode === 'raw') setMode('preview');
  whenRendered(() => {
    const top = el.getBoundingClientRect().top - previewPane.getBoundingClientRect().top + previewPane.scrollTop;
    previewPane.scrollTop = top - 16;
  });
}

/* --------------------------------------------------------------- find --- */
let findOpen = false;
let findMatches = [];
let findIndex = -1;

const findSurface = () => (state.mode === 'preview' ? 'preview' : 'editor');

function openFind() {
  const bar = $('#findbar');
  const input = $('#find-input');
  const sel = state.mode === 'preview' ? String(window.getSelection())
    : editor.value.slice(editor.selectionStart, editor.selectionEnd);
  if (sel && !sel.includes('\n') && sel.length < 200) input.value = sel;
  bar.hidden = false;
  findOpen = true;
  input.focus();
  input.select();
  runFind(true);
}

function closeFind() {
  findOpen = false;
  $('#findbar').hidden = true;
  $('#backdrop-inner').replaceChildren();
  if (window.CSS && CSS.highlights) { CSS.highlights.delete('find'); CSS.highlights.delete('find-current'); }
  if (state.mode === 'preview') previewPane.focus({ preventScroll: true });
  else editor.focus({ preventScroll: true });
}

function runFind(jump, keepIndex = false) {
  const q = $('#find-input').value;
  const surface = findSurface();
  const prevIndex = findIndex;
  findMatches = [];
  findIndex = -1;
  $('#backdrop-inner').replaceChildren();
  if (CSS.highlights) { CSS.highlights.delete('find'); CSS.highlights.delete('find-current'); }
  if (!q) { updateFindCount(); return; }
  const needle = q.toLowerCase();

  if (surface === 'editor') {
    const hay = editor.value.toLowerCase();
    for (let i = hay.indexOf(needle); i >= 0 && findMatches.length < 10000; i = hay.indexOf(needle, i + needle.length)) {
      findMatches.push({ start: i, end: i + q.length });
    }
    // first match at/after the caret
    const from = editor.selectionStart;
    findIndex = findMatches.length ? Math.max(0, findMatches.findIndex((m) => m.start >= from)) : -1;
  } else {
    const walker = document.createTreeWalker(preview, NodeFilter.SHOW_TEXT);
    const nodes = [];
    let text = '';
    for (let n = walker.nextNode(); n; n = walker.nextNode()) { nodes.push({ n, at: text.length }); text += n.data; }
    const hay = text.toLowerCase();
    const locate = (off, isEnd) => {
      let lo = 0, hi = nodes.length - 1;
      while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (nodes[mid].at < off || (!isEnd && nodes[mid].at === off)) lo = mid; else hi = mid - 1; }
      return [nodes[lo].n, off - nodes[lo].at];
    };
    for (let i = hay.indexOf(needle); i >= 0 && findMatches.length < 10000; i = hay.indexOf(needle, i + needle.length)) {
      const r = new Range();
      const [sn, so] = locate(i, false);
      const [en, eo] = locate(i + q.length, true);
      try { r.setStart(sn, so); r.setEnd(en, eo); findMatches.push({ range: r }); } catch { /* skip */ }
    }
    findIndex = findMatches.length ? 0 : -1;
    if (findMatches.length && CSS.highlights) CSS.highlights.set('find', new Highlight(...findMatches.map((m) => m.range)));
  }
  if (keepIndex && prevIndex >= 0 && prevIndex < findMatches.length) findIndex = prevIndex;
  showCurrentMatch(jump);
}

function stepFind(dir) {
  if (!findOpen) { openFind(); return; }
  if (!findMatches.length) { runFind(true); return; }
  findIndex = (findIndex + dir + findMatches.length) % findMatches.length;
  showCurrentMatch(true);
}

function showCurrentMatch(scroll) {
  updateFindCount();
  if (findIndex < 0) return;
  const m = findMatches[findIndex];
  if (findSurface() === 'editor') {
    paintEditorMatches();
    editor.setSelectionRange(m.start, m.end);
    if (scroll) {
      const mark = $('#backdrop-inner mark.current');
      if (mark) {
        const top = mark.offsetTop;
        if (top < editor.scrollTop + 20 || top > editor.scrollTop + editor.clientHeight - 60) {
          editor.scrollTop = top - editor.clientHeight / 3;
        }
      }
    }
  } else {
    if (CSS.highlights) CSS.highlights.set('find-current', new Highlight(m.range));
    if (scroll) {
      const r = m.range.getBoundingClientRect();
      const p = previewPane.getBoundingClientRect();
      if (r.top < p.top + 20 || r.bottom > p.bottom - 20) previewPane.scrollTop += r.top - p.top - previewPane.clientHeight / 3;
    }
  }
}

function paintEditorMatches() {
  const v = editor.value;
  const frag = document.createDocumentFragment();
  let at = 0;
  findMatches.forEach((m, i) => {
    frag.append(v.slice(at, m.start));
    const mark = document.createElement('mark');
    mark.textContent = v.slice(m.start, m.end);
    if (i === findIndex) mark.className = 'current';
    frag.append(mark);
    at = m.end;
  });
  frag.append(v.slice(at) + '​');
  $('#backdrop-inner').replaceChildren(frag);
  const bd = $('#backdrop');
  bd.scrollTop = editor.scrollTop;
  bd.scrollLeft = editor.scrollLeft;
}

function updateFindCount() {
  const q = $('#find-input').value;
  const el = $('#find-count');
  $('#findbar').classList.toggle('no-results', !!q && !findMatches.length);
  el.textContent = !q ? '' : findMatches.length ? `${findIndex + 1}/${findMatches.length}` : T.noResults;
}

$('#find-input').addEventListener('input', () => runFind(true));
$('#find-input').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.preventDefault(); stepFind(e.shiftKey ? -1 : 1); }
  else if (e.key === 'Escape') { e.preventDefault(); closeFind(); }
});
$('#find-next').addEventListener('click', () => stepFind(1));
$('#find-prev').addEventListener('click', () => stepFind(-1));
$('#find-close').addEventListener('click', closeFind);

/* ------------------------------------------------- dirty / title / status */
function setDirty(v) {
  if (state.dirty === v) return;
  state.dirty = v;
  $('#dirty-dot').hidden = !v;
  send('dirty', { value: v });
}

function dirtyPayload() {
  return state.dirty ? { text: editor.value, eol: state.eol } : {};
}

function updateTitle() {
  $('#doc-name').textContent = state.name;
  $('#doc-title').title = state.path || state.name;
}

let statusTimer = 0;
function scheduleStatus() {
  cancelAnimationFrame(statusTimer);
  statusTimer = requestAnimationFrame(updateStatus);
}
let wordsTimer = 0;
function updateStatus() {
  const v = editor.value;
  const pos = editor.selectionStart;
  let line = 1, lastNl = -1;
  for (let i = v.indexOf('\n'); i >= 0 && i < pos; i = v.indexOf('\n', i + 1)) { line++; lastNl = i; }
  $('#st-pos').textContent = `${T.ln} ${line}, ${T.col} ${pos - lastNl}`;
  clearTimeout(wordsTimer);
  wordsTimer = setTimeout(() => {
    const words = (v.match(/[\p{L}\p{N}][\p{L}\p{N}'’_-]*/gu) || []).length;
    $('#st-words').textContent = T.words(words);
    $('#st-words').title = T.chars(v.length);
  }, v.length > 200000 ? 400 : 60);
  $('#st-enc').textContent = state.encoding;
  $('#st-eol').textContent = state.eol === 'crlf' ? 'Windows (CRLF)' : 'Unix (LF)';
}

let flashTimer = 0;
function flash(text) {
  const el = $('#st-msg');
  el.textContent = text;
  el.classList.remove('fade');
  clearTimeout(flashTimer);
  flashTimer = setTimeout(() => el.classList.add('fade'), 3500);
}

function onSaved(msg) {
  state.path = msg.path;
  state.name = msg.name;
  state.encoding = msg.encoding || state.encoding;
  state.isMarkdown = msg.isMarkdown !== false;
  state.savedText = editor.value;
  state.savedEol = state.eol;
  setDirty(isDirty());
  updateTitle();
  updateStatus();
  flash(msg.note || T.saved);
}

$('#st-eol').addEventListener('click', () => {
  state.eol = state.eol === 'crlf' ? 'lf' : 'crlf';
  send('eol', { value: state.eol });
  setDirty(isDirty());
  updateStatus();
});

/* ------------------------------------------------- theme / wrap / zoom -- */
const darkQuery = matchMedia('(prefers-color-scheme: dark)');
function applyTheme() {
  const t = state.settings.theme;
  const dark = t === 'dark' || (t === 'auto' && darkQuery.matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  send('theme', { dark });
}
darkQuery.addEventListener('change', () => { if (state.settings.theme === 'auto') applyTheme(); });

function setTheme(t) { state.settings.theme = t; applyTheme(); saveSettings(); }
function cycleTheme() {
  const order = ['auto', 'light', 'dark'];
  setTheme(order[(order.indexOf(state.settings.theme) + 1) % order.length]);
  flash(`${T.themeGroup}: ${T['theme' + state.settings.theme[0].toUpperCase() + state.settings.theme.slice(1)]}`);
}

function applyWrap() {
  main.classList.toggle('nowrap', !state.settings.wrap);
  editor.wrap = state.settings.wrap ? 'soft' : 'off';
  lineCache = null;
}
function toggleWrap() { state.settings.wrap = !state.settings.wrap; applyWrap(); saveSettings(); }

function applyZoom() {
  document.documentElement.style.setProperty('--zoom', String(state.settings.zoom));
  $('#st-zoom').textContent = `${Math.round(state.settings.zoom * 100)}%`;
  lineCache = null;
  blockCache = null;
}
function zoom(delta) {
  const line = state.mode === 'raw' ? editorTopLine() : previewTopLine();
  const z = delta === 0 ? 1 : Math.round(Math.min(3, Math.max(0.5, state.settings.zoom + delta)) * 10) / 10;
  state.settings.zoom = z;
  applyZoom();
  saveSettings();
  if (state.mode === 'raw') scrollEditorToLine(line); else scrollPreviewToLine(line);
}
$('#st-zoom').addEventListener('click', () => zoom(0));
window.addEventListener('wheel', (e) => {
  if (!e.ctrlKey) return;
  e.preventDefault();
  zoom(e.deltaY < 0 ? 0.1 : -0.1);
}, { passive: false });

let settingsTimer = 0;
function saveSettings() {
  clearTimeout(settingsTimer);
  settingsTimer = setTimeout(() => send('settings', { settings: state.settings }), 300);
}

/* ------------------------------------------------------------ splitter -- */
$('#splitter').addEventListener('pointerdown', (e) => {
  const sp = e.currentTarget;
  sp.setPointerCapture(e.pointerId);
  sp.classList.add('dragging');
  const rect = main.getBoundingClientRect();
  const move = (ev) => {
    const f = Math.min(0.85, Math.max(0.15, (ev.clientX - rect.left) / rect.width));
    state.settings.split = f;
    main.style.setProperty('--split', `${f * 100}%`);
  };
  const up = () => {
    sp.classList.remove('dragging');
    sp.removeEventListener('pointermove', move);
    sp.removeEventListener('pointerup', up);
    lineCache = null; blockCache = null;
    saveSettings();
  };
  sp.addEventListener('pointermove', move);
  sp.addEventListener('pointerup', up);
});
$('#splitter').addEventListener('dblclick', () => {
  state.settings.split = 0.5;
  main.style.setProperty('--split', '50%');
  saveSettings();
});

/* ---------------------------------------------------------------- menu -- */
function menuModel() {
  const th = state.settings.theme;
  return [
    { label: T.new, kbd: 'Ctrl+N', run: () => send('new', dirtyPayload()) },
    { label: T.newWindow, kbd: 'Ctrl+Shift+N', run: () => send('newWindow') },
    { label: T.open, kbd: 'Ctrl+O', run: () => send('open', dirtyPayload()) },
    { label: T.save, kbd: 'Ctrl+S', run: () => save(false) },
    { label: T.saveAs, kbd: 'Ctrl+Shift+S', run: () => save(true) },
    '-',
    { label: T.find, kbd: 'Ctrl+F', run: openFind },
    { label: T.timeDate, kbd: 'F5', run: insertTimeDate },
    { label: T.wrap, kbd: 'Alt+Z', checked: state.settings.wrap, run: toggleWrap },
    { label: T.zoomIn, kbd: 'Ctrl++', run: () => zoom(0.1) },
    { label: T.zoomOut, kbd: 'Ctrl+−', run: () => zoom(-0.1) },
    { label: T.zoomReset, kbd: 'Ctrl+0', run: () => zoom(0) },
    { group: T.themeGroup },
    { label: T.themeAuto, checked: th === 'auto', run: () => setTheme('auto') },
    { label: T.themeLight, checked: th === 'light', run: () => setTheme('light') },
    { label: T.themeDark, checked: th === 'dark', run: () => setTheme('dark') },
    '-',
    { label: T.print, kbd: 'Ctrl+P', run: printDoc },
    ...(state.path ? [{ label: T.reveal, run: () => send('reveal') }] : []),
    state.installed ? { label: T.uninstall, run: () => send('uninstall') } : { label: T.install, run: () => send('install') },
  ];
}

const CHECK_SVG = '<svg class="check" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7"/></svg>';
function openMenu() {
  const menu = $('#menu');
  const btn = $('#btn-menu');
  menu.replaceChildren();
  for (const item of menuModel()) {
    if (item === '-') { const s = document.createElement('div'); s.className = 'menu-sep'; menu.append(s); continue; }
    if (item.group) { const g = document.createElement('div'); g.className = 'menu-group'; g.textContent = item.group; menu.append(g); continue; }
    const b = document.createElement('button');
    b.className = 'menu-item';
    b.setAttribute('role', item.checked === undefined ? 'menuitem' : 'menuitemcheckbox');
    if (item.checked !== undefined) b.setAttribute('aria-checked', String(item.checked));
    b.innerHTML = CHECK_SVG;
    const l = document.createElement('span'); l.textContent = item.label;
    const k = document.createElement('span'); k.className = 'kbd'; k.textContent = item.kbd || '';
    b.append(l, k);
    b.addEventListener('click', () => { closeMenu(); item.run(); });
    menu.append(b);
  }
  menu.hidden = false;
  const r = btn.getBoundingClientRect();
  menu.style.top = `${r.bottom + 6}px`;
  menu.style.right = `${Math.max(8, innerWidth - r.right)}px`;
  btn.setAttribute('aria-expanded', 'true');
  menu.querySelector('.menu-item')?.focus();
}
function closeMenu() {
  $('#menu').hidden = true;
  $('#btn-menu').setAttribute('aria-expanded', 'false');
}
$('#btn-menu').addEventListener('click', (e) => { e.stopPropagation(); $('#menu').hidden ? openMenu() : closeMenu(); });
document.addEventListener('pointerdown', (e) => { if (!$('#menu').hidden && !e.target.closest('#menu, #btn-menu')) closeMenu(); });
$('#menu').addEventListener('keydown', (e) => {
  const items = [...$('#menu').querySelectorAll('.menu-item')];
  const i = items.indexOf(document.activeElement);
  if (e.key === 'ArrowDown') { e.preventDefault(); items[(i + 1) % items.length].focus(); }
  else if (e.key === 'ArrowUp') { e.preventDefault(); items[(i - 1 + items.length) % items.length].focus(); }
  else if (e.key === 'Escape') { e.preventDefault(); closeMenu(); $('#btn-menu').focus(); }
});

/* ------------------------------------------------------------- actions -- */
function save(as) { send('save', { text: editor.value, eol: state.eol, as }); }

function printDoc() {
  const theme = document.documentElement.dataset.theme;
  whenRendered(() => {
    document.documentElement.dataset.theme = 'light';
    requestAnimationFrame(() => {
      window.print();
      document.documentElement.dataset.theme = theme;
    });
  });
}

$('#modes').addEventListener('click', (e) => {
  const b = e.target.closest('[data-mode]');
  if (b) setMode(b.dataset.mode);
});
$('#btn-open').addEventListener('click', () => send('open', dirtyPayload()));
$('#btn-save').addEventListener('click', () => save(false));
$('#btn-theme').addEventListener('click', cycleTheme);

/* keyboard shortcuts (browser accelerators are disabled in the host) */
// e.code (physical key) so shortcuts also work with the Russian keyboard layout.
window.addEventListener('keydown', (e) => {
  const c = e.code;
  const ctrl = e.ctrlKey && !e.altKey;
  const stop = () => { e.preventDefault(); e.stopPropagation(); };
  if (ctrl && c === 'KeyS') { stop(); save(e.shiftKey); }
  else if (ctrl && c === 'KeyO') { stop(); send('open', dirtyPayload()); }
  else if (ctrl && c === 'KeyN') { stop(); e.shiftKey ? send('newWindow') : send('new', dirtyPayload()); }
  else if (ctrl && c === 'KeyW') { stop(); send('close', dirtyPayload()); }
  else if (ctrl && c === 'KeyP') { stop(); printDoc(); }
  else if (ctrl && (c === 'KeyF' || c === 'KeyH')) { stop(); openFind(); }
  else if (ctrl && c === 'KeyE') { stop(); setMode(state.mode === 'preview' ? 'raw' : 'preview'); }
  else if (ctrl && (c === 'Digit1' || c === 'Numpad1')) { stop(); setMode('preview'); }
  else if (ctrl && (c === 'Digit2' || c === 'Numpad2')) { stop(); setMode('raw'); }
  else if (ctrl && (c === 'Digit3' || c === 'Numpad3')) { stop(); setMode('split'); }
  else if (ctrl && (c === 'Equal' || c === 'NumpadAdd')) { stop(); zoom(0.1); }
  else if (ctrl && (c === 'Minus' || c === 'NumpadSubtract')) { stop(); zoom(-0.1); }
  else if (ctrl && (c === 'Digit0' || c === 'Numpad0')) { stop(); zoom(0); }
  else if (e.altKey && !e.ctrlKey && c === 'KeyZ') { stop(); toggleWrap(); }
  else if (c === 'F3') { stop(); stepFind(e.shiftKey ? -1 : 1); }
  else if (c === 'F5') { stop(); if (document.activeElement === editor) insertTimeDate(); }
  else if (c === 'Escape') {
    if (!$('#menu').hidden) { stop(); closeMenu(); }
    else if (findOpen) { stop(); closeFind(); }
  }
  else if (c === 'F12' || (e.ctrlKey && e.shiftKey && (c === 'KeyI' || c === 'KeyJ')) || (ctrl && c === 'KeyR')) e.preventDefault();
}, true);

/* default context menu only where it is useful (text editing / copying) */
document.addEventListener('contextmenu', (e) => {
  if (e.target.closest('textarea, input')) return;
  if (String(window.getSelection()).length) return;
  e.preventDefault();
});
document.addEventListener('dragover', (e) => e.preventDefault());
document.addEventListener('drop', (e) => e.preventDefault());
window.addEventListener('beforeunload', () => { /* host handles closing */ });

applyI18n();
send('ready');
