// Checks the pure parts of app.js in Node: the word marks of an edit, the
// place of a commented fragment, the place of a removed paragraph and the
// edits of a page. Run by render::tests::app_logic: node app_test.js app.js
'use strict';
const fs = require('fs');
const assert = require('assert');

const data = {
  key: 'test', title: 'T', nav: { tree: [], orphans: [] }, brand: {}, comments: [],
  pages: [{ id: 'p', title: 'P', file: 'p.json', blocks: [
    { p: '/body/0', s: 'Alpha' }, { p: '/body/1', s: 'Beta now' }, { p: '/body/2', s: 'Gamma' }] }],
  hist: { base: true, commits: [{ h: 'a', d: '2026-10-06T09:00:00Z' }, { h: 'b', d: '2026-10-07T09:00:00Z' }], docs: { 'p.json': {
    b: { '/body/0': [{ c: 0, t: 'Alpha' }], '/body/1': [{ c: 0, t: 'Beta' }, { c: 1, t: 'Beta now' }], '/body/2': [{ c: 1, t: 'Gamma' }] },
    ch: [{ c: 1, ch: [['/body/1', 'Beta', 'Beta now']], add: [['/body/2', 'Gamma']], rm: [['/body/3', 'Gone at the end'], ['/body/1', 'Gone in the middle']] }] } } },
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

// a table row reads as its cells in the lists
assert.strictEqual(T.snip('| Module | Owner |\n|---|---|', 60), 'Module Owner');
console.log('ok');
