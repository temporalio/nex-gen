// Go runner. Compiles the pinned email regex with regexp (RE2) and applies it
// with MatchString (unanchored search -- irrelevant here, the regex is fully
// ^...$ anchored). RE2 uses ASCII classes and rune `.`; the pinned regex uses
// only explicit ASCII classes, so no extra config is needed.
//
// Go/JS keep `$` as-is (end-of-input only), so no anchor normalization.
//
// Emits JSON Lines: {"id","engine":"go","compiled":bool,"matched":bool|null}
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
)

type pair struct {
	ID       string `json:"id"`
	Instance string `json:"instance"`
}

type corpus struct {
	PinnedRegex string `json:"pinned_regex"`
	Pairs       []pair `json:"pairs"`
}

type result struct {
	ID       string `json:"id"`
	Engine   string `json:"engine"`
	Compiled bool   `json:"compiled"`
	Matched  *bool  `json:"matched"`
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
	re, cErr := regexp.Compile(c.PinnedRegex)
	for _, p := range c.Pairs {
		r := result{ID: p.ID, Engine: "go"}
		if cErr == nil {
			r.Compiled = true
			m := re.MatchString(p.Instance)
			r.Matched = &m
		}
		_ = enc.Encode(r)
	}
}
