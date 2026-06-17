// Go runner. Uses regexp.Compile (RE2) then MatchString (UNANCHORED search),
// mirroring the pinned runtime semantics for the Go target.
//
// Reads corpus.json (argv[1] or ./corpus.json) and emits JSON Lines to stdout:
//   {"id","engine":"go","compiled":bool,"matched":bool|null}
// `matched` is null when the pattern failed to compile.
//
// Run: go run runner.go [corpus.json]
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
)

type pair struct {
	ID       string `json:"id"`
	Pattern  string `json:"pattern"`
	Instance string `json:"instance"`
}

type corpus struct {
	Pairs []pair `json:"pairs"`
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
	for _, p := range c.Pairs {
		re, err := regexp.Compile(p.Pattern)
		r := result{ID: p.ID, Engine: "go"}
		if err == nil {
			r.Compiled = true
			m := re.MatchString(p.Instance)
			r.Matched = &m
		}
		_ = enc.Encode(r)
	}
}
