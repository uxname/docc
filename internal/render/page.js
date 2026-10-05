(function () {
  'use strict';
  try {
    var $ = function (s, r) { return (r || document).querySelector(s); };
    var $$ = function (s, r) { return [].slice.call((r || document).querySelectorAll(s)); };
    var pages = $$('main > section.page');
    var side = $('#side'), menu = $('#menu'), tocBox = $('#toc'), barTitle = $('#bar-title');
    var rootTitle = document.title, current = null, spy = null;
    var links = $$('#tree a[href^="#"]');

    function decode(h) {
      h = h.replace(/^#/, '');
      try { return decodeURIComponent(h); } catch (e) { return h; }
    }

    // heading anchors and copy buttons
    pages.forEach(function (p) {
      $$('.doc h2[id], .doc h3[id], .doc h4[id]', p).forEach(function (h) {
        h.dataset.t = h.textContent;
        var a = document.createElement('a');
        a.className = 'anchor'; a.href = '#' + h.id; a.textContent = '#';
        a.setAttribute('aria-label', 'Link to this section');
        h.insertBefore(a, h.firstChild);
      });
      $$('pre', p).forEach(function (pre) {
        var b = document.createElement('button');
        b.type = 'button'; b.className = 'copy'; b.textContent = 'Copy';
        b.addEventListener('click', function () {
          var code = $('code', pre), text = (code || pre).textContent;
          var done = function (m) { b.textContent = m; setTimeout(function () { b.textContent = 'Copy'; }, 1500); };
          if (navigator.clipboard) { navigator.clipboard.writeText(text).then(function () { done('Copied'); }, function () { done('Failed'); }); }
          else { done('Failed'); }
        });
        pre.appendChild(b);
      });
    });

    // sidebar tree
    function setOpen(tw, open) {
      tw.setAttribute('aria-expanded', open ? 'true' : 'false');
      tw.setAttribute('aria-label', open ? 'Collapse' : 'Expand');
    }
    function setGroup(h, open) {
      h.classList.toggle('closed', !open);
      $('.gt', h).setAttribute('aria-expanded', open ? 'true' : 'false');
    }
    $('#tree').addEventListener('click', function (e) {
      var tw = e.target.closest && e.target.closest('.tw');
      if (tw) { setOpen(tw, tw.getAttribute('aria-expanded') !== 'true'); return; }
      var gt = e.target.closest && e.target.closest('.gt');
      if (gt) { var h = gt.parentNode; setGroup(h, h.classList.contains('closed')); }
    });
    // long lists show a window of items with buttons for the rest; the window
    // moves to the current page, so a list of a thousand siblings stays short
    function limit(list, items, n, wrap, label) {
      var all = items(), lo = 0, hi = n, step = 30;
      if (all.length <= n) { return; }
      function button(text, fn) {
        var b = document.createElement('button');
        b.type = 'button'; b.className = 'more'; b.addEventListener('click', fn);
        var holder = b;
        if (wrap) { holder = document.createElement('li'); holder.appendChild(b); }
        return { b: b, el: holder, set: function (t) { b.textContent = t; } };
      }
      var up = button('', function () { lo = Math.max(0, lo - step); draw(); });
      var down = button('', function () { hi = Math.min(all.length, hi + step); draw(); });
      list.insertBefore(up.el, all[0]);
      list.appendChild(down.el);
      function draw() {
        all.forEach(function (x, k) { x.classList.toggle('extra', k < lo || k >= hi); });
        up.el.style.display = lo > 0 ? '' : 'none';
        down.el.style.display = hi < all.length ? '' : 'none';
        up.set('↑ ' + lo + ' earlier');
        down.set((all.length - hi) + ' more ' + (label || '') + '↓');
      }
      list.reveal = function (el) {
        var k = -1;
        all.forEach(function (x, m) { if (x.contains(el)) { k = m; } });
        if (k >= 0 && (k < lo || k >= hi)) { lo = Math.max(0, k - 10); hi = Math.min(all.length, k + 11); draw(); }
      };
      draw();
    }
    $$('#tree ul').forEach(function (ul) {
      limit(ul, function () { return [].slice.call(ul.children).filter(function (c) { return c.tagName === 'LI'; }); }, 30, true);
    });
    $$('.rows').forEach(function (r) { limit(r, function () { return $$(':scope > .row', r); }, 30, false); });
    $$('.rg').forEach(function (g) { limit(g, function () { return $$(':scope > .chip', g); }, 12, false); });
    // the tree starts folded; show() opens the way to the current page
    $$('#tree .tw').forEach(function (tw) { setOpen(tw, false); });
    $$('#tree .gh').forEach(function (h) { setGroup(h, false); });
    function markTree(id) {
      links.forEach(function (a) {
        if (decode(a.hash) !== id) { a.removeAttribute('aria-current'); return; }
        a.setAttribute('aria-current', 'page');
        for (var li = a.parentNode; li && li.id !== 'tree'; li = li.parentNode) {
          var tw = li.tagName === 'LI' && $(':scope > .tw', li);
          if (tw) { setOpen(tw, true); }
          if (li.reveal) { li.reveal(a); }
          if (li.classList && li.classList.contains('gb')) { setGroup(li.previousElementSibling, true); }
        }
      });
      var cur = $('#tree [aria-current]');
      if (cur) {
        var r = cur.getBoundingClientRect(), s = side.getBoundingClientRect();
        if (r.top < s.top + 60 || r.bottom > s.bottom - 20) { side.scrollTop += r.top - s.top - 120; }
      }
    }

    // "On this page"
    function buildToc(page) {
      if (spy) { spy.disconnect(); spy = null; }
      var hs = $$('.doc h2[id], .doc h3[id], .doc h4[id]', page);
      tocBox.textContent = '';
      if (hs.length < 2) { tocBox.hidden = true; return; }
      var h = document.createElement('h2');
      h.textContent = 'On this page';
      tocBox.appendChild(h);
      var map = {};
      hs.forEach(function (x) {
        var a = document.createElement('a');
        a.href = '#' + x.id; a.textContent = x.dataset.t;
        a.className = 'l' + x.tagName.charAt(1);
        tocBox.appendChild(a); map[x.id] = a;
      });
      tocBox.hidden = false;
      if (window.IntersectionObserver) {
        spy = new IntersectionObserver(function (es) {
          es.forEach(function (e) {
            if (!e.isIntersecting) { return; }
            $$('a', tocBox).forEach(function (a) { a.classList.remove('on'); });
            map[e.target.id].classList.add('on');
          });
        }, { rootMargin: '0px 0px -75% 0px' });
        hs.forEach(function (x) { spy.observe(x); });
      }
    }

    // router: the hash names a page or a section inside it
    function show() {
      var id = decode(location.hash), target = id ? document.getElementById(id) : null;
      var page = target && target.closest ? target.closest('section.page') : null;
      if (!page) { page = pages[0]; target = null; }
      if (page !== current) {
        pages.forEach(function (p) { p.classList.toggle('on', p === page); });
        current = page;
        document.title = page === pages[0] ? rootTitle : page.dataset.title + ' · ' + rootTitle;
        barTitle.textContent = page.dataset.title;
        markTree(page.id);
        buildToc(page);
      }
      if (target && target !== page) { target.scrollIntoView({ behavior: 'instant' }); }
      else { window.scrollTo({ top: 0, behavior: 'instant' }); }
      closeMenu();
    }

    // drawer on narrow screens
    function closeMenu() {
      side.classList.remove('open');
      menu.setAttribute('aria-expanded', 'false');
    }
    menu.addEventListener('click', function () {
      var open = !side.classList.contains('open');
      side.classList.toggle('open', open);
      menu.setAttribute('aria-expanded', open ? 'true' : 'false');
    });

    // search: every word must occur on the page; titles rank first
    var q = $('#q'), res = $('#results'), hits = [], sel = -1;
    var index = pages.map(function (p) {
      return { id: p.id, title: p.dataset.title, text: $('.doc', p).textContent.replace(/\s+/g, ' ') };
    });
    function esc(w) { return w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }
    function snippet(t, re) {
      var m = re.exec(t.text), i = m ? m.index : 0;
      var from = Math.max(0, i - 40), to = Math.min(t.text.length, i + 110);
      if (from > 0) { var sp = t.text.indexOf(' ', from); from = sp >= 0 && sp < i ? sp + 1 : from; }
      if (to < t.text.length) { var sl = t.text.lastIndexOf(' ', to); to = sl > i ? sl : to; }
      var s = (from > 0 ? '… ' : '') + t.text.slice(from, to) + (to < t.text.length ? ' …' : '');
      var span = document.createElement('span');
      s.split(re).forEach(function (part, k) {
        if (k % 2) { var mk = document.createElement('mark'); mk.textContent = part; span.appendChild(mk); }
        else { span.appendChild(document.createTextNode(part)); }
      });
      return span;
    }
    function select(i) {
      var items = $$('li[role=option]', res);
      items.forEach(function (x, k) { x.setAttribute('aria-selected', k === i ? 'true' : 'false'); });
      sel = i;
      if (items[i]) { items[i].scrollIntoView({ block: 'nearest' }); }
    }
    function search() {
      var v = q.value.trim();
      res.textContent = ''; hits = []; sel = -1;
      if (!v) { res.hidden = true; return; }
      var words = v.split(/\s+/).map(function (w) { return new RegExp(esc(w), 'i'); });
      var all = new RegExp('(' + v.split(/\s+/).map(esc).join('|') + ')', 'gi');
      index.forEach(function (t) {
        var ok = words.every(function (w) { return w.test(t.title) || w.test(t.text); });
        if (!ok) { return; }
        var score = words.reduce(function (s, w) { return s + (w.test(t.title) ? 10 : 0); }, 0);
        hits.push({ t: t, score: score });
      });
      hits.sort(function (a, b) { return b.score - a.score; });
      hits = hits.slice(0, 20);
      if (!hits.length) {
        var none = document.createElement('li');
        none.className = 'none'; none.textContent = 'Nothing found';
        res.appendChild(none);
      }
      hits.forEach(function (h) {
        var li = document.createElement('li'), b = document.createElement('b');
        li.setAttribute('role', 'option'); li.dataset.id = h.t.id;
        b.textContent = h.t.title;
        all.lastIndex = 0;
        li.appendChild(b); li.appendChild(snippet(h.t, all));
        res.appendChild(li);
      });
      res.hidden = false;
      if (hits.length) { select(0); }
    }
    function open(id) {
      q.value = ''; res.hidden = true; hits = []; q.blur();
      if (decode(location.hash) === id) { show(); } else { location.hash = '#' + id; }
    }
    q.addEventListener('input', search);
    q.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown' && hits.length) { e.preventDefault(); select((sel + 1) % hits.length); }
      else if (e.key === 'ArrowUp' && hits.length) { e.preventDefault(); select((sel + hits.length - 1) % hits.length); }
      else if (e.key === 'Enter' && hits[sel]) { e.preventDefault(); open(hits[sel].t.id); }
      else if (e.key === 'Escape') { q.value = ''; search(); q.blur(); }
    });
    res.addEventListener('click', function (e) {
      var li = e.target.closest && e.target.closest('li[data-id]');
      if (li) { open(li.dataset.id); }
    });
    document.addEventListener('click', function (e) {
      if (!e.target.closest('.search')) { res.hidden = true; }
      if (!e.target.closest('#side, #menu')) { closeMenu(); }
    });
    document.addEventListener('keydown', function (e) {
      var typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement.tagName);
      if ((e.key === '/' && !typing) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k')) {
        e.preventDefault();
        if (getComputedStyle(menu).display !== 'none') { side.classList.add('open'); menu.setAttribute('aria-expanded', 'true'); }
        q.focus(); q.select();
      } else if (e.key === 'Escape') { closeMenu(); }
    });

    window.addEventListener('hashchange', show);
    show();
  } catch (err) {
    document.documentElement.className = ''; // without the script every page stays visible
  }
})();
