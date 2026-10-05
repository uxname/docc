package rules

import "testing"

func TestRegistry(t *testing.T) {
	if n := len(All()); n != 37 {
		t.Fatalf("want 37 rules, got %d", n)
	}
	core := 0
	for _, r := range All() {
		if Lookup(r.Code) != r || Lookup(r.Name) != r {
			t.Errorf("lookup %s", r.Code)
		}
		if r.Core {
			core++
		}
		if !IsGroup(r.Group) {
			t.Errorf("%s: bad group %s", r.Code, r.Group)
		}
	}
	if core != 18 { // 101-105, 108, 109, 201-205, 210-212, 401, 501, 503
		t.Errorf("core rules: %d", core)
	}
}
