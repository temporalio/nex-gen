// Go runner for the `hostname` format conformance corpus.
//
// Implements the PINNED generator-owned check for the Go target:
//   1. package-level compiled regex (RE2, ASCII), fully anchored ^...$
//   2. a total-length guard (1..=253 code points) OUTSIDE the regex
//      (RE2 has no lookahead for a whole-input length assertion)
// The verdict is (regex matches) AND (length in range).
//
// Reads corpus.json (argv[1] or ./corpus.json) and emits JSON Lines to stdout:
//   {"id","engine":"go","valid":bool,"regex":bool,"len_ok":bool}
//
// Run: go run runner.go [corpus.json]
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"unicode/utf8"
)

// PINNED hostname regex: labels of [A-Za-z0-9-], 1-63 chars, no leading/
// trailing hyphen, separated by '.', at least one label. Fully anchored.
// Go's `$` matches end-of-input only (no trailing-\n exception) -- the
// portable choice.
var hostRe = regexp.MustCompile(`^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$`)

const maxTotalLen = 253

type kase struct {
	ID       string `json:"id"`
	Instance string `json:"instance"`
	Valid    bool   `json:"valid"`
}
type corpus struct {
	Cases []kase `json:"cases"`
}
type result struct {
	ID     string `json:"id"`
	Engine string `json:"engine"`
	Valid  bool   `json:"valid"`
	Regex  bool   `json:"regex"`
	LenOK  bool   `json:"len_ok"`
}

func main() {
	path := "corpus.json"
	if len(os.Args) > 1 {
		path = os.Args[1]
	}
	data, err := os.ReadFile(path)
	if err != nil {
		fmt.Fprintln(os.Stderr, "read error:", err)
		os.Exit(1)
	}
	var c corpus
	if err := json.Unmarshal(data, &c); err != nil {
		fmt.Fprintln(os.Stderr, "parse error:", err)
		os.Exit(1)
	}
	enc := json.NewEncoder(os.Stdout)
	for _, k := range c.Cases {
		n := utf8.RuneCountInString(k.Instance)
		lenOK := n >= 1 && n <= maxTotalLen
		rx := hostRe.MatchString(k.Instance)
		enc.Encode(result{
			ID: k.ID, Engine: "go",
			Regex: rx, LenOK: lenOK, Valid: rx && lenOK,
		})
	}
}
