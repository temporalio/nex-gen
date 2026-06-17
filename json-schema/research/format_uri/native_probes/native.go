// Go NATIVE URI-parser probe. For each input, reports what Go's net/url thinks:
//   - url.Parse succeeds AND result.IsAbs() (has a scheme) => "valid absolute"
// This mirrors what a naive "use the stdlib" format:uri validator would do.
//
// Emits JSON Lines: {"id","engine":"go-native","valid":bool,"detail":string}
// Run: go run native.go ../native_inputs.json
package main

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
)

type input struct {
	ID    string `json:"id"`
	Value string `json:"value"`
}
type corpus struct {
	Inputs []input `json:"inputs"`
}

func main() {
	path := "../native_inputs.json"
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
	for _, in := range c.Inputs {
		valid := false
		detail := ""
		u, perr := url.Parse(in.Value)
		if perr != nil {
			detail = "parse-error: " + perr.Error()
		} else if !u.IsAbs() {
			detail = "not-absolute (no scheme)"
		} else {
			valid = true
			detail = "scheme=" + u.Scheme
		}
		_ = enc.Encode(map[string]any{
			"id": in.ID, "engine": "go-native", "valid": valid, "detail": detail,
		})
	}
}
