// Go runner for the JSON-Schema `format` conformance corpus.
//
// Implements the SPEC'S PINNED CHECK for each asserted-v1 format:
//   - a pinned, fully-anchored, RE2-safe regex (no lookaround/backref), compiled
//     once at package init with regexp.MustCompile, then MatchString; PLUS
//   - for the temporal formats (date/time/date-time), a shared integer-arithmetic
//     calendar predicate (month 01-12, day within the month's Gregorian length,
//     leap-year Feb 29).
//
// This is the OWNED check. We deliberately do NOT delegate to time.Parse / net.ParseIP
// as the source of truth. As a SECONDARY column we ALSO record what Go's native
// typed parser accepts, purely to document divergence -- it does not decide the verdict.
//
// Reads corpus.json (argv[1] or ./corpus.json) and emits JSON Lines to stdout:
//   {"id","engine":"go","valid":bool,"native":bool}
//
// Run: go run runner.go [corpus.json]
package main

import (
	"encoding/json"
	"fmt"
	"net/netip"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// ---- pinned patterns (anchored, RE2-safe, compiled once) --------------------

const octet = `(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])`

var (
	uuidRe = regexp.MustCompile(`^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$`)
	ipv4Re = regexp.MustCompile(`^` + octet + `\.` + octet + `\.` + octet + `\.` + octet + `$`)
	ipv6Re = regexp.MustCompile(ipv6Pattern())

	// RFC 3339 syntactic fragments (case-insensitive T/Z handled with (?i) inline
	// only over the separators -- but for RE2-safety and portability we instead
	// accept both cases explicitly in the character classes below).
	dateRe = regexp.MustCompile(`^([0-9]{4})-([0-9]{2})-([0-9]{2})$`)
	// partial-time = HH:MM:SS(.frac)? ; full-time adds an offset (Z|z|+-HH:MM).
	timeRe     = regexp.MustCompile(`^([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})?$`)
	dateTimeRe = regexp.MustCompile(`^([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$`)
)

// ipv6Pattern builds the RFC 4291 IPv6 regex (full, ::-compressed, and IPv4-tail
// forms). Standard portable construction; RE2-safe.
func ipv6Pattern() string {
	h16 := `[0-9a-fA-F]{1,4}`
	v4 := `(` + octet + `\.` + octet + `\.` + octet + `\.` + octet + `)`
	ls32 := `(` + h16 + `:` + h16 + `|` + v4 + `)`
	p := `(` +
		`(` + h16 + `:){6}` + ls32 + `|` +
		`::(` + h16 + `:){5}` + ls32 + `|` +
		`(` + h16 + `)?::(` + h16 + `:){4}` + ls32 + `|` +
		`((` + h16 + `:){0,1}` + h16 + `)?::(` + h16 + `:){3}` + ls32 + `|` +
		`((` + h16 + `:){0,2}` + h16 + `)?::(` + h16 + `:){2}` + ls32 + `|` +
		`((` + h16 + `:){0,3}` + h16 + `)?::(` + h16 + `:)` + ls32 + `|` +
		`((` + h16 + `:){0,4}` + h16 + `)?::` + ls32 + `|` +
		`((` + h16 + `:){0,5}` + h16 + `)?::` + h16 + `|` +
		`((` + h16 + `:){0,6}` + h16 + `)?::` +
		`)`
	return `^` + p + `$`
}

// ---- shared calendar predicate (integer arithmetic only) --------------------

func isLeap(y int) bool { return (y%4 == 0 && y%100 != 0) || y%400 == 0 }

func daysInMonth(y, m int) int {
	switch m {
	case 1, 3, 5, 7, 8, 10, 12:
		return 31
	case 4, 6, 9, 11:
		return 30
	case 2:
		if isLeap(y) {
			return 29
		}
		return 28
	}
	return 0
}

// validCalendarDate checks month 01-12 and day within the month's length.
func validCalendarDate(y, m, d int) bool {
	if m < 1 || m > 12 {
		return false
	}
	return d >= 1 && d <= daysInMonth(y, m)
}

// validTimeFields checks HH:MM:SS with leap-second :60 accepted per pinned rule.
func validTimeFields(hh, mm, ss int) bool {
	if hh > 23 || mm > 59 {
		return false
	}
	return ss <= 60 // :60 leap second accepted syntactically
}

// validOffset checks +-HH:MM ranges (offset hour 0-23, minute 0-59).
func validOffset(off string) bool {
	if off == "" || off == "Z" || off == "z" {
		return true
	}
	// off is +HH:MM or -HH:MM
	oh, _ := strconv.Atoi(off[1:3])
	om, _ := strconv.Atoi(off[4:6])
	return oh <= 23 && om <= 59
}

func atoi(s string) int { n, _ := strconv.Atoi(s); return n }

// ---- pinned per-format check ------------------------------------------------

func pinnedValid(format, v string) bool {
	switch format {
	case "uuid":
		return uuidRe.MatchString(v)
	case "ipv4":
		return ipv4Re.MatchString(v)
	case "ipv6":
		return ipv6Re.MatchString(v)
	case "date":
		g := dateRe.FindStringSubmatch(v)
		if g == nil {
			return false
		}
		return validCalendarDate(atoi(g[1]), atoi(g[2]), atoi(g[3]))
	case "time":
		g := timeRe.FindStringSubmatch(v)
		if g == nil {
			return false
		}
		return validTimeFields(atoi(g[1]), atoi(g[2]), atoi(g[3])) && validOffset(g[5])
	case "date-time":
		g := dateTimeRe.FindStringSubmatch(v)
		if g == nil {
			return false
		}
		return validCalendarDate(atoi(g[1]), atoi(g[2]), atoi(g[3])) &&
			validTimeFields(atoi(g[4]), atoi(g[5]), atoi(g[6])) && validOffset(g[8])
	}
	return false
}

// ---- SECONDARY: Go native typed parser (documentation only) -----------------

func nativeValid(format, v string) bool {
	switch format {
	case "uuid":
		return false // no stdlib uuid parser; omit from native column
	case "ipv4":
		ip, err := netip.ParseAddr(v)
		return err == nil && ip.Is4()
	case "ipv6":
		ip, err := netip.ParseAddr(v)
		return err == nil && strings.Contains(v, ":") && ip.Is6()
	case "date":
		_, err := time.Parse("2006-01-02", v)
		return err == nil
	case "time":
		_, err := time.Parse("15:04:05Z07:00", v)
		if err == nil {
			return true
		}
		_, err2 := time.Parse("15:04:05", v)
		return err2 == nil
	case "date-time":
		_, err := time.Parse(time.RFC3339Nano, v)
		return err == nil
	}
	return false
}

type pair struct {
	ID     string `json:"id"`
	Format string `json:"format"`
	Value  string `json:"value"`
}

type corpus struct {
	Pairs []pair `json:"pairs"`
}

type result struct {
	ID     string `json:"id"`
	Engine string `json:"engine"`
	Valid  bool   `json:"valid"`
	Native bool   `json:"native"`
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
		_ = enc.Encode(result{
			ID:     p.ID,
			Engine: "go",
			Valid:  pinnedValid(p.Format, p.Value),
			Native: nativeValid(p.Format, p.Value),
		})
	}
}
