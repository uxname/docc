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
  // the page a hash belongs to: a page id, or <page id>--<anchor>
  function route(hash) {
    var id = decode(hash);
    if (byId[id]) { return { page: byId[id], target: null }; }
    var best = null;
    pages.forEach(function (p) {
      if (id.indexOf(p.id + '--') === 0 && (!best || p.id.length > best.id.length)) { best = p; }
    });
    return best ? { page: best, target: id } : { page: pages[0], target: null };
  }
  // "/13.1 Migration sources/2" -> "13.1 Migration sources › 3"
  function ptrLabel(p) {
    return p.split('/').slice(1).map(function (s) {
      s = s.replace(/~1/g, '/').replace(/~0/g, '~');
      return /^\d+$/.test(s) ? String(+s + 1) : s;
    }).join(' › ');
  }
  function when(c) {
    var cm = commits[c];
    if (!cm || !cm.h) { return '{{t:not committed}}'; }
    var d = new Date(cm.d);
    var pad = function (n) { return (n < 10 ? '0' : '') + n; };
    return pad(d.getDate()) + '.' + pad(d.getMonth() + 1) + '.' + d.getFullYear() + ' ' + pad(d.getHours()) + ':' + pad(d.getMinutes());
  }
  function commitLine(c) {
    var cm = commits[c] || {};
    return cm.h ? when(c) + ' · ' + cm.a + ' · ' + cm.h + ' · ' + cm.m : '{{t:not committed}}';
  }
  function save(name, type, data) {
    var a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([data], { type: type }));
    a.download = name;
    document.body.appendChild(a); a.click(); a.remove();
    setTimeout(function () { URL.revokeObjectURL(a.href); }, 1000);
  }
  function unzip64(s) {
    var b = atob(s), u = new Uint8Array(b.length);
    for (var i = 0; i < b.length; i++) { u[i] = b.charCodeAt(i); }
    return u;
  }
  function uuid() {
    if (window.crypto && crypto.randomUUID) { return crypto.randomUUID(); }
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
      var r = Math.random() * 16 | 0;
      return (c === 'x' ? r : (r & 3 | 8)).toString(16);
    });
  }
  var plain = document.createElement('div');
  function textOf(h) { plain.innerHTML = h; return plain.textContent.replace(/\s+/g, ' ').trim(); }

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
  function Diff(props) {
    if (props.a == null) { return html`<div class="dv"><ins>${props.b}</ins></div>`; }
    var ds = diff(props.a, props.b), whole = ds.length === 2 && ds[0][1] === props.a && ds[1][1] === props.b;
    return html`<div class=${'dv' + (whole ? ' whole' : '')}>${ds.map(function (d) {
      return d[0] < 0 ? html`<del>${d[1]}</del>` : d[0] > 0 ? html`<ins>${d[1]}</ins>` : d[1];
    })}</div>`;
  }

  // ---- history ----
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
  function pageChangedSince(page, since) {
    return since != null && since >= 0 && pageChanges(page).some(function (c) { return c.c > since; });
  }

  // ---- comments: kept in localStorage until downloaded as JSON ----
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
  function exportComments() {
    var d = new Date().toISOString().slice(0, 10);
    save('comments-' + d + '.json', 'application/json',
      JSON.stringify({ docc: 'comments', version: 1, comments: local }, null, 2) + '\n');
  }

  var Rec = window.SpeechRecognition || window.webkitSpeechRecognition;
  var speechLang = D.lang === 'ru' ? 'ru-RU' : 'en-US';

  // hold the button and speak; the words go into the text box as they come
  function Mic(props) {
    var rec = useRef(null), st = useState(''), on = useState(false);
    if (!Rec) { return null; }
    function start(e) {
      e.preventDefault();
      if (rec.current) { return; }
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
          ev.error === 'no-speech' ? '{{t:No speech heard}}' : ev.error);
      };
      r.onend = function () { rec.current = null; on[1](false); };
      try { r.start(); rec.current = r; on[1](true); st[1](''); } catch (err) { st[1](String(err)); }
    }
    function stop() { if (rec.current) { rec.current.stop(); } }
    return html`<span class="mic-w">
      <button type="button" class=${'mic' + (on[0] ? ' on' : '')} onPointerDown=${start} onPointerUp=${stop}
        onPointerLeave=${stop} onPointerCancel=${stop} onContextMenu=${function (e) { e.preventDefault(); }}
        title="{{t:Hold and speak}}">${on[0] ? '● {{t:Listening…}}' : '🎤 {{t:Hold and speak}}'}</button>
      ${st[0] && html`<small class="err">${st[0]}</small>`}
    </span>`;
  }

  function Editor(props) {
    var text = useState(''), author = useState(store('author') || ''), ta = useRef(null);
    var cur = useRef(''); cur.current = text[0];
    useEffect(function () { if (ta.current) { ta.current.focus(); } }, []);
    function ok() {
      var t = text[0].trim();
      if (!t) { return; }
      if (author[0].trim()) { store('author', author[0].trim()); }
      setLocal(local.concat([{ id: uuid(), doc: props.page.id, path: props.ptr, quote: props.quote, text: t,
        author: author[0].trim(), created: new Date().toISOString(), build: H.head || '' }]));
      props.close();
    }
    return html`<div class="editor">
      ${!store('author') && html`<input class="who" placeholder="{{t:Your name}}" value=${author[0]}
        onInput=${function (e) { author[1](e.target.value); }} />`}
      <textarea ref=${ta} rows="3" placeholder="{{t:Comment on this block}}" value=${text[0]}
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
    var c = props.c;
    return html`<div class=${'note' + (props.repo ? ' repo' : '')}>
      <div class="nh"><b>${c.author || '{{t:Anonymous}}'}</b>
        <small>${c.created ? new Date(c.created).toLocaleString() : ''}</small>
        ${props.repo ? html`<span class=${'badge ' + (c.status === 'done' ? 'ok' : 'warn')}>
          ${c.status === 'done' ? '{{t:processed}}' : '{{t:not processed}}'}</span>`
          : html`<button type="button" class="link" onClick=${function () {
            setLocal(local.filter(function (x) { return x.id !== c.id; }));
          }}>{{t:Delete}}</button>`}
      </div>
      <div class="nt">${c.text}</div>
    </div>`;
  }

  // ---- one block: its HTML with tools, comments, history and changes ----
  function History(props) {
    var vs = props.vs.slice().reverse();
    return html`<div class="hist">
      <div class="hh"><b>{{t:History of the block}}</b><button type="button" class="link" onClick=${props.close}>{{t:Close}}</button></div>
      ${vs.map(function (v, k) {
        var prev = vs[k + 1];
        return html`<div class="ver">
          <div class="vh">${commitLine(v.c)}${!prev && html` · <i>{{t:created}}</i>`}</div>
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
    var changed = before !== undefined;
    var body = props.tag === 'dd'
      ? html`<dd class="bc" id=${props.id} dangerouslySetInnerHTML=${{ __html: b.h }}></dd>`
      : html`<div class="bc" dangerouslySetInnerHTML=${{ __html: b.h }}></div>`;
    if (!ptr) { return props.tag === 'dd' ? body : html`<div class="blk">${body}</div>`; }
    var extra = html`
      ${changed && showDiff[0] && html`<div class="since"><small>{{t:Text at the chosen version → now}}</small><${Diff} a=${before} b=${b.s} /></div>`}
      ${repo.map(function (c) { return html`<${Note} c=${c} repo=${true} />`; })}
      ${mine.map(function (c) { return html`<${Note} c=${c} />`; })}
      ${mode[0] === 'c' && html`<${Editor} page=${page} ptr=${ptr} quote=${b.s} close=${function () { mode[1](null); }} />`}
      ${mode[0] === 'h' && html`<${History} vs=${vs} close=${function () { mode[1](null); }} />`}`;
    var tools = html`<span class="tools">
      ${changed && html`<button type="button" class="chg-b" title="{{t:Changed after the chosen version}}"
        onClick=${function () { showDiff[1](!showDiff[0]); }}>Δ</button>`}
      <button type="button" title="{{t:Comment}}" onClick=${function () { mode[1](mode[0] === 'c' ? null : 'c'); }}>💬${mine.length + repo.length ? ' ' + (mine.length + repo.length) : ''}</button>
      ${vs.length > 1 && html`<button type="button" title="{{t:History of the block}}" onClick=${function () { mode[1](mode[0] === 'h' ? null : 'h'); }}>🕘 ${vs.length}</button>`}
    </span>`;
    var cls = 'blk' + (changed ? ' chg' : '') + (mine.length + repo.length ? ' has-c' : '');
    if (props.tag === 'dd') {
      return html`<dd class=${cls} id=${props.id}><div class="bc" dangerouslySetInnerHTML=${{ __html: b.h }}></div>${tools}${extra}</dd>`;
    }
    return html`<div class=${cls} data-p=${ptr}>${body}${tools}${extra}</div>`;
  }

  // ---- page ----
  function PageHistory(props) {
    var ch = pageChanges(props.page).slice().reverse();
    function list(label, items, fn) {
      return items && items.length ? html`<div class="pg-g"><small>${label}</small>${items.map(fn)}</div>` : null;
    }
    return html`<div class="hist page-hist">
      <div class="hh"><b>{{t:History of the page}}</b><button type="button" class="link" onClick=${props.close}>{{t:Close}}</button></div>
      ${!ch.length && html`<p class="muted">{{t:No history: the project is not in Git.}}</p>`}
      ${ch.map(function (c) {
        return html`<div class="ver">
          <div class="vh">${commitLine(c.c)}${c.new && html` · <i>{{t:page created}}</i>`}</div>
          ${list('{{t:Changed}}', c.ch, function (x) { return html`<div class="pg-i"><span class="ptr">${ptrLabel(x[0])}</span><${Diff} a=${x[1]} b=${x[2]} /></div>`; })}
          ${list('{{t:Added}}', c.add, function (x) { return html`<div class="pg-i"><span class="ptr">${ptrLabel(x[0])}</span><${Diff} a=${null} b=${x[1]} /></div>`; })}
          ${list('{{t:Removed}}', c.rm, function (x) { return html`<div class="pg-i"><span class="ptr">${ptrLabel(x[0])}</span><div class="dv"><del>${x[1]}</del></div></div>`; })}
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
          if (navigator.clipboard) { navigator.clipboard.writeText(text).then(function () { done('{{t:Copied}}'); }, function () { done('{{t:Failed}}'); }); }
          else { done('{{t:Failed}}'); }
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
    var refs = (page.refs || []).filter(function (r) { return !shown[r]; });
    var bl = { comments: props.comments, since: props.since, page: page };
    return html`<article class="page" ref=${main}>
      ${D.brand.print && html`<div dangerouslySetInnerHTML=${{ __html: D.brand.print }}></div>`}
      ${page.crumbs && html`<nav class="crumbs" aria-label="{{t:Breadcrumb}}">${page.crumbs.map(function (id, k) {
        return html`${k > 0 && html`<span aria-hidden="true">›</span>`}<a href=${'#' + id}>${byId[id] ? byId[id].title : id}</a>`;
      })}</nav>`}
      <div class="ttl">
        <h1>${page.title}</h1>
        ${commits.length > 0 && html`<button type="button" class="ph-b" onClick=${function () { ph[1](!ph[0]); }}>🕘 {{t:Page history}}</button>`}
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
      ${page.kids && html`<h2 class="rh">{{t:Pages in this section}}</h2><div class="rows">${page.kids.map(function (id) {
        var k = byId[id];
        return k && html`<a class="row" href=${'#' + id}><span class="ct">${k.title}${k.kind && html` <span class="badge kind">${k.kind}</span>`}</span>${k.summary && html`<span class="cs">${k.summary}</span>`}</a>`;
      })}</div>`}
      ${(page.parent || refs.length || page.covers) && html`<h2 class="rh">{{t:Related}}</h2><div class="related">
        ${page.parent && html`<div class="rg"><span class="rl">{{t:Parent}}</span><${Chip} id=${page.parent} /></div>`}
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
    var mark = pageChangedSince(p, props.since);
    return html`<li class=${x.kids.length ? 'parent' : ''}>
      ${x.kids.length > 0 && html`<button class="tw" type="button" aria-expanded=${isOpen ? 'true' : 'false'}
        aria-label=${isOpen ? '{{t:Collapse}}' : '{{t:Expand}}'} onClick=${function () { open[1](!isOpen); }}></button>`}
      <a href=${'#' + p.id} aria-current=${p.id === props.cur ? 'page' : undefined}>${p.title}${p.hidden && html` <span class="badge bad">{{t:hidden}}</span>`}${mark && html` <span class="dot" title="{{t:Changed after the chosen version}}">●</span>`}</a>
      ${x.kids.length > 0 && isOpen && html`<${Items} nodes=${x.kids} cur=${props.cur} since=${props.since} />`}
    </li>`;
  }
  function Group(props) {
    var open = useState(false);
    return html`<h2 class=${'gh' + (open[0] ? '' : ' closed')}><button class="gt" type="button" aria-expanded=${open[0] ? 'true' : 'false'}
      onClick=${function () { open[1](!open[0]); }}>${props.label} <span>${props.n}</span></button></h2>
      ${open[0] && html`<div class="gb">${props.children}</div>`}`;
  }

  var index = null;
  function Search(props) {
    var q = useState(''), sel = useState(0), box = useRef(null);
    useEffect(function () {
      function key(e) {
        var typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement.tagName);
        if ((e.key === '/' && !typing) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k')) {
          e.preventDefault(); props.openSide(); box.current.focus(); box.current.select();
        }
      }
      document.addEventListener('keydown', key);
      return function () { document.removeEventListener('keydown', key); };
    }, []);
    var v = q[0].trim(), hits = [];
    if (v) {
      index = index || pages.map(function (p) {
        return { p: p, text: p.blocks.concat(p.props || []).map(function (b) { return textOf(b.h); }).join(' ') };
      });
      var esc = function (w) { return w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); };
      var words = v.split(/\s+/).map(function (w) { return new RegExp(esc(w), 'i'); });
      index.forEach(function (t) {
        if (!words.every(function (w) { return w.test(t.p.title) || w.test(t.text); })) { return; }
        var score = words.reduce(function (s, w) { return s + (w.test(t.p.title) ? 10 : 0); }, 0);
        var m = words[0].exec(t.text), i = m ? m.index : 0;
        var from = Math.max(0, i - 40), snip = (from > 0 ? '… ' : '') + t.text.slice(from, i + 110) + (i + 110 < t.text.length ? ' …' : '');
        hits.push({ p: t.p, score: score, snip: snip });
      });
      hits.sort(function (a, b) { return b.score - a.score; });
      hits = hits.slice(0, 20);
    }
    var re = v ? new RegExp('(' + v.split(/\s+/).map(function (w) { return w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }).join('|') + ')', 'gi') : null;
    function go(h) { q[1](''); box.current.blur(); location.hash = '#' + h.p.id; }
    return html`<div class="search">
      <input type="search" id="q" ref=${box} placeholder="{{t:Search  ( / )}}" aria-label="{{t:Search the documentation}}" autocomplete="off"
        value=${q[0]} onInput=${function (e) { q[1](e.target.value); sel[1](0); }}
        onKeyDown=${function (e) {
          if (e.key === 'ArrowDown' && hits.length) { e.preventDefault(); sel[1]((sel[0] + 1) % hits.length); }
          else if (e.key === 'ArrowUp' && hits.length) { e.preventDefault(); sel[1]((sel[0] + hits.length - 1) % hits.length); }
          else if (e.key === 'Enter' && hits[sel[0]]) { e.preventDefault(); go(hits[sel[0]]); }
          else if (e.key === 'Escape') { q[1](''); box.current.blur(); }
        }} />
      ${v && html`<ul id="results" role="listbox" aria-label="{{t:Search results}}">
        ${!hits.length && html`<li class="none">{{t:Nothing found}}</li>`}
        ${hits.map(function (h, k) {
          return html`<li role="option" aria-selected=${k === sel[0] ? 'true' : 'false'} onClick=${function () { go(h); }}>
            <b>${h.p.title}</b><span>${h.snip.split(re).map(function (part, n) { return n % 2 ? html`<mark>${part}</mark>` : part; })}</span></li>`;
        })}
      </ul>`}
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

  // ---- toolbar: changes since a version, comments, export ----
  function Comments(props) {
    var list = props.comments;
    var open = imported.filter(function (c) { return c.status !== 'done'; }).length;
    return html`<div class="panel">
      <div class="hh"><b>{{t:Comments}}</b><button type="button" class="link" onClick=${props.close}>{{t:Close}}</button></div>
      <p class="muted">{{t:New comments stay in this browser until you download them and run docc comments import.}}</p>
      <div class="ed-row">
        <button type="button" class="pri" disabled=${!list.length} onClick=${exportComments}>{{t:Download JSON}}</button>
        <button type="button" disabled=${!list.length} onClick=${function () {
          if (confirm('{{t:Delete all comments of this browser?}}')) { setLocal([]); }
        }}>{{t:Clear}}</button>
      </div>
      ${!list.length && html`<p class="muted">{{t:No new comments. Point at a block and press 💬.}}</p>`}
      ${list.map(function (c) {
        var p = byId[c.doc];
        return html`<div class="pc"><a href=${'#' + c.doc} onClick=${function () { props.goto(c); }}>${p ? p.title : c.doc}</a>
          <small class="ptr">${ptrLabel(c.path)}</small><${Note} c=${c} /></div>`;
      })}
      ${imported.length > 0 && html`<p class="muted">{{t:In the repository}}: ${imported.length}, {{t:not processed}}: ${open}</p>`}
    </div>`;
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
    return html`<div class="toolbar">
      ${real.length > 0 && html`<label class="since-l">{{t:Changes since}}
        <select value=${props.since == null ? '' : String(props.since)} onChange=${function (e) {
          props.setSince(e.target.value === '' ? null : +e.target.value);
        }}>
          <option value="">{{t:off}}</option>
          ${real.slice().reverse().map(function (k) { return html`<option value=${String(k)}>${when(k)} · ${commits[k].m}</option>`; })}
        </select></label>`}
      <span class="sp"></span>
      <button type="button" class=${props.panel === 'c' ? 'on' : ''} onClick=${function () { props.setPanel(props.panel === 'c' ? null : 'c'); }}>💬 {{t:Comments}}${props.comments.length ? ' (' + props.comments.length + ')' : ''}</button>
      <button type="button" class=${props.panel === 'e' ? 'on' : ''} onClick=${function () { props.setPanel(props.panel === 'e' ? null : 'e'); }}>⬇ {{t:Export}}</button>
    </div>`;
  }

  // ---- application ----
  function App() {
    var r = useState(route(location.hash)), side = useState(false), toc = useState([]);
    var sinceHash = useState(store('since')), panel = useState(null);
    var comments = useComments();
    var page = r[0].page;
    // the chosen version is kept by hash: indexes change with every commit
    var since = [null];
    commits.forEach(function (c, k) { if (c.h && c.h === sinceHash[0]) { since[0] = k; } });
    useEffect(function () {
      function on() { r[1](route(location.hash)); side[1](false); }
      window.addEventListener('hashchange', on);
      return function () { window.removeEventListener('hashchange', on); };
    }, []);
    useEffect(function () {
      document.title = page === pages[0] ? D.title : page.title + ' · ' + D.title;
      var t = r[0].target && document.getElementById(r[0].target);
      if (t) { t.scrollIntoView({ behavior: 'instant' }); } else { window.scrollTo({ top: 0, behavior: 'instant' }); }
      var cur = document.querySelector('#tree [aria-current]'), sd = document.getElementById('side');
      if (cur && sd) {
        var a = cur.getBoundingClientRect(), s = sd.getBoundingClientRect();
        if (a.top < s.top + 60 || a.bottom > s.bottom - 20) { sd.scrollTop += a.top - s.top - 120; }
      }
    }, [r[0]]);
    function setSince(v) { var h = v == null ? null : commits[v].h; sinceHash[1](h); store('since', h); }
    function setToc(items) { if (JSON.stringify(items) !== JSON.stringify(toc[0])) { toc[1](items); } }
    function goto(c) {
      panel[1](null);
      setTimeout(function () {
        var el = document.querySelector('.blk[data-p="' + CSS.escape(c.path) + '"]');
        if (el) { el.scrollIntoView({ block: 'center' }); el.classList.add('flash'); setTimeout(function () { el.classList.remove('flash'); }, 1500); }
      }, 50);
    }
    return html`
      <a class="skip" href="#main">{{t:Skip to content}}</a>
      <header class="bar">
        <button id="menu" type="button" aria-label="{{t:Menu}}" aria-expanded=${side[0] ? 'true' : 'false'} onClick=${function () { side[1](!side[0]); }}>☰</button>
        ${D.brand.head && html`<span dangerouslySetInnerHTML=${{ __html: D.brand.head }}></span>`}
        <span id="bar-title">${page.title}</span>
      </header>
      <div class="shell">
        <aside id="side" class=${side[0] ? 'open' : ''}>
          ${D.brand.head && html`<div dangerouslySetInnerHTML=${{ __html: D.brand.head }}></div>`}
          <${Search} openSide=${function () { side[1](true); }} />
          ${D.full && html`<p class="banner">{{t:Full build: includes hidden documents and notes. Do not publish.}}</p>`}
          <nav id="tree" aria-label="{{t:Document tree}}">
            ${tree.length > 0 && html`<h2>{{t:Pages}}</h2><${Items} nodes=${tree} cur=${page.id} since=${since[0]} />`}
            ${orphans.length > 0 && html`<${Group} label="{{t:Without parent}}" n=${D.nav.orphans.length}><${Items} nodes=${orphans} cur=${page.id} since=${since[0]} /><//>`}
            ${D.nav.uncovered && html`<${Group} label="{{t:Documents without code coverage}}" n=${D.nav.uncovered.length}>
              <ul>${D.nav.uncovered.map(function (id) { return html`<li><a href=${'#' + id}>${byId[id] ? byId[id].title : id}</a></li>`; })}</ul><//>`}
          </nav>
        </aside>
        <main id="main" tabindex="-1">
          <${Toolbar} since=${since[0]} setSince=${setSince} panel=${panel[0]} setPanel=${panel[1]} comments=${comments} />
          ${panel[0] === 'c' && html`<${Comments} comments=${comments} close=${function () { panel[1](null); }} goto=${goto} />`}
          ${panel[0] === 'e' && html`<${Export} page=${page} close=${function () { panel[1](null); }} />`}
          <${Page} key=${page.id} page=${page} comments=${comments} since=${since[0]} onToc=${setToc} />
        </main>
        <${Toc} items=${toc[0]} />
      </div>`;
  }

  render(html`<${App} />`, document.getElementById('app'));
})();
