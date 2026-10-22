// The documentation application: one page at a time drawn from the JSON
// island #docc-data with Preact and htm (vendor/htm-preact-standalone.js).
// Labels are marked as t:English in double braces and translated by the build (lang.rs).
// A timeline over the saved versions sets where the edits start and the
// moment the page is shown at; the edits are drawn into the text (word diff,
// gutter bars, removed paragraphs in place). Comments are left on selected
// words and kept in the browser until they are copied out.
(function () {
  'use strict';
  var P = window.htmPreact;
  var html = P.html, render = P.render;
  var useState = P.useState, useEffect = P.useEffect, useRef = P.useRef;
  var D = JSON.parse(document.getElementById('docc-data').textContent);
  var pages = D.pages, byId = {};
  pages.forEach(function (p, i) { p.i = i; byId[p.id] = p; });
  var H = D.hist || { commits: [], docs: {} };
  var commits = H.commits || [];
  var mq = function (q) { return !!(window.matchMedia && matchMedia(q).matches); };
  var coarse = mq('(pointer: coarse)');
  var reduced = mq('(prefers-reduced-motion: reduce)');

  // ---- small helpers ----
  function store(k, v) {
    try {
      if (v === undefined) { return JSON.parse(localStorage.getItem(D.key + ':' + k)); }
      localStorage.setItem(D.key + ':' + k, JSON.stringify(v));
    } catch (e) { return null; }
  }
  function decode(h) {
    h = h.replace(/^#/, '');
    try { return decodeURIComponent(h); } catch (e) { return h; }
  }
  // the page a hash belongs to: a page id, <page id>--<anchor> (a heading or
  // field) or <page id>/<JSON Pointer> (a paragraph)
  function route(hash) {
    var id = decode(hash);
    if (byId[id]) { return { page: byId[id] }; }
    var best = null;
    pages.forEach(function (p) {
      if (id.indexOf(p.id + '--') === 0 && (!best || p.id.length > best.id.length)) { best = p; }
    });
    if (best) { return { page: best, target: id }; }
    var k = id.indexOf('/');
    if (k > 0 && byId[id.slice(0, k)]) { return { page: byId[id.slice(0, k)], block: id.slice(k) }; }
    return { page: pages[0], missing: id && id !== 'main' ? id : null };
  }
  function blockHash(doc, ptr) { return '#' + doc + (ptr || ''); }
  // dates in the language of the file; '' for the working tree, which has none.
  // A formatter is made once per options: making one costs more than the
  // whole list of saves, which used to make thousands of them on every step.
  var fmts = {};
  function intl(Kind, o) {
    var k = Kind.name + JSON.stringify(o);
    return fmts[k] || (fmts[k] = new Kind(D.lang || 'en', o));
  }
  function fmt(c, o) {
    var cm = commits[c];
    if (!cm || !cm.h) { return ''; }
    try { return intl(Intl.DateTimeFormat, o).format(new Date(cm.d)); } catch (e) { return cm.d.slice(0, 16).replace('T', ' '); }
  }
  // the exact moment of a version, «14 Oct 2026, 21:40»
  function exact(c) { return fmt(c, { dateStyle: 'medium', timeStyle: 'short' }) || '{{t:not saved yet}}'; }
  // the full build adds the author and message
  function versionLine(c) {
    var cm = commits[c] || {};
    return exact(c) + (cm.a ? ' · ' + cm.a : '') + (cm.m ? ' · ' + cm.m : '');
  }
  function save(name, type, data) {
    var a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([data], { type: type }));
    a.download = name;
    document.body.appendChild(a); a.click(); a.remove();
    setTimeout(function () { URL.revokeObjectURL(a.href); }, 1000);
  }
  function copy(text) {
    function old() {
      var t = document.createElement('textarea');
      t.value = text; t.setAttribute('readonly', ''); t.style.position = 'fixed'; t.style.opacity = '0';
      document.body.appendChild(t); t.select();
      var ok = false;
      try { ok = document.execCommand('copy'); } catch (e) { ok = false; }
      t.remove();
      return ok ? Promise.resolve() : Promise.reject();
    }
    return navigator.clipboard ? navigator.clipboard.writeText(text).catch(old) : old();
  }
  function b64(s) {
    var u = new TextEncoder().encode(s), b = '';
    for (var i = 0; i < u.length; i++) { b += String.fromCharCode(u[i]); }
    return btoa(b);
  }
  // the id of a new comment, as docc comments apply makes one: c- and 10 of a-z, 2-7
  function uuid() {
    var a = new Uint8Array(10), abc = 'abcdefghijklmnopqrstuvwxyz234567', s = 'c-';
    if (window.crypto && crypto.getRandomValues) { crypto.getRandomValues(a); } else { for (var i = 0; i < 10; i++) { a[i] = Math.random() * 256; } }
    for (var k = 0; k < 10; k++) { s += abc[a[k] & 31]; }
    return s;
  }
  // a short hash of a text: the id of a removed block
  function hash(s) {
    var h = 5381;
    for (var i = 0; i < s.length; i++) { h = (h * 33 + s.charCodeAt(i)) | 0; }
    return (h >>> 0).toString(36);
  }
  var box = document.createElement('div');
  function textOf(h) { box.innerHTML = h; return box.textContent.replace(/\s+/g, ' ').trim(); }
  function esc(s) { box.textContent = s; return box.innerHTML; }
  // the source text of a block as a reader sees it: links by their names, no marks
  // ponytail: a few regexes for the usual marks, not a Markdown parser
  function plain(s) {
    // a form field reads as its question and choices
    if (/^\$input: /.test(s || '')) { s = fieldWords(s); }
    // code (fenced or in backticks) is kept as written: marks inside it are not marks
    return (s || '').split(/(```[\s\S]*?```|`[^`\n]+`)/).map(function (t, i) {
      return i % 2 ? t.replace(/^`{3}[^\n]*\n?|\n?`{3}$/g, '').replace(/^`|`$/g, '') : marks(t);
    }).join('');
  }
  function marks(s) {
    return s
      .replace(/\[\[([^\]#|]+)(?:#([^\]|]*))?\]\]/g, function (m, id, a) {
        if (a && a.indexOf('/') > 0) { return a.slice(a.lastIndexOf('/') + 1); }
        return byId[id] ? byId[id].title : id;
      })
      .replace(/\{\{([^}#]+)#[^}]*\}\}/g, function (m, id) { return '«' + (byId[id] ? byId[id].title : id) + '»'; })
      .replace(/!?\[([^\]]*)\]\((?:[^()]|\([^()]*\))*\)/g, '$1')
      .replace(/(\*\*|__)(.+?)\1/g, '$2')
      .replace(/(^|[^*\w])\*(?=\S)([^*\n]*\S)\*/g, '$1$2')
      .replace(/^#{1,6}\s+/gm, '')
      .replace(/^>\s?/gm, '')
      .replace(/\s\^[a-z0-9-]+$/gm, '');
  }
  // the text of a form field as the build writes it («$input: radio; id: x;
  // label: Q; options: A\nB»): «Q — A / B»
  function fieldWords(s) {
    var o = {};
    s.split(/; (?=(?:\$input|id|label|options): )/).forEach(function (p) { var k = p.indexOf(': '); o[p.slice(0, k)] = p.slice(k + 2); });
    return (o.label || '') + (o.options ? ' — ' + o.options.split('\n').join(' / ') : '');
  }
  function snip(s, n) {
    // table rows read as their cells
    s = plain(s).replace(/\|?\s*:?-{3,}:?\s*(?=\|)/g, '').replace(/\s*\|\s*/g, ' ').replace(/\s+/g, ' ').trim();
    return s.length > n ? s.slice(0, n).replace(/\s\S*$/, '') + '…' : s;
  }
  function typing() {
    var a = document.activeElement;
    return a && ((/^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName) && !/^(radio|checkbox|button|range)$/.test(a.type)) || a.isContentEditable);
  }

  // ---- toasts: one line at the bottom, with an optional action ----
  var toastFns = [];
  function toast(text, action, fn) { toastFns.forEach(function (f) { f({ text: text, action: action, fn: fn, k: Date.now() }); }); }
  function Toast() {
    var s = useState(null);
    useEffect(function () {
      var f = function (t) { s[1](t); };
      toastFns.push(f);
      return function () { toastFns.splice(toastFns.indexOf(f), 1); };
    }, []);
    useEffect(function () {
      if (!s[0]) { return; }
      var t = setTimeout(function () { s[1](null); }, 6000);
      return function () { clearTimeout(t); };
    }, [s[0]]);
    return html`<div class="toast-w" role="status" aria-live="polite">${s[0] && html`<div class="toast">${s[0].text}
      ${s[0].action && html`<button type="button" class="link" onClick=${function () { s[0].fn(); s[1](null); }}>${s[0].action}</button>`}</div>`}</div>`;
  }

  // the longest common subsequence of two lists: [op, item] with op -1 (only
  // in x), 0 (both), 1 (only in y); null when the lists are too long to compare
  function lcs(x, y) {
    var n = x.length, m = y.length;
    if (n * m > 4000000) { return null; } // ponytail: huge blocks are shown as replaced
    var dp = [];
    for (var i = 0; i <= n; i++) { dp.push(new Uint32Array(m + 1)); }
    for (i = n - 1; i >= 0; i--) {
      for (var j = m - 1; j >= 0; j--) {
        dp[i][j] = x[i] === y[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
    var out = [];
    i = 0; j = 0;
    while (i < n && j < m) {
      if (x[i] === y[j]) { out.push([0, j, i]); i++; j++; }
      else if (dp[i + 1][j] >= dp[i][j + 1]) { out.push([-1, i]); i++; }
      else { out.push([1, j]); j++; }
    }
    while (i < n) { out.push([-1, i++]); }
    while (j < m) { out.push([1, j++]); }
    return out;
  }
  // a rewrite reads better as old text, then new text, than as confetti
  function rewrite(ops, x, y) {
    var same = 0, len = function (l) { return l.join('').replace(/\s+/g, '').length; };
    ops.forEach(function (o) { if (o[0] === 0) { same += y[o[1]].replace(/\s+/g, '').length; } });
    return same < 0.4 * Math.max(len(x), len(y));
  }
  // ---- edits drawn into the rendered paragraph ----
  // The old source words are matched with the words of the rendered new text,
  // so the paragraph keeps its links, lists and tables: added words are wrapped
  // in <ins>, removed runs go in as <del> before the next word that stayed.
  // Markdown marks that render as no text (table bars, list bullets) are dropped.
  var markOnly = /^([-*+>]|\d+[.)]|:?-{2,}:?|#{1,6})$/;
  function words(s) { return s.replace(/\|/g, ' ').split(/\s+/).filter(function (w) { return w && !markOnly.test(w); }); }
  function textNodes(el, clean) {
    var out = [], w = document.createTreeWalker(el, NodeFilter.SHOW_TEXT), n;
    while ((n = w.nextNode())) { if (!clean || !n.parentNode.closest('del,.sr')) { out.push(n); } }
    return out;
  }
  function run(tag, label, text) {
    var e = document.createElement(tag), s = document.createElement('span');
    s.className = 'sr'; s.textContent = label + ' ';
    e.appendChild(s); e.appendChild(document.createTextNode(text));
    return e;
  }
  // the marks of the new words against the old ones: per new word whether it
  // was added (ins), the removed words before it (del) and, on the last word,
  // the removed words after it (after); null for a rewrite
  function wordMarks(x, y) {
    var ops = lcs(x, y);
    if (!ops || rewrite(ops, x, y)) { return null; }
    // A run of added (or removed) words that ends with the word standing just
    // before it slides over that word when this joins it to the run before:
    // «by SMS [or] via [Telegram via]» reads as «by SMS [or via Telegram] via».
    for (var again = true; again;) {
      again = false;
      for (var i = 1; i < ops.length - 1 && !again; i++) {
        var s = ops[i + 1][0];
        if (ops[i][0] !== 0 || !s || ops[i - 1][0] !== s) { continue; }
        var k = i + 1;
        while (k + 1 < ops.length && ops[k + 1][0] === s) { k++; }
        var last = ops[k], w = s > 0 ? y[last[1]] : x[last[1]];
        if (w !== y[ops[i][1]]) { continue; }
        var moved = s > 0 ? [[1, ops[i][1]]].concat(ops.slice(i + 1, k), [[0, last[1], ops[i][2]]])
          : [[-1, ops[i][2]]].concat(ops.slice(i + 1, k), [[0, ops[i][1], last[1]]]);
        Array.prototype.splice.apply(ops, [i, k - i + 1].concat(moved));
        again = true;
      }
    }
    var out = y.map(function () { return {}; }), gone = [];
    ops.forEach(function (o) {
      if (o[0] < 0) { gone.push(x[o[1]]); return; }
      if (gone.length) { out[o[1]].del = gone.join(' '); gone = []; }
      out[o[1]].ins = o[0] > 0;
    });
    if (gone.length && out.length) { out[out.length - 1].after = gone.join(' '); }
    return out;
  }
  // false when the text was rewritten (or is code): the caller shows the old text whole
  function drawDiff(el, oldSrc) {
    if (el.querySelector('pre')) { return false; }
    var toks = [];
    textNodes(el).forEach(function (n) {
      var re = /\S+/g, m;
      while ((m = re.exec(n.data))) { toks.push({ n: n, s: m.index, e: m.index + m[0].length, w: m[0] }); }
    });
    var ms = wordMarks(words(plain(oldSrc)), toks.map(function (t) { return t.w; }));
    if (!ms) { return false; }
    toks.forEach(function (t, i) { Object.assign(t, ms[i]); });
    // rebuild each text node that has marks, from its tokens
    var byNode = new Map();
    toks.forEach(function (t) { if (!byNode.has(t.n)) { byNode.set(t.n, []); } byNode.get(t.n).push(t); });
    byNode.forEach(function (ts, n) {
      if (!ts.some(function (t) { return t.ins || t.del || t.after; })) { return; }
      // plain text gathers in buf and goes in as one node before each mark
      var f = document.createDocumentFragment(), at = 0, open = null, buf = '';
      var put = function (e) { if (buf) { f.appendChild(document.createTextNode(buf)); buf = ''; } f.appendChild(e); };
      ts.forEach(function (t) {
        var gap = n.data.slice(at, t.s);
        if (t.del) { open = null; buf += gap; put(run('del', '{{t:removed:}}', t.del)); gap = ' '; }
        if (t.ins) {
          if (open) { open.appendChild(document.createTextNode(gap + t.w)); } else { buf += gap; open = run('ins', '{{t:added:}}', t.w); put(open); }
        } else { open = null; buf += gap + t.w; }
        if (t.after) { buf += ' '; put(run('del', '{{t:removed:}}', t.after)); open = null; }
        at = t.e;
      });
      buf += n.data.slice(at);
      if (buf) { f.appendChild(document.createTextNode(buf)); }
      n.parentNode.replaceChild(f, n);
    });
    return true;
  }
  // the text of a rendered block without the drawn edits and hidden labels
  function cleanText(node) {
    var c = node.cloneNode(true);
    [].forEach.call(c.querySelectorAll('del,.sr,.anchor,.copy'), function (e) { e.remove(); });
    return c.textContent;
  }
  // the text of a block as the reader reads it: no drawn removals, hidden
  // labels or doubled spaces; at maps each character back to [node, offset]
  function readText(el) {
    var text = '', at = [];
    textNodes(el, true).forEach(function (n) {
      for (var i = 0; i < n.data.length; i++) {
        var c = n.data[i], sp = /\s/.test(c);
        if (sp && (!text || text[text.length - 1] === ' ')) { continue; }
        text += sp ? ' ' : c; at.push([n, i]);
      }
    });
    return { text: text, at: at };
  }
  // wraps characters [from, to) of readText(el) in elements made by make()
  function wrapText(el, from, to, make) {
    var at = readText(el).at.slice(from, to), parts = [];
    at.forEach(function (p) {
      var last = parts[parts.length - 1];
      if (last && last[0] === p[0]) { last[2] = p[1] + 1; } else { parts.push([p[0], p[1], p[1] + 1]); }
    });
    parts.forEach(function (p) {
      var r = document.createRange();
      r.setStart(p[0], p[1]); r.setEnd(p[0], p[2]);
      r.surroundContents(make());
    });
  }
  // the place of a fragment in a block's text: the occurrence whose
  // surroundings match the saved prefix and suffix best; -1 when it is gone
  function findFragment(text, c) {
    var best = -1, score = -1, i = text.indexOf(c.fragment);
    while (i >= 0) {
      var p = c.prefix || '', s = c.suffix || '', k = 0, q = 0;
      while (k < p.length && text[i - 1 - k] === p[p.length - 1 - k]) { k++; }
      while (q < s.length && text[i + c.fragment.length + q] === s[q]) { q++; }
      if (k + q > score) { score = k + q; best = i; }
      i = text.indexOf(c.fragment, i + 1);
    }
    return best;
  }

  // ---- history: a timeline over the commits. S is where the edits start,
  // T the moment the text is shown at (the last commit is now) ----
  var last = commits.length - 1;
  function versions(page, ptr) {
    var h = H.docs && H.docs[page.file];
    return (h && h.b && h.b[ptr]) || [];
  }
  function pageChanges(page) {
    var h = H.docs && H.docs[page.file];
    return (h && h.ch) || [];
  }
  // the version of a block at commit k: null when it did not exist yet
  function versionAt(vs, k) {
    var at = null;
    vs.forEach(function (v) { if (v.c <= k) { at = v; } });
    return at;
  }
  // the text of a block at S when it changed between S and T: undefined when
  // it did not change, null when it did not exist at S
  function between(page, ptr, s, t) {
    if (s == null || s >= t) { return undefined; }
    var vs = versions(page, ptr), at = versionAt(vs, t);
    if (!at || at.c <= s) { return undefined; }
    var was = versionAt(vs, s);
    return was ? was.t : null;
  }
  // a block of the page now that did not exist at T
  function absent(page, ptr, t) {
    var vs = versions(page, ptr);
    return t < last && vs.length > 0 && !versionAt(vs, t);
  }
  function blocksOf(page) { return (page.props || []).concat(page.blocks || []).filter(function (b) { return b.p; }); }
  // the text of a block at T, for the lists
  function textAt(page, b, t) { var at = versionAt(versions(page, b.p), t); return at ? at.t : b.s; }
  // Where a removed block stood: it is known by its old pointer only, so it
  // goes before the block that has that pointer now, else after the last
  // block of the same array, else at the end of the page.
  // ponytail: an index guess, exact once history keeps the neighbours of removed blocks
  function placeGhost(page, ptr) {
    var bs = page.blocks || [], parent = ptr.slice(0, ptr.lastIndexOf('/')), last = -1;
    for (var i = 0; i < bs.length; i++) {
      if (bs[i].p === ptr) { return i; }
      if (bs[i].p && bs[i].p.indexOf(parent + '/') === 0) { last = i; }
    }
    return last >= 0 ? last + 1 : bs.length;
  }
  // What the page shows between S and T, in page order: changed and new
  // blocks, ghosts (removed between S and T) and olds (removed after T, so
  // still there at T).
  // ponytail: an old block shows the last text it had, not its text at T
  var deltaCache = {};
  function pageDelta(page, s, t) {
    if (t == null) { t = last; }
    var key = page.id + ':' + s + ':' + t;
    if (deltaCache[key]) { return deltaCache[key]; }
    var d = { items: [], chg: 0, add: 0, rm: 0, ghosts: [], olds: [] }, now = {}, before = {}, live = s != null && s < t;
    blocksOf(page).forEach(function (b) {
      if (!absent(page, b.p, t)) { now[textAt(page, b, t)] = true; }
      var was = live ? between(page, b.p, s, t) : undefined;
      if (was !== undefined) { before[b.p] = was; }
    });
    // late, later: texts of blocks that came after S (or T); they were not there then
    var gone = {}, late = {}, later = {};
    pageChanges(page).forEach(function (c) {
      if (c.c <= (live ? s : t)) { return; }
      var after = c.c > t, l = after ? later : late;
      (c.add || []).forEach(function (x) { l[x[1]] = true; });
      (c.ch || []).forEach(function (x) { if (l[x[1]]) { l[x[2]] = true; } });
      (c.rm || []).forEach(function (x) {
        if (now[x[1]] || gone[x[1]] || l[x[1]]) { return; }
        gone[x[1]] = true;
        var g = { a: page.id + '~' + x[0] + '~' + hash(x[1]), ptr: x[0], t: x[1], at: placeGhost(page, x[0]) };
        if (after) { g.isNew = !!late[x[1]]; d.olds.push(g); } else { d.ghosts.push(g); }
      });
    });
    var item = function (b) {
      if (!(b.p in before)) { return; }
      var isNew = before[b.p] === null;
      d.items.push({ a: page.id + b.p, page: page, ptr: b.p, k: isNew ? 'new' : 'chg', t: textAt(page, b, t) });
      if (isNew) { d.add++; } else { d.chg++; }
    };
    (page.props || []).forEach(item);
    var gh = function (i) {
      d.ghosts.forEach(function (g) { if (g.at === i) { d.items.push({ a: g.a, page: page, ptr: g.ptr, k: 'rm', t: g.t }); d.rm++; } });
      d.olds.forEach(function (g) { if (g.at === i && g.isNew) { d.items.push({ a: g.a, page: page, ptr: g.ptr, k: 'new', t: g.t }); d.add++; } });
    };
    (page.blocks || []).forEach(function (b, i) { gh(i); if (b.p) { item(b); } });
    gh((page.blocks || []).length);
    deltaCache[key] = d;
    return d;
  }
  // every edit between S and T in the order of the pages
  function allChanges(s, t) {
    var list = [];
    pages.forEach(function (p) { list = list.concat(pageDelta(p, s, t).items); });
    return { list: list };
  }
  // the marks of the tree, as the file status of an IDE: per page with edits
  // between S and T its number of edits, and new when the document was
  // created then (a new page may have only headings, so no edits)
  var marksCache = {};
  function pageMarks(list, s, t) {
    var key = s + ':' + t;
    if (marksCache[key]) { return marksCache[key]; }
    var m = {};
    list.forEach(function (it) { (m[it.page.id] = m[it.page.id] || { k: 'chg', n: 0 }).n++; });
    if (s < t) {
      pages.forEach(function (p) {
        if (pageChanges(p).some(function (c) { return c['new'] && c.c > s && c.c <= t; })) { (m[p.id] = m[p.id] || { n: 0 }).k = 'new'; }
      });
    }
    return (marksCache[key] = m);
  }
  // a moment of the timeline for the reader: «3 days ago», the exact date in the title
  function ago(k, narrow) {
    var cm = commits[k];
    if (!cm) { return ''; }
    if (!cm.h) { return '{{t:now, not saved yet}}'; }
    var sec = (new Date(cm.d) - Date.now()) / 1000, a = Math.abs(sec);
    var u = a < 60 ? ['second', 1] : a < 3600 ? ['minute', 60] : a < 86400 ? ['hour', 3600] : a < 2592000 ? ['day', 86400] : a < 31536000 ? ['month', 2592000] : ['year', 31536000];
    try { return intl(Intl.RelativeTimeFormat, { numeric: 'auto', style: narrow ? 'short' : 'long' }).format(Math.round(sec / u[1]), u[0]); } catch (e) { return exact(k); }
  }
  // saves (newest first) by the day they were made: [{ day, ks }]
  function saveDays(ks) {
    var days = [];
    ks.forEach(function (k) {
      var day = fmt(k, { weekday: 'short', day: 'numeric', month: 'long', year: 'numeric' }) || '{{t:now}}';
      if (!days.length || days[days.length - 1].day !== day) { days.push({ day: day, ks: [] }); }
      days[days.length - 1].ks.push(k);
    });
    return days;
  }
  // the last save made on or before a day (yyyy-mm-dd, local time); 0 when none was
  function atDate(v) {
    var end = new Date(v + 'T23:59:59.999'), at = 0;
    commits.forEach(function (c, k) { if (c.h && new Date(c.d) <= end) { at = k; } });
    return at;
  }
  // the day of a save as <input type=date> reads it
  function dayOf(k) {
    var cm = commits[k], d = cm && cm.h && new Date(cm.d), p = function (n) { return (n < 10 ? '0' : '') + n; };
    return d ? d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) : '';
  }
  // the end of the timeline a press at x takes, the thumbs of S and T being at
  // xs and xt: the nearer one; of two closer than a thumb, a press between them
  // waits for its first move (dir < 0 is left: S, dir > 0 right: T) and is null until then
  function thumbFor(x, xs, xt, th, dir) {
    if (xt - xs < th && Math.abs(x - (xs + xt) / 2) < th) { return dir ? (dir < 0 ? 's' : 't') : null; }
    var ds = Math.abs(x - xs), dt = Math.abs(x - xt);
    return ds < dt || (ds === dt && x < xs) ? 's' : 't';
  }
  function sinceLabel(s) { return s >= last ? '{{t:now, edits are not shown}}' : ago(s); }
  function atLabel(t) { return t >= last ? '{{t:now}}' : ago(t); }
  // where a pointer is, for the reader: the start of its text
  function placeOf(page, ptr, fallback) {
    var b = page && blocksOf(page).filter(function (x) { return x.p === ptr; })[0];
    return '«' + snip(b ? b.s : fallback || '', 70) + '»';
  }

  // ---- comments: kept in localStorage until they are imported into the documents ----
  var listeners = [];
  var imported = D.comments || [], inRepo = {};
  imported.forEach(function (c) { inRepo[c.id] = true; });
  // comments already imported into the documents leave the browser
  var kept = store('comments') || [], local = kept.filter(function (c) { return !inRepo[c.id]; });
  if (local.length !== kept.length) { store('comments', local); }
  function setLocal(list) {
    local = list; store('comments', list);
    listeners.forEach(function (f) { f(list); });
  }
  function useComments() {
    var s = useState(local);
    useEffect(function () {
      var f = function (l) { s[1](l); };
      listeners.push(f);
      return function () { listeners.splice(listeners.indexOf(f), 1); };
    }, []);
    return s[0];
  }
  function exportJSON() {
    // version 2 has answers to form fields: an older docc refuses it rather than lose them
    var v = local.some(function (c) { return c.input; }) ? 2 : 1;
    return JSON.stringify({ docc: 'comments', version: v, comments: local.map(function (c) {
      var o = Object.assign({}, c); delete o.sent; return o;
    }) }, null, 2) + '\n';
  }
  function exportText() {
    var lines = ['{{t:Comments on}} «' + D.title + '» — ' + local.length, ''];
    local.forEach(function (c, k) {
      var p = byId[c.doc];
      lines.push((k + 1) + '. ' + (p ? p.title : c.doc) + ' — «' + snip(c.fragment || (c.input ? fieldWords(c.quote) : c.quote), 80) + '»');
      lines.push(c.text);
      if (c.author) { lines.push('— ' + c.author); }
      lines.push('');
    });
    lines.push('{{t:For the import, do not edit:}}');
    lines.push('docc-comments:' + b64(exportJSON()));
    return lines.join('\n') + '\n';
  }
  function sendCopy() {
    copy(exportText()).then(function () { toast('{{t:Copied. Paste it into a message.}}'); },
      function () { toast('{{t:Could not copy: download the file instead.}}'); });
  }
  function sendFile() {
    save('comments-' + new Date().toISOString().slice(0, 10) + '.json', 'application/json', exportJSON());
  }
  function removeComment(c) {
    var at = local.indexOf(c);
    setLocal(local.filter(function (x) { return x.id !== c.id; }));
    // put back only this comment: others may have been added or removed since
    toast('{{t:Comment deleted.}}', '{{t:Undo}}', function () {
      var rest = local.slice(); rest.splice(Math.max(0, Math.min(at, rest.length)), 0, c); setLocal(rest);
    });
  }
  // The paragraph of a comment. A pointer is a place in an array: a paragraph
  // inserted above moves the words commented in an older build down by one,
  // so such a comment goes to the nearest paragraph that has its words.
  // Imported comments are moved by the build itself.
  var pathCache = {};
  function pathOf(c) {
    // an answer stands at its form field, wherever the field went
    if (c.input) {
      var f = byId[c.doc] && blocksOf(byId[c.doc]).filter(function (b) { return b.i && b.i.id === c.input; })[0];
      return f ? f.p : c.path;
    }
    if (!c.fragment || inRepo[c.id] || !c.build || c.build === H.head) { return c.path; }
    var k = c.id + ':' + c.path;
    if (k in pathCache) { return pathCache[k]; }
    var bs = byId[c.doc] ? blocksOf(byId[c.doc]) : [], has = function (b) { return findFragment(plain(b.s).replace(/\s+/g, ' '), c) >= 0; };
    var at = -1, best = c.path;
    bs.forEach(function (b, i) { if (b.p === c.path) { at = i; } });
    if (at < 0 || !has(bs[at])) {
      var d = Infinity;
      bs.forEach(function (b, i) { if (Math.abs(i - at) < d && has(b)) { d = Math.abs(i - at); best = b.p; } });
    }
    return (pathCache[k] = best);
  }
  // the comments of a block as cards; answers are shown by their field
  function commentsOn(list, doc, ptr) { return list.filter(function (c) { return c.doc === doc && !c.input && !c.lost && pathOf(c) === ptr; }); }
  // Where a comment stands now: its page and paragraph, or which of them is
  // gone (docc marks a comment of the documents that lost its place as lost).
  function commentPlace(c) {
    var p = byId[c.doc], ptr = pathOf(c);
    if (!p) { return { gone: 'page' }; }
    var b = ptr && blocksOf(p).filter(function (x) { return x.p === ptr; })[0];
    // an answer stands at its field wherever the field went
    if ((ptr && !b) || (c.lost && !(c.input && b && b.i && b.i.id === c.input))) { return { page: p, gone: 'block' }; }
    return { page: p, ptr: ptr };
  }
  // the comments of a page that lost their place, shown at its top
  function lostOn(list, doc) { return list.filter(function (c) { return c.doc === doc && commentPlace(c).gone === 'block'; }); }

  var Rec = window.SpeechRecognition || window.webkitSpeechRecognition;
  var speechLang = D.lang === 'ru' ? 'ru-RU' : 'en-US';

  // press to dictate, press again to stop; the words go into the text box as
  // they come. The browser ends the recognition after a pause, so it is
  // started again until the second press.
  function Mic(props) {
    var rec = useRef(null), want = useRef(false), st = useState(''), on = useState(false);
    if (!Rec) { return null; }
    function stop(msg) {
      want.current = false; on[1](false); props.listen(false);
      if (msg) { st[1](msg); }
    }
    function start() {
      var base = props.get(), r = new Rec();
      r.lang = speechLang; r.interimResults = true; r.continuous = true;
      r.onresult = function (ev) {
        var s = '';
        for (var i = 0; i < ev.results.length; i++) { s += ev.results[i][0].transcript; }
        props.set((base ? base.replace(/\s*$/, ' ') : '') + s.trim());
      };
      r.onerror = function (ev) {
        if (ev.error === 'no-speech' || ev.error === 'aborted') { return; } // a pause: onend starts it again
        stop(ev.error === 'not-allowed' ? '{{t:No access to the microphone}}' :
          ev.error === 'network' ? '{{t:Speech service unreachable}}' : '{{t:Dictation failed, type the text instead}}');
      };
      r.onend = function () { rec.current = null; if (want.current) { start(); } else { stop(); } };
      try { r.start(); rec.current = r; on[1](true); props.listen(true); st[1](''); } catch (err) { stop('{{t:Dictation failed, type the text instead}}'); }
    }
    useEffect(function () { return function () { want.current = false; if (rec.current) { rec.current.abort(); } }; }, []);
    function toggle() {
      if (want.current) { want.current = false; if (rec.current) { rec.current.stop(); } else { stop(); } } else { want.current = true; start(); }
    }
    return html`<span class="mic-w">
      <button type="button" class=${'btn mic' + (on[0] ? ' on' : '')} aria-pressed=${on[0] ? 'true' : 'false'} onClick=${toggle}>
        ${on[0] ? '● {{t:Recording, press to stop}}' : '🎤 {{t:Dictate}}'}</button>
      ${st[0] && html`<small class="err">${st[0]}</small>`}
    </span>`;
  }

  var draft = false; // an editor is open with text not saved yet
  function leaveDraft() { return !draft || confirm('{{t:Discard the unsaved comment?}}'); }
  function Editor(props) {
    var edit = props.edit, frag = edit || props.frag || {};
    var text = useState(edit ? edit.text : ''), author = useState(edit ? edit.author || '' : store('author') || ''), ta = useRef(null), who = useRef(null);
    var cur = useRef(''); cur.current = text[0];
    var listening = useState(false); // dictation rewrites the text, so typing waits
    draft = text[0].trim() !== (edit ? edit.text : '');
    useEffect(function () { return function () { draft = false; }; }, []);
    function cancel() { if (leaveDraft()) { props.close(); } }
    // a reader who skipped the name gives it here first
    useEffect(function () {
      var f = author[0] ? ta.current : who.current;
      if (f) { f.focus({ preventScroll: true }); f.closest('.editor').scrollIntoView({ block: 'nearest' }); }
    }, []);
    function ok() {
      var t = text[0].trim(), a = author[0].trim();
      if (!t) { return; }
      if (a) { store('author', a); }
      if (edit) {
        setLocal(local.map(function (c) { return c.id === edit.id ? Object.assign({}, c, { text: t, author: a }) : c; }));
      } else {
        var first = !local.length, c = { id: uuid(), doc: props.page.id, path: props.ptr, quote: props.quote, text: t,
          author: a, created: new Date().toISOString(), build: H.head || '' };
        if (props.frag) { c.fragment = props.frag.fragment; c.prefix = props.frag.prefix; c.suffix = props.frag.suffix; }
        setLocal(local.concat([c]));
        // nothing leaves the file by itself: say so once
        if (first) { toast('{{t:Saved in this browser only. Copy your comments in the Comments tab to pass them on.}}'); }
      }
      draft = false;
      props.close();
    }
    return html`<div class="editor">
      ${frag.fragment && html`<blockquote class="fq">${frag.fragment}</blockquote>`}
      <label class="who-l">{{t:From}} <input ref=${who} class="who" autocomplete="name" enterkeyhint="next" placeholder="{{t:Your name}}" value=${author[0]}
        onInput=${function (e) { author[1](e.target.value); }} /></label>
      <textarea ref=${ta} rows="3" aria-label="{{t:Comment}}" placeholder="{{t:Your comment}}" value=${text[0]} readOnly=${listening[0]}
        onInput=${function (e) {
          text[1](e.target.value); e.target.style.height = 'auto'; e.target.style.height = e.target.scrollHeight + 2 + 'px';
          // the box grows up to its limit: Save stays in sight under it
          e.target.nextElementSibling.scrollIntoView({ block: 'nearest' });
        }}
        onKeyDown=${function (e) {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { ok(); }
          if (e.key === 'Escape') { e.stopPropagation(); cancel(); }
        }}></textarea>
      <div class="row">
        <${Mic} get=${function () { return cur.current; }} set=${text[1]} listen=${listening[1]} />
        <span class="sp"></span>
        <button type="button" class="btn" onClick=${cancel}>{{t:Cancel}}</button>
        <button type="button" class="btn pri" onClick=${ok} disabled=${!text[0].trim()}>{{t:Save}}</button>
      </div>
    </div>`;
  }

  // one comment as a card on the margin (or under the paragraph on a narrow screen)
  function Card(props) {
    var c = props.c, edit = useState(false);
    if (edit[0]) { return html`<${Editor} edit=${c} close=${function () { edit[1](false); }} />`; }
    return html`<aside class=${'card' + (props.active ? ' on' : '')} aria-label="{{t:Comment}}" data-c=${c.id} tabindex="-1">
      ${c.fragment && html`<div class="fq">${props.lost ? html`<i>{{t:Fragment changed}}:</i> ` : ''}${c.fragment}</div>`}
      <div class="nt">${c.text}</div>
      <div class="meta"><b>${c.author || '{{t:Anonymous}}'}</b>
        <small>${c.created ? new Date(c.created).toLocaleDateString() : ''}</small>
        ${!props.repo && html`<span class="acts">
          <button type="button" class="link" onClick=${function () { edit[1](true); }}>{{t:Edit}}</button>
          <button type="button" class="link" onClick=${function () { removeComment(c); }}>{{t:Delete}}</button></span>`}
      </div>
    </aside>`;
  }

  // ---- form fields: an answer is a comment with the id of its field ----
  // the answers to a field, the latest of each author (one without a name
  // stands by itself), oldest first
  function answersFor(list, doc, id) {
    var by = {};
    list.forEach(function (c) {
      if (c.doc !== doc || c.input !== id) { return; }
      var k = c.author ? 'a:' + c.author : 'c:' + c.id;
      if (!by[k] || (c.created || '') >= (by[k].created || '')) { by[k] = c; }
    });
    return Object.keys(by).map(function (k) { return by[k]; })
      .sort(function (a, b) { return (a.created || '') < (b.created || '') ? -1 : 1; });
  }
  // a text box, radio buttons or check boxes in the text; the answers given
  // so far below it, with their authors
  function Field(props) {
    var f = props.f, doc = props.page.id, me = store('author') || '';
    var given = answersFor(imported.concat(props.comments), doc, f.id);
    // what this reader answered: in this browser, else imported under their name
    var own = props.comments.filter(function (c) { return c.doc === doc && c.input === f.id; })[0] ||
      (me && given.filter(function (c) { return c.author === me; })[0]);
    var start = (own && own.answer || []).filter(function (a) { return f.t === 'text' || f.options.indexOf(a) >= 0; });
    var val = useState(start), who = useState(me), v = val[0];
    useEffect(function () { val[1](start); }, [own ? own.id : '']);
    var dirty = JSON.stringify(v) !== JSON.stringify(start), ok = f.t === 'text' ? !!(v[0] || '').trim() : v.length > 0;
    var label = plain(f.label), name = 'f-' + doc + '-' + f.id;
    function pick(o, on) {
      val[1](f.t === 'radio' ? [o] : f.options.filter(function (x) { return x === o ? on : v.indexOf(x) >= 0; }));
    }
    function save() {
      var a = f.t === 'text' ? [v[0].trim()] : v, w = who[0].trim(), first = !local.length;
      if (w) { store('author', w); }
      var c = { id: uuid(), doc: doc, path: props.ptr, quote: props.quote, text: a.join(', '), input: f.id, answer: a,
        author: w, created: new Date().toISOString(), build: H.head || '' };
      // one answer of this browser per field: a new one takes its place
      setLocal(local.filter(function (x) { return !(x.doc === doc && x.input === f.id); }).concat([c]));
      toast(first ? '{{t:Saved in this browser only. Copy your comments in the Comments tab to pass them on.}}' : '{{t:Answer saved.}}');
    }
    return html`<fieldset class="fld" disabled=${props.past}>
      <legend>${label}</legend>
      ${f.t === 'text' ? html`<textarea rows="3" aria-label=${label} value=${v[0] || ''} onInput=${function (e) { val[1]([e.target.value]); }}></textarea>`
        : f.options.map(function (o) {
          return html`<label class="opt"><input type=${f.t} name=${name} checked=${v.indexOf(o) >= 0}
            onChange=${function (e) { pick(o, e.target.checked); }} /> ${plain(o)}</label>`;
        })}
      ${!me && html`<label class="who-l">{{t:From}} <input class="who" autocomplete="name" placeholder="{{t:Your name}}" value=${who[0]}
        onInput=${function (e) { who[1](e.target.value); }} /></label>`}
      ${!props.past && html`<div class="row"><span class="sp"></span>
        ${dirty && own && html`<button type="button" class="btn" onClick=${function () { val[1](start); }}>{{t:Cancel}}</button>`}
        <button type="button" class="btn pri" disabled=${!dirty || !ok} onClick=${save}>{{t:Save}}</button></div>`}
      ${given.length > 0 && html`<ul class="fans" aria-label="{{t:Answers}}">${given.map(function (c) {
        return html`<li><b>${c.author || '{{t:Anonymous}}'}</b> — ${c.text}
          ${c.quote !== props.quote && html` <span class="chip">{{t:to an earlier question}}</span>`}
          ${!inRepo[c.id] && html` <span class="muted">· {{t:in this browser}} · </span><button type="button" class="link" onClick=${function () { removeComment(c); }}>{{t:Delete}}</button>`}</li>`;
      })}</ul>`}
    </fieldset>`;
  }

  // ---- one block: its HTML at T, the edits since S drawn into it, its comments ----
  function Block(props) {
    var b = props.b, page = props.page, ptr = b.p, s = props.s, t = props.t, past = t < last, a = page.id + (ptr || '');
    var body = useRef(null), whole = useState(null), lost = useState({}), more = useState(false);
    var at = past && ptr ? versionAt(versions(page, ptr), t) : null, h = b.h;
    if (at && at.t !== b.s) {
      // tables, lists and objects have no old HTML: their old text is shown plain
      h = at.h || (props.label ? '' : '<p>') + esc(plain(at.t)).replace(/\n/g, '<br>') + (props.label ? '' : '</p>');
    }
    var before = ptr ? between(page, ptr, s, t) : undefined;
    var changed = before !== undefined, isNew = before === null;
    var mine = ptr && !past ? commentsOn(props.comments, page.id, ptr) : [];
    var repo = ptr && !past ? commentsOn(imported, page.id, ptr) : [];
    var frags = mine.concat(repo).filter(function (c) { return c.fragment; });
    var key = s + ':' + t + ':' + frags.map(function (c) { return c.id; }).join();
    // a form field is drawn as a field at its current text; an old text of it reads as text
    var field = b.i && !(at && at.t !== b.s);
    useEffect(function () {
      var el = body.current, gone = {};
      whole[1](field && changed && !isNew ? 'text' : null);
      if (!el || field) { lost[1](gone); return; }
      if (changed && !isNew && !drawDiff(el, before)) { whole[1](el.querySelector('pre') ? 'code' : 'text'); }
      var text = readText(el).text;
      frags.forEach(function (c) {
        var i = findFragment(text, c);
        if (i < 0) { gone[c.id] = true; return; }
        wrapText(el, i, i + c.fragment.length, function () {
          var m = document.createElement('mark'); m.className = 'frag'; m.setAttribute('data-c', c.id); return m;
        });
      });
      lost[1](gone);
    }, [key]);
    var inner = field ? html`<div class="blk-body" key=${key} id=${props.id}><${Field} f=${b.i} page=${page} ptr=${ptr} quote=${b.s} comments=${props.comments} past=${past} /></div>`
      : html`<div class="blk-body" key=${key} ref=${body} id=${props.id} dangerouslySetInnerHTML=${{ __html: h }}></div>`;
    if (props.label) { inner = html`<div class="blk-body prop" key=${key} id=${props.id}><div class="pk">${props.label}</div><div ref=${body} dangerouslySetInnerHTML=${{ __html: h }}></div></div>`; }
    if (!ptr) { return html`<div class=${'blk' + (props.label ? ' is-prop' : '')}>${inner}</div>`; }
    var cards = repo.map(function (c) { return [c, true]; }).concat(mine.map(function (c) { return [c, false]; }));
    // a comment opened from the list is never folded away
    var unfold = more[0] || cards.length <= 2 || cards.slice(2).some(function (x) { return x[0].id === props.activeC; });
    var shown = unfold ? cards : cards.slice(0, 2);
    var cls = 'blk' + (props.label ? ' is-prop' : '') + (changed ? (isNew ? ' new' : ' chg') : '') + (props.cur === a ? ' cur' : '') + (cards.length ? ' has-c' : '');
    return html`<div class=${cls} data-p=${ptr} data-a=${a} tabindex="-1">
      ${changed && html`<span class="sr">${isNew ? '{{t:new paragraph}}' : '{{t:changed paragraph}}'}</span>`}
      ${whole[0] && html`<details class="old" open=${whole[0] === 'code'}><summary>{{t:Previous text}}</summary><div class="dv"><del>${plain(before)}</del></div></details>`}
      ${inner}
      <div class="blk-side">
        ${shown.map(function (x) { return html`<${Card} key=${x[0].id} c=${x[0]} repo=${x[1]} lost=${lost[0][x[0].id]} active=${props.activeC === x[0].id} />`; })}
        ${!unfold && html`<button type="button" class="link" onClick=${function () { more[1](true); }}>{{t:More comments:}} ${cards.length - 2}</button>`}
        ${props.editor && props.editor.ptr === ptr && props.editor.page === page && html`<${Editor} page=${page} ptr=${ptr} quote=${b.s} frag=${props.editor.frag} close=${props.closeEditor} />`}
      </div>
    </div>`;
  }

  // a paragraph removed between S and T, in its old place, folded
  function Ghost(props) {
    var g = props.g;
    return html`<details class=${'ghost' + (props.cur === g.a ? ' cur' : '')} data-a=${g.a} tabindex="-1">
      <summary><span class="sr">{{t:removed paragraph}}. </span>{{t:Removed paragraph:}} “${snip(g.t, 60)}”</summary>
      <div class="dv"><del>${plain(g.t)}</del></div>
    </details>`;
  }
  // a paragraph removed after T: at T it was still there
  function Old(props) {
    var g = props.g;
    return html`<div class=${'blk' + (g.isNew ? ' new' : '') + (props.cur === g.a ? ' cur' : '')} data-a=${g.a} tabindex="-1">
      ${g.isNew && html`<span class="sr">{{t:new paragraph}}</span>`}<div class="blk-body"><p class="dv">${plain(g.t)}</p></div></div>`;
  }

  // The timeline: the saved versions on one track with two ends, S where the
  // edits start and T the moment the text is shown at, and below it the
  // versions by day, newest first, a day folded to one line. A version picked
  // in the list shows what it changed (S just before it, T at it), as in the
  // version history of Google Docs. The points are spaced evenly: saves come
  // in bursts, so a track by time would pile them up. A long history on touch
  // has no track: a finger cannot pick one of thousands of saves on it, the
  // days and the date can.
  var ROWS = 15, LONG = 120;
  function Timeline(props) {
    var s = props.s, t = props.t, touched = {}, mine = 0;
    pageChanges(props.page).forEach(function (c) { if (!touched[c.c]) { touched[c.c] = true; mine++; } });
    var only = useState(last > LONG && mine > 0), open = useState({}), more = useState({}), nDays = useState(ROWS), drag = useRef(null);
    var pc = function (k) { return (k / last * 100).toFixed(2) + '%'; };
    // a range input that was clamped keeps the clamped value on screen
    function input(fn) { return function (e) { var v = fn(+e.target.value); if (v !== +e.target.value) { e.target.value = v; } }; }
    // The thumbs are dragged here, not by the range inputs (they stay for the
    // keyboard): a press takes the nearer thumb, also on the bare track, and
    // two thumbs on one place come apart by the first move, by mouse or finger.
    function geo(el) {
      var r = el.getBoundingClientRect(), th = parseFloat(getComputedStyle(el).getPropertyValue('--th')) || 18;
      return { x: function (v) { return r.left + th / 2 + (last ? v / last : 0) * (r.width - th); }, th: th,
        k: function (x) { return Math.round(Math.min(1, Math.max(0, (x - r.left - th / 2) / (r.width - th))) * last); } };
    }
    function move(end, k) {
      if (end === 's') { k = Math.min(k, t); if (k !== s) { props.setRange(k, t); } }
      else if (k !== t) { props.setRange(Math.min(s, k), k); }
    }
    function down(e) {
      if (e.button) { return; }
      var g = geo(e.currentTarget), end = thumbFor(e.clientX, g.x(s), g.x(t), g.th, 0);
      e.preventDefault();
      try { e.currentTarget.setPointerCapture(e.pointerId); } catch (err) { /* a pointer the browser does not know: no capture */ }
      drag.current = { end: end, x0: e.clientX, g: g };
      if (end) { e.currentTarget.querySelector(end === 's' ? '.tl-s' : '.tl-t').focus({ preventScroll: true }); move(end, g.k(e.clientX)); }
    }
    function drift(e) {
      var d = drag.current;
      if (!d) { return; }
      if (!d.end) {
        if (Math.abs(e.clientX - d.x0) < 3) { return; }
        d.end = thumbFor(e.clientX, d.g.x(s), d.g.x(t), d.g.th, e.clientX - d.x0);
      }
      move(d.end, d.g.k(e.clientX));
    }
    function up() { drag.current = null; }
    function pick(a, b) { props.setRange(a, b); props.done(); }
    var ks = [];
    for (var k = last; k >= 0; k--) { if (!only[0] || touched[k] || k === t || k === last) { ks.push(k); } }
    var days = saveDays(ks), atDay = 0;
    days.forEach(function (d, i) { if (d.ks.indexOf(t) >= 0) { atDay = i; } });
    var shown = days.slice(0, Math.max(nDays[0], atDay + 1));
    // ponytail: with very many saves only the ones of this page get a mark
    var marks = ks.filter(function (k) { return last <= LONG || touched[k]; });
    var track = !(coarse && last > LONG);
    return html`<div class="tl" role="group" aria-label="{{t:Timeline}}">
      <div class="tl-top">
        <div class="tl-h">
          <span class="tl-s-l">{{t:Edits since:}} <b>${sinceLabel(s)}</b><small>${s < last ? exact(s) : ''}</small></span>
          <span class="tl-t-l">{{t:Text as of:}} <b>${atLabel(t)}</b><small>${exact(t)}</small></span>
          <button type="button" class="btn sm tl-x" popovertarget="when" popovertargetaction="hide">✕ {{t:Close}}</button>
        </div>
        ${track && html`<div class="tl-r" onPointerDown=${down} onPointerMove=${drift} onPointerUp=${up} onPointerCancel=${up}>
          <div class="tl-in">
            <div class="tl-line"></div>
            <div class="tl-sel" style=${'left:' + pc(s) + ';width:' + pc(t - s)}></div>
            ${marks.map(function (k) { return html`<span class=${'tl-k' + (touched[k] ? ' on' : '')} style=${'left:' + pc(k)} title=${versionLine(k)}></span>`; })}
          </div>
          <input type="range" class="tl-s" min="0" max=${last} step="1" value=${s} aria-label="{{t:Edits since:}}" aria-valuetext=${sinceLabel(s) + ', ' + exact(s)}
            onInput=${input(function (v) { v = Math.min(v, t); props.setRange(v, t); return v; })} />
          <input type="range" class="tl-t" min="0" max=${last} step="1" value=${t} aria-label="{{t:Text as of:}}" aria-valuetext=${atLabel(t) + ', ' + exact(t)}
            onInput=${input(function (v) { props.setRange(Math.min(s, v), v); return v; })} />
        </div>`}
        <div class="tl-f">
          <label class="tl-d">{{t:Text as of the date}} <input type="date" min=${dayOf(0)} max=${dayOf(last) || dayOf(last - 1)} value=${t < last ? dayOf(t) : ''}
            onChange=${function (e) { if (e.target.value) { var k = atDate(e.target.value); pick(Math.min(s, k), k); } }} /></label>
          ${mine > 0 && html`<label class="tl-o"><input type="checkbox" checked=${only[0]} onChange=${function (e) { only[1](e.target.checked); }} />
            {{t:Only saves that changed this page}} (${mine})</label>`}
        </div>
      </div>
      <p class="tl-hint">{{t:Pick a save to see what it changed. Marked saves changed this page; the shaded ones are drawn in the text.}}</p>
      <ol class="tl-days">${shown.map(function (d, i) {
        var isOpen = open[0][d.day] != null ? open[0][d.day] : i === atDay || i === 0, n = Math.max(more[0][d.day] || ROWS, d.ks.indexOf(t) + 1);
        var here = d.ks.filter(function (k) { return touched[k]; }).length;
        return html`<li><details open=${isOpen} onToggle=${function (e) { if (e.target.open !== isOpen) { var o = Object.assign({}, open[0]); o[d.day] = e.target.open; open[1](o); } }}>
          <summary><b>${d.day}</b><span class="tl-n">{{t:Saves:}} ${d.ks.length}${here > 0 && !only[0] ? ' · {{t:this page}}: ' + here : ''}</span></summary>
          ${isOpen && html`<ul>${d.ks.slice(0, n).map(function (k) {
            var cm = commits[k];
            return html`<li class=${'tl-v' + (k > s && k <= t ? ' in' : '') + (k === t ? ' at' : '')}>
              <button type="button" class="tl-go" aria-current=${k === t ? 'true' : undefined} onClick=${function () { pick(Math.max(0, k - 1), k); }}>
                <b>${fmt(k, { timeStyle: 'short' }) || '{{t:not saved yet}}'}</b>
                <span class="tl-ago">${ago(k)}</span>
                ${touched[k] && html`<span class="tl-pg">{{t:this page}}</span>`}
                ${(cm.a || cm.m) && html`<span class="tl-msg">${[cm.a, cm.m].filter(Boolean).join(' · ')}</span>`}
              </button>
              ${k === t && k < last && html`<button type="button" class="btn sm tl-from" onClick=${function () { pick(k, last); }}>{{t:Edits after this save}}</button>`}
            </li>`;
          })}</ul>
          ${d.ks.length > n && html`<button type="button" class="more" onClick=${function () { var m = Object.assign({}, more[0]); m[d.day] = n + ROWS; more[1](m); }}>
            {{t:Show more:}} ${Math.min(ROWS, d.ks.length - n)} {{t:of}} ${d.ks.length - n}</button>`}`}
        </details></li>`;
      })}</ol>
      ${days.length > shown.length && html`<button type="button" class="more" onClick=${function () { nDays[1](shown.length + ROWS); }}>
        {{t:Show more:}} ${Math.min(ROWS, days.length - shown.length)} {{t:of}} ${days.length - shown.length}</button>`}
    </div>`;
  }
  // the timeline in the header: a short summary that opens it
  function When(props) {
    var s = props.s, t = props.t, past = t < last, pop = useRef(null);
    // a long list is drawn only while it is open: drawing it on every step of the page is slow
    var open = useState(last <= LONG || !('popover' in HTMLElement.prototype));
    // on a narrow screen the sheet covers the text: it goes away after a pick
    function done() { if (mq('(max-width: 820px)') && pop.current.hidePopover) { pop.current.hidePopover(); } }
    // a moment in the past reads by its exact time: «3 days ago → 3 days ago» says nothing
    var short = function (k) { return fmt(k, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }); };
    var to = past ? short(t) : '{{t:now}}', from = past && ago(s, true) === ago(t, true) ? short(s) : ago(s, true);
    // one save picked in the list, or a date, reads as that moment
    var sum = s >= last ? '{{t:edits are not shown}}' : past && s >= t - 1 ? to : from + ' → ' + to;
    return html`<button type="button" class=${'btn when-b' + (past ? ' past' : '')} popovertarget="when" title=${'{{t:Timeline}}: ' + sinceLabel(s) + ' → ' + atLabel(t)}>
        <span aria-hidden="true">🕘</span><span class="when-l">{{t:Edits:}}</span><span class="when-s">${sum}</span><span aria-hidden="true">▾</span></button>
      <div id="when" popover="auto" class="pop when" ref=${pop} onToggle=${function (e) { if (last > LONG) { open[1](e.newState === 'open'); } }}>
        ${open[0] && html`<${Timeline} ...${props} done=${done} />`}</div>`;
  }

  // ---- page ----
  // On a wide screen the cards hang beside their paragraph without making it
  // taller; a card that would cover the one above moves down below it.
  function layoutCards(root) {
    var low = -Infinity;
    [].forEach.call(root.querySelectorAll('.blk-side'), function (s) {
      s.style.transform = '';
      if (getComputedStyle(s).height !== '0px') { return; } // narrow: under the paragraph
      var h = s.scrollHeight;
      if (!h) { return; }
      var top = s.getBoundingClientRect().top, shift = Math.max(0, low + 8 - top);
      if (shift) { s.style.transform = 'translateY(' + shift + 'px)'; }
      low = top + shift + h;
    });
  }

  function Page(props) {
    var page = props.page, main = useRef(null), s = props.s, t = props.t, past = t < last;
    // heading anchors and copy buttons inside the rendered Markdown
    useEffect(function () {
      var el = main.current;
      [].forEach.call(el.querySelectorAll('.blk-body :is(h2,h3,h4)[id]'), function (h) {
        if (h.querySelector('.anchor')) { return; }
        var a = document.createElement('a');
        a.className = 'anchor'; a.href = '#' + h.id; a.textContent = '#';
        a.setAttribute('aria-label', '{{t:Link to this section}}');
        h.insertBefore(a, h.firstChild);
      });
      // a wide table scrolls from the keyboard too, named by its heading for a screen reader
      [].forEach.call(el.querySelectorAll('.blk-body table'), function (t) {
        if (t.scrollWidth <= t.clientWidth) { return; }
        var hs = [].filter.call(el.querySelectorAll('.blk-body :is(h2,h3,h4)'), function (h) { return h.compareDocumentPosition(t) & 4; });
        t.setAttribute('tabindex', '0');
        t.setAttribute('aria-label', '{{t:Table}}' + (hs.length ? ': ' + hs[hs.length - 1].textContent.replace(/^#/, '') : ''));
      });
      [].forEach.call(el.querySelectorAll('.blk-body pre'), function (pre) {
        if (pre.querySelector('.copy')) { return; }
        pre.setAttribute('tabindex', '0');
        var b = document.createElement('button');
        b.type = 'button'; b.className = 'copy'; b.textContent = '{{t:Copy}}';
        b.addEventListener('click', function () {
          var code = pre.querySelector('code'), text = (code || pre).textContent;
          var done = function (m) { b.textContent = m; setTimeout(function () { b.textContent = '{{t:Copy}}'; }, 1500); };
          copy(text).then(function () { done('{{t:Copied}}'); }, function () { done('{{t:Failed}}'); });
        });
        pre.appendChild(b);
      });
      layoutCards(el);
      var frame = 0, relayout = function () { cancelAnimationFrame(frame); frame = requestAnimationFrame(function () { layoutCards(el); }); };
      var mo = window.MutationObserver && new MutationObserver(relayout);
      if (mo) { mo.observe(el, { childList: true, subtree: true }); }
      window.addEventListener('resize', relayout);
      document.addEventListener('toggle', relayout, true);
      document.addEventListener('input', relayout, true);
      props.onToc([].map.call(el.querySelectorAll('.blk-body :is(h2,h3)[id]'), function (h) {
        return { id: h.id, level: h.tagName.charAt(1), text: h.textContent.replace(/^#/, '') };
      }));
      return function () {
        if (mo) { mo.disconnect(); }
        cancelAnimationFrame(frame);
        window.removeEventListener('resize', relayout); document.removeEventListener('toggle', relayout, true); document.removeEventListener('input', relayout, true);
      };
    });
    var prev = pages[page.i - 1], next = pages[page.i + 1], d = pageDelta(page, s, t), lost = lostOn(imported.concat(props.comments), page.id);
    var bl = { comments: props.comments, s: s, t: t, page: page, cur: props.cur, editor: props.editor, closeEditor: props.closeEditor, activeC: props.activeC };
    function extra(i) {
      return d.ghosts.filter(function (g) { return g.at === i; }).map(function (g) { return html`<${Ghost} key=${g.a} g=${g} cur=${props.cur} />`; })
        .concat(d.olds.filter(function (g) { return g.at === i; }).map(function (g) { return html`<${Old} key=${g.a} g=${g} cur=${props.cur} />`; }));
    }
    var here = function (b) { return !b.p || !absent(page, b.p, t); };
    var ps = (page.props || []).filter(here);
    return html`<article class="page" ref=${main}>
      ${D.brand.print && html`<div dangerouslySetInnerHTML=${{ __html: D.brand.print }}></div>`}
      ${props.missing && html`<p class="notice" role="alert">{{t:Nothing is found at this link:}} «${props.missing}». {{t:The first page is shown; find the place in the tree or the search.}}</p>`}
      ${page.crumbs && html`<nav class="crumbs" aria-label="{{t:Breadcrumb}}">${page.crumbs.map(function (id, k) {
        return html`${k > 0 && html`<span aria-hidden="true">›</span>`}<a href=${'#' + id}>${byId[id] ? byId[id].title : id}</a>`;
      })}</nav>`}
      ${past && html`<p class="notice past" role="status">{{t:You are looking at the page as it was}} ${ago(t)} (${exact(t)}). {{t:Comments can be left on the current text only.}}
        <button type="button" class="btn sm" onClick=${function () { props.setT(last); }}>{{t:Back to now}}</button></p>`}
      <h1>${page.title}</h1>
      ${page.hidden && html`<p class="badges"><span class="chip bad">{{t:hidden}}</span></p>`}
      ${!past && lost.length > 0 && html`<aside class="notice lostc" aria-label="{{t:Comments that lost their place:}}"><b>💬 {{t:Comments that lost their place:}}</b>
        <ul class="clist">${lost.map(function (c) {
          return html`<li><span class="cp">«${snip(c.fragment || (c.input ? fieldWords(c.quote) : c.quote), 70)}»</span> <span class="ctx">${c.text}</span>
            <span class="cp">— ${c.author || '{{t:Anonymous}}'}</span></li>`;
        })}</ul></aside>`}
      <div class="doc">
        ${ps.length > 0 && html`<div class="props">${ps.map(function (p) {
          return html`<${Block} ...${bl} key=${p.p || p.k} b=${p} label=${p.k} id=${p.id} />`;
        })}</div>`}
        ${(page.blocks || []).map(function (b, i) {
          return html`${extra(i)}${here(b) && html`<${Block} ...${bl} key=${b.p || 'x' + i} b=${b} />`}`;
        })}
        ${extra((page.blocks || []).length)}
      </div>
      ${page.kids && html`<h2 class="rh">{{t:Pages in this section}}</h2><div class="rows">${page.kids.map(function (id) {
        var k = byId[id];
        return k && html`<a class="row" href=${'#' + id}><span class="ct">${k.title}</span>${k.summary && html`<span class="cs">${k.summary}</span>`}</a>`;
      })}</div>`}
      ${(prev || next) && html`<nav class="pn" aria-label="{{t:Neighbouring pages}}">
        ${prev && html`<a class="prev" href=${'#' + prev.id}><small>{{t:Previous}}</small><span>${prev.title}</span></a>`}
        ${next && html`<a class="next" href=${'#' + next.id}><small>{{t:Next}}</small><span>${next.title}</span></a>`}
      </nav>`}
      ${D.brand.foot && html`<div dangerouslySetInnerHTML=${{ __html: D.brand.foot }}></div>`}
    </article>`;
  }

  // ---- left column: tree, edits, comments; search on top ----
  // icons of the tree (Lucide, ISC): a page by the usual kinds, any other
  // kind a plain file, a heading of the open page #
  var ICONS = {
    page: '<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>',
    decision: '<path d="M12 3v18"/><path d="m19 8 3 8a5 5 0 0 1-6 0zV7"/><path d="M3 7h1a17 17 0 0 0 8-2 17 17 0 0 0 8 2h1"/><path d="m5 8 3 8a5 5 0 0 1-6 0zV7"/><path d="M7 21h10"/>',
    requirement: '<path d="M13 5h8"/><path d="M13 12h8"/><path d="M13 19h8"/><path d="m3 17 2 2 4-4"/><path d="m3 7 2 2 4-4"/>',
    goal: '<circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/>',
    role: '<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
    scenario: '<circle cx="6" cy="19" r="3"/><path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15"/><circle cx="18" cy="5" r="3"/>',
    screen: '<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>',
    story: '<path d="M12 5v16"/><path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z"/>',
    question: '<circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/>',
    note: '<path d="M21 9a2.4 2.4 0 0 0-.706-1.706l-3.588-3.588A2.4 2.4 0 0 0 15 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2z"/><path d="M15 3v5a1 1 0 0 0 1 1h5"/>',
    overview: '<path d="M14.106 5.553a2 2 0 0 0 1.788 0l3.659-1.83A1 1 0 0 1 21 4.619v12.764a1 1 0 0 1-.553.894l-4.553 2.277a2 2 0 0 1-1.788 0l-4.212-2.106a2 2 0 0 0-1.788 0l-3.659 1.83A1 1 0 0 1 3 19.381V6.618a1 1 0 0 1 .553-.894l4.553-2.277a2 2 0 0 1 1.788 0z"/><path d="M15 5.764v15"/><path d="M9 3.236v15"/>',
    file: '<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/>',
    folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
    '#': '<path d="M4 9h16"/><path d="M4 15h16"/><path d="M10 3 8 21"/><path d="M16 3l-2 18"/>'
  };
  ICONS.spec = ICONS.requirement;
  function Icon(props) {
    return html`<span class="ic" aria-hidden="true" dangerouslySetInnerHTML=${{ __html: '<svg viewBox="0 0 24 24">' + (ICONS[props.name] || ICONS.file) + '</svg>' }}></span>`;
  }
  // the tree is [id, depth] per page in order; a folder without a page of its
  // own (tree "folders") is ["", depth, title]
  function nest(list) {
    var root = { kids: [] }, stack = [root];
    list.forEach(function (e, k) {
      var node = { id: e[0], key: e[0] || 'folder:' + k, title: e[2], kids: [] };
      stack.length = e[1] + 1;
      (stack[e[1]] || root).kids.push(node);
      stack.push(node);
    });
    return root.kids;
  }
  // pages without a parent follow the tree at its top level
  var tree = nest(D.nav.tree).concat(nest(D.nav.orphans));
  function contains(node, id) {
    return node.id === id || node.kids.some(function (k) { return contains(k, id); });
  }
  // long lists show a window of 30 items with buttons for the rest
  function Items(props) {
    var all = props.nodes, n = all.length, at = -1;
    all.forEach(function (x, k) { if (contains(x, props.cur)) { at = k; } });
    var win = useState(null);
    var lo = 0, hi = Math.min(n, 30);
    if (win[0]) { lo = win[0][0]; hi = win[0][1]; }
    if (at >= 0 && (at < lo || at >= hi)) { lo = Math.max(0, at - 10); hi = Math.min(n, at + 11); }
    return html`<ul>
      ${lo > 0 && html`<li><button type="button" class="more" onClick=${function () { win[1]([Math.max(0, lo - 30), hi]); }}>↑ ${lo} {{t:earlier}}</button></li>`}
      ${all.slice(lo, hi).map(function (x) { return html`<${Node} ...${props} key=${x.key} node=${x} />`; })}
      ${hi < n && html`<li><button type="button" class="more" onClick=${function () { win[1]([lo, Math.min(n, hi + 30)]); }}>${n - hi} {{t:more}} ↓</button></li>`}
    </ul>`;
  }
  // the top level is always open: folding it would leave an empty tree
  function Node(props) {
    var x = props.node, p = byId[x.id], inside = contains(x, props.cur), top = !props.depth;
    var open = useState(null);
    var isOpen = top || (open[0] == null ? inside : open[0]);
    useEffect(function () { if (inside) { open[1](null); } }, [props.cur]);
    if (!p && x.id) { return null; }
    if (!p) {
      // a folder without a page of its own: its name opens and folds it
      var fin = !isOpen && props.inner[x.key];
      return html`<li class="parent">
        <button type="button" class="fd" aria-expanded=${isOpen ? 'true' : 'false'} disabled=${top} onClick=${function () { open[1](!isOpen); }}><${Icon} name="folder" /><span class="tt">${x.title}</span>
          ${fin && html`<span class="td" title="{{t:Edits inside}}"><span class="sr">{{t:Edits inside}}</span></span>`}</button>
        ${x.kids.length > 0 && isOpen && html`<${Items} ...${props} depth=${(props.depth || 0) + 1} nodes=${x.kids} />`}
      </li>`;
    }
    var here = p.id === props.cur, m = props.marks[p.id], inner = !isOpen && props.inner[x.key];
    // the colour of the title says the kind, the mark beside it says it without colour
    return html`<li class=${x.kids.length ? 'parent' : ''}>
      <a href=${'#' + p.id} class=${m ? 'm-' + m.k : undefined} aria-current=${here ? 'page' : undefined} onClick=${props.picked}><${Icon} name=${p.kind} /><span class="tt">${p.title}</span>${p.hidden && html` <span class="chip bad">{{t:hidden}}</span>`}
        ${m && html`<span class=${'tm ' + m.k} title=${m.k === 'new' ? '{{t:New page}}' : '{{t:Edits:}} ' + m.n}>${m.k === 'new' ? html`+<span class="sr">{{t:New page}}</span>` : html`<span class="sr">{{t:Edits:}} </span>${m.n}`}</span>`}
        ${inner && html`<span class="td" title="{{t:Edits inside}}"><span class="sr">{{t:Edits inside}}</span></span>`}</a>
      ${x.kids.length > 0 && !top && html`<button class="tw" type="button" aria-expanded=${isOpen ? 'true' : 'false'}
        aria-label=${(isOpen ? '{{t:Collapse}}' : '{{t:Expand}}') + ': ' + p.title} onClick=${function () { open[1](!isOpen); }}></button>`}
      ${here && props.toc.length > 0 && html`<ul class="toc" aria-label="{{t:Sections of the page}}">${props.toc.map(function (h) {
        return html`<li><a href=${'#' + h.id} class=${'l' + h.level + (props.spy === h.id ? ' on' : '')} onClick=${props.picked}><${Icon} name="#" /><span class="tt">${h.text}</span></a></li>`;
      })}</ul>`}
      ${x.kids.length > 0 && isOpen && html`<${Items} ...${props} depth=${(props.depth || 0) + 1} nodes=${x.kids} />`}
    </li>`;
  }
  // the pages that have marked pages below them, for a folded parent
  function marked(nodes, m, out) {
    var any = false;
    nodes.forEach(function (x) {
      if (marked(x.kids, m, out)) { out[x.key] = true; any = true; }
      if (m[x.id]) { any = true; }
    });
    return any;
  }
  function Tree(props) {
    var inner = {};
    marked(tree, props.marks, inner);
    return html`<nav id="tree" aria-label="{{t:Document tree}}">
      ${tree.length > 0 && html`<${Items} ...${props} inner=${inner} depth=${0} nodes=${tree} />`}
    </nav>`;
  }

  // the search finds paragraphs: every word in the paragraph or in the title of its page
  var index = null, LIMIT = 30;
  function Search(props) {
    var q = props.q, sel = useState(0);
    var v = q[0].trim(), hits = [], total = 0;
    if (v) {
      index = index || [].concat.apply([], pages.map(function (p) {
        return [{ p: p, ptr: '', text: '' }].concat(blocksOf(p).map(function (b) { return { p: p, ptr: b.p, text: b.i ? plain(b.s) : textOf(b.h) }; }));
      }));
      var esc = function (w) { return w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); };
      var words = v.split(/\s+/).map(function (w) { return new RegExp(esc(w), 'i'); });
      index.forEach(function (t) {
        if (!words.every(function (w) { return w.test(t.p.title) || w.test(t.text); })) { return; }
        if (!t.ptr && !words.every(function (w) { return w.test(t.p.title); })) { return; } // a page by its title alone
        if (t.ptr && !words.some(function (w) { return w.test(t.text); })) { return; }
        var score = words.reduce(function (s, w) { return s + (w.test(t.p.title) ? 10 : 0); }, 0) + (t.ptr ? 0 : 5);
        var m = words[0].exec(t.text) || (words.length > 1 && words[1].exec(t.text)), i = m ? m.index : 0;
        var from = Math.max(0, i - 40), snp = t.text ? (from > 0 ? '… ' : '') + t.text.slice(from, i + 110) + (i + 110 < t.text.length ? ' …' : '') : '';
        hits.push({ p: t.p, ptr: t.ptr, score: score, snip: snp });
      });
      hits.sort(function (a, b) { return b.score - a.score; });
      total = hits.length;
      hits = hits.slice(0, LIMIT);
    }
    var re = v ? new RegExp('(' + v.split(/\s+/).map(function (w) { return w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }).join('|') + ')', 'gi') : null;
    function go(h) {
      var to = blockHash(h.p.id, h.ptr);
      q[1](''); props.picked();
      if (decode(location.hash) === decode(to)) { props.go(to); } else { location.hash = to; }
    }
    return html`<div class="search">
      <input type="search" id="q" placeholder="{{t:Search  ( / )}}" aria-label="{{t:Search the documentation}}" autocomplete="off"
        role="combobox" aria-expanded=${v ? 'true' : 'false'} aria-controls="results"
        aria-activedescendant=${v && hits.length ? 'hit-' + sel[0] : undefined}
        value=${q[0]} onInput=${function (e) { q[1](e.target.value); sel[1](0); }}
        onKeyDown=${function (e) {
          if (e.key === 'ArrowDown' && hits.length) { e.preventDefault(); sel[1]((sel[0] + 1) % hits.length); }
          else if (e.key === 'ArrowUp' && hits.length) { e.preventDefault(); sel[1]((sel[0] + hits.length - 1) % hits.length); }
          else if (e.key === 'Enter' && hits[sel[0]]) { e.preventDefault(); go(hits[sel[0]]); }
          else if (e.key === 'Escape' && q[0]) { e.preventDefault(); e.stopPropagation(); q[1](''); }
        }} />
      ${v && html`<div id="results">
        <p class="count" role="status">${total ? '{{t:Found}}: ' + total + (total > LIMIT ? ' · {{t:the first ones are shown, add a word}}' : '') : '{{t:Nothing found}}'}</p>
        <ul role="listbox" aria-label="{{t:Search results}}">
        ${hits.map(function (h, k) {
          return html`<li id=${'hit-' + k} role="option" aria-selected=${k === sel[0] ? 'true' : 'false'} onClick=${function () { go(h); }}>
            <b>${h.p.title}</b>${h.snip && html`<span>${h.snip.split(re).map(function (part, n) { return n % 2 ? html`<mark>${part}</mark>` : part; })}</span>`}</li>`;
        })}
      </ul></div>`}
    </div>`;
  }

  var kindWord = { chg: '{{t:edited}}', 'new': '{{t:new}}', rm: '{{t:removed}}' };
  // the heading above every block of a page, for the edits of a long page
  var headCache = {};
  function headingAt(page, i) {
    if (!headCache[page.id]) {
      var h = '', out = [];
      (page.blocks || []).forEach(function (b) {
        var m = !b.p && /<h[234][^>]*>([\s\S]*?)<\/h[234]>/.exec(b.h);
        if (m) { h = textOf(m[1]); }
        out.push(h);
      });
      headCache[page.id] = out;
    }
    return headCache[page.id][i] || '';
  }
  function itemHeading(it) {
    var bs = it.page.blocks || [], i = -1;
    if (it.a.indexOf('~') > 0) { i = placeGhost(it.page, it.ptr); } else { bs.forEach(function (b, k) { if (b.p === it.ptr) { i = k; } }); }
    return i < 0 ? '' : headingAt(it.page, Math.min(i, bs.length - 1));
  }
  function counts(rows) {
    var n = { chg: 0, 'new': 0, rm: 0 };
    rows.forEach(function (x) { n[x[0].k]++; });
    return [['{{t:Changed:}}', n.chg], ['{{t:Added:}}', n['new']], ['{{t:Removed:}}', n.rm]].filter(function (x) { return x[1]; })
      .map(function (x) { return x[0] + ' ' + x[1]; }).join(' · ');
  }
  // the edits of one page: [edit, its index in the list]
  function pageEdits(list, id) {
    var rows = [];
    list.forEach(function (it, k) { if (it.page.id === id) { rows.push([it, k]); } });
    return rows;
  }
  // the next page with marks in the order of the pages, from the start again after the last; null when no other has any
  function nextEdited(marks, id) {
    var at = byId[id] ? byId[id].i : -1;
    for (var k = 1; k <= pages.length; k++) {
      var p = pages[(at + k) % pages.length];
      if (p.id !== id && marks[p.id]) { return p.id; }
    }
    return null;
  }
  // The edits of the open page under the headings they stand under, 15 rows
  // at a time: the tree shows where else the text changed, and the button
  // below goes on to the next page with edits.
  var EDIT_ROWS = 15;
  function EditsTab(props) {
    var list = props.changes.list, page = props.page, more = useState(EDIT_ROWS);
    // the list opens at the current edit
    useEffect(function () {
      var on = document.querySelector('.er.on');
      if (on) { on.scrollIntoView({ block: 'center' }); }
    }, [props.cur]);
    if (last < 1) { return html`<p class="muted pad">{{t:The documentation has no saved history yet.}}</p>`; }
    if (!list.length) { return html`<p class="muted pad">{{t:No edits in the chosen period. Open the timeline above the page and pick an earlier save or date to see them.}}</p>`; }
    var rows = pageEdits(list, page.id), nx = nextEdited(pageMarks(list, props.s, props.t), page.id), lastHead = null;
    var n = more[0];
    rows.forEach(function (x, i) { if (x[1] === props.cur) { n = Math.max(n, i + 1); } });
    return html`<div class="pad">
      <p class="left">${page.title}</p>
      <p class="es">${rows.length ? counts(rows) : '{{t:No edits on this page}}'} · ${sinceLabel(props.s)} → ${atLabel(props.t)}</p>
      <ul class="edits">${rows.slice(0, n).map(function (x) {
        var it = x[0], hd = itemHeading(it), head = hd && hd !== lastHead ? hd : null;
        lastHead = hd;
        return html`${head && html`<li class="eh">${head}</li>`}
          <li><button type="button" class=${'er ' + it.k + (x[1] === props.cur ? ' on' : '')} onClick=${function () { props.go(x[1]); }}>
          <span class=${'g ' + it.k}>${kindWord[it.k]}</span><span class="et" title=${snip(it.t, 300)}>${snip(it.t, 120)}</span></button></li>`;
      })}</ul>
      ${rows.length > n && html`<button type="button" class="more" onClick=${function () { more[1](n + EDIT_ROWS); }}>
        {{t:Show more:}} ${Math.min(EDIT_ROWS, rows.length - n)} {{t:of}} ${rows.length - n}</button>`}
      ${nx && html`<button type="button" class="btn nx" onClick=${function () { props.next(nx); }}>
        <span><small>{{t:Next page with edits:}}</small>${byId[nx].title}</span><span aria-hidden="true">→</span></button>`}
    </div>`;
  }
  function CommentsTab(props) {
    var list = props.comments, rows = [];
    list.forEach(function (c) { rows.push([c, false]); });
    imported.forEach(function (c) { rows.push([c, true]); });
    function row(x) {
        var c = x[0], at = commentPlace(c), p = at.page;
        var body = html`
          <span class="cp">${p ? p.title : c.doc} · ${c.fragment ? '«' + snip(c.fragment, 50) + '»' : c.input ? '«' + snip(c.quote, 70) + '»' : placeOf(p, c.path, c.quote)}
            ${at.gone && html` <span class="chip">${at.gone === 'page' ? '{{t:page removed}}' : '{{t:paragraph removed}}'}</span>`}</span>
          <span class="ctx">${c.text}</span>
          ${c.author && html`<span class="cp">— ${c.author}</span>`}`;
        if (!at.gone) { return html`<li><button type="button" class="cr" onClick=${function () { props.show(c); }}>${body}</button></li>`; }
        // its place is not in the text any more: the comment opens here, with the words it was left on
        return html`<li><details class="cg"><summary class="cr">${body}</summary>
          <p class="cp">{{t:It was left on:}}</p><blockquote class="fq">${c.fragment || plain(c.quote)}</blockquote>
          ${p && html`<a href=${'#' + p.id} onClick=${props.picked}>{{t:Open the page}}</a>`}</details></li>`;
    }
    return html`<div class="pad">
      ${list.length > 0 && html`<div class="send" id="send">
        <p>{{t:Your comments are kept only in this browser. Copy them or download a file and pass it to whoever keeps the documentation.}}</p>
        <div class="row"><button type="button" class="btn pri" onClick=${sendCopy}>📋 {{t:Copy comments}}</button>
          <button type="button" class="btn" onClick=${sendFile}>{{t:Download as a file}}</button></div>
      </div>`}
      ${!rows.length && html`<p class="muted">{{t:No comments yet. Select text on the page to comment on it.}}</p>`}
      <ul class="clist">${rows.map(row)}</ul>
    </div>`;
  }
  // the edits listed beside the tree (in its place in the drawer of a narrow screen)
  function EditsPanel(props) {
    return html`<div class="eds-in">
      <div class="eds-h"><h2 id="eds-h">{{t:Edits on the page}}</h2>
        <button type="button" class="btn sm" onClick=${props.close}>${props.wide ? '✕ {{t:Close}}' : '← {{t:Pages}}'}</button></div>
      <div class="panel"><${EditsTab} key=${props.page.id} changes=${props.changes} s=${props.s} t=${props.t} cur=${props.cur} page=${props.page} go=${props.goEdit} next=${props.nextPage} /></div>
    </div>`;
  }
  // over the tree: the button of the edits of the open page, while edits are shown;
  // a page without edits says so, and the open list still leads to the next one
  function EditsBar(props) {
    if (!props.changes.list.length) { return null; }
    var n = pageEdits(props.changes.list, props.page.id).length;
    return html`<button type="button" class="btn sm ebar" aria-expanded=${props.open ? 'true' : 'false'} aria-controls="edits" onClick=${props.toggle} disabled=${!n && !props.open}>
      ${n ? html`{{t:Edits on the page:}} <span class="tc">${n}</span>` : '{{t:No edits on this page}}'}<span class="sp"></span>${props.wide && props.open ? '◂' : '▸'}</button>`;
  }
  // tabs after the WAI-ARIA pattern: arrows move between them. In the drawer
  // the list of edits takes the place of the tabs until it is closed, also
  // across the drawer opening again after a pick.
  var drawerEdits = false;
  // the focus goes with the list: into it when it opens, back to its button when it closes
  function focusEdits(opened) {
    setTimeout(function () { var b = document.querySelector(opened ? '#edits .eds-h button' : '.ebar'); if (b) { b.focus(); } }, 0);
  }
  function Side(props) {
    var de = useState(drawerEdits);
    var open = props.wide ? props.editsOpen : de[0];
    function toggle() { if (props.wide) { props.toggleEdits(); } else { drawerEdits = !de[0]; de[1](drawerEdits); focusEdits(drawerEdits); } }
    if (open && !props.wide) {
      return html`<div class="side-in" id="edits" role="region" aria-labelledby="eds-h"><${EditsPanel} ...${props} close=${toggle} /></div>`;
    }
    var tabs = [['pages', '{{t:Pages}}', 0], ['comments', '{{t:Comments}}', props.comments.length + imported.length]];
    var tab = props.tab === 'comments' ? 'comments' : 'pages';
    function key(e) {
      var k = tabs.map(function (t) { return t[0]; }).indexOf(tab), n = tabs.length;
      var to = e.key === 'ArrowRight' ? (k + 1) % n : e.key === 'ArrowLeft' ? (k + n - 1) % n : e.key === 'Home' ? 0 : e.key === 'End' ? n - 1 : -1;
      if (to < 0) { return; }
      e.preventDefault(); props.setTab(tabs[to][0]);
      setTimeout(function () { var b = document.getElementById('tab-' + tabs[to][0]); if (b) { b.focus(); } }, 0);
    }
    return html`<div class="side-in">
      <div class="side-top">
        ${D.brand.head && html`<div class="side-brand" dangerouslySetInnerHTML=${{ __html: D.brand.head }}></div>`}
        <${Search} q=${props.q} picked=${props.picked} go=${props.goHash} />
        ${D.full && html`<p class="banner">{{t:Full build: includes hidden documents and notes. Do not publish.}}</p>`}
      </div>
      <div class="tabs" role="tablist" aria-label="{{t:Navigation}}" onKeyDown=${key}>${tabs.map(function (t) {
        return html`<button type="button" role="tab" id=${'tab-' + t[0]} aria-selected=${t[0] === tab ? 'true' : 'false'} aria-controls="tabpanel"
          tabindex=${t[0] === tab ? '0' : '-1'} onClick=${function () { props.setTab(t[0]); }}>${t[1]}${t[2] > 0 && html` <span class="tc">${t[2]}</span>`}</button>`;
      })}</div>
      <div id="tabpanel" role="tabpanel" aria-labelledby=${'tab-' + tab} class="panel">
        ${tab === 'pages' && html`<${EditsBar} changes=${props.changes} page=${props.page} open=${open} wide=${props.wide} toggle=${toggle} />
          <${Tree} cur=${props.page.id} toc=${props.toc} spy=${props.spy} picked=${props.picked} marks=${pageMarks(props.changes.list, props.s, props.t)} />`}
        ${tab === 'comments' && html`<${CommentsTab} comments=${props.comments} picked=${props.picked} show=${function (c) { props.picked(); props.showComment(c); }} />`}
      </div>
    </div>`;
  }

  // The name of the reader, asked once on the first visit: it fills the
  // field From of every comment. Skipping is remembered; the editor then
  // asks for it, and ⋯ changes it.
  function NameDialog(props) {
    var d = useRef(null), v = useState(store('author') || '');
    useEffect(function () { var el = d.current; if (el.showModal && !el.open) { el.showModal(); } }, []);
    function done(keep) {
      var n = v[0].trim();
      if (keep && n) { store('author', n); }
      store('asked', true); d.current.close();
    }
    return html`<dialog ref=${d} class="name-d" aria-labelledby="name-h" onClose=${props.close}>
      <form onSubmit=${function (e) { e.preventDefault(); done(true); }}>
        <h2 id="name-h">{{t:What is your name?}}</h2>
        <p class="muted">{{t:It is put into your comments, so whoever keeps the documentation knows who wrote them. You can change it in ⋯.}}</p>
        <input class="who" autocomplete="name" autofocus placeholder="{{t:Your name}}" value=${v[0]} onInput=${function (e) { v[1](e.target.value); }} />
        <div class="row"><span class="sp"></span>
          <button type="button" class="btn" onClick=${function () { done(false); }}>{{t:Skip}}</button>
          <button type="submit" class="btn pri" disabled=${!v[0].trim()}>{{t:Save}}</button></div>
      </form>
    </dialog>`;
  }

  // «⋯»: a native popover; it holds the reset
  function More(props) {
    var pop = useRef(null);
    return html`<button type="button" class="btn ico more-b" popovertarget="more" aria-label="{{t:More actions}}" title="{{t:More actions}}">⋯</button>
      <div id="more" popover="auto" ref=${pop} class="pop">
        <button type="button" class="mi" onClick=${function () { if (pop.current.hidePopover) { pop.current.hidePopover(); } props.name(); }}>
          {{t:Your name:}} <b>${store('author') || '{{t:not given}}'}</b> · {{t:change}}</button>
        <button type="button" class="mi bad" onClick=${function () { if (pop.current.hidePopover) { pop.current.hidePopover(); } props.reset(); }}>{{t:Reset everything…}}</button>
      </div>`;
  }
  // everything this file keeps in the browser: the name, the timeline, the tab, the comments
  function resetAll() {
    if (!confirm('{{t:Delete from this browser your name, the chosen point of the timeline, the open tab and your comments}} (' + local.length + ')?')) { return; }
    draft = false;
    try {
      for (var i = localStorage.length - 1; i >= 0; i--) {
        var k = localStorage.key(i);
        if (k && k.indexOf(D.key + ':') === 0) { localStorage.removeItem(k); }
      }
    } catch (e) { /* storage blocked: nothing kept */ }
    location.reload();
  }

  // the selected text of one block, with what stands around it, for a comment on a fragment
  function selected() {
    var s = window.getSelection();
    if (!s || s.isCollapsed || !s.rangeCount) { return null; }
    var r = s.getRangeAt(0), start = r.startContainer.nodeType === 1 ? r.startContainer : r.startContainer.parentNode;
    var body = start.closest && start.closest('.blk-body'), blk = body && body.closest('.blk[data-p]');
    // a form field is answered, not commented on
    if (!blk || start.closest('.fld')) { return null; }
    if (!body.contains(r.endContainer)) { r = r.cloneRange(); r.setEnd(body, body.childNodes.length); }
    var frag = cleanText(r.cloneContents()).replace(/\s+/g, ' ').trim();
    if (frag.length < 2) { return null; }
    var pre = document.createRange(), post = document.createRange();
    pre.setStart(body, 0); pre.setEnd(r.startContainer, r.startOffset);
    post.setStart(r.endContainer, r.endOffset); post.setEnd(body, body.childNodes.length);
    var rect = r.getBoundingClientRect();
    return { a: blk.getAttribute('data-a'), ptr: blk.getAttribute('data-p'), x: rect.right, y: rect.bottom,
      frag: { fragment: frag, prefix: cleanText(pre.cloneContents()).replace(/\s+/g, ' ').slice(-32), suffix: cleanText(post.cloneContents()).replace(/\s+/g, ' ').slice(0, 32) } };
  }

  function App() {
    var r = useState(route(location.hash)), wide = useState(mq('(min-width: 1280px)')), drawer = useState(false);
    var q = useState(''), tab = useState(store('tab') || 'pages'), edits = useState(!!store('edits'));
    // S is kept by hash (indexes change with every commit), «now» as now;
    // the starting point of docc.json is the default. T always opens at now.
    var sinceHash = useState(store('since')), atK = useState(last), cur = useState(-1), editor = useState(null);
    var toc = useState([]), spy = useState(null), frag = useState(null), activeC = useState(null);
    var comments = useComments(), pending = useRef(null), dr = useRef(null), askName = useState(store('author') == null && !store('asked'));
    var page = r[0].page, t = atK[0], s = -1, past = t < last;
    if (sinceHash[0] === 'now') { s = last; } else { commits.forEach(function (c, k) { if (c.h && c.h === sinceHash[0]) { s = k; } }); }
    if (s < 0) { s = H.base ? 0 : last; }
    s = Math.min(s, t);
    var all = allChanges(s, t);

    function setTab(t) { tab[1](t); store('tab', t); }
    function setS(v) { var h = v >= last ? 'now' : commits[v].h; sinceHash[1](h); store('since', h); cur[1](-1); }
    function setRange(a, b) {
      if (b < last && editor[0]) { if (!leaveDraft()) { return; } draft = false; editor[1](null); }
      atK[1](b); setS(a); frag[1](null);
    }
    function setT(v) { setRange(Math.min(s, v), v); }
    function find(a) { return a && document.querySelector('[data-a="' + CSS.escape(a) + '"]'); }
    function focusEl(el) {
      (el.tagName === 'DETAILS' ? el.querySelector('summary') : el).focus({ preventScroll: true });
      var tall = el.offsetHeight > window.innerHeight - 160;
      el.scrollIntoView({ block: tall ? 'start' : 'center', behavior: reduced ? 'auto' : 'smooth' });
      if (!reduced) { el.classList.add('flash'); setTimeout(function () { el.classList.remove('flash'); }, 900); }
    }
    function goItem(k) {
      var it = all.list[k];
      if (!it) { return; }
      cur[1](k); pending.current = it.a;
      // removed blocks have no address of their own: the page, then the block
      var gone = it.a.indexOf('~') > 0, h = gone ? '#' + it.page.id : blockHash(it.page.id, it.ptr);
      if (it.page === page && (gone || decode(location.hash) === decode(h))) { var el = find(it.a); pending.current = null; if (el) { focusEl(el); } }
      else { location.hash = h; }
    }
    function openEditor(e) {
      if (past || (editor[0] && !leaveDraft())) { return; }
      draft = false; editor[1](e); frag[1](null);
    }
    function blockOf(a) {
      if (!a || a.indexOf(page.id + '/') !== 0) { return null; }
      var ptr = a.slice(page.id.length);
      return blocksOf(page).filter(function (b) { return b.p === ptr; })[0] || null;
    }
    function commentSel() {
      var f = frag[0] || selected();
      if (f) { openEditor({ page: page, ptr: f.ptr, quote: (blockOf(f.a) || {}).s, frag: f.frag }); }
    }
    function showComment(c) {
      var at = commentPlace(c);
      if (at.gone) { return; } // the list shows it in place
      if (past) { setT(last); }
      activeC[1](c.id); pending.current = 'c:' + c.id;
      var h = blockHash(c.doc, at.ptr);
      if (decode(location.hash) === decode(h)) { r[1](route(h)); } else { location.hash = h; }
    }
    function picked() { if (!wide[0]) { drawer[1](false); } }
    // the focus goes back to the menu, unless a pick has put it on the place it opened:
    // taking it from there would also stop the scroll to that place halfway
    function closeDrawer() {
      drawer[1](false);
      var a = document.activeElement, m = document.getElementById('menu');
      if (m && (!a || a === document.body || dr.current.contains(a))) { m.focus({ preventScroll: true }); }
    }

    useEffect(function () {
      function on() { r[1](route(location.hash)); }
      window.addEventListener('hashchange', on);
      // the place of reading goes into the history entry, so Back returns to it
      var t;
      function onScroll() {
        clearTimeout(t);
        t = setTimeout(function () { try { history.replaceState({ y: window.scrollY }, ''); } catch (e) { /* not allowed */ } }, 150);
      }
      window.addEventListener('scroll', onScroll, { passive: true });
      var m = window.matchMedia && matchMedia('(min-width: 1280px)'), w = function () { wide[1](m.matches); drawer[1](false); };
      if (m && m.addEventListener) { m.addEventListener('change', w); }
      // the folded old texts and removed paragraphs are printed open
      function bp() { [].forEach.call(document.querySelectorAll('details.ghost,details.old'), function (d) { d.dataset.was = d.open ? '1' : ''; d.open = true; }); }
      function ap() { [].forEach.call(document.querySelectorAll('details.ghost,details.old'), function (d) { d.open = !!d.dataset.was; }); }
      window.addEventListener('beforeprint', bp); window.addEventListener('afterprint', ap);
      return function () {
        window.removeEventListener('hashchange', on); window.removeEventListener('scroll', onScroll);
        if (m && m.removeEventListener) { m.removeEventListener('change', w); }
        window.removeEventListener('beforeprint', bp); window.removeEventListener('afterprint', ap);
      };
    }, []);
    // after a new page or address: the place to show
    useEffect(function () {
      document.title = page === pages[0] ? D.title : page.title + ' · ' + D.title;
      var st = history.state, tg = r[0].target && document.getElementById(r[0].target);
      var b = r[0].block && document.querySelector('[data-p="' + CSS.escape(r[0].block) + '"]');
      var p = pending.current;
      pending.current = null;
      if (p && p.indexOf('c:') === 0) {
        var card = document.querySelector('.card[data-c="' + CSS.escape(p.slice(2)) + '"]');
        if (card) { focusEl(card); } else if (b) { focusEl(b); }
      } else if (p && find(p)) { focusEl(find(p)); }
      else if (st && st.y != null && !r[0].block) { window.scrollTo({ top: st.y, behavior: 'instant' }); }
      else if (b) {
        focusEl(b);
        all.list.forEach(function (x, k) { if (x.page === page && x.ptr === r[0].block && x.k !== 'rm') { cur[1](k); } });
      } else if (tg) { tg.scrollIntoView({ behavior: 'instant' }); } else { window.scrollTo({ top: 0, behavior: 'instant' }); }
      var here = document.querySelector('#tree [aria-current]');
      if (here) { here.scrollIntoView({ block: 'nearest' }); }
    }, [r[0]]);
    // the drawer is a modal <dialog> below 1280px
    useEffect(function () {
      var d = dr.current;
      if (!d) { return; }
      if (drawer[0] && !d.open) {
        d.showModal();
        var f = drawer[0] === 'search' ? document.getElementById('q') : d.querySelector('[role=tab][aria-selected=true]') || d.querySelector('[aria-current]');
        if (f) { f.focus(); }
      } else if (!drawer[0] && d.open) { d.close(); }
    }, [drawer[0]]);
    // the headings of the page in the tree follow the reading
    useEffect(function () {
      if (!window.IntersectionObserver || toc[0].length < 2) { return; }
      var o = new IntersectionObserver(function (es) { es.forEach(function (e) { if (e.isIntersecting) { spy[1](e.target.id); } }); }, { rootMargin: '0px 0px -75% 0px' });
      toc[0].forEach(function (x) { var el = document.getElementById(x.id); if (el) { o.observe(el); } });
      return function () { o.disconnect(); };
    }, [toc[0]]);
    // a comment on the selected words: a button under the selection, or in the bottom bar on touch
    useEffect(function () {
      if (past) { frag[1](null); return; }
      var tm;
      function up() { setTimeout(function () { frag[1](selected()); }, 0); }
      function change() {
        var s = window.getSelection();
        if (!s || s.isCollapsed) { frag[1](null); return; }
        if (coarse) { clearTimeout(tm); tm = setTimeout(function () { frag[1](selected()); }, 300); }
      }
      function scroll() { if (!coarse) { frag[1](null); } }
      document.addEventListener('mouseup', up); document.addEventListener('selectionchange', change);
      window.addEventListener('scroll', scroll, { passive: true });
      return function () { document.removeEventListener('mouseup', up); document.removeEventListener('selectionchange', change); window.removeEventListener('scroll', scroll); };
    }, [past]);
    // keys: the search
    useEffect(function () {
      function key(e) {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); goSearch(); return; }
        if (e.key === 'Escape') { frag[1](null); return; }
        if (e.ctrlKey || e.metaKey || e.altKey || typing() || document.querySelector('dialog[open]')) { return; }
        if (e.key === '/') { e.preventDefault(); goSearch(); }
      }
      document.addEventListener('keydown', key);
      return function () { document.removeEventListener('keydown', key); };
    });
    function goSearch() {
      if (wide[0]) { var i = document.getElementById('q'); if (i) { i.focus(); i.select(); } } else { drawer[1]('search'); }
    }
    function setToc(items) { if (JSON.stringify(items) !== JSON.stringify(toc[0])) { toc[1](items); } }

    function toggleEdits() { var v = !edits[0]; edits[1](v); store('edits', v); focusEdits(v); }
    function goEdit(k) { picked(); goItem(k); }
    // the next page with edits opens at its first edit
    function nextPage(id) {
      var k = -1;
      all.list.forEach(function (it, i) { if (k < 0 && it.page.id === id) { k = i; } });
      if (k >= 0) { goEdit(k); } else { picked(); location.hash = '#' + id; }
    }
    var sideProps = { q: q, page: page, s: s, t: t, toc: toc[0], spy: spy[0], picked: picked, changes: all, cur: cur[0],
      goEdit: goEdit, nextPage: nextPage, comments: comments, tab: tab[0], setTab: setTab, showComment: showComment, goHash: function (h) { r[1](route(h)); },
      wide: wide[0], editsOpen: edits[0], toggleEdits: toggleEdits };
    var side = html`<${Side} ...${sideProps} />`;
    var eds = wide[0] && edits[0];
    var curA = all.list[cur[0]] ? all.list[cur[0]].a : null;
    // the margin for the cards opens only when the page has comments or an editor
    var rail = !past && (!!editor[0] || comments.concat(imported).some(function (c) { return c.doc === page.id && !c.input && !c.lost; }));
    return html`
      <a class="skip" href="#main" onClick=${function (e) { e.preventDefault(); var m = document.getElementById('main'); m.focus(); m.scrollIntoView(); }}>{{t:Skip to content}}</a>
      <header class=${'top' + (last > 0 ? ' has-tl' : '')} role="banner">
        ${!wide[0] && html`<button id="menu" type="button" class="btn ico" aria-label="{{t:Menu}}" aria-expanded=${drawer[0] ? 'true' : 'false'} onClick=${function () { drawer[1](true); }}>☰</button>`}
        <div class="proj">${D.brand.head && html`<span dangerouslySetInnerHTML=${{ __html: D.brand.head }}></span>`}<a href=${'#' + pages[0].id}>${D.title}</a></div>
        <span class="sp"></span>
        ${last > 0 && html`<${When} page=${page} s=${s} t=${t} setRange=${setRange} />`}
        ${!wide[0] && html`<button type="button" class="btn mid" aria-label="{{t:Search}}" onClick=${goSearch}>🔍<span class="mid-t"> {{t:Search}}</span></button>`}
        <${More} reset=${resetAll} name=${function () { askName[1](true); }} />
      </header>
      <div class=${'shell' + (rail ? ' work' : '') + (eds ? ' eds' : '')}>
        ${wide[0] ? html`<aside id="side" aria-label="{{t:Navigation}}">${side}</aside>
          ${eds && html`<aside id="edits" aria-labelledby="eds-h"><${EditsPanel} ...${sideProps} wide=${true} close=${toggleEdits} /></aside>`}`
          : html`<dialog id="drawer" ref=${dr} class="drawer" aria-label="{{t:Navigation}}" onClose=${closeDrawer}
              onClick=${function (e) { if (e.target === dr.current) { dr.current.close(); } }}>
              <button type="button" class="btn side-x" onClick=${function () { dr.current.close(); }}>✕ {{t:Close}}</button>
              ${drawer[0] && side}</dialog>`}
        <main id="main" tabindex="-1">
          <${Page} key=${page.id} page=${page} comments=${comments} s=${s} t=${t} setT=${setT} onToc=${setToc} missing=${r[0].missing}
            cur=${curA} editor=${editor[0]} closeEditor=${function () { editor[1](null); }} activeC=${activeC[0]} />
        </main>
      </div>
      ${frag[0] && coarse && html`<div class="nav" role="region" aria-label="{{t:Comment on the selection}}">
        <button type="button" class="btn pri" onMouseDown=${function (e) { e.preventDefault(); }} onClick=${commentSel}>💬 {{t:Comment on the selection}}</button></div>`}
      ${frag[0] && !coarse && html`<button type="button" class="btn pri fragb" style=${'left:' + Math.min(frag[0].x, window.innerWidth - 220) + 'px;top:' + (frag[0].y + 8) + 'px'}
        onMouseDown=${function (e) { e.preventDefault(); }} onClick=${commentSel}>💬 {{t:Comment on the selection}}</button>`}
      ${askName[0] && html`<${NameDialog} close=${function () { askName[1](false); }} />`}
      <${Toast} />`;
  }

  // the pure parts, for app_test.js; nothing else sets this
  if (window.doccTest) { window.doccTest({ wordMarks: wordMarks, words: words, plain: plain, snip: snip, findFragment: findFragment, placeGhost: placeGhost, pageDelta: pageDelta, between: between,
    saveDays: saveDays, atDate: atDate, pathOf: pathOf, answersFor: answersFor, fieldWords: fieldWords, thumbFor: thumbFor, pageMarks: pageMarks, allChanges: allChanges, commentPlace: commentPlace, lostOn: lostOn, nest: nest, pageEdits: pageEdits, nextEdited: nextEdited }); return; }
  render(html`<${App} />`, document.getElementById('app'));
})();
