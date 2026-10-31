# HTML blocks

An HTML block is a fenced code block ```` ```html ```` inside a text block. Use it for what Markdown draws badly: boxes and arrows, groups of servers or stands, a layout of cards, a table with merged cells. Plain prose, lists and simple tables stay Markdown.

```json
"body": [
  "**Prod server.**\n\n```html\n<div class=\"g\"><h4>Prod server</h4>\n  <div class=\"row\">\n    <div class=\"n red\"><b>Docspace</b><small>prod</small></div>\n  </div>\n</div>\n```"
]
```

The block is an ordinary text block: `docc build` gives it a code after the closing fence (` ^SKO-3`), links to it carry a hash, a change to the HTML is a change of the block (DOC209 on links to it). Readers comment on the block as a whole; the words inside cannot be selected for a comment, so put the sentence people will discuss outside the HTML, in the same block before the fence.

## How it is shown

The single HTML file (`docc build --html`) and Docspace show the block in a sandboxed frame as tall as its content, on the page's background, in the page's light or dark theme:

- HTML, CSS and inline SVG work. Scripts never run, forms never send, the frame cannot navigate or draw outside itself; links open in a new tab.
- Markdown output and `docc compile` keep the source as code.
- The frame is the width of the text column (about 720px) and as narrow as a phone (360px). Lay out for both: at most four columns side by side.

## The kit

Every frame gets these classes and variables. Build from them: blocks stay short, look alike across documents, and follow the theme.

| Class | What it is |
|---|---|
| `.n` | A box: `<div class="n"><b>Title</b><small>one line under it</small></div>`. Neutral without a hue. |
| `.blue` `.amber` `.red` `.green` `.violet` `.gray` | The hue of a box (`class="n red"`). Give hues a fixed meaning per project and state it once in the documents (e.g. dev blue, stage amber, prod red). |
| `.g` | A group with a heading: `<div class="g"><h4>Server X</h4>…</div>`. |
| `.row` | Items side by side, wrapping on a narrow screen. |
| `.flow` | A diagram read top to bottom: a grid of `--cols` columns (`style="--cols:2"`, default 1) holding boxes and arrows in reading order. |
| `.ar` | An arrow down, its label as its text: `<div class="ar">restic</div>`; `.ar.dash` is dashed. |
| `.span` | An item across all columns of a `.flow`. |

Branches in a `.flow`: a box across the columns (`span`), then one arrow per column, then one box per column. Joins: arrows from each column, then a box across them. Place an item in a column with `style="grid-column:2"`, across some with `grid-column:2/4`; an arrow that passes beside other rows takes `grid-row:span 3`; an empty cell is `<div></div>`.

```html
<div class="flow" style="--cols:2">
  <div class="n green span"><b>platform/komodo</b><small>in Gitea</small></div>
  <div class="ar span">webhook on push</div>
  <div class="n span"><b>Komodo Core</b></div>
  <div class="ar">commands</div><div class="ar">commands</div>
  <div class="n blue"><b>Periphery on X</b><small>dev and stage</small></div>
  <div class="n red"><b>Periphery on prod</b><small>prod</small></div>
</div>
```

Variables, for your own `<style>` or SVG: `--bg`, `--fg`, `--muted`, `--line`, `--surface`, `--font`, `--mono`, and each hue with its background (`--blue`, `--blue-soft`, … `--gray-soft`). Colours come from these variables only, so the block reads in both themes. A block's own `<style>` applies to that block alone.

## Check

Run `docc build --html` and open `docs/out.html` in a browser when you can, in a wide window and a narrow one: every label readable, no box wider than the column, arrows landing on the box they mean.
