// Go runner for the PINNED `uri` check. Reads pinned_body.json (the anchor-less
// regex body) and corpus.json. Anchors the body with ^...$ (RE2 `$` = end of
// text, no trailing-\n exception) and matches each corpus value.
//
// Emits JSON Lines: {"id","engine":"go","compiled":bool,"matched":bool|null}
// Run: go run runner.go [corpus.json] [pinned_body.json]
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
)

type pair struct {
	ID    string `json:"id"`
	Value string `json:"value"`
}
type corpus struct {
	Pairs []pair `json:"pairs"`
}
type body struct {
	Body string `json:"body"`
}

func main() {
	corpusPath := "corpus.json"
	bodyPath := "pinned_body.json"
	if len(os.Args) > 1 {
		corpusPath = os.Args[1]
	}
	if len(os.Args) > 2 {
		bodyPath = os.Args[2]
	}
	cd, err := os.ReadFile(corpusPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, "read corpus:", err)
		os.Exit(1)
	}
	bd, err := os.ReadFile(bodyPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, "read body:", err)
		os.Exit(1)
	}
	var c corpus
	if err := json.Unmarshal(cd, &c); err != nil {
		fmt.Fprintln(os.Stderr, "parse corpus:", err)
		os.Exit(1)
	}
	var b body
	if err := json.Unmarshal(bd, &b); err != nil {
		fmt.Fprintln(os.Stderr, "parse body:", err)
		os.Exit(1)
	}

	enc := json.NewEncoder(os.Stdout)
	re, cerr := regexp.Compile("^" + b.Body + "$")
	for _, p := range c.Pairs {
		rec := map[string]any{"id": p.ID, "engine": "go"}
		if cerr != nil {
			rec["compiled"] = false
			rec["matched"] = nil
		} else {
			rec["compiled"] = true
			rec["matched"] = re.MatchString(p.Value)
		}
		_ = enc.Encode(rec)
	}
	if cerr != nil {
		fmt.Fprintln(os.Stderr, "GO COMPILE ERROR:", cerr)
	}
}
