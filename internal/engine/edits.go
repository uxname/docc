package engine

import (
	"strings"

	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/loader"
)

// Patches below edit the JSON text at byte offsets and leave all other
// formatting alone.

// removeMember deletes member i of an object, with the comma that goes with it.
func removeMember(obj *loader.Node, i int) diag.Edit {
	ms := obj.Members
	switch {
	case len(ms) == 1:
		return diag.Edit{Start: ms[0].KeyStart, End: ms[0].Value.End}
	case i < len(ms)-1:
		return diag.Edit{Start: ms[i].KeyStart, End: ms[i+1].KeyStart}
	default:
		return diag.Edit{Start: ms[i-1].Value.End, End: ms[i].Value.End}
	}
}

// removeItem deletes element i of an array, with the comma that goes with it.
func removeItem(arr *loader.Node, i int) diag.Edit {
	its := arr.Items
	switch {
	case len(its) == 1:
		return diag.Edit{Start: its[0].Start, End: its[0].End}
	case i < len(its)-1:
		return diag.Edit{Start: its[i].Start, End: its[i+1].Start}
	default:
		return diag.Edit{Start: its[i-1].End, End: its[i].End}
	}
}

// appendItem adds a string element at the end of an array.
func appendItem(arr *loader.Node, quoted string) diag.Edit {
	if len(arr.Items) == 0 {
		return diag.Edit{Start: arr.Start + 1, End: arr.Start + 1, Text: quoted}
	}
	at := arr.Items[len(arr.Items)-1].End
	return diag.Edit{Start: at, End: at, Text: ", " + quoted}
}

// addMember appends `"key": value` to an object, taking the indentation of
// the last member's line (or staying on one line when the object is one).
func addMember(src *loader.Source, obj *loader.Node, key, value string) diag.Edit {
	if len(obj.Members) == 0 {
		return diag.Edit{Start: obj.Start + 1, End: obj.Start + 1, Text: ` "` + key + `": ` + value + ` `}
	}
	last := obj.Members[len(obj.Members)-1]
	at := last.Value.End
	line, _ := src.Position(last.KeyStart)
	firstLine, _ := src.Position(obj.Members[0].KeyStart)
	openLine, _ := src.Position(obj.Start)
	if line == openLine && firstLine == openLine {
		return diag.Edit{Start: at, End: at, Text: `, "` + key + `": ` + value}
	}
	ls := lineStart(src.Data, last.KeyStart)
	indent := string(src.Data[ls:last.KeyStart])
	if strings.TrimSpace(indent) != "" {
		indent = "  " // the key shares its line with other text
	}
	return diag.Edit{Start: at, End: at, Text: ",\n" + indent + `"` + key + `": ` + value}
}

func lineStart(data []byte, off int) int {
	for off > 0 && data[off-1] != '\n' {
		off--
	}
	return off
}
