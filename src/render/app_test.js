// Checks the pure parts of app.js in Node: the word marks of an edit, the
// place of a commented fragment, the place of a removed paragraph, the
// edits of a page and the page at an earlier moment of the timeline. Run by render::tests::app_logic: node app_test.js app.js
'use strict';
const fs = require('fs');
const assert = require('assert');

const data = {
  key: 'test', title: 'T', nav: { tree: [], orphans: [] }, brand: {},
  // accepted comments: on a paragraph, on a paragraph rewritten away, on a deleted question
  comments: [{ id: 'ok', doc: 'p', path: '/body/1', quote: 'Beta' }, { id: 'rm', doc: 'p', path: '/body/7', quote: 'Old' },
    { id: 'q', doc: 'q48', path: '/options/0', quote: 'A) yes' }],
  pages: [{ id: 'p', title: 'P', file: 'p.json', blocks: [
    { p: '/body/0', s: 'Alpha' }, { p: '/body/1', s: 'Beta now' }, { p: '/body/2', s: 'Gamma' }] },
  { id: 'n', title: 'N', file: 'n.json', blocks: [{ s: '', h: '<h2>Only a heading</h2>' }] }],
  hist: { base: true, commits: [{ h: 'a', d: '2026-10-06T09:00:00Z' }, { h: 'b', d: '2026-10-07T09:00:00Z' }], docs: { 'p.json': {
    b: { '/body/0': [{ c: 0, t: 'Alpha' }], '/body/1': [{ c: 0, t: 'Beta' }, { c: 1, t: 'Beta now' }], '/body/2': [{ c: 1, t: 'Gamma' }] },
    ch: [{ c: 1, ch: [['/body/1', 'Beta', 'Beta now']], add: [['/body/2', 'Gamma']], rm: [['/body/3', 'Gone at the end'], ['/body/1', 'Gone in the middle']] }] },
  'n.json': { b: {}, ch: [{ c: 1, new: true }] } } },
};
const noop = () => {};
global.window = global;
global.htmPreact = { html: () => null, render: noop, useState: (v) => [v, noop], useEffect: noop, useRef: (v) => ({ current: v }) };
global.document = { getElementById: () => ({ textContent: JSON.stringify(data) }), createElement: () => ({}) };
global.localStorage = { getItem: () => null, setItem: noop };
global.addEventListener = noop;
let T;
global.doccTest = (t) => { T = t; };
new Function(fs.readFileSync(process.argv[2], 'utf8'))();
assert.ok(T, 'app.js did not hand over its parts');

// an edit keeps the words that stayed, marks the added ones and puts the removed ones before the next word
const w = (s) => s.split(' ');
let m = T.wordMarks(w('the manager creates a project in 3 days'), w('the manager creates a project in 1 day and sends the bill'));
assert.deepStrictEqual(m.map((x) => !!x.ins), [false, false, false, false, false, false, true, true, true, true, true, true]);
assert.strictEqual(m[6].del, '3 days');
// an added run stays whole when the word before it repeats its last word
m = T.wordMarks(w('по SMS в течение 5 минут'), w('по SMS или в Telegram в течение 1 минуты'));
assert.deepStrictEqual(m.map((x) => !!x.ins), [false, false, true, true, true, false, false, true, true]);
assert.strictEqual(m[7].del, '5 минут');
m = T.wordMarks(w('по SMS или в Telegram в течение'), w('по SMS в течение'));
assert.deepStrictEqual(m.map((x) => x.del || ''), ['', '', 'или в Telegram', ''], 'a removed run stays whole too');
m = T.wordMarks(w('one two three four five'), w('one two three'));
assert.strictEqual(m[2].after, 'four five', 'removed words at the end go after the last word');
assert.strictEqual(T.wordMarks(w('a b c d e f g h'), w('x y z')), null, 'a rewrite is not drawn word by word');
// table bars and list bullets render as no text, so they are not words
assert.deepStrictEqual(T.words('| a | b |\n|---|---|\n- c'), ['a', 'b', 'c']);

// a fragment that occurs twice is found by its surroundings
const text = 'the term is short; later the term is long';
assert.strictEqual(T.findFragment(text, { fragment: 'the term', prefix: 'later ', suffix: ' is long' }), 25);
assert.strictEqual(T.findFragment(text, { fragment: 'the term' }), 0);
assert.strictEqual(T.findFragment(text, { fragment: 'missing' }), -1);

// a removed block goes before the block that has its pointer now, else after its array
const page = data.pages[0];
assert.strictEqual(T.placeGhost(page, '/body/1'), 1);
assert.strictEqual(T.placeGhost(page, '/body/9'), 3);
assert.strictEqual(T.placeGhost(page, '/other/0'), 3);

// the edits of a page since the first commit, in page order with the removed blocks in place
const d = T.pageDelta(page, 0);
assert.deepStrictEqual(d.items.map((x) => x.k + ' ' + x.ptr), ['rm /body/1', 'chg /body/1', 'new /body/2', 'rm /body/3']);
assert.deepStrictEqual([d.chg, d.add, d.rm], [1, 1, 2]);
assert.strictEqual(T.pageDelta(page, 1).items.length, 0, 'nothing changed after the last commit');

// the page at the first commit: the blocks removed later were still there
const p0 = T.pageDelta(page, 0, 0);
assert.strictEqual(p0.items.length, 0, 'no edits between a moment and itself');
assert.deepStrictEqual(p0.olds.map((g) => g.ptr), ['/body/3', '/body/1']);
assert.strictEqual(T.between(page, '/body/1', 0, 1), 'Beta', 'the text at the start of the edits');
assert.strictEqual(T.between(page, '/body/2', 0, 1), null, 'a block that came later did not exist then');
assert.strictEqual(T.between(page, '/body/0', 0, 1), undefined, 'an unchanged block');
assert.strictEqual(T.between(page, '/body/1', 0, 0), undefined, 'nothing changes between a moment and itself');

// saves by day for the timeline, and the save to show for a date
assert.deepStrictEqual(T.saveDays([1, 0]).map((x) => x.ks), [[1], [0]]);
assert.strictEqual(T.atDate('2026-10-06'), 0);
assert.strictEqual(T.atDate('2026-10-07'), 1, 'the last save of that day');
assert.strictEqual(T.atDate('2020-01-01'), 0, 'a date before the history shows its start');

// a press on the timeline takes the nearer thumb; two thumbs closer than a thumb (28 px) part by the first move
assert.strictEqual(T.thumbFor(10, 0, 100, 28, 0), 's');
assert.strictEqual(T.thumbFor(90, 0, 100, 28, 0), 't');
assert.strictEqual(T.thumbFor(50, 0, 100, 28, 0), 't', 'halfway takes the end');
assert.strictEqual(T.thumbFor(52, 50, 60, 28, 0), null, 'on two thumbs the press waits for its move');
assert.strictEqual(T.thumbFor(52, 50, 60, 28, -5), 's', 'a move to the left takes the start');
assert.strictEqual(T.thumbFor(52, 50, 50, 28, 5), 't', 'a move to the right takes the end');
assert.strictEqual(T.thumbFor(5, 100, 100, 28, 0), 's', 'far left of two thumbs on one place takes the start');

// a browser comment from an older build follows its words to the paragraph that has them now
const c = (path, fragment) => ({ id: path + fragment, doc: 'p', path, fragment, build: 'old' });
assert.strictEqual(T.pathOf(c('/body/1', 'Gamma')), '/body/2', 'a paragraph was inserted above');
assert.strictEqual(T.pathOf(c('/body/1', 'now')), '/body/1', 'its words are still there');
assert.strictEqual(T.pathOf(c('/body/1', 'missing')), '/body/1', 'words found nowhere stay where they were');

// the tree marks a page with edits by their number, and a page created in the period as new even with no edits
const marks = (s, t) => T.pageMarks(T.allChanges(s, t).list, s, t);
assert.deepStrictEqual(marks(0, 1), { p: { k: 'chg', n: 4 }, n: { k: 'new', n: 0 } });
assert.deepStrictEqual(marks(1, 1), {}, 'no marks when no edits are shown');

// the list of edits holds the open page only; its button goes on to the next page with marks, from the start again after the last
assert.deepStrictEqual(T.pageEdits(T.allChanges(0, 1).list, 'p').map((x) => x[0].k + ' ' + x[1]), ['rm 0', 'chg 1', 'new 2', 'rm 3']);
assert.strictEqual(T.pageEdits(T.allChanges(0, 1).list, 'n').length, 0, 'a new page with only headings has no edits');
assert.strictEqual(T.nextEdited(marks(0, 1), 'p'), 'n');
assert.strictEqual(T.nextEdited(marks(0, 1), 'n'), 'p', 'after the last page comes the first');
assert.strictEqual(T.nextEdited({ p: { k: 'chg', n: 1 } }, 'p'), null, 'no other page has edits');

// an accepted comment opens its paragraph; one whose paragraph or page is gone says so instead of opening the first page
const at = (id) => T.commentPlace(data.comments.find((x) => x.id === id));
assert.deepStrictEqual([at('ok').page.id, at('ok').ptr, at('ok').gone], ['p', '/body/1', undefined]);
assert.deepStrictEqual([at('rm').page.id, at('rm').gone], ['p', 'block']);
assert.deepStrictEqual(at('q'), { gone: 'page' });

// a table row reads as its cells in the lists
assert.strictEqual(T.snip('| Module | Owner |\n|---|---|', 60), 'Module Owner');
console.log('ok');
