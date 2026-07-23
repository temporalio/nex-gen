// Go materialization probe for the `duration` format.
//
// Three questions:
//  1. Can Go's stdlib fixed-duration type (time.Duration, int64 ns) hold the
//     FULL accepted grammar (incl. Y/M/W)? -> NO. Prove it.
//  2. A generated custom struct (design B) holding the 7 integer components:
//     does it round-trip every `full` wire string byte-identically?
//  3. The narrowed time-only subset (design C) -> time.Duration: does it
//     round-trip the CANONICAL PTnHnMnS form? And what happens to a
//     non-canonical input like PT90M?
//
// Run: cd go_full && go run .
package main

import (
	"fmt"
	"strconv"
	"strings"
	"time"
)

// ---- design B: the custom component struct ------------------------------

type ISODuration struct {
	Years, Months, Weeks, Days, Hours, Minutes, Seconds int64
	Week                                                bool // week form was used
}

// parseISO decodes an already-validated duration string into components.
// (Validation is unchanged; this only runs on strings the pinned regex passed.)
func parseISO(s string) ISODuration {
	var d ISODuration
	body := s[1:] // drop 'P'
	if strings.HasPrefix(body, "T") {
		// pure time
		parseTime(body[1:], &d)
		return d
	}
	if strings.HasSuffix(body, "W") {
		d.Week = true
		d.Weeks, _ = strconv.ParseInt(body[:len(body)-1], 10, 64)
		return d
	}
	// split date part / time part on 'T'
	datePart := body
	if i := strings.IndexByte(body, 'T'); i >= 0 {
		datePart = body[:i]
		parseTime(body[i+1:], &d)
	}
	num := ""
	for _, c := range datePart {
		if c >= '0' && c <= '9' {
			num += string(c)
			continue
		}
		v, _ := strconv.ParseInt(num, 10, 64)
		switch c {
		case 'Y':
			d.Years = v
		case 'M':
			d.Months = v
		case 'D':
			d.Days = v
		}
		num = ""
	}
	return d
}

func parseTime(t string, d *ISODuration) {
	num := ""
	for _, c := range t {
		if c >= '0' && c <= '9' {
			num += string(c)
			continue
		}
		v, _ := strconv.ParseInt(num, 10, 64)
		switch c {
		case 'H':
			d.Hours = v
		case 'M':
			d.Minutes = v
		case 'S':
			d.Seconds = v
		}
		num = ""
	}
}

// CANONICAL serialization for design B: reproduce components in ISO order,
// omitting zero components; whole-zero -> "PT0S"; week form -> "PnW".
func (d ISODuration) String() string {
	if d.Week {
		return "P" + strconv.FormatInt(d.Weeks, 10) + "W"
	}
	var date, tim strings.Builder
	if d.Years != 0 {
		fmt.Fprintf(&date, "%dY", d.Years)
	}
	if d.Months != 0 {
		fmt.Fprintf(&date, "%dM", d.Months)
	}
	if d.Days != 0 {
		fmt.Fprintf(&date, "%dD", d.Days)
	}
	if d.Hours != 0 {
		fmt.Fprintf(&tim, "%dH", d.Hours)
	}
	if d.Minutes != 0 {
		fmt.Fprintf(&tim, "%dM", d.Minutes)
	}
	if d.Seconds != 0 {
		fmt.Fprintf(&tim, "%dS", d.Seconds)
	}
	if date.Len() == 0 && tim.Len() == 0 {
		return "PT0S"
	}
	out := "P" + date.String()
	if tim.Len() > 0 {
		out += "T" + tim.String()
	}
	return out
}

// ---- design C: native time.Duration for the narrowed time-only subset ----

// nativeTimeOnly parses a PTnHnMnS string into time.Duration (int64 ns).
func nativeTimeOnly(s string) (time.Duration, bool) {
	if !strings.HasPrefix(s, "PT") {
		return 0, false
	}
	d := parseISO(s)
	if d.Years|d.Months|d.Weeks|d.Days != 0 {
		return 0, false // has date component -> not representable
	}
	return time.Duration(d.Hours)*time.Hour +
		time.Duration(d.Minutes)*time.Minute +
		time.Duration(d.Seconds)*time.Second, true
}

// canonical PTnHnMnS from a time.Duration total.
func nativeCanonical(d time.Duration) string {
	total := int64(d / time.Second)
	h := total / 3600
	m := (total % 3600) / 60
	s := total % 60
	var b strings.Builder
	if h != 0 {
		fmt.Fprintf(&b, "%dH", h)
	}
	if m != 0 {
		fmt.Fprintf(&b, "%dM", m)
	}
	if s != 0 || (h == 0 && m == 0) {
		fmt.Fprintf(&b, "%dS", s)
	}
	return "PT" + b.String()
}

func main() {
	fmt.Println("=== Q1: can time.Duration hold the full grammar? ===")
	// time.Duration is int64 nanoseconds. There is no field for years/months/
	// weeks and no reference date, so P1Y / P1M / P4W are UNREPRESENTABLE.
	fmt.Println("time.Duration = int64 nanoseconds; no Y/M/W field, no reference date.")
	fmt.Println("P1Y  -> cannot store (a year is not a fixed ns count)")
	fmt.Println("P1M  -> cannot store (a month is calendar-variable)")
	fmt.Println("P4W  -> could store as 4*7*24h BUT loses the week form on re-emit")
	fmt.Println()

	fmt.Println("=== Q2: design B custom struct round-trip (full corpus) ===")
	full := []string{
		"P3Y6M4DT12H30M5S", "P1Y", "P2M", "P10D", "P4W", "P1W",
		"P1Y6M", "P1Y6M4D", "P6M4D", "P1YT1H", "P1DT12H",
		"P100Y200M300DT400H500M600S", "P0Y",
	}
	allOK := true
	for _, w := range full {
		got := parseISO(w).String()
		// P0Y canonicalizes to PT0S (whole-zero rule); note it.
		ok := got == w
		if w == "P0Y" {
			ok = got == "PT0S"
		}
		if !ok {
			allOK = false
		}
		fmt.Printf("  %-30s -> %-20s %v\n", w, got, ok)
	}
	fmt.Printf("  design B full round-trip (P0Y->PT0S expected): allOK=%v\n\n", allOK)

	fmt.Println("=== Q3: design C native time.Duration round-trip (time-only) ===")
	timeonly := []string{"PT1H", "PT30M", "PT15S", "PT1H30M15S", "PT1H30M", "PT30M15S", "PT0S"}
	for _, w := range timeonly {
		nd, ok := nativeTimeOnly(w)
		got := nativeCanonical(nd)
		fmt.Printf("  %-12s -> native %-14v -> %-12s roundtrip=%v\n", w, nd, got, ok && got == w)
	}
	fmt.Println()
	fmt.Println("  non-canonical inputs (native total collapses them):")
	for _, w := range []string{"PT90M", "PT3600S", "PT24H"} {
		nd, _ := nativeTimeOnly(w)
		got := nativeCanonical(nd)
		fmt.Printf("  %-10s -> native %-10v -> canonical %-10s (byte-equal to input=%v)\n", w, nd, got, got == w)
	}
}
