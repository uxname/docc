package render

import (
	"os/exec"
	"testing"
)

// The pure parts of app.js (word marks of an edit, fragments, removed
// paragraphs, the edits of a page) are checked by app_test.js in Node;
// without Node the test is skipped.
func TestAppLogic(t *testing.T) {
	node, err := exec.LookPath("node")
	if err != nil {
		t.Skip("node is not installed")
	}
	out, err := exec.Command(node, "app_test.js", "app.js").CombinedOutput()
	if err != nil {
		t.Fatalf("app_test.js: %v\n%s", err, out)
	}
}
