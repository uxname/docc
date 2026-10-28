// Checks the pure parts of app.js in Node: the word marks of an edit, the
// place of a commented fragment, the place of a removed paragraph, the
// edits of a page and the page at an earlier moment of the timeline. Run by render::tests::app_logic: node app_test.js app.js
'use strict';
const fs = require('fs');
const assert = require('assert');

const data = {
  key: 'test', title: 'T', nav: { tree: [], orphans: [] }, brand: {},
  // the comments of the documents: on a paragraph, one docc marked lost, one on a paragraph gone, one on a deleted page
  comments: [{ id: 'ok', doc: 'p', path: '/body/1', quote: 'Beta', text: 'Why?' }, { id: 'lost', doc: 'p', path: '/body/0', quote: 'Old', fragment: 'Old words', text: 'x', lost: true },
    { id: 'rm', doc: 'p', path: '/body/7', quote: 'Old', text: 'y' }, { id: 'q', doc: 'q48', path: '/options/0', quote: 'A) yes', text: 'z' }],
  pages: [{ id: 'p', title: 'P', file: 'p.json', blocks: [
    { p: '/body/0', s: 'Alpha ^PA-1', c: 'PA-1', v: '3a9c' }, { p: '/body/1', s: 'Beta now' }, { p: '/body/2', s: 'Gamma' }] },
  { id: 'n', title: 'N', file: 'n.json', blocks: [{ s: '', h: '<h2>Only a heading</h2>' }] },
  // a form field, moved down by a paragraph inserted above it
  { id: 'f', title: 'F', file: 'f.json', blocks: [{ p: '/body/0', s: 'New.', h: '<p>New.</p>' },
    { p: '/body/1', s: '$input: radio; id: export; label: Export; to Excel?; options: Yes\nNo', h: '', i: { t: 'radio', id: 'export', label: 'Export; to Excel?', options: ['Yes', 'No'] } }] }],
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

// a paragraph is found by its code in any case, with or without its version; old pointer links still work
for (const h of ['#p--PA-1@3a9c', '#p--pa-1', '#p--PA-1@zzzz']) {
  const r = T.route(h);
  assert.ok(r.page.id === 'p' && r.block === '/body/0', h + ': ' + JSON.stringify(r));
}
assert.strictEqual(T.route('#p/body/2').block, '/body/2');
assert.strictEqual(T.route('#p--PA-7').block, undefined, 'an unknown code is no paragraph');

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

// a comment opens its paragraph; one whose paragraph or page is gone says so instead of opening the first page
const at = (id) => T.commentPlace(data.comments.find((x) => x.id === id));
assert.deepStrictEqual([at('ok').page.id, at('ok').ptr, at('ok').gone], ['p', '/body/1', undefined]);
assert.deepStrictEqual([at('rm').page.id, at('rm').gone], ['p', 'block']);
assert.deepStrictEqual([at('lost').page.id, at('lost').gone], ['p', 'block'], 'a comment docc marked lost has no block, though its pointer has one');
assert.deepStrictEqual(at('q'), { gone: 'page' });
// the comments that lost their place stand at the top of their page
assert.deepStrictEqual(T.lostOn(data.comments, 'p').map((c) => c.id), ['lost', 'rm']);
assert.deepStrictEqual(T.lostOn(data.comments, 'n'), []);

// the tree of folders: a folder without a page of its own is a line with its title
const tree = T.nest([['index', 0], ['', 0, 'notes'], ['a', 1], ['', 1, 'old'], ['b', 2]]);
assert.deepStrictEqual(tree.map((x) => [x.id, x.title, x.kids.length]), [['index', undefined, 0], ['', 'notes', 2]]);
assert.deepStrictEqual(tree[1].kids.map((x) => x.key), ['a', 'folder:3']);
assert.strictEqual(tree[1].kids[1].kids[0].id, 'b');

// a form field reads as its question and choices; an answer stands at its field and the latest answer of each author counts
assert.strictEqual(T.fieldWords(data.pages[2].blocks[1].s), 'Export; to Excel? — Yes / No');
assert.strictEqual(T.snip(data.pages[2].blocks[1].s, 60), 'Export; to Excel? — Yes / No');
assert.strictEqual(T.pathOf({ id: 'a', doc: 'f', path: '/body/0', input: 'export', quote: 'x' }), '/body/1');
assert.strictEqual(T.pathOf({ id: 'a', doc: 'f', path: '/body/5', input: 'gone' }), '/body/5', 'a removed field keeps its old place');
const ans = (id, author, created, doc) => ({ id, doc: doc || 'f', input: 'export', author, created, text: id });
const given = T.answersFor([ans('i1', 'Ivan', '2026-10-01'), ans('o1', 'Olga', '2026-10-02'), ans('i2', 'Ivan', '2026-10-03'),
  ans('x1', '', '2026-10-04'), ans('x2', '', '2026-10-05'), ans('z', 'Zoe', '2026-10-06', 'p')], 'f', 'export');
assert.deepStrictEqual(given.map((c) => c.id), ['o1', 'i2', 'x1', 'x2'], 'one per author, oldest first; nameless ones each');
// radio and checkbox fields take an own answer: a text that is none of the options
const radio = { t: 'radio', options: ['Yes', 'No'] }, box = { t: 'checkbox', options: ['A', 'B', 'C'] };
assert.deepStrictEqual(T.splitAnswer(radio, ['Maybe']), [[], 'Maybe']);
assert.deepStrictEqual(T.splitAnswer(box, ['C', 'Mine', 'A', 'More']), [['C', 'A'], 'Mine'], 'one own answer is shown');
assert.deepStrictEqual(T.splitAnswer({ t: 'text' }, ['Free']), [['Free'], '']);
assert.deepStrictEqual(T.joinAnswer(radio, ['Yes'], '  '), ['Yes']);
assert.deepStrictEqual(T.joinAnswer(radio, ['Yes'], ' Maybe '), ['Maybe'], 'radio: the own answer instead of an option');
assert.deepStrictEqual(T.joinAnswer(box, ['A', 'C'], 'Mine'), ['A', 'C', 'Mine'], 'checkbox: the own answer besides the options');
assert.deepStrictEqual(T.joinAnswer(box, [], ''), []);

// a table row reads as its cells in the lists
assert.strictEqual(T.snip('| Module | Owner |\n|---|---|', 60), 'Module Owner');
console.log('ok');
