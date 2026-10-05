// Package rules is the registry of all rules: code, name, group, default level
// and the text for `docc explain`. docc rules, docc explain and the
// agent skill are built from it.
package rules

import (
	"slices"
	"strings"

	"github.com/santiment/docc/internal/diag"
)

type Rule struct {
	Code    string
	Name    string
	Group   string
	Default diag.Level // level in the recommended preset
	Core    bool       // cannot be suppressed, disabled or re-levelled
	Desc    string
	Bad     string
	Good    string
}

const E, W = diag.Error, diag.Warn

var all = []Rule{
	{"DOC101", "syntax/invalid-json", "syntax", E, true, "The file is not strict JSON (RFC 8259, UTF-8, an object at the root, at most 5 MB and 64 levels deep). A merge conflict marker also triggers it.", `{ "title": "A", }`, `{ "title": "A" }`},
	{"DOC102", "syntax/duplicate-key", "syntax", E, true, "An object has the same key twice; JSON parsers silently keep only one.", `{ "title": "A", "title": "B" }`, `{ "title": "B" }`},
	{"DOC103", "syntax/missing-title", "syntax", E, true, "The document has no title, or it is empty or only spaces.", `{ "kind": "note" }`, `{ "title": "Login" }`},
	{"DOC104", "syntax/invalid-id", "syntax", E, true, "The file or folder name is not a valid id: use a-z, 0-9 and '-', not starting or ending with '-', and no reserved Windows names.", "docs/src/My Doc.json", "docs/src/my-doc.json"},
	{"DOC105", "syntax/document-schema", "syntax", E, true, "A field does not match the built-in document schema: wrong type, or a repeated element in links or tags.", `{ "title": "A", "links": ["b", "b"] }`, `{ "title": "A", "links": ["b"] }`},
	{"DOC106", "schema/violation", "schema", E, false, "The document violates the schema of its kind from docs/schemas/<kind>.json.", `{ "title": "A", "kind": "requirement" }  (schema requires "statement")`, `{ "title": "A", "kind": "requirement", "statement": "..." }`},
	{"DOC107", "schema/unregistered", "schema", E, false, "A tag is not in the tags registry in docc.json.", `"tags": ["memo"]`, `"tags": ["note"]`},
	{"DOC108", "syntax/invalid-ignore", "syntax", E, true, "docIgnore has no reason, has a wrong shape, or targets a rule that cannot be suppressed.", `"docIgnore": [{ "rule": "DOC301" }]`, `"docIgnore": [{ "rule": "DOC301", "reason": "term from the customer spec" }]`},
	{"DOC109", "schema/missing-kind-schema", "schema", E, false, "The document has no kind, or there is no schema docs/schemas/<kind>.json for its kind. Every document must follow a kind schema; switch the rule off in docc.json to allow free documents.", `{ "title": "A" }`, `{ "title": "A", "kind": "page" }  (with docs/schemas/page.json)`},
	{"DOC201", "graph/duplicate-id", "graph", E, true, "Two documents have the same id (file name without extension) in different folders.", "docs/src/a/x.json and docs/src/b/x.json", "docs/src/a/x.json and docs/src/b/y.json"},
	{"DOC202", "graph/broken-ref", "graph", E, true, "parent, links or [[id]] (also inside notes) point to a document that does not exist.", `"links": ["missing"]`, `"links": ["existing-doc"]`},
	{"DOC203", "graph/orphan", "graph", E, true, "The document is not reachable from any root in roots.", "a document nobody links to", `add the document to links of a reachable document, or set its parent`},
	{"DOC204", "graph/parent-cycle", "graph", E, true, "The parent chain forms a cycle.", `a: parent b; b: parent a`, `a: parent b; b: parent index`},
	{"DOC205", "graph/root-not-found", "graph", E, true, "A root from roots does not exist (or is hidden); an empty src gives it for the default root.", `"roots": ["start"]`, `"roots": ["index"]`},
	{"DOC206", "graph/self-link", "graph", W, false, "A document links to itself.", `{ "title": "A", "links": ["a"] }  (file a.json)`, `{ "title": "A" }`},
	{"DOC208", "graph/undeclared-link", "graph", W, false, "[[id]] is used in the text but is neither in links nor in parent.", `"body": "See [[b]]."`, `"body": "See [[b]].", "links": ["b"]`},
	{"DOC209", "graph/suspect-link", "graph", W, false, "A link in links or notes is not confirmed in reviewed, or the target changed after it was reviewed.", `"links": ["b"]`, `"links": ["b"], "reviewed": { "b": "0a1b2c3d" }`},
	{"DOC210", "graph/link-to-hidden", "graph", E, true, "A visible document links to a hidden one (parent, links, [[id]] outside notes, or an include).", `visible doc: "links": ["secret"]  (secret is hidden)`, `put the reference in notes, or make the target visible`},
	{"DOC211", "graph/bad-include", "graph", E, true, "An include {{id#field}} is impossible: no document or field, a cycle, a value that is not text, or a service field.", `"body": "{{b}}"`, `"body": "{{b#statement}}"`},
	{"DOC301", "text/forbidden-words", "text", W, false, "Forbidden words and phrases from the list (built-in plus words in docc.json).", `"body": "The app is fast."`, `"body": "The app answers in under 200 ms."`},
	{"DOC303", "text/placeholder", "text", W, false, "A placeholder is left: TODO, TBD, FIXME, lorem, ???.", `"body": "TODO"`, `"body": "The final text."`},
	{"DOC304", "text/empty-field", "text", W, false, "A string is empty or only spaces.", `"rationale": ""`, `remove the field or fill it`},
	{"DOC308", "markdown/unclosed-fence", "markdown", W, false, "A code fence is opened but not closed in the same value.", "```go", "```go ... ```"},
	{"DOC309", "markdown/heading-in-field", "markdown", W, false, "A line starting with # inside a field breaks the heading structure of the output.", `"body": "# Title"`, `"body": "Title"`},
	{"DOC310", "markdown/raw-html", "markdown", W, false, "Raw HTML or script in text.", `"body": "<div>x</div>"`, `"body": "x"`},
	{"DOC312", "markdown/broken-anchor", "markdown", E, false, "[[id#field]] points to a field that does not exist or is a service field.", `[[b#missing]]`, `[[b#statement]]`},
	{"DOC313", "text/repeated-word", "text", W, false, "The same word twice in a row.", `"body": "the the system"`, `"body": "the system"`},
	{"DOC352", "style/hedging", "style", W, false, "Hedging and vague-quantity words: maybe, probably, usually, several, various.", `"body": "It will probably work."`, `"body": "It works."`},
	{"DOC353", "style/timeless-words", "style", W, false, "Words that age: currently, recently, soon, new.", `"body": "Currently we use X."`, `"body": "We use X."`},
	{"DOC401", "trace/unknown-id", "trace", E, true, "A @covers tag in code points to a document that does not exist. Works only when trace.scan is set.", "// @covers missing-doc", "// @covers existing-doc"},
	{"DOC501", "build/stale-output", "build", E, true, "out/ differs from what the sources produce: a file is missing, outdated or extra.", "docs/out/a.md edited by hand", "run docc build"},
	{"DOC800", "custom/<name>", "custom", W, false, "A rule from customRules in docc.json; the rule name is in the rule field.", "see customRules", "see customRules"},
	{"DOC901", "meta/unused-ignore", "meta", W, false, "A docIgnore entry suppresses nothing, or targets a disabled rule.", `"docIgnore": [{ "rule": "DOC301", "reason": "x" }]  (nothing to suppress)`, "remove the entry"},
}

var byCode = map[string]*Rule{}
var byName = map[string]*Rule{}

func init() {
	for i := range all {
		r := &all[i]
		byCode[r.Code] = r
		byName[r.Name] = r
	}
}

// All returns every rule ordered by code.
func All() []*Rule {
	out := make([]*Rule, len(all))
	for i := range all {
		out[i] = &all[i]
	}
	return out
}

func ByCode(code string) *Rule { return byCode[code] }
func ByName(name string) *Rule { return byName[name] }

// Lookup resolves a code (DOC301) or a name (text/forbidden-words).
func Lookup(key string) *Rule {
	if r := byCode[key]; r != nil {
		return r
	}
	return byName[key]
}

// IsCustomName reports whether name looks like a custom rule name.
func IsCustomName(name string) bool { return strings.HasPrefix(name, "custom/") }

// Groups are the group keys accepted in the rules section of docc.json.
var Groups = []string{"syntax", "schema", "graph", "text", "markdown", "style", "trace", "build", "meta", "custom"}

func IsGroup(s string) bool { return slices.Contains(Groups, s) }
