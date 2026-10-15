// The documentation application: one page at a time drawn from the JSON
// island #docc-data with Preact and htm (vendor/htm-preact-standalone.js).
// Labels are marked as t:English in double braces and translated by the build (lang.go).
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
  var touch = window.matchMedia && matchMedia('(hover: none)').matches;

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
  function date(c) {
    var cm = commits[c];
    if (!cm || !cm.d) { return ''; }
    var d = new Date(cm.d);
    var pad = function (n) { return (n < 10 ? '0' : '') + n; };
    return pad(d.getDate()) + '.' + pad(d.getMonth() + 1) + '.' + d.getFullYear();
  }
  function when(c) {
    var cm = commits[c];
    if (!cm || !cm.h) { return '{{t:not saved yet}}'; }
    var d = new Date(cm.d);
    var pad = function (n) { return (n < 10 ? '0' : '') + n; };
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
  var box = document.createElement('div');
  function textOf(h) { box.innerHTML = h; return box.textContent.replace(/\s+/g, ' ').trim(); }
  // the source text of a block as a reader sees it: links by their names, no marks
  // ponytail: a few regexes for the usual marks, not a Markdown parser
  function plain(s) {
    return (s || '')
      .replace(/\[\[([^\]#|]+)(?:#([^\]|]*))?\]\]/g, function (m, id, a) {
        if (a && a.indexOf('/') > 0) { return a.slice(a.lastIndexOf('/') + 1); }
        return byId[id] ? byId[id].title : id;
      })
      .replace(/\{\{([^}#]+)#[^}]*\}\}/g, function (m, id) { return '«' + (byId[id] ? byId[id].title : id) + '»'; })
      .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
      .replace(/(\*\*|__)(.+?)\1/g, '$2')
      .replace(/(^|[^*\w])\*([^*\n]+)\*/g, '$1$2')
      .replace(/`([^`]+)`/g, '$1')
      .replace(/^#{1,6}\s+/gm, '')
      .replace(/^>\s?/gm, '')
      .replace(/\s\^[a-z0-9-]+$/gm, '');
  }
  function snip(s, n) {
    s = plain(s).replace(/\s+/g, ' ').trim();
    return s.length > n ? s.slice(0, n).replace(/\s\S*$/, '') + '…' : s;
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

  // word diff: a longest common subsequence over words and spaces
  function diff(a, b) {
    var x = (a || '').split(/(\s+)/), y = (b || '').split(/(\s+)/);
    var n = x.length, m = y.length;
    if (n * m > 4000000) { return [[-1, a], [1, b]]; } // ponytail: huge blocks are shown as replaced
    var dp = [];
    for (var i = 0; i <= n; i++) { dp.push(new Uint32Array(m + 1)); }
    for (i = n - 1; i >= 0; i--) {
      for (var j = m - 1; j >= 0; j--) {
        dp[i][j] = x[i] === y[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
    var out = [];
    function push(op, s) {
      if (out.length && out[out.length - 1][0] === op) { out[out.length - 1][1] += s; } else { out.push([op, s]); }
    }
    i = 0; j = 0;
    while (i < n && j < m) {
      if (x[i] === y[j]) { push(0, x[i]); i++; j++; }
      else if (dp[i + 1][j] >= dp[i][j + 1]) { push(-1, x[i]); i++; }
      else { push(1, y[j]); j++; }
    }
    while (i < n) { push(-1, x[i++]); }
    while (j < m) { push(1, y[j++]); }
    // a rewrite reads better as old text, then new text, than as confetti
    var same = 0;
    out.forEach(function (d) { if (d[0] === 0) { same += d[1].replace(/\s+/g, '').length; } });
    if (same < 0.4 * Math.max(a.replace(/\s+/g, '').length, b.replace(/\s+/g, '').length)) { return [[-1, a], [1, b]]; }
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
  function blocksOf(page) { return (page.props || []).concat(page.blocks).filter(function (b) { return b.p; }); }
  // what changed on a page after `since`: changed and new blocks in page order, removed texts
  var deltaCache = {};
  function pageDelta(page, since) {
    var key = page.id + ':' + since;
    if (deltaCache[key]) { return deltaCache[key]; }
    var d = { list: [], chg: 0, add: 0, rm: [] }, now = {};
    if (since != null && since >= 0) {
      blocksOf(page).forEach(function (b) {
        now[b.s] = true;
        var before = changedSince(page, b.p, since);
        if (before === undefined) { return; }
        d.list.push(b.p);
        if (before === null) { d.add++; } else { d.chg++; }
      });
      var gone = {};
      pageChanges(page).forEach(function (c) {
        if (c.c <= since) { return; }
        (c.rm || []).forEach(function (x) { if (!now[x[1]] && !gone[x[1]]) { gone[x[1]] = true; d.rm.push(x[1]); } });
      });
    }
    deltaCache[key] = d;
    return d;
  }
  // every change after `since` in the order of the pages
  function allChanges(since) {
    var list = [], pagesN = 0, rm = 0;
    pages.forEach(function (p) {
      var d = pageDelta(p, since);
      if (d.list.length || d.rm.length) { pagesN++; }
      rm += d.rm.length;
      d.list.forEach(function (ptr) { list.push({ page: p, ptr: ptr }); });
    });
    return { list: list, pages: pagesN, rm: rm };
  }
  // where a pointer is, for the reader: the start of its text
  function placeOf(page, ptr, fallback) {
    var b = page && blocksOf(page).filter(function (x) { return x.p === ptr; })[0];
    return '«' + snip(b ? b.s : fallback || '', 70) + '»';
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
      lines.push((k + 1) + '. ' + (p ? p.title : c.doc) + ' — «' + snip(c.quote, 80) + '»');
      lines.push(c.text);
      if (c.author) { lines.push('— ' + c.author); }
      lines.push('');
    });
    lines.push('{{t:For the team, do not edit:}}');
    lines.push('docc-comments:' + b64(exportJSON()));
    return lines.join('\n') + '\n';
  }
  window.addEventListener('beforeunload', function (e) {
    if (unsent(local)) { e.preventDefault(); e.returnValue = ''; }
  });
  function removeComment(c) {
    var before = local;
    setLocal(local.filter(function (x) { return x.id !== c.id; }));
    toast('{{t:Comment deleted.}}', '{{t:Undo}}', function () { setLocal(before); });
  }

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
      r.onend = function () { rec.current = null; on[1](false); };
      try { r.start(); rec.current = r; on[1](true); st[1](''); } catch (err) { st[1]('{{t:Dictation failed, type the text instead}}'); }
    }
    function toggle() { if (rec.current) { rec.current.stop(); } else { start(); } }
    return html`<span class="mic-w">
      <button type="button" class=${'mic' + (on[0] ? ' on' : '')} aria-pressed=${on[0] ? 'true' : 'false'} onClick=${toggle}>
        ${on[0] ? '■ {{t:Stop dictation}}' : '🎤 {{t:Dictate}}'}</button>
      ${st[0] && html`<small class="err">${st[0]}</small>`}
    </span>`;
  }

  function Editor(props) {
    var edit = props.edit;
    var text = useState(edit ? edit.text : ''), author = useState(edit ? edit.author || '' : store('author') || ''), ta = useRef(null);
    var cur = useRef(''); cur.current = text[0];
    useEffect(function () { if (ta.current) { ta.current.focus(); } }, []);
    function ok() {
      var t = text[0].trim(), a = author[0].trim();
      if (!t) { return; }
      if (a) { store('author', a); }
      if (edit) {
        setLocal(local.map(function (c) { return c.id === edit.id ? Object.assign({}, c, { text: t, author: a, sent: false }) : c; }));
      } else {
        setLocal(local.concat([{ id: uuid(), doc: props.page.id, path: props.ptr, quote: props.quote, text: t,
          author: a, created: new Date().toISOString(), build: H.head || '' }]));
      }
      props.close();
    }
    return html`<div class="editor">
      <label class="who-l">{{t:From}} <input class="who" autocomplete="name" enterkeyhint="next" placeholder="{{t:Your name}}" value=${author[0]}
        onInput=${function (e) { author[1](e.target.value); }} /></label>
      <textarea ref=${ta} rows="3" aria-label="{{t:Comment on this paragraph}}" placeholder="{{t:Comment on this paragraph}}" value=${text[0]}
        onInput=${function (e) { text[1](e.target.value); }}
        onKeyDown=${function (e) {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { ok(); }
          if (e.key === 'Escape') { props.close(); }
        }}></textarea>
      <div class="ed-row">
        <${Mic} get=${function () { return cur.current; }} set=${text[1]} />
        <span class="sp"></span>
        <button type="button" onClick=${props.close}>{{t:Cancel}}</button>
        <button type="button" class="pri" onClick=${ok} disabled=${!text[0].trim()}>{{t:Save}}</button>
      </div>
    </div>`;
  }

  function Note(props) {
    var c = props.c, edit = useState(false);
    if (edit[0]) { return html`<${Editor} edit=${c} close=${function () { edit[1](false); }} />`; }
    return html`<div class=${'note' + (props.repo ? ' repo' : '')}>
      <div class="nh"><b>💬 ${c.author || '{{t:Anonymous}}'}</b>
        <small>${c.created ? new Date(c.created).toLocaleDateString() : ''}</small>
        ${props.repo ? html`<span class=${'badge ' + (c.status === 'done' ? 'ok' : 'warn')}>
          ${c.status === 'done' ? '{{t:taken into account}}' : '{{t:the team is working on it}}'}</span>`
          : html`${!c.sent && html`<span class="badge warn">{{t:not sent}}</span>`}<span class="acts">
            <button type="button" class="link" onClick=${function () { edit[1](true); }}>{{t:Edit}}</button>
            <button type="button" class="link" onClick=${function () { removeComment(c); }}>{{t:Delete}}</button></span>`}
      </div>
      <div class="nt">${c.text}</div>
      ${c.reply && html`<div class="reply"><small>{{t:Team's answer}}</small>${c.reply}</div>`}
    </div>`;
  }

  // ---- one block: its HTML with tools, comments, history and changes ----
  function History(props) {
    var vs = props.vs.slice().reverse();
    return html`<div class="hist">
      <div class="hh"><b>{{t:History of the paragraph}}</b><button type="button" class="link" onClick=${props.close}>{{t:Close}}</button></div>
      ${vs.map(function (v, k) {
        var prev = vs[k + 1];
        return html`<div class="ver">
          <div class="vh">${versionLine(v.c)}${!prev && html` · <i>{{t:first version}}</i>`}</div>
          <${Diff} a=${prev ? prev.t : null} b=${v.t} />
        </div>`;
      })}
    </div>`;
  }

  function Block(props) {
    var b = props.b, page = props.page, ptr = b.p;
    var mode = useState(null), showDiff = useState(false);
    var mine = props.comments.filter(function (c) { return c.doc === page.id && c.path === ptr; });
    var repo = imported.filter(function (c) { return c.doc === page.id && c.path === ptr; });
    var vs = ptr ? versions(page, ptr) : [];
    var before = ptr ? changedSince(page, ptr, props.since) : undefined;
    var changed = before !== undefined, isNew = before === null;
    var body = props.tag === 'dd'
      ? html`<dd class="bc" id=${props.id} dangerouslySetInnerHTML=${{ __html: b.h }}></dd>`
      : html`<div class="bc" dangerouslySetInnerHTML=${{ __html: b.h }}></div>`;
    if (!ptr) { return props.tag === 'dd' ? body : html`<div class="blk">${body}</div>`; }
    var n = mine.length + repo.length;
    var extra = html`
      ${changed && !isNew && showDiff[0] && html`<div class="since"><small>{{t:Was → now}}</small><${Diff} a=${before} b=${b.s} /></div>`}
      ${repo.map(function (c) { return html`<${Note} c=${c} repo=${true} />`; })}
      ${mine.map(function (c) { return html`<${Note} c=${c} />`; })}
      ${mode[0] === 'c' && html`<${Editor} page=${page} ptr=${ptr} quote=${b.s} close=${function () { mode[1](null); }} />`}
      ${mode[0] === 'h' && html`<${History} vs=${vs} close=${function () { mode[1](null); }} />`}`;
    var tools = html`<span class="tools">
      ${isNew && html`<span class="tag-new">{{t:new}}</span>`}
      ${changed && !isNew && html`<button type="button" class="chg-b" aria-expanded=${showDiff[0] ? 'true' : 'false'}
        onClick=${function () { showDiff[1](!showDiff[0]); }}>{{t:What changed}}</button>`}
      <button type="button" aria-label=${n ? '{{t:Comments}}: ' + n : '{{t:Comment on this paragraph}}'} title="{{t:Comment on this paragraph}}"
        onClick=${function () { mode[1](mode[0] === 'c' ? null : 'c'); }}>💬${n ? ' ' + n : ''}</button>
      ${vs.length > 1 && html`<button type="button" aria-label=${'{{t:History of the paragraph}}: ' + vs.length} title="{{t:History of the paragraph}}"
        onClick=${function () { mode[1](mode[0] === 'h' ? null : 'h'); }}>🕘 ${vs.length}</button>`}
    </span>`;
    var cls = 'blk' + (changed ? (isNew ? ' new' : ' chg') : '') + (n ? ' has-c' : '');
    if (props.tag === 'dd') {
      return html`<dd class=${cls} id=${props.id} data-p=${ptr}><div class="bc" dangerouslySetInnerHTML=${{ __html: b.h }}></div>${tools}${extra}</dd>`;
    }
    return html`<div class=${cls} data-p=${ptr}>${body}${tools}${extra}</div>`;
  }

  // ---- page ----
  function PageHistory(props) {
    var page = props.page, ch = pageChanges(page).slice().reverse();
    function list(label, items, fn) {
      return items && items.length ? html`<div class="pg-g"><small>${label}</small>${items.map(fn)}</div>` : null;
    }
    function place(ptr, text) {
      var b = blocksOf(page).some(function (x) { return x.p === ptr; });
      return b ? html`<a class="ptr" href=${blockHash(page.id, ptr)}>${placeOf(page, ptr, text)}</a>` : html`<span class="ptr">${placeOf(page, ptr, text)}</span>`;
    }
    return html`<div class="hist page-hist">
      <div class="hh"><b>{{t:History of the page}}</b><button type="button" class="link" onClick=${props.close}>{{t:Close}}</button></div>
      ${!ch.length && html`<p class="muted">{{t:No changes in this file's history.}}</p>`}
      ${ch.map(function (c) {
        return html`<div class="ver">
          <div class="vh">${versionLine(c.c)}${c.new && html` · <i>{{t:page created}}</i>`}</div>
          ${list('{{t:Changed}}', c.ch, function (x) { return html`<div class="pg-i">${place(x[0], x[2])}<${Diff} a=${x[1]} b=${x[2]} /></div>`; })}
          ${list('{{t:Added}}', c.add, function (x) { return html`<div class="pg-i">${place(x[0], x[1])}<${Diff} a=${null} b=${x[1]} /></div>`; })}
          ${list('{{t:Removed}}', c.rm, function (x) { return html`<div class="pg-i"><div class="dv"><del>${plain(x[1])}</del></div></div>`; })}
        </div>`;
      })}
    </div>`;
  }

  function Chip(props) {
    var p = byId[props.id];
    return p ? html`<a class="chip" href=${'#' + p.id}>${p.title}</a>` : null;
  }

  function Page(props) {
    var page = props.page, ph = useState(false), main = useRef(null);
    useEffect(function () { ph[1](false); }, [page.id]);
    // heading anchors and copy buttons inside the rendered Markdown
    useEffect(function () {
      var el = main.current;
      [].forEach.call(el.querySelectorAll('.bc :is(h2,h3,h4)[id]'), function (h) {
        if (h.querySelector('.anchor')) { return; }
        var a = document.createElement('a');
        a.className = 'anchor'; a.href = '#' + h.id; a.textContent = '#';
        a.setAttribute('aria-label', '{{t:Link to this section}}');
        h.insertBefore(a, h.firstChild);
      });
      [].forEach.call(el.querySelectorAll('.bc pre'), function (pre) {
        if (pre.querySelector('.copy')) { return; }
        var b = document.createElement('button');
        b.type = 'button'; b.className = 'copy'; b.textContent = '{{t:Copy}}';
        b.addEventListener('click', function () {
          var code = pre.querySelector('code'), text = (code || pre).textContent;
          var done = function (m) { b.textContent = m; setTimeout(function () { b.textContent = '{{t:Copy}}'; }, 1500); };
          copy(text).then(function () { done('{{t:Copied}}'); }, function () { done('{{t:Failed}}'); });
        });
        pre.appendChild(b);
      });
      props.onToc([].map.call(el.querySelectorAll('.bc :is(h2,h3,h4)[id]'), function (h) {
        return { id: h.id, level: h.tagName.charAt(1), text: h.textContent.replace(/^#/, '') };
      }));
    });
    var prev = pages[page.i - 1], next = pages[page.i + 1];
    var shown = {}; shown[page.id] = true;
    (page.kids || []).forEach(function (k) { shown[k] = true; });
    if (page.parent) { shown[page.parent] = true; }
    (page.crumbs || []).forEach(function (k) { shown[k] = true; });
    var refs = (page.refs || []).filter(function (r) { return !shown[r]; });
    var bl = { comments: props.comments, since: props.since, page: page };
    var gone = pageDelta(page, props.since).rm;
    return html`<article class="page" ref=${main}>
      ${D.brand.print && html`<div dangerouslySetInnerHTML=${{ __html: D.brand.print }}></div>`}
      ${props.missing && html`<p class="notice" role="alert">{{t:Nothing is found at this link:}} «${props.missing}». {{t:The first page is shown; find the place in the tree or the search.}}</p>`}
      ${page.crumbs && html`<nav class="crumbs" aria-label="{{t:Breadcrumb}}">${page.crumbs.map(function (id, k) {
        return html`${k > 0 && html`<span aria-hidden="true">›</span>`}<a href=${'#' + id}>${byId[id] ? byId[id].title : id}</a>`;
      })}</nav>`}
      <div class="ttl">
        <h1>${page.title}</h1>
        ${commits.length > 0 && html`<button type="button" class="ph-b" aria-expanded=${ph[0] ? 'true' : 'false'} onClick=${function () { ph[1](!ph[0]); }}>🕘 {{t:Page history}}</button>`}
      </div>
      ${(page.kind || page.status || page.priority || page.hidden || page.tags) && html`<div class="badges">
        ${page.kind && html`<span class="badge kind">${page.kind}</span>`}
        ${page.status && html`<span class=${'badge ' + (page.sc || '')}>${page.status}</span>`}
        ${page.priority && html`<span class="badge">{{t:priority}} ${page.priority}</span>`}
        ${page.hidden && html`<span class="badge bad">{{t:hidden}}</span>`}
        ${(page.tags || []).map(function (t) { return html`<span class="tag">#${t}</span>`; })}
      </div>`}
      ${ph[0] && html`<${PageHistory} page=${page} close=${function () { ph[1](false); }} />`}
      <div class="doc">
        ${page.props && html`<dl>${page.props.map(function (p) {
          return html`<dt>${p.k}</dt><${Block} ...${bl} b=${p} tag="dd" id=${p.id} />`;
        })}</dl>`}
        ${page.blocks.map(function (b) { return html`<${Block} ...${bl} b=${b} />`; })}
      </div>
      ${gone.length > 0 && html`<details class="gone"><summary>{{t:Removed from this page since the chosen version}}: ${gone.length}</summary>
        ${gone.map(function (t) { return html`<div class="dv"><del>${plain(t)}</del></div>`; })}</details>`}
      ${page.kids && html`<h2 class="rh">{{t:Pages in this section}}</h2><div class="rows">${page.kids.map(function (id) {
        var k = byId[id];
        return k && html`<a class="row" href=${'#' + id}><span class="ct">${k.title}${k.kind && html` <span class="badge kind">${k.kind}</span>`}</span>${k.summary && html`<span class="cs">${k.summary}</span>`}</a>`;
      })}</div>`}
      ${(refs.length || page.covers) && html`<h2 class="rh">{{t:Related}}</h2><div class="related">
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

  // ---- sidebar: tree and search ----
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
      ${all.slice(lo, hi).map(function (x) { return html`<${Node} node=${x} cur=${props.cur} since=${props.since} />`; })}
      ${hi < n && html`<li><button type="button" class="more" onClick=${function () { win[1]([lo, Math.min(n, hi + 30)]); }}>${n - hi} {{t:more}} ↓</button></li>`}
    </ul>`;
  }
  function Node(props) {
    var x = props.node, p = byId[x.id], inside = contains(x, props.cur);
    var open = useState(null);
    var isOpen = open[0] == null ? inside : open[0];
    useEffect(function () { if (inside) { open[1](null); } }, [props.cur]);
    if (!p) { return null; }
    var d = pageDelta(p, props.since), n = d.list.length + d.rm.length;
    return html`<li class=${x.kids.length ? 'parent' : ''}>
      ${x.kids.length > 0 && html`<button class="tw" type="button" aria-expanded=${isOpen ? 'true' : 'false'}
        aria-label=${(isOpen ? '{{t:Collapse}}' : '{{t:Expand}}') + ': ' + p.title} onClick=${function () { open[1](!isOpen); }}></button>`}
      <a href=${'#' + p.id} aria-current=${p.id === props.cur ? 'page' : undefined}>${p.title}${p.hidden && html` <span class="badge bad">{{t:hidden}}</span>`}${n > 0 && html` <span class="cnt" title="{{t:Changes on the page}}"><span class="sr">{{t:Changes on the page}}: </span>${n}</span>`}</a>
      ${x.kids.length > 0 && isOpen && html`<${Items} nodes=${x.kids} cur=${props.cur} since=${props.since} />`}
    </li>`;
  }
  function Group(props) {
    var open = useState(false);
    return html`<h2 class=${'gh' + (open[0] ? '' : ' closed')}><button class="gt" type="button" aria-expanded=${open[0] ? 'true' : 'false'}
      onClick=${function () { open[1](!open[0]); }}>${props.label} <span>${props.n}</span></button></h2>
      ${open[0] && html`<div class="gb">${props.children}</div>`}`;
  }

  // the search finds paragraphs: every word in the paragraph or in the title of its page
  var index = null, LIMIT = 30;
  function Search(props) {
    var q = useState(''), sel = useState(0), inp = useRef(null);
    useEffect(function () {
      function key(e) {
        var typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement.tagName);
        if ((e.key === '/' && !typing) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k')) {
          e.preventDefault(); props.openSide(); inp.current.focus(); inp.current.select();
        }
      }
      document.addEventListener('keydown', key);
      return function () { document.removeEventListener('keydown', key); };
    }, []);
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
    function go(h) { q[1](''); inp.current.blur(); location.hash = blockHash(h.p.id, h.ptr); }
    return html`<div class="search">
      <input type="search" id="q" ref=${inp} placeholder="{{t:Search  ( / )}}" aria-label="{{t:Search the documentation}}" autocomplete="off"
        role="combobox" aria-expanded=${v ? 'true' : 'false'} aria-controls="results"
        aria-activedescendant=${v && hits.length ? 'hit-' + sel[0] : undefined}
        value=${q[0]} onInput=${function (e) { q[1](e.target.value); sel[1](0); }}
        onKeyDown=${function (e) {
          if (e.key === 'ArrowDown' && hits.length) { e.preventDefault(); sel[1]((sel[0] + 1) % hits.length); }
          else if (e.key === 'ArrowUp' && hits.length) { e.preventDefault(); sel[1]((sel[0] + hits.length - 1) % hits.length); }
          else if (e.key === 'Enter' && hits[sel[0]]) { e.preventDefault(); go(hits[sel[0]]); }
          else if (e.key === 'Escape') { q[1](''); inp.current.blur(); }
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

  // ---- "On this page" ----
  function Toc(props) {
    var on = useState(null);
    useEffect(function () {
      if (!window.IntersectionObserver || props.items.length < 2) { return; }
      var spy = new IntersectionObserver(function (es) {
        es.forEach(function (e) { if (e.isIntersecting) { on[1](e.target.id); } });
      }, { rootMargin: '0px 0px -75% 0px' });
      props.items.forEach(function (x) { var el = document.getElementById(x.id); if (el) { spy.observe(el); } });
      return function () { spy.disconnect(); };
    }, [props.items]);
    if (props.items.length < 2) { return null; }
    return html`<aside id="toc" aria-label="{{t:On this page}}"><h2>{{t:On this page}}</h2>
      ${props.items.map(function (x) { return html`<a href=${'#' + x.id} class=${'l' + x.level + (on[0] === x.id ? ' on' : '')}>${x.text}</a>`; })}
    </aside>`;
  }

  // ---- panels: comments, changes, export ----
  function Comments(props) {
    var list = props.comments, wait = unsent(list);
    var open = imported.filter(function (c) { return c.status !== 'done'; }).length;
    function send() {
      copy(exportText()).then(function () {
        markSent(); toast('{{t:Copied. Paste it into a message to the team.}}');
      }, function () { toast('{{t:Could not copy: download the file instead.}}'); });
    }
    function item(c, repo) {
      var p = byId[c.doc];
      return html`<div class="pc"><a href=${blockHash(c.doc, c.path)} onClick=${props.close}>${p ? p.title : c.doc}</a>
        <small class="ptr">${placeOf(p, c.path, c.quote)}</small><${Note} c=${c} repo=${repo} /></div>`;
    }
    return html`<section class="panel" aria-label="{{t:Comments}}">
      <div class="hh"><b>{{t:Your comments}}</b><button type="button" class="link" onClick=${props.close}>{{t:Close}}</button></div>
      ${list.length > 0 && html`<p class="muted">${wait ? '{{t:Comments are kept only in this browser until you send them to the team.}}' : '{{t:All comments are sent.}}'}</p>
      <div class="ed-row">
        <button type="button" class="pri" onClick=${send}>📋 {{t:Copy for sending}}</button>
        <button type="button" onClick=${function () { save('comments-' + new Date().toISOString().slice(0, 10) + '.json', 'application/json', exportJSON()); markSent(); }}>{{t:Download as a file}}</button>
        <button type="button" onClick=${function () {
          if (confirm('{{t:Delete all comments of this browser?}}')) { setLocal([]); }
        }}>{{t:Delete all}}</button>
      </div>`}
      ${!list.length && html`<p class="muted">${touch ? '{{t:No comments yet. Tap 💬 next to a paragraph to write one.}}' : '{{t:No comments yet. Point at a paragraph and press 💬.}}'}</p>`}
      ${list.map(function (c) { return item(c, false); })}
      ${imported.length > 0 && html`<h3 class="rh">{{t:Received by the team}}: ${imported.length} · {{t:in work}}: ${open}</h3>
        ${imported.map(function (c) { return item(c, true); })}`}
    </section>`;
  }

  function Changes(props) {
    var since = props.since;
    var rows = pages.map(function (p) { return { p: p, d: pageDelta(p, since) }; })
      .filter(function (r) { return r.d.list.length || r.d.rm.length; });
    return html`<section class="panel" aria-label="{{t:Changes}}">
      <div class="hh"><b>{{t:Changes by page}}</b><button type="button" class="link" onClick=${props.close}>{{t:Close}}</button></div>
      <ul class="chg-list">${rows.map(function (r) {
        return html`<li><a href=${r.d.list.length ? blockHash(r.p.id, r.d.list[0]) : '#' + r.p.id} onClick=${props.close}>${r.p.title}</a>
          <small>${[r.d.chg && '{{t:changed}}: ' + r.d.chg, r.d.add && '{{t:new}}: ' + r.d.add, r.d.rm.length && '{{t:removed}}: ' + r.d.rm.length].filter(Boolean).join(' · ')}</small></li>`;
      })}</ul>
    </section>`;
  }

  function Export(props) {
    var page = props.page;
    return html`<div class="panel menu">
      <button type="button" onClick=${function () { save(page.id + '.md', 'text/markdown', page.md); props.close(); }}>{{t:This page (.md)}}</button>
      <button type="button" onClick=${function () { save('docs.md', 'text/markdown', D.single); props.close(); }}>{{t:All pages in one file (.md)}}</button>
      <button type="button" onClick=${function () { save('docs.zip', 'application/zip', unzip64(D.zip)); props.close(); }}>{{t:All pages as separate files (.zip)}}</button>
    </div>`;
  }

  function Toolbar(props) {
    var real = [];
    commits.forEach(function (c, k) { if (c.h) { real.push(k); } });
    var ch = props.changes, last = commits.length - 1;
    function label(k) { return k === 0 && H.base ? '{{t:the version sent on}} ' + date(0) : versionLine(k); }
    var ver = html`${last >= 0 && commits[last].h && html`<small class="ver-l">{{t:Version of}} ${date(last)}</small>`}
      <button type="button" class=${props.panel === 'e' ? 'on' : ''} onClick=${function () { props.setPanel(props.panel === 'e' ? null : 'e'); }}>⬇ {{t:Export}}</button>`;
    return html`<div class="toolbar"><div class="tb-row">
      ${real.length > 0 && html`<label class="since-l">{{t:Show changes since}}
        <select value=${props.since == null ? '' : String(props.since)} onChange=${function (e) {
          props.setSince(e.target.value === '' ? null : +e.target.value);
        }}>
          <option value="">{{t:do not show}}</option>
          ${real.slice().reverse().map(function (k) { return html`<option value=${String(k)}>${label(k)}</option>`; })}
        </select></label>`}
      <span class="sp"></span>
      ${!ch && ver}
      <button type="button" class=${props.panel === 'c' ? 'on' : ''} onClick=${function () { props.setPanel(props.panel === 'c' ? null : 'c'); }}>💬 {{t:Comments}}${props.comments.length ? ' ' + props.comments.length : ''}${unsent(props.comments) ? html` <span class="badge warn">${unsent(props.comments)} {{t:not sent}}</span>` : ''}</button>
      </div>
      ${ch && html`<div class="tb-row"><span class="chg-nav">
        <button type="button" class=${props.panel === 'd' ? 'on' : ''} onClick=${function () { props.setPanel(props.panel === 'd' ? null : 'd'); }}>
          ${ch.list.length ? '{{t:Changed paragraphs}}: ' + ch.list.length + ' · {{t:pages}}: ' + ch.pages : (ch.rm ? '{{t:Only removals}}' : '{{t:No changes}}')}</button>
        ${ch.list.length > 0 && html`<button type="button" aria-label="{{t:Previous change}}" onClick=${function () { props.step(-1); }}>←</button>
          <span class="pos">${props.at >= 0 ? (props.at + 1) + ' {{t:of}} ' + ch.list.length : ''}</span>
          <button type="button" aria-label="{{t:Next change}}" onClick=${function () { props.step(1); }}>→</button>`}
      </span><span class="sp"></span>${ver}</div>`}
    </div>`;
  }

  // ---- application ----
  function App() {
    var r = useState(route(location.hash)), side = useState(false), toc = useState([]);
    var sinceHash = useState(store('since')), panel = useState(null), at = useState(-1);
    var comments = useComments();
    var page = r[0].page;
    // the chosen version is kept by hash: indexes change with every commit;
    // the file's own starting point is the default, "off" turns it off
    var since = null;
    commits.forEach(function (c, k) { if (c.h && c.h === sinceHash[0]) { since = k; } });
    if (since == null && sinceHash[0] !== 'off' && H.base && commits.length) { since = 0; }
    var changes = since != null ? allChanges(since) : null;
    useEffect(function () {
      function on() { r[1](route(location.hash)); side[1](false); }
      window.addEventListener('hashchange', on);
      // the place of reading goes into the history entry, so Back returns to it
      var t;
      function onScroll() {
        clearTimeout(t);
        t = setTimeout(function () { try { history.replaceState({ y: window.scrollY }, ''); } catch (e) { /* not allowed */ } }, 150);
      }
      window.addEventListener('scroll', onScroll, { passive: true });
      return function () { window.removeEventListener('hashchange', on); window.removeEventListener('scroll', onScroll); };
    }, []);
    useEffect(function () {
      function key(e) { if (e.key === 'Escape' && side[0]) { side[1](false); document.getElementById('menu').focus(); } }
      document.addEventListener('keydown', key);
      return function () { document.removeEventListener('keydown', key); };
    }, [side[0]]);
    useEffect(function () {
      document.title = page === pages[0] ? D.title : page.title + ' · ' + D.title;
      var st = history.state, t = r[0].target && document.getElementById(r[0].target);
      var b = r[0].block && document.querySelector('[data-p="' + CSS.escape(r[0].block) + '"]');
      if (st && st.y != null && !r[0].block) { window.scrollTo({ top: st.y, behavior: 'instant' }); }
      else if (b) {
        b.scrollIntoView({ block: 'center', behavior: 'instant' }); b.classList.add('flash');
        setTimeout(function () { b.classList.remove('flash'); }, 1600);
        if (changes) { changes.list.forEach(function (x, k) { if (x.page === page && x.ptr === r[0].block) { at[1](k); } }); }
      } else if (t) { t.scrollIntoView({ behavior: 'instant' }); } else { window.scrollTo({ top: 0, behavior: 'instant' }); }
      var cur = document.querySelector('#tree [aria-current]'), sd = document.getElementById('side');
      if (cur && sd) {
        var a = cur.getBoundingClientRect(), s = sd.getBoundingClientRect();
        if (a.top < s.top + 60 || a.bottom > s.bottom - 20) { sd.scrollTop += a.top - s.top - 120; }
      }
    }, [r[0]]);
    function setSince(v) { var h = v == null ? 'off' : commits[v].h; sinceHash[1](h); store('since', h); at[1](-1); }
    function setToc(items) { if (JSON.stringify(items) !== JSON.stringify(toc[0])) { toc[1](items); } }
    function step(d) {
      var n = changes.list.length, k = at[0] < 0 ? (d > 0 ? 0 : n - 1) : (at[0] + d + n) % n, x = changes.list[k];
      at[1](k);
      var h = blockHash(x.page.id, x.ptr);
      if (location.hash === h) { r[1](route(h)); } else { location.hash = h; }
    }
    function closeSide() { side[1](false); }
    return html`
      <a class="skip" href="#main" onClick=${function (e) { e.preventDefault(); var m = document.getElementById('main'); m.focus(); m.scrollIntoView(); }}>{{t:Skip to content}}</a>
      <header class="bar">
        <button id="menu" type="button" aria-label="{{t:Menu}}" aria-expanded=${side[0] ? 'true' : 'false'} aria-controls="side" onClick=${function () { side[1](!side[0]); }}>☰</button>
        ${D.brand.head && html`<span dangerouslySetInnerHTML=${{ __html: D.brand.head }}></span>`}
      </header>
      <div class="shell">
        ${side[0] && html`<div class="scrim" onClick=${closeSide}></div>`}
        <aside id="side" class=${side[0] ? 'open' : ''} aria-label="{{t:Navigation}}">
          <button type="button" class="side-x" onClick=${closeSide}>✕ {{t:Close}}</button>
          ${D.brand.head && html`<div class="side-brand" dangerouslySetInnerHTML=${{ __html: D.brand.head }}></div>`}
          <${Search} openSide=${function () { side[1](true); }} />
          ${D.full && html`<p class="banner">{{t:Full build: includes hidden documents and notes. Do not publish.}}</p>`}
          <nav id="tree" aria-label="{{t:Document tree}}">
            ${tree.length > 0 && html`<h2>{{t:Pages}}</h2><${Items} nodes=${tree} cur=${page.id} since=${since} />`}
            ${orphans.length > 0 && html`<${Group} label="{{t:Without parent}}" n=${D.nav.orphans.length}><${Items} nodes=${orphans} cur=${page.id} since=${since} /><//>`}
            ${D.nav.uncovered && html`<${Group} label="{{t:Documents without code coverage}}" n=${D.nav.uncovered.length}>
              <ul>${D.nav.uncovered.map(function (id) { return html`<li><a href=${'#' + id}>${byId[id] ? byId[id].title : id}</a></li>`; })}</ul><//>`}
          </nav>
        </aside>
        <main id="main" tabindex="-1">
          <${Toolbar} since=${since} setSince=${setSince} panel=${panel[0]} setPanel=${panel[1]} comments=${comments}
            changes=${changes} at=${at[0]} step=${step} />
          ${panel[0] === 'c' && html`<${Comments} comments=${comments} close=${function () { panel[1](null); }} />`}
          ${panel[0] === 'd' && changes && html`<${Changes} since=${since} close=${function () { panel[1](null); }} />`}
          ${panel[0] === 'e' && html`<${Export} page=${page} close=${function () { panel[1](null); }} />`}
          <${Page} key=${page.id} page=${page} comments=${comments} since=${since} onToc=${setToc} missing=${r[0].missing} />
        </main>
        <${Toc} items=${toc[0]} />
      </div>
      <${Toast} />`;
  }

  render(html`<${App} />`, document.getElementById('app'));
})();
