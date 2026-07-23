// Probe: MATERIALIZE model (B). Parse each validated wire string into Go's
// stdlib typed construct, then re-serialize via the GENERATOR-OWNED serializer
// (RFC 3339, original offset preserved with +00:00/-00:00 -> Z, T/Z uppercased
// on the parse path, fractional seconds at the value's own precision with
// trailing zeros trimmed, no fractional part when zero) -- NO TRUNCATION.
//
//	date-time -> time.Time            (offset + nanosecond preserved, lossless)
//	date      -> time.Time (phantom)  (YYYY-MM-DD, lossless)
//	time      -> time.Time (phantom)  (offset preserved when present, lossless)
//
// go run runner.go corpus.json
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"
)

type row struct {
	ID   string `json:"id"`
	Wire string `json:"wire"`
}
type corpus struct {
	DateTime []row `json:"date-time"`
	Date     []row `json:"date"`
	Time     []row `json:"time"`
}
type out struct {
	ID        string `json:"id"`
	Engine    string `json:"engine"`
	Format    string `json:"format"`
	Canonical string `json:"canonical"` // serialized bytes, or "" if cannot materialize
	Err       string `json:"err"`
}

const engine = "go"

func emit(o out) {
	b, _ := json.Marshal(o)
	fmt.Println(string(b))
}

// fractional part of a nanosecond count: ".ddd" with trailing zeros trimmed,
// or "" when zero.
func fracNanos(nanos int) string {
	if nanos == 0 {
		return ""
	}
	s := fmt.Sprintf("%09d", nanos)
	s = strings.TrimRight(s, "0")
	return "." + s
}

// offset from a count of seconds: "Z" for zero, else "+HH:MM" / "-HH:MM".
func offsetStr(secs int) string {
	if secs == 0 {
		return "Z"
	}
	sign := "+"
	if secs < 0 {
		sign = "-"
		secs = -secs
	}
	return fmt.Sprintf("%s%02d:%02d", sign, secs/3600, (secs%3600)/60)
}

func main() {
	data, _ := os.ReadFile(os.Args[1])
	var c corpus
	json.Unmarshal(data, &c)

	// date-time : time.Time. Parse path uppercases the case-insensitive t/z
	// (pinned grammar accepts lowercase; Go's native parser rejects it). Safe
	// because date-time has no other letters (offset is digits only). Go
	// REJECTS leap :60, so that row errors -> proves the narrowing.
	// time.RFC3339Nano preserves the offset (Z for zero) and nanoseconds and
	// trims trailing fractional zeros -- exactly the generator-owned form.
	for _, r := range c.DateTime {
		t, err := time.Parse(time.RFC3339Nano, strings.ToUpper(r.Wire))
		if err != nil {
			emit(out{r.ID, engine, "date-time", "", err.Error()})
			continue
		}
		emit(out{r.ID, engine, "date-time", t.Format(time.RFC3339Nano), ""})
	}

	// date : Go has no date-only type; time.Time carries a phantom time-of-day.
	for _, r := range c.Date {
		t, err := time.Parse("2006-01-02", r.Wire)
		if err != nil {
			emit(out{r.ID, engine, "date", "", err.Error()})
			continue
		}
		emit(out{r.ID, engine, "date", t.Format("2006-01-02"), ""})
	}

	// time : Go has no time-of-day type; time.Time carries a phantom date. The
	// offset is PRESERVED (rides in the zone) when present; an offset-less value
	// stays offset-less. Serialize manually so trailing fractional zeros are
	// trimmed and +00:00/-00:00 -> Z consistently.
	for _, r := range c.Time {
		w := strings.ToUpper(r.Wire)
		var t time.Time
		var err error
		hasOffset := true
		t, err = time.Parse("15:04:05.999999999Z07:00", w)
		if err != nil {
			hasOffset = false
			t, err = time.Parse("15:04:05.999999999", w)
		}
		if err != nil {
			emit(out{r.ID, engine, "time", "", err.Error()})
			continue
		}
		s := fmt.Sprintf("%02d:%02d:%02d%s", t.Hour(), t.Minute(), t.Second(), fracNanos(t.Nanosecond()))
		if hasOffset {
			_, off := t.Zone()
			s += offsetStr(off)
		}
		emit(out{r.ID, engine, "time", s, ""})
	}
}
