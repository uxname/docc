package export

import (
	"encoding/csv"
	"fmt"
	"io"
	"time"
)

// FileName returns the name of the report file.
func FileName(from time.Time) string {
	return fmt.Sprintf("report-%s.csv", from.Format("2006-01-02"))
}

// WriteCSV writes the report rows as CSV.
func WriteCSV(w io.Writer, rows [][]string) error {
	if _, err := w.Write([]byte("\xEF\xBB\xBF")); err != nil {
		return err
	}
	cw := csv.NewWriter(w)
	cw.Comma = ';'
	if err := cw.WriteAll(rows); err != nil {
		return err
	}
	cw.Flush()
	return cw.Error()
}
