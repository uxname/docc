// The documentation application: one page at a time drawn from the JSON
// island #docc-data with Preact and htm (vendor/htm-preact-standalone.js).
// Labels are marked as t:English in double braces and translated by the build (lang.go).
// Two modes: Reading shows the final text only; Work adds the edits since the
// starting point (word diff in the text, gutter bars, removed paragraphs in
// place, a navigator), comments on the margin and the tools.
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
  function pad(n) { return (n < 10 ? '0' : '') + n; }
  function date(c) {
    var cm = commits[c];
    if (!cm || !cm.d) { return ''; }
    var d = new Date(cm.d);
    return pad(d.getDate()) + '.' + pad(d.getMonth() + 1) + '.' + d.getFullYear();
  }
  function when(c) {
    var cm = commits[c];
    if (!cm || !cm.h) { return '{{t:not saved yet}}'; }
    var d = new Date(cm.d);
    return date(c) + ' ' + pad(d.getHours()) + ':' + pad(d.getMinutes());
  }
  // a version for the reader is its date; the full build adds the author and message
  function versionLine(c) {
    var cm = commits[c] || {};
    if (!cm.h) { return '{{t:not saved yet}}'; }
    return when(c) + (cm.a ? ' · ' + cm.a : '') + (cm.m ? ' · ' + cm.m : '');
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
  function unzip64(s) {
    var b = atob(s), u = new Uint8Array(b.length);
    for (var i = 0; i < b.length; i++) { u[i] = b.charCodeAt(i); }
    return u;
  }
  function b64(s) {
    var u = new TextEncoder().encode(s), b = '';
    for (var i = 0; i < u.length; i++) { b += String.fromCharCode(u[i]); }
    return btoa(b);
  }
  function uuid() {
    if (window.crypto && crypto.randomUUID) { return crypto.randomUUID(); }
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
      var r = Math.random() * 16 | 0;
      return (c === 'x' ? r : (r & 3 | 8)).toString(16);
    });
  }
  // a short hash of a text: a viewed edit is kept with the text it had
  function hash(s) {
    var h = 5381;
    for (var i = 0; i < s.length; i++) { h = (h * 33 + s.charCodeAt(i)) | 0; }
    return (h >>> 0).toString(36);
  }
  var box = document.createElement('div');
  function textOf(h) { box.innerHTML = h; return box.textContent.replace(/\s+/g, ' ').trim(); }
  // the source text of a block as a reader sees it: links by their names, no marks
  // ponytail: a few regexes for the usual marks, not a Markdown parser
  function plain(s) {
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
  // word diff of two texts for the history: [op, text] runs
  function diff(a, b) {
    var x = (a || '').split(/(\s+)/), y = (b || '').split(/(\s+)/), ops = lcs(x, y);
    if (!ops || rewrite(ops, x, y)) { return [[-1, a], [1, b]]; }
    var out = [];
    ops.forEach(function (o) {
      var s = o[0] < 0 ? x[o[1]] : y[o[1]];
      if (out.length && out[out.length - 1][0] === o[0]) { out[out.length - 1][1] += s; } else { out.push([o[0], s]); }
    });
    return out;
  }
  // was and now in reading form; removed words are struck out, added underlined
  function Diff(props) {
    var b = plain(props.b);
    if (props.a == null) { return html`<div class="dv"><ins>${b}</ins></div>`; }
    var a = plain(props.a), ds = diff(a, b), whole = ds.length === 2 && ds[0][1] === a && ds[1][1] === b;
    return html`<div class=${'dv' + (whole ? ' whole' : '')}>${ds.map(function (d) {
      return d[0] < 0 ? html`<del><span class="sr">{{t:removed:}} </span>${d[1]}</del>` : d[0] > 0 ? html`<ins><span class="sr">{{t:added:}} </span>${d[1]}</ins>` : d[1];
    })}</div>`;
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

  // ---- history and changes ----
  function versions(page, ptr) {
    var h = H.docs && H.docs[page.file];
    return (h && h.b && h.b[ptr]) || [];
  }
  function pageChanges(page) {
    var h = H.docs && H.docs[page.file];
    return (h && h.ch) || [];
  }
  // the text of a block right after commit `since`: undefined when the block
  // did not change after it, null when it did not exist yet
  function changedSince(page, ptr, since) {
    if (since == null || since < 0) { return undefined; }
    var vs = versions(page, ptr), before = null, after = false;
    vs.forEach(function (v) { if (v.c <= since) { before = v.t; } else { after = true; } });
    return after ? before : undefined;
  }
  function blocksOf(page) { return (page.props || []).concat(page.blocks || []).filter(function (b) { return b.p; }); }
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
  // what changed on a page after `since`, in page order: changed and new
  // blocks and removed ones (ghosts: {a, ptr, t, at})
  var deltaCache = {};
  function pageDelta(page, since) {
    var key = page.id + ':' + since;
    if (deltaCache[key]) { return deltaCache[key]; }
    var d = { items: [], chg: 0, add: 0, rm: 0, ghosts: [] }, now = {}, before = {};
    if (since != null && since >= 0) {
      blocksOf(page).forEach(function (b) {
        now[b.s] = true;
        var was = changedSince(page, b.p, since);
        if (was !== undefined) { before[b.p] = was; }
      });
      var gone = {}, late = {}; // late: texts of blocks that came after `since`; they were not in that version
      pageChanges(page).forEach(function (c) {
        if (c.c <= since) { return; }
        (c.add || []).forEach(function (x) { late[x[1]] = true; });
        (c.ch || []).forEach(function (x) { if (late[x[1]]) { late[x[2]] = true; } });
        (c.rm || []).forEach(function (x) {
          if (now[x[1]] || gone[x[1]] || late[x[1]]) { return; }
          gone[x[1]] = true;
          d.ghosts.push({ a: page.id + '~' + x[0] + '~' + hash(x[1]), ptr: x[0], t: x[1], at: placeGhost(page, x[0]) });
        });
      });
      var item = function (b) {
        if (!(b.p in before)) { return; }
        var isNew = before[b.p] === null;
        d.items.push({ a: page.id + b.p, page: page, ptr: b.p, k: isNew ? 'new' : 'chg', t: b.s });
        if (isNew) { d.add++; } else { d.chg++; }
      };
      (page.props || []).forEach(item);
      var gh = function (i) {
        d.ghosts.forEach(function (g) { if (g.at === i) { d.items.push({ a: g.a, page: page, ptr: g.ptr, k: 'rm', t: g.t }); d.rm++; } });
      };
      (page.blocks || []).forEach(function (b, i) { gh(i); if (b.p) { item(b); } });
      gh((page.blocks || []).length);
    }
    deltaCache[key] = d;
    return d;
  }
  // every change after `since` in the order of the pages
  function allChanges(since) {
    var list = [], n = { chg: 0, add: 0, rm: 0, pages: 0 };
    pages.forEach(function (p) {
      var d = pageDelta(p, since);
      if (d.items.length) { n.pages++; }
      n.chg += d.chg; n.add += d.add; n.rm += d.rm;
      list = list.concat(d.items);
    });
    n.list = list;
    return n;
  }
  // where a pointer is, for the reader: the start of its text
  function placeOf(page, ptr, fallback) {
    var b = page && blocksOf(page).filter(function (x) { return x.p === ptr; })[0];
    return '«' + snip(b ? b.s : fallback || '', 70) + '»';
  }

  // ---- viewed edits: kept with the hash of the text they had ----
  var viewed = store('viewed') || {}, viewFns = [];
  function viewState(it) {
    var v = viewed[it.a];
    return v == null ? '' : v === hash(it.t) ? 'yes' : 'again';
  }
  function setViewed(it, on) {
    viewed = Object.assign({}, viewed);
    if (on) { viewed[it.a] = hash(it.t); } else { delete viewed[it.a]; }
    store('viewed', viewed);
    viewFns.forEach(function (f) { f(viewed); });
  }
  function useViewed() {
    var s = useState(viewed);
    useEffect(function () {
      var f = function (v) { s[1](v); };
      viewFns.push(f);
      return function () { viewFns.splice(viewFns.indexOf(f), 1); };
    }, []);
    return s[0];
  }

  // ---- comments: kept in localStorage until they are sent ----
  var listeners = [];
  var imported = D.comments || [], inRepo = {};
  imported.forEach(function (c) { inRepo[c.id] = true; });
  // comments already imported into the repository leave the browser
  var local = (store('comments') || []).filter(function (c) { return !inRepo[c.id]; });
  store('comments', local);
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
  function unsent(list) { return list.filter(function (c) { return !c.sent; }).length; }
  function markSent() { setLocal(local.map(function (c) { return Object.assign({}, c, { sent: true }); })); }
  function exportJSON() {
    return JSON.stringify({ docc: 'comments', version: 1, comments: local.map(function (c) {
      var o = Object.assign({}, c); delete o.sent; return o;
    }) }, null, 2) + '\n';
  }
  function exportText() {
    var lines = ['{{t:Comments on}} «' + D.title + '» — ' + local.length, ''];
    local.forEach(function (c, k) {
      var p = byId[c.doc];
      lines.push((k + 1) + '. ' + (p ? p.title : c.doc) + ' — «' + snip(c.fragment || c.quote, 80) + '»');
      lines.push(c.text);
      if (c.author) { lines.push('— ' + c.author); }
      lines.push('');
    });
    lines.push('{{t:For the team, do not edit:}}');
    lines.push('docc-comments:' + b64(exportJSON()));
    return lines.join('\n') + '\n';
  }
  function sendCopy() {
    copy(exportText()).then(function () {
      var was = local;
      markSent(); toast('{{t:Copied. Paste it into a message to the team.}}', '{{t:Not sent yet}}', function () { setLocal(was); });
    }, function () { toast('{{t:Could not copy: download the file instead.}}'); });
  }
  function sendFile() {
    save('comments-' + new Date().toISOString().slice(0, 10) + '.json', 'application/json', exportJSON()); markSent();
  }
  window.addEventListener('beforeunload', function (e) {
    if (unsent(local)) { e.preventDefault(); e.returnValue = ''; }
  });
  function removeComment(c) {
    var at = local.indexOf(c);
    setLocal(local.filter(function (x) { return x.id !== c.id; }));
    // put back only this comment: others may have been added or removed since
    toast('{{t:Comment deleted.}}', '{{t:Undo}}', function () {
      var rest = local.slice(); rest.splice(Math.max(0, Math.min(at, rest.length)), 0, c); setLocal(rest);
    });
  }
  function commentsOn(list, doc, ptr) { return list.filter(function (c) { return c.doc === doc && c.path === ptr; }); }

  var Rec = window.SpeechRecognition || window.webkitSpeechRecognition;
  var speechLang = D.lang === 'ru' ? 'ru-RU' : 'en-US';

  // press to dictate, press again to stop; the words go into the text box as they come
  function Mic(props) {
    var rec = useRef(null), st = useState(''), on = useState(false);
    if (!Rec) { return null; }
    function start() {
      var base = props.get(), r = new Rec();
      r.lang = speechLang; r.interimResults = true; r.continuous = true;
      r.onresult = function (ev) {
        var s = '';
        for (var i = 0; i < ev.results.length; i++) { s += ev.results[i][0].transcript; }
        props.set((base ? base.replace(/\s*$/, ' ') : '') + s.trim());
      };
      r.onerror = function (ev) {
        st[1](ev.error === 'not-allowed' ? '{{t:No access to the microphone}}' :
          ev.error === 'network' ? '{{t:Speech service unreachable}}' :
          ev.error === 'no-speech' ? '{{t:No speech heard}}' : '{{t:Dictation failed, type the text instead}}');
      };
      r.onend = function () { rec.current = null; on[1](false); props.listen(false); };
      try { r.start(); rec.current = r; on[1](true); props.listen(true); st[1](''); } catch (err) { st[1]('{{t:Dictation failed, type the text instead}}'); }
    }
    useEffect(function () { return function () { if (rec.current) { rec.current.abort(); } }; }, []);
    function toggle() { if (rec.current) { rec.current.stop(); } else { start(); } }
    return html`<span class="mic-w">
      <button type="button" class=${'btn mic' + (on[0] ? ' on' : '')} aria-pressed=${on[0] ? 'true' : 'false'} onClick=${toggle}>
        ${on[0] ? '■ {{t:Stop dictation}}' : '🎤 {{t:Dictate}}'}</button>
      ${st[0] && html`<small class="err">${st[0]}</small>`}
    </span>`;
  }

  var draft = false; // an editor is open with text not saved yet
  function leaveDraft() { return !draft || confirm('{{t:Discard the unsaved comment?}}'); }
  function Editor(props) {
    var edit = props.edit, frag = edit || props.frag || {};
    var text = useState(edit ? edit.text : ''), author = useState(edit ? edit.author || '' : store('author') || ''), ta = useRef(null);
    var cur = useRef(''); cur.current = text[0];
    var listening = useState(false); // dictation rewrites the text, so typing waits
    draft = text[0].trim() !== (edit ? edit.text : '');
    useEffect(function () { return function () { draft = false; }; }, []);
    function cancel() { if (leaveDraft()) { props.close(); } }
    useEffect(function () { if (ta.current) { ta.current.focus({ preventScroll: true }); ta.current.closest('.editor').scrollIntoView({ block: 'nearest' }); } }, []);
    function ok() {
      var t = text[0].trim(), a = author[0].trim();
      if (!t) { return; }
      if (a) { store('author', a); }
      if (edit) {
        setLocal(local.map(function (c) { return c.id === edit.id ? Object.assign({}, c, { text: t, author: a, sent: false }) : c; }));
      } else {
        var first = !unsent(local), c = { id: uuid(), doc: props.page.id, path: props.ptr, quote: props.quote, text: t,
          author: a, created: new Date().toISOString(), build: H.head || '' };
        if (props.frag) { c.fragment = props.frag.fragment; c.prefix = props.frag.prefix; c.suffix = props.frag.suffix; }
        setLocal(local.concat([c]));
        // nothing leaves the file by itself: say so once
        if (first) { toast('{{t:Saved in this browser only. Send your comments to the team from the Comments tab.}}'); }
      }
      draft = false;
      props.close();
    }
    return html`<div class="editor">
      ${frag.fragment && html`<blockquote class="fq">${frag.fragment}</blockquote>`}
      <label class="who-l">{{t:From}} <input class="who" autocomplete="name" enterkeyhint="next" placeholder="{{t:Your name}}" value=${author[0]}
        onInput=${function (e) { author[1](e.target.value); }} /></label>
      <textarea ref=${ta} rows="3" aria-label="{{t:Comment}}" placeholder="{{t:Your comment}}" value=${text[0]} readOnly=${listening[0]}
        onInput=${function (e) { text[1](e.target.value); e.target.style.height = 'auto'; e.target.style.height = e.target.scrollHeight + 2 + 'px'; }}
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

  function Status(props) {
    var c = props.c;
    if (!props.repo) { return c.sent ? html`<span class="chip">{{t:sent}}</span>` : html`<span class="chip warn">{{t:not sent}}</span>`; }
    return c.status === 'done' ? html`<span class="chip ok">{{t:taken into account}}</span>` : html`<span class="chip">{{t:the team is working on it}}</span>`;
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
        <${Status} c=${c} repo=${props.repo} />
        ${!props.repo && html`<span class="acts">
          ${c.sent && html`<button type="button" class="link" onClick=${function () {
            setLocal(local.map(function (x) { return x.id === c.id ? Object.assign({}, x, { sent: false }) : x; }));
          }}>{{t:Not sent yet}}</button>`}
          <button type="button" class="link" onClick=${function () { edit[1](true); }}>{{t:Edit}}</button>
          <button type="button" class="link" onClick=${function () { removeComment(c); }}>{{t:Delete}}</button></span>`}
      </div>
      ${c.reply && html`<div class="reply"><small>{{t:Team's answer}}</small>${c.reply}</div>`}
    </aside>`;
  }

  // a dialog: native <dialog>, so focus stays inside and Esc closes it
  function Modal(props) {
    var d = useRef(null);
    useEffect(function () { var el = d.current; if (el.showModal && !el.open) { el.showModal(); } }, []);
    return html`<dialog ref=${d} class=${'dlg ' + (props.cls || '')} aria-label=${props.title} onClose=${props.close}
      onClick=${function (e) { if (e.target === d.current) { d.current.close(); } }}>
      <div class="dlg-h"><h2>${props.title}</h2><button type="button" class="btn" onClick=${function () { d.current.close(); }}>{{t:Close}}</button></div>
      <div class="dlg-b">${props.children}</div>
    </dialog>`;
  }

  function History(props) {
    var vs = props.vs.slice().reverse();
    return html`${vs.map(function (v, k) {
      var prev = vs[k + 1];
      return html`<div class="ver">
        <div class="vh">${versionLine(v.c)}${!prev && html` · <i>{{t:first version}}</i>`}</div>
        <${Diff} a=${prev ? prev.t : null} b=${v.t} />
      </div>`;
    })}`;
  }
  function PageHistory(props) {
    var page = props.page, ch = pageChanges(page).slice().reverse();
    function list(label, items, fn) {
      return items && items.length ? html`<div class="pg-g"><small>${label}</small>${items.map(fn)}</div>` : null;
    }
    function place(ptr, text) {
      var b = blocksOf(page).some(function (x) { return x.p === ptr; });
      return b ? html`<a class="ptr" href=${blockHash(page.id, ptr)} onClick=${props.close}>${placeOf(page, ptr, text)}</a>` : html`<span class="ptr">${placeOf(page, ptr, text)}</span>`;
    }
    return html`${!ch.length && html`<p class="muted">{{t:No changes in this file's history.}}</p>`}
      ${ch.map(function (c) {
        return html`<div class="ver">
          <div class="vh">${versionLine(c.c)}${c.new && html` · <i>{{t:page created}}</i>`}</div>
          ${list('{{t:Changed}}', c.ch, function (x) { return html`<div class="pg-i">${place(x[0], x[2])}<${Diff} a=${x[1]} b=${x[2]} /></div>`; })}
          ${list('{{t:Added}}', c.add, function (x) { return html`<div class="pg-i">${place(x[0], x[1])}<${Diff} a=${null} b=${x[1]} /></div>`; })}
          ${list('{{t:Removed}}', c.rm, function (x) { return html`<div class="pg-i"><div class="dv"><del>${plain(x[1])}</del></div></div>`; })}
        </div>`;
      })}`;
  }

  // ---- one block: its HTML, the edits drawn into it, its comments and tools ----
  function Block(props) {
    var b = props.b, page = props.page, ptr = b.p, work = props.work, a = page.id + (ptr || '');
    var body = useRef(null), whole = useState(null), lost = useState({}), more = useState(false);
    var before = work && ptr ? changedSince(page, ptr, props.since) : undefined;
    var changed = before !== undefined, isNew = before === null;
    var mine = work && ptr ? commentsOn(props.comments, page.id, ptr) : [];
    var repo = work && ptr ? commentsOn(imported, page.id, ptr) : [];
    var frags = mine.concat(repo).filter(function (c) { return c.fragment; });
    var key = (work ? 'w' : 'r') + props.since + frags.map(function (c) { return c.id; }).join();
    useEffect(function () {
      var el = body.current, gone = {};
      whole[1](null);
      if (!el || !work) { lost[1](gone); return; }
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
    var inner = html`<div class="blk-body" key=${key} ref=${body} id=${props.id} dangerouslySetInnerHTML=${{ __html: b.h }}></div>`;
    if (props.label) { inner = html`<div class="blk-body prop" key=${key} id=${props.id}><div class="pk">${props.label}</div><div ref=${body} dangerouslySetInnerHTML=${{ __html: b.h }}></div></div>`; }
    if (!ptr || !work) { return html`<div class=${'blk' + (props.label ? ' is-prop' : '')} data-p=${ptr || undefined} data-a=${ptr ? a : undefined} tabindex=${ptr ? '-1' : undefined}>${inner}</div>`; }
    var cards = repo.map(function (c) { return [c, true]; }).concat(mine.map(function (c) { return [c, false]; }));
    var shown = more[0] || cards.length <= 2 ? cards : cards.slice(0, 2);
    var vs = versions(page, ptr), item = props.item, seen = item ? viewState(item) : '';
    var cls = 'blk' + (props.label ? ' is-prop' : '') + (changed ? (isNew ? ' new' : ' chg') : '') + (seen === 'yes' ? ' seen' : '') +
      (props.cur === a ? ' cur' : '') + (cards.length ? ' has-c' : '');
    return html`<div class=${cls} data-p=${ptr} data-a=${a} tabindex="-1"
      onClick=${function (e) { if (!e.target.closest('a,button,input,textarea,summary,.blk-side')) { props.setCur(a); } }}>
      ${changed && html`<span class="sr">${isNew ? '{{t:new paragraph}}' : '{{t:changed paragraph}}'}</span>`}
      ${whole[0] && html`<details class="old" open=${whole[0] === 'code'}><summary>{{t:Previous text}}</summary><div class="dv"><del>${plain(before)}</del></div></details>`}
      ${inner}
      <div class="blk-side">
        <div class="tools">
          <button type="button" class="btn sm" onClick=${function () { props.setCur(a); props.openEditor({ page: page, ptr: ptr, quote: b.s }); }}>💬 {{t:Comment}}</button>
          ${vs.length > 1 && html`<button type="button" class="btn sm" onClick=${function () { props.history(page, ptr); }}>🕘 {{t:History}}</button>`}
        </div>
        ${shown.map(function (x) { return html`<${Card} key=${x[0].id} c=${x[0]} repo=${x[1]} lost=${lost[0][x[0].id]} active=${props.activeC === x[0].id} />`; })}
        ${cards.length > 2 && !more[0] && html`<button type="button" class="link" onClick=${function () { more[1](true); }}>{{t:More comments:}} ${cards.length - 2}</button>`}
        ${props.editor && props.editor.ptr === ptr && props.editor.page === page && html`<${Editor} page=${page} ptr=${ptr} quote=${b.s} frag=${props.editor.frag} close=${props.closeEditor} />`}
      </div>
    </div>`;
  }

  // a removed paragraph in its old place, folded
  function Ghost(props) {
    var g = props.g, it = { a: g.a, t: g.t };
    return html`<details class=${'ghost' + (viewState(it) === 'yes' ? ' seen' : '') + (props.cur === g.a ? ' cur' : '')} data-a=${g.a} tabindex="-1">
      <summary><span class="sr">{{t:removed paragraph}}. </span>{{t:Removed paragraph:}} “${snip(g.t, 60)}”</summary>
      <div class="dv"><del>${plain(g.t)}</del></div>
    </details>`;
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
  function Chip(props) {
    var p = byId[props.id];
    return p ? html`<a class="pchip" href=${'#' + p.id}>${p.title}</a>` : null;
  }
  function Overview(props) {
    var ch = props.changes, rows = pages.filter(function (p) { return pageDelta(p, props.since).items.length; });
    return html`<section class="overview" aria-label="{{t:Overview of edits}}">
      <h2>{{t:Edits since}} ${props.from}</h2>
      <p>{{t:Changed:}} ${ch.chg} · {{t:Added:}} ${ch.add} · {{t:Removed:}} ${ch.rm}</p>
      <ul>${rows.map(function (p) { return html`<li><a href=${'#' + p.id}>${p.title}</a> — ${pageDelta(p, props.since).items.length}</li>`; })}</ul>
      <div class="row"><button type="button" class="btn pri" onClick=${props.start}>{{t:Start review}}</button>
        <button type="button" class="btn" onClick=${props.hide}>{{t:Not now}}</button></div>
    </section>`;
  }

  function Page(props) {
    var page = props.page, main = useRef(null), work = props.work;
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
    var prev = pages[page.i - 1], next = pages[page.i + 1];
    var shown = {}; shown[page.id] = true;
    (page.kids || []).forEach(function (k) { shown[k] = true; });
    if (page.parent) { shown[page.parent] = true; }
    (page.crumbs || []).forEach(function (k) { shown[k] = true; });
    var refs = (page.refs || []).filter(function (r) { return !shown[r]; });
    var d = pageDelta(page, work ? props.since : null), items = {};
    d.items.forEach(function (it) { items[it.a] = it; });
    var bl = { comments: props.comments, since: props.since, page: page, work: work, cur: props.cur, setCur: props.setCur,
      editor: props.editor, openEditor: props.openEditor, closeEditor: props.closeEditor, history: props.history, activeC: props.activeC };
    function ghosts(i) {
      return work ? d.ghosts.filter(function (g) { return g.at === i; }).map(function (g) { return html`<${Ghost} key=${g.a} g=${g} cur=${props.cur} />`; }) : null;
    }
    var badges = work && (page.kind || page.status || page.priority || page.hidden || page.tags);
    return html`<article class=${'page' + (work ? ' work' : '')} ref=${main}>
      ${D.brand.print && html`<div dangerouslySetInnerHTML=${{ __html: D.brand.print }}></div>`}
      ${props.missing && html`<p class="notice" role="alert">{{t:Nothing is found at this link:}} «${props.missing}». {{t:The first page is shown; find the place in the tree or the search.}}</p>`}
      ${props.overview}
      ${page.crumbs && html`<nav class="crumbs" aria-label="{{t:Breadcrumb}}">${page.crumbs.map(function (id, k) {
        return html`${k > 0 && html`<span aria-hidden="true">›</span>`}<a href=${'#' + id}>${byId[id] ? byId[id].title : id}</a>`;
      })}</nav>`}
      <h1>${page.title}</h1>
      ${page.hidden && !work && html`<p class="badges"><span class="chip bad">{{t:hidden}}</span></p>`}
      ${badges && html`<p class="badges">
        ${page.kind && html`<span class="chip kind">${page.kind}</span>`}
        ${page.status && html`<span class=${'chip ' + (page.sc || '')}>${page.status}</span>`}
        ${page.priority && html`<span class="chip">{{t:priority}} ${page.priority}</span>`}
        ${page.hidden && html`<span class="chip bad">{{t:hidden}}</span>`}
        ${(page.tags || []).map(function (t) { return html`<span class="chip tag">#${t}</span>`; })}
      </p>`}
      <div class="doc">
        ${page.props && html`<div class="props">${page.props.map(function (p) {
          return html`<${Block} ...${bl} key=${p.p || p.k} b=${p} label=${p.k} id=${p.id} item=${items[page.id + p.p]} />`;
        })}</div>`}
        ${(page.blocks || []).map(function (b, i) {
          return html`${ghosts(i)}<${Block} ...${bl} key=${b.p || 'x' + i} b=${b} item=${items[page.id + b.p]} />`;
        })}
        ${ghosts((page.blocks || []).length)}
      </div>
      ${page.kids && html`<h2 class="rh">{{t:Pages in this section}}</h2><div class="rows">${page.kids.map(function (id) {
        var k = byId[id];
        return k && html`<a class="row" href=${'#' + id}><span class="ct">${k.title}</span>${k.summary && html`<span class="cs">${k.summary}</span>`}</a>`;
      })}</div>`}
      ${work && (refs.length || page.covers) && html`<h2 class="rh">{{t:Related}}</h2><div class="related">
        ${refs.length > 0 && html`<div class="rg"><span class="rl">{{t:Referenced by}}</span>${refs.map(function (id) { return html`<${Chip} id=${id} />`; })}</div>`}
        ${page.covers && html`<div class="rg"><span class="rl">{{t:Covered by}}</span>${page.covers.map(function (c) { return html`<code>${c}</code>`; })}</div>`}
      </div>`}
      ${(prev || next) && html`<nav class="pn" aria-label="{{t:Neighbouring pages}}">
        ${prev && html`<a class="prev" href=${'#' + prev.id}><small>{{t:Previous}}</small><span>${prev.title}</span></a>`}
        ${next && html`<a class="next" href=${'#' + next.id}><small>{{t:Next}}</small><span>${next.title}</span></a>`}
      </nav>`}
      ${D.brand.foot && html`<div dangerouslySetInnerHTML=${{ __html: D.brand.foot }}></div>`}
    </article>`;
  }

  // ---- left column: tree, edits, comments; search on top ----
  function nest(list) {
    var root = { kids: [] }, stack = [root];
    list.forEach(function (e) {
      var node = { id: e[0], kids: [] };
      stack.length = e[1] + 1;
      (stack[e[1]] || root).kids.push(node);
      stack.push(node);
    });
    return root.kids;
  }
  var tree = nest(D.nav.tree), orphans = nest(D.nav.orphans);
  function contains(node, id) {
    return node.id === id || node.kids.some(function (k) { return contains(k, id); });
  }
  function subtreeEdits(node, since) {
    var n = byId[node.id] ? pageDelta(byId[node.id], since).items.length : 0;
    node.kids.forEach(function (k) { n += subtreeEdits(k, since); });
    return n;
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
      ${all.slice(lo, hi).map(function (x) { return html`<${Node} ...${props} key=${x.id} node=${x} />`; })}
      ${hi < n && html`<li><button type="button" class="more" onClick=${function () { win[1]([lo, Math.min(n, hi + 30)]); }}>${n - hi} {{t:more}} ↓</button></li>`}
    </ul>`;
  }
  function Node(props) {
    var x = props.node, p = byId[x.id], inside = contains(x, props.cur);
    var open = useState(null);
    var isOpen = open[0] == null ? inside : open[0];
    useEffect(function () { if (inside) { open[1](null); } }, [props.cur]);
    if (!p) { return null; }
    var n = props.work ? subtreeEdits(x, props.since) : 0, here = p.id === props.cur;
    return html`<li class=${x.kids.length ? 'parent' : ''}>
      ${x.kids.length > 0 && html`<button class="tw" type="button" aria-expanded=${isOpen ? 'true' : 'false'}
        aria-label=${(isOpen ? '{{t:Collapse}}' : '{{t:Expand}}') + ': ' + p.title} onClick=${function () { open[1](!isOpen); }}></button>`}
      <a href=${'#' + p.id} aria-current=${here ? 'page' : undefined} onClick=${props.picked}><span>${p.title}</span>${p.hidden && html` <span class="chip bad">{{t:hidden}}</span>`}${n > 0 && html`<span class="cnt" title=${'{{t:Edits}}: ' + n}><span class="sr">{{t:Edits}}: </span>${n}</span>`}</a>
      ${here && props.toc.length > 0 && html`<ul class="toc">${props.toc.map(function (h) {
        return html`<li><a href=${'#' + h.id} class=${'l' + h.level + (props.spy === h.id ? ' on' : '')} onClick=${props.picked}>${h.text}</a></li>`;
      })}</ul>`}
      ${x.kids.length > 0 && isOpen && html`<${Items} ...${props} nodes=${x.kids} />`}
    </li>`;
  }
  function Group(props) {
    var open = useState(false);
    return html`<h3 class="gh"><button class="gt" type="button" aria-expanded=${open[0] ? 'true' : 'false'}
      onClick=${function () { open[1](!open[0]); }}>${props.label} <span>${props.n}</span></button></h3>
      ${open[0] && html`<div class="gb">${props.children}</div>`}`;
  }
  function Tree(props) {
    return html`<nav id="tree" aria-label="{{t:Document tree}}">
      ${tree.length > 0 && html`<${Items} ...${props} nodes=${tree} />`}
      ${orphans.length > 0 && html`<${Group} label="{{t:Without parent}}" n=${D.nav.orphans.length}><${Items} ...${props} nodes=${orphans} /><//>`}
      ${D.nav.uncovered && html`<${Group} label="{{t:Documents without code coverage}}" n=${D.nav.uncovered.length}>
        <ul>${D.nav.uncovered.map(function (id) { return html`<li><a href=${'#' + id} onClick=${props.picked}>${byId[id] ? byId[id].title : id}</a></li>`; })}</ul><//>`}
    </nav>`;
  }

  // the search finds paragraphs: every word in the paragraph or in the title of its page
  var index = null, LIMIT = 30;
  function Search(props) {
    var q = props.q, sel = useState(0);
    var v = q[0].trim(), hits = [], total = 0;
    if (v) {
      index = index || [].concat.apply([], pages.map(function (p) {
        return [{ p: p, ptr: '', text: '' }].concat(blocksOf(p).map(function (b) { return { p: p, ptr: b.p, text: textOf(b.h) }; }));
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

  function glyph(k) { return k === 'new' ? '+' : k === 'rm' ? '−' : '~'; }
  function left(list) { return list.filter(function (it) { return viewState(it) !== 'yes'; }).length; }
  function EditsTab(props) {
    var list = props.changes ? props.changes.list : [], groups = [];
    list.forEach(function (it, k) {
      var g = groups[groups.length - 1];
      if (!g || g.page !== it.page) { g = { page: it.page, rows: [] }; groups.push(g); }
      g.rows.push([it, k]);
    });
    // the list opens at the current edit, else at the current page
    useEffect(function () {
      var on = document.querySelector('.er.on') || document.querySelector('.eg[data-pg="' + CSS.escape(props.page.id) + '"]');
      if (on) { on.scrollIntoView({ block: 'center' }); }
    }, [props.cur, props.page]);
    if (!list.length) { return html`<p class="muted pad">{{t:No changes}}</p>`; }
    var rest = left(list);
    return html`<div class="pad">
      <p class="left">${rest ? '{{t:Left}} ' + rest + ' {{t:of}} ' + list.length : '{{t:All changes viewed}}'}</p>
      <progress max=${list.length} value=${list.length - rest} aria-label="{{t:Viewed}}"></progress>
      ${groups.map(function (g) {
        return html`<h3 class="eg" data-pg=${g.page.id}>${g.page.title} <span>${g.rows.length}</span></h3><ul class="edits">${g.rows.map(function (x) {
          var it = x[0], st = viewState(it);
          return html`<li><button type="button" class=${'er ' + it.k + (x[1] === props.cur ? ' on' : '')} onClick=${function () { props.go(x[1]); }}>
            <span class=${'g ' + it.k} aria-hidden="true">${st === 'yes' ? '✓' : glyph(it.k)}</span>
            <span class="sr">${it.k === 'new' ? '{{t:new paragraph}}' : it.k === 'rm' ? '{{t:removed paragraph}}' : '{{t:changed paragraph}}'}${st === 'yes' ? ', {{t:viewed}}' : ''}: </span>
            <span class="et">${snip(it.t, 60)}${st === 'again' && html` <small class="again">{{t:Changed after you viewed it}}</small>`}</span></button></li>`;
        })}</ul>`;
      })}
    </div>`;
  }
  function CommentsTab(props) {
    var list = props.comments, wait = unsent(list), rows = [];
    list.forEach(function (c) { rows.push([c, false]); });
    imported.forEach(function (c) { rows.push([c, true]); });
    return html`<div class="pad">
      ${wait > 0 && html`<div class="send" id="send">
        <p>{{t:Comments are kept only in this browser until you send them to the team.}}</p>
        <div class="row"><button type="button" class="btn pri" onClick=${sendCopy}>📋 {{t:Copy for sending}}</button>
          <button type="button" class="btn" onClick=${sendFile}>{{t:Download as a file}}</button></div>
      </div>`}
      ${!rows.length && html`<p class="muted">{{t:No comments yet. Select text or press «Comment» next to a paragraph.}}</p>`}
      <ul class="clist">${rows.map(function (x) {
        var c = x[0], p = byId[c.doc];
        return html`<li><button type="button" class="cr" onClick=${function () { props.show(c); }}>
          <span class="cp">${p ? p.title : c.doc} · ${c.fragment ? '«' + snip(c.fragment, 50) + '»' : placeOf(p, c.path, c.quote)}</span>
          <span class="ctx">${c.text}</span><${Status} c=${c} repo=${x[1]} /></button></li>`;
      })}</ul>
    </div>`;
  }
  // tabs after the WAI-ARIA pattern: arrows move between them
  function Side(props) {
    var tabs = props.work ? [['pages', '{{t:Pages}}', 0], ['edits', '{{t:Edits}}', props.changes ? props.changes.list.length : 0],
      ['comments', '{{t:Comments}}', props.comments.length + imported.length]] : null;
    var tab = props.work ? props.tab : 'pages';
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
      ${tabs && html`<div class="tabs" role="tablist" aria-label="{{t:Navigation}}" onKeyDown=${key}>${tabs.map(function (t) {
        return html`<button type="button" role="tab" id=${'tab-' + t[0]} aria-selected=${t[0] === tab ? 'true' : 'false'} aria-controls="tabpanel"
          tabindex=${t[0] === tab ? '0' : '-1'} onClick=${function () { props.setTab(t[0]); }}>${t[1]}${t[2] > 0 && html` <span class="tc">${t[2]}</span>`}</button>`;
      })}</div>`}
      <div id="tabpanel" role=${tabs ? 'tabpanel' : undefined} aria-labelledby=${tabs ? 'tab-' + tab : undefined} class="panel">
        ${tab === 'pages' && html`<${Tree} cur=${props.page.id} since=${props.since} work=${props.work} toc=${props.toc} spy=${props.spy} picked=${props.picked} />`}
        ${tab === 'edits' && html`<${EditsTab} changes=${props.changes} cur=${props.cur} page=${props.page} go=${function (k) { props.picked(); props.goItem(k); }} />`}
        ${tab === 'comments' && html`<${CommentsTab} comments=${props.comments} show=${function (c) { props.picked(); props.showComment(c); }} />`}
      </div>
    </div>`;
  }

  // the edits one by one, like F7 in an IDE
  function Navigator(props) {
    var list = props.changes.list, it = list[props.cur], rest = left(list);
    var text = props.cur < 0 ? '{{t:Edits}}: ' + list.length : '{{t:Change}} ' + (props.cur + 1) + ' {{t:of}} ' + list.length;
    if (props.frag) {
      return html`<div class="nav" role="region" aria-label="{{t:Comment on the selection}}">
        <button type="button" class="btn pri" onMouseDown=${function (e) { e.preventDefault(); }} onClick=${props.fragGo}>💬 {{t:Comment on the selection}}</button></div>`;
    }
    return html`<div class="nav" role="region" aria-label="{{t:Edit navigation}}">
      <button type="button" class="btn ico" aria-label="{{t:Previous change}}" title="{{t:Previous change}} (K)" onClick=${function () { props.step(-1); }}>◀</button>
      <span class="pos">${text}${rest ? html`<span class="rest"> · {{t:left}} ${rest}</span>` : html`<span class="rest"> · {{t:All changes viewed}}</span>`}
        ${it && viewState(it) === 'again' && html`<small class="again"> · {{t:Changed after you viewed it}}</small>`}
        ${it && html`<span class="npg">${it.page.title}</span>`}</span>
      <button type="button" class="btn ico" aria-label="{{t:Next change}}" title="{{t:Next change}} (J)" onClick=${function () { props.step(1); }}>▶</button>
      ${it && html`<label class="seen-l"><input type="checkbox" checked=${viewState(it) === 'yes'} onChange=${function (e) { setViewed(it, e.target.checked); }} /> {{t:Viewed}}</label>`}
    </div>`;
  }
  // ticks of the page's edits along the window edge, like the scrollbar of an IDE
  function ChangeMap(props) {
    var ticks = useState([]);
    useEffect(function () {
      function measure() {
        var h = document.documentElement.scrollHeight || 1;
        ticks[1](props.items.map(function (it) {
          var el = document.querySelector('[data-a="' + CSS.escape(it.a) + '"]');
          return el ? { it: it, y: (el.getBoundingClientRect().top + window.scrollY) / h } : null;
        }).filter(Boolean));
      }
      measure();
      var t = setTimeout(measure, 300);
      window.addEventListener('resize', measure);
      document.addEventListener('toggle', measure, true);
      return function () { clearTimeout(t); window.removeEventListener('resize', measure); document.removeEventListener('toggle', measure, true); };
    }, [props.items, props.comments]);
    return html`<div class="cmap" aria-hidden="true">${ticks[0].map(function (x) {
      return html`<span class=${'tk ' + x.it.k + (viewState(x.it) === 'yes' ? ' seen' : '') + (props.curA === x.it.a ? ' on' : '')}
        style=${'top:' + (x.y * 100).toFixed(2) + '%'} title=${snip(x.it.t, 40)} onClick=${function () { props.go(x.it); }}></span>`;
    })}</div>`;
  }

  function Keys() {
    var rows = [['F7 · J', '{{t:next change}}'], ['Shift+F7 · K', '{{t:previous change}}'], ['V', '{{t:mark viewed and go to the next}}'],
      ['C', '{{t:comment on the selection or the paragraph}}'], ['/ · Ctrl+K', '{{t:Search}}'], ['?', '{{t:this help}}'], ['Esc', '{{t:close}}'], ['Ctrl+Enter', '{{t:save the comment}}']];
    return html`<h3>{{t:Colours}}</h3>
      <ul class="legend">
        <li><ins>{{t:added text}}</ins></li><li><del>{{t:removed text}}</del></li>
        <li><span class="lb chg"></span>{{t:changed paragraph}}</li><li><span class="lb new"></span>{{t:new paragraph}}</li>
        <li><span class="lb rm"></span>{{t:removed paragraph}}</li><li><span class="lb chg seen"></span>{{t:viewed}}</li>
        <li><mark class="frag">{{t:commented fragment}}</mark></li>
      </ul>
      <h3>{{t:Keys}}</h3>
      <dl class="keys">${rows.map(function (r) { return html`<dt><kbd>${r[0]}</kbd></dt><dd>${r[1]}</dd>`; })}</dl>
      <p class="muted">{{t:The letter keys work in the Work mode, outside text fields.}}</p>`;
  }
  function SinceDialog(props) {
    var real = [];
    commits.forEach(function (c, k) { if (c.h) { real.push(k); } });
    function label(k) { return k === 0 && H.base ? '{{t:the version sent on}} ' + date(0) : versionLine(k); }
    return html`<label class="since-l">{{t:Show changes since}}
      <select value=${props.since == null ? '' : String(props.since)} onChange=${function (e) { props.set(e.target.value === '' ? null : +e.target.value); }}>
        <option value="">{{t:do not show}}</option>
        ${real.slice().reverse().map(function (k) { return html`<option value=${String(k)}>${label(k)}</option>`; })}
      </select></label>`;
  }
  // «⋯»: a native popover with plain buttons
  function More(props) {
    var pop = useRef(null);
    function item(label, fn, cls) {
      return html`<button type="button" class=${'mi' + (cls ? ' ' + cls : '')} onClick=${function () { if (pop.current.hidePopover) { pop.current.hidePopover(); } fn(); }}>${label}</button>`;
    }
    var page = props.page, work = props.work, n = unsent(props.comments);
    return html`<button type="button" class="btn ico more-b" popovertarget="more" aria-label="{{t:More actions}}" title="{{t:More actions}}">⋯${n > 0 && html`<span class="dot" aria-hidden="true"></span>`}</button>
      <div id="more" popover="auto" ref=${pop} class="pop">
        ${work && props.changes && props.changes.list.length > 0 && item('{{t:Overview of edits}}', props.overview)}
        ${commits.some(function (c) { return c.h; }) && item('{{t:Starting point…}}', function () { props.dialog('since'); })}
        ${work && commits.length > 0 && item('{{t:History of the page}}', function () { props.dialog('page'); })}
        ${work && props.blockHist && item('{{t:History of the paragraph}}', props.blockHist)}
        <hr />
        ${item('{{t:Download this page (.md)}}', function () { save(page.id + '.md', 'text/markdown', page.md); })}
        ${item('{{t:Download all in one file (.md)}}', function () { save('docs.md', 'text/markdown', D.single); })}
        ${item('{{t:Download all as an archive (.zip)}}', function () { save('docs.zip', 'application/zip', unzip64(D.zip)); })}
        ${item('{{t:Print}}', function () { window.print(); })}
        <hr />
        ${n > 0 && item('{{t:Send comments}} (' + n + ')', props.send, 'phone')}
        ${item('{{t:Keys and colours}}', function () { props.dialog('keys'); })}
        ${props.comments.length > 0 && item('{{t:Delete all my comments}}', function () {
          if (confirm('{{t:Delete all comments of this browser?}}')) { setLocal([]); }
        }, 'bad')}
      </div>`;
  }

  // the selected text of one block, with what stands around it, for a comment on a fragment
  function selected() {
    var s = window.getSelection();
    if (!s || s.isCollapsed || !s.rangeCount) { return null; }
    var r = s.getRangeAt(0), start = r.startContainer.nodeType === 1 ? r.startContainer : r.startContainer.parentNode;
    var body = start.closest && start.closest('.blk-body'), blk = body && body.closest('.blk[data-a]');
    if (!blk) { return null; }
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
    var q = useState(''), tab = useState(store('tab') || 'pages'), mode = useState(store('mode') || 'read');
    var sinceHash = useState(store('since')), cur = useState(-1), curA = useState(null), editor = useState(null);
    var dlg = useState(null), toc = useState([]), spy = useState(null), frag = useState(null), activeC = useState(null);
    var ovOff = useState(!!store('overview')), hint = useState(!!store('hint')), live = useState('');
    var comments = useComments(), seen = useViewed(), pending = useRef(null), keep = useRef(null), dr = useRef(null);
    var page = r[0].page, work = mode[0] === 'work';
    // the chosen version is kept by hash: indexes change with every commit;
    // the file's own starting point is the default, "off" turns it off
    var since = null;
    commits.forEach(function (c, k) { if (c.h && c.h === sinceHash[0]) { since = k; } });
    if (since == null && sinceHash[0] !== 'off' && H.base && commits.length) { since = 0; }
    var all = since != null ? allChanges(since) : null, changes = work ? all : null;
    var count = all ? all.list.length : 0, from = since != null ? date(since) : '';
    void seen;

    function setMode(m) {
      // keep the paragraph at the top of the window where it is
      var top = [].filter.call(document.querySelectorAll('.blk[data-a]'), function (b) { return b.getBoundingClientRect().bottom > 80; })[0];
      keep.current = top && window.scrollY > 40 ? top.getAttribute('data-a') : null;
      mode[1](m); store('mode', m);
      if (m === 'work') { hint[1](true); store('hint', true); }
    }
    function setTab(t) { tab[1](t); store('tab', t); }
    function setSince(v) { var h = v == null ? 'off' : commits[v].h; sinceHash[1](h); store('since', h); cur[1](-1); }
    function find(a) { return a && document.querySelector('[data-a="' + CSS.escape(a) + '"]'); }
    function focusEl(el, say) {
      (el.tagName === 'DETAILS' ? el.querySelector('summary') : el).focus({ preventScroll: true });
      var tall = el.offsetHeight > window.innerHeight - 160;
      el.scrollIntoView({ block: tall ? 'start' : 'center', behavior: reduced ? 'auto' : 'smooth' });
      if (!reduced) { el.classList.add('flash'); setTimeout(function () { el.classList.remove('flash'); }, 900); }
      if (say) { live[1](say); }
    }
    function goItem(k) {
      var list = all.list, it = list[k];
      if (!it) { return; }
      cur[1](k); curA[1](it.a); pending.current = it.a;
      if (!ovOff[0]) { ovOff[1](true); store('overview', true); }
      var kind = it.k === 'new' ? '{{t:new paragraph}}' : it.k === 'rm' ? '{{t:removed paragraph}}' : '{{t:changed paragraph}}';
      pending.say = '{{t:Change}} ' + (k + 1) + ' {{t:of}} ' + list.length + ': ' + kind + ', ' + it.page.title;
      if (it.page !== page) { toast('{{t:Page:}} «' + it.page.title + '»'); }
      var h = it.k === 'rm' ? '#' + it.page.id : blockHash(it.page.id, it.ptr);
      if (it.page === page && (it.k === 'rm' || decode(location.hash) === decode(h))) { var el = find(it.a); pending.current = null; if (el) { focusEl(el, pending.say); } }
      else { location.hash = h; }
    }
    function step(d) {
      var n = all.list.length;
      if (!n) { return; }
      goItem(cur[0] < 0 ? (d > 0 ? 0 : n - 1) : (cur[0] + d + n) % n);
    }
    function nextUnviewed() {
      var list = all.list, n = list.length;
      for (var i = 1; i <= n; i++) {
        var k = ((cur[0] < 0 ? -1 : cur[0]) + i) % n;
        if (viewState(list[k]) !== 'yes') { goItem(k); return; }
      }
      toast('{{t:All changes viewed}}');
    }
    function markViewed() {
      var it = all.list[cur[0]];
      if (!it) { nextUnviewed(); return; }
      setViewed(it, true);
      toast('{{t:Marked as viewed.}}', '{{t:Undo}}', function () { setViewed(it, false); });
      nextUnviewed();
    }
    function openEditor(e) {
      if (!work || (editor[0] && !leaveDraft())) { return; }
      draft = false; editor[1](e); frag[1](null);
    }
    function blockOf(a) {
      if (!a || a.indexOf(page.id + '/') !== 0) { return null; }
      var ptr = a.slice(page.id.length);
      return blocksOf(page).filter(function (b) { return b.p === ptr; })[0] || null;
    }
    function commentKey() {
      var f = frag[0] || selected();
      if (f) { openEditor({ page: page, ptr: f.ptr, quote: (blockOf(f.a) || {}).s, frag: f.frag }); return; }
      var b = blockOf(curA[0]) || blockOf((function () {
        // only a paragraph in the window: an editor out of sight would take the typing
        var seen = [].filter.call(document.querySelectorAll('.blk[data-a]'), function (x) {
          var q = x.getBoundingClientRect(); return q.bottom > 80 && q.top < window.innerHeight * 0.6;
        });
        var mid = seen[seen.length - 1];
        return mid && mid.getAttribute('data-a');
      })());
      if (b) { curA[1](page.id + b.p); openEditor({ page: page, ptr: b.p, quote: b.s }); } else { toast('{{t:No paragraph in the window to comment on.}}'); }
    }
    function showComment(c) {
      if (!work) { setMode('work'); }
      activeC[1](c.id); pending.current = 'c:' + c.id;
      var h = blockHash(c.doc, c.path);
      if (decode(location.hash) === decode(h)) { r[1](route(h)); } else { location.hash = h; }
    }
    function openSend() {
      if (!work) { setMode('work'); }
      setTab('comments'); if (!wide[0]) { drawer[1](true); }
      setTimeout(function () { var s = document.querySelector('#send .btn'); if (s) { s.focus(); } }, 50);
    }
    function picked() { if (!wide[0]) { drawer[1](false); } }

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
      var st = history.state, t = r[0].target && document.getElementById(r[0].target);
      var b = r[0].block && document.querySelector('[data-p="' + CSS.escape(r[0].block) + '"]');
      var p = pending.current;
      pending.current = null;
      if (p && p.indexOf('c:') === 0) {
        var card = document.querySelector('.card[data-c="' + CSS.escape(p.slice(2)) + '"]');
        if (card) { focusEl(card); } else if (b) { focusEl(b); }
      } else if (p && find(p)) { focusEl(find(p), pending.say); }
      else if (st && st.y != null && !r[0].block) { window.scrollTo({ top: st.y, behavior: 'instant' }); }
      else if (b) {
        focusEl(b);
        if (all) { all.list.forEach(function (x, k) { if (x.page === page && x.ptr === r[0].block && x.k !== 'rm') { cur[1](k); curA[1](x.a); } }); }
      } else if (t) { t.scrollIntoView({ behavior: 'instant' }); } else { window.scrollTo({ top: 0, behavior: 'instant' }); }
      var here = document.querySelector('#tree [aria-current]');
      if (here) { here.scrollIntoView({ block: 'nearest' }); }
    }, [r[0]]);
    useEffect(function () {
      var el = keep.current && find(keep.current);
      keep.current = null;
      if (el) { el.scrollIntoView({ block: 'start', behavior: 'instant' }); }
      document.documentElement.classList.toggle('w-mode', work && count > 0);
    }, [mode[0]]);
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
    // a comment on the selected words: a button under the selection, or a pill in the bottom bar on touch
    useEffect(function () {
      if (!work) { frag[1](null); return; }
      var t;
      function up() { setTimeout(function () { frag[1](selected()); }, 0); }
      function change() {
        var s = window.getSelection();
        if (!s || s.isCollapsed) { frag[1](null); return; }
        if (coarse) { clearTimeout(t); t = setTimeout(function () { frag[1](selected()); }, 300); }
      }
      function scroll() { if (!coarse) { frag[1](null); } }
      document.addEventListener('mouseup', up); document.addEventListener('selectionchange', change);
      window.addEventListener('scroll', scroll, { passive: true });
      return function () { document.removeEventListener('mouseup', up); document.removeEventListener('selectionchange', change); window.removeEventListener('scroll', scroll); };
    }, [work]);
    // keys: search, help, and the edits in the Work mode
    useEffect(function () {
      function key(e) {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); goSearch(); return; }
        if (e.key === 'Escape') { frag[1](null); return; }
        if (e.ctrlKey || e.metaKey || e.altKey || typing() || document.querySelector('dialog[open]')) { return; }
        var k = e.key;
        if (k === '/') { e.preventDefault(); goSearch(); }
        else if (k === '?') { dlg[1]('keys'); }
        else if (!work) { return; }
        else if ((k === 'F7' && !e.shiftKey) || k === 'j' || k === 'о') { e.preventDefault(); step(1); }
        else if ((k === 'F7' && e.shiftKey) || k === 'k' || k === 'л') { e.preventDefault(); step(-1); }
        else if (k === 'v' || k === 'м') { markViewed(); }
        else if (k === 'c' || k === 'с') { e.preventDefault(); commentKey(); }
      }
      document.addEventListener('keydown', key);
      return function () { document.removeEventListener('keydown', key); };
    });
    function goSearch() {
      if (wide[0]) { var i = document.getElementById('q'); if (i) { i.focus(); i.select(); } } else { drawer[1]('search'); }
    }
    function setToc(items) { if (JSON.stringify(items) !== JSON.stringify(toc[0])) { toc[1](items); } }

    var sideProps = { q: q, page: page, since: since, work: work, toc: toc[0], spy: spy[0], picked: picked, changes: changes, cur: cur[0],
      goItem: goItem, comments: comments, tab: tab[0], setTab: setTab, showComment: showComment, goHash: function (h) { r[1](route(h)); } };
    var side = html`<${Side} ...${sideProps} />`;
    var n = unsent(comments), pageItems = changes ? pageDelta(page, since).items : [];
    var curBlock = blockOf(curA[0]), curHist = curBlock && versions(page, curBlock.p).length > 1;
    var overview = work && changes && changes.list.length > 0 && !ovOff[0] && html`<${Overview} changes=${changes} since=${since} from=${from}
      start=${function () { ovOff[1](true); store('overview', true); nextUnviewed(); }} hide=${function () { ovOff[1](true); store('overview', true); }} />`;
    return html`
      <a class="skip" href="#main" onClick=${function (e) { e.preventDefault(); var m = document.getElementById('main'); m.focus(); m.scrollIntoView(); }}>{{t:Skip to content}}</a>
      <header class="top" role="banner">
        ${!wide[0] && html`<button id="menu" type="button" class="btn ico" aria-label="{{t:Menu}}" aria-expanded=${drawer[0] ? 'true' : 'false'} onClick=${function () { drawer[1](true); }}>☰</button>`}
        <div class="proj">${D.brand.head && html`<span dangerouslySetInnerHTML=${{ __html: D.brand.head }}></span>`}<a href=${'#' + pages[0].id}>${D.title}</a></div>
        <fieldset class="mode"><legend class="sr">{{t:Mode}}</legend>
          <label class=${work ? '' : 'on'}><input type="radio" name="mode" checked=${!work} onChange=${function () { setMode('read'); }} />{{t:Reading}}</label>
          <label class=${work ? 'on' : ''}><input type="radio" name="mode" checked=${work} onChange=${function () { setMode('work'); }} />{{t:Work}}${count > 0 && html`<span class="mc"> · <span class="sr">{{t:changes:}} </span>${count}</span>`}</label>
        </fieldset>
        <span class="sp"></span>
        ${!wide[0] && html`<button type="button" class="btn mid" onClick=${goSearch}>🔍 {{t:Search}}</button>`}
        ${n > 0 && html`<button type="button" class="chip warn unsent" onClick=${openSend}>{{t:Not sent:}} ${n}</button>`}
        <button type="button" class="btn ico mid" aria-label="{{t:Keys and colours}}" title="{{t:Keys and colours}} (?)" onClick=${function () { dlg[1]('keys'); }}>?</button>
        <${More} page=${page} work=${work} comments=${comments} changes=${changes} dialog=${dlg[1]} send=${openSend}
          overview=${function () { ovOff[1](false); store('overview', false); window.scrollTo({ top: 0 }); }}
          blockHist=${curHist ? function () { dlg[1]({ ptr: curBlock.p }); } : null} />
      </header>
      ${!work && !hint[0] && html`<p class="hint">${count ? (H.base ? '{{t:There are changes since the version sent on}} ' : '{{t:There are changes since}} ') + from + '. {{t:Turn on «Work» to see them and leave comments.}}' : '{{t:Turn on «Work» to leave comments.}}'}
        <button type="button" class="btn sm" onClick=${function () { hint[1](true); store('hint', true); }}>{{t:Got it}}</button></p>`}
      <div class=${'shell' + (work ? ' work' : '')}>
        ${wide[0] ? html`<aside id="side" aria-label="{{t:Navigation}}">${side}</aside>`
          : html`<dialog id="drawer" ref=${dr} class="drawer" aria-label="{{t:Navigation}}" onClose=${function () { drawer[1](false); var m = document.getElementById('menu'); if (m) { m.focus(); } }}
              onClick=${function (e) { if (e.target === dr.current) { dr.current.close(); } }}>
              <button type="button" class="btn side-x" onClick=${function () { dr.current.close(); }}>✕ {{t:Close}}</button>
              ${drawer[0] && side}</dialog>`}
        <main id="main" tabindex="-1">
          <${Page} key=${page.id} page=${page} comments=${comments} since=${since} work=${work} onToc=${setToc} missing=${r[0].missing}
            cur=${curA[0]} setCur=${curA[1]} editor=${editor[0]} openEditor=${openEditor} closeEditor=${function () { editor[1](null); }}
            history=${function (pg, ptr) { dlg[1]({ ptr: ptr }); }} activeC=${activeC[0]} overview=${overview} />
        </main>
      </div>
      ${work && pageItems.length > 0 && !coarse && html`<${ChangeMap} items=${pageItems} curA=${curA[0]} comments=${comments}
        go=${function (it) { goItem(all.list.indexOf(it)); }} />`}
      ${work && ((changes && changes.list.length > 0) || (coarse && frag[0])) && html`<${Navigator} changes=${changes || { list: [] }} cur=${cur[0]} step=${step}
        frag=${coarse ? frag[0] : null} fragGo=${commentKey} />`}
      ${work && frag[0] && !coarse && html`<button type="button" class="btn pri fragb" style=${'left:' + Math.min(frag[0].x, window.innerWidth - 220) + 'px;top:' + (frag[0].y + 8) + 'px'}
        onMouseDown=${function (e) { e.preventDefault(); }} onClick=${commentKey}>💬 {{t:Comment on the selection}}</button>`}
      ${dlg[0] === 'keys' && html`<${Modal} title="{{t:Keys and colours}}" close=${function () { dlg[1](null); }}><${Keys} /><//>`}
      ${dlg[0] === 'since' && html`<${Modal} title="{{t:Starting point}}" close=${function () { dlg[1](null); }}><${SinceDialog} since=${since} set=${setSince} /><//>`}
      ${dlg[0] === 'page' && html`<${Modal} title=${'{{t:History of the page}}: ' + page.title} cls="wide" close=${function () { dlg[1](null); }}><${PageHistory} page=${page} close=${function () { dlg[1](null); }} /><//>`}
      ${dlg[0] && dlg[0].ptr && html`<${Modal} title="{{t:History of the paragraph}}" cls="wide" close=${function () { dlg[1](null); }}><${History} vs=${versions(page, dlg[0].ptr)} /><//>`}
      <div class="sr" aria-live="polite">${live[0]}</div>
      <${Toast} />`;
  }

  // the pure parts, for app_test.js; nothing else sets this
  if (window.doccTest) { window.doccTest({ wordMarks: wordMarks, words: words, plain: plain, snip: snip, findFragment: findFragment, placeGhost: placeGhost, pageDelta: pageDelta, hash: hash }); return; }
  render(html`<${App} />`, document.getElementById('app'));
})();
