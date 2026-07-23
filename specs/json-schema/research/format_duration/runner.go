// Go runner for the `duration` format conformance corpus.
//
// Compiles the SINGLE generator-owned pinned regex (from corpus.json's
// `pinned_regex`) with Go's regexp (RE2) and, for each corpus value, reports
// whether it matches. The pinned regex is fully anchored (^...$), so a plain
// MatchString gives the whole-string verdict.
//
// Reads corpus.json (argv[1] or ./corpus.json) and emits JSON Lines to stdout:
//
//	{"id","engine":"go","compiled":bool,"matched":bool|null}
//
// `compiled` reports whether the pinned regex compiled at all (same value for
// every row) and `matched` is null only if it did not.
//
// Run: go run runner.go [corpus.json]
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
)

type kase struct {
	ID    string `json:"id"`
	Value string `json:"value"`
}

type corpus struct {
	PinnedRegex string `json:"pinned_regex"`
	Cases       []kase `json:"cases"`
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
	re, compileErr := regexp.Compile(c.PinnedRegex)
	enc := json.NewEncoder(os.Stdout)
	for _, k := range c.Cases {
		r := result{ID: k.ID, Engine: "go"}
		if compileErr == nil {
			r.Compiled = true
			m := re.MatchString(k.Value)
			r.Matched = &m
		}
		_ = enc.Encode(r)
	}
}
