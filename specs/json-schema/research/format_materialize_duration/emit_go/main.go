// Emits canonical re-serialization for the corpus, as JSON on stdout.
// design B struct for the `full` group; native time.Duration for `timeonly`.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type comp struct{ y, mo, w, d, h, mi, s int64; week bool }

func parseISO(s string) comp {
	var c comp
	body := s[1:]
	if strings.HasPrefix(body, "T") { parseTime(body[1:], &c); return c }
	if strings.HasSuffix(body, "W") { c.week = true; c.w, _ = strconv.ParseInt(body[:len(body)-1], 10, 64); return c }
	datePart := body
	if i := strings.IndexByte(body, 'T'); i >= 0 { datePart = body[:i]; parseTime(body[i+1:], &c) }
	num := ""
	for _, ch := range datePart {
		if ch >= '0' && ch <= '9' { num += string(ch); continue }
		v, _ := strconv.ParseInt(num, 10, 64)
		switch ch { case 'Y': c.y = v; case 'M': c.mo = v; case 'D': c.d = v }
		num = ""
	}
	return c
}
func parseTime(t string, c *comp) {
	num := ""
	for _, ch := range t {
		if ch >= '0' && ch <= '9' { num += string(ch); continue }
		v, _ := strconv.ParseInt(num, 10, 64)
		switch ch { case 'H': c.h = v; case 'M': c.mi = v; case 'S': c.s = v }
		num = ""
	}
}
func serializeB(c comp) string {
	if c.week { return "P" + strconv.FormatInt(c.w, 10) + "W" }
	var date, tim strings.Builder
	if c.y != 0 { fmt.Fprintf(&date, "%dY", c.y) }
	if c.mo != 0 { fmt.Fprintf(&date, "%dM", c.mo) }
	if c.d != 0 { fmt.Fprintf(&date, "%dD", c.d) }
	if c.h != 0 { fmt.Fprintf(&tim, "%dH", c.h) }
	if c.mi != 0 { fmt.Fprintf(&tim, "%dM", c.mi) }
	if c.s != 0 { fmt.Fprintf(&tim, "%dS", c.s) }
	if date.Len() == 0 && tim.Len() == 0 { return "PT0S" }
	out := "P" + date.String()
	if tim.Len() > 0 { out += "T" + tim.String() }
	return out
}
func nativeCanonical(s string) string {
	c := parseISO(s)
	d := time.Duration(c.h)*time.Hour + time.Duration(c.mi)*time.Minute + time.Duration(c.s)*time.Second
	total := int64(d / time.Second)
	h := total / 3600; m := (total % 3600) / 60; sec := total % 60
	var b strings.Builder
	if h != 0 { fmt.Fprintf(&b, "%dH", h) }
	if m != 0 { fmt.Fprintf(&b, "%dM", m) }
	if sec != 0 || (h == 0 && m == 0) { fmt.Fprintf(&b, "%dS", sec) }
	return "PT" + b.String()
}

func main() {
	var corpus map[string]json.RawMessage
	data, _ := os.ReadFile("../corpus.json")
	json.Unmarshal(data, &corpus)
	type row struct{ Id, Wire string }
	out := map[string]map[string]string{"full": {}, "timeonly": {}}
	for _, group := range []string{"full", "timeonly"} {
		var rows []row
		json.Unmarshal(corpus[group], &rows)
		for _, r := range rows {
			if group == "full" { out["full"][r.Id] = serializeB(parseISO(r.Wire)) } else { out["timeonly"][r.Id] = nativeCanonical(r.Wire) }
		}
	}
	b, _ := json.Marshal(out)
	fmt.Println(string(b))
}
