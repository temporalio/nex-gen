// Probe: can Go's STANDARD LIBRARY construct an idiomatic typed in-memory
// representation for each format (time, date, date-time, uuid, ipv4, ipv6)
// from the validated string, and does it round-trip / match the pinned
// RFC 3339 grammar? Backs features/format typed-repr research.
//
//	go run .
package main

import (
	"fmt"
	"net"
	"net/netip"
	"time"
)

func try[T any](f func() (T, error)) string {
	v, err := f()
	if err != nil {
		return "ERR: " + err.Error()
	}
	return fmt.Sprintf("OK -> %v", v)
}

func main() {
	fmt.Println("=== Go stdlib typed representations ===")

	// ---- date-time (RFC 3339) : time.Time ----
	fmt.Println("\n[date-time] type=time.Time  ctor=time.Parse(time.RFC3339Nano, s)")
	for _, s := range []string{
		"2021-02-28T23:59:60Z", // leap second (pinned: ACCEPT)
		"2006-01-02T15:04:05Z",
		"2006-01-02T15:04:05+00:00", // offset +00:00
		"2006-01-02T15:04:05-00:00", // -00:00 unknown offset (pinned: ACCEPT)
		"2006-01-02T15:04:05.123456789Z",
		"2006-01-02t15:04:05z",     // lowercase t/z (pinned: ACCEPT)
		"2006-01-02T15:04:05",      // missing offset (pinned: REJECT)
		"2021-02-30T00:00:00Z",     // bad calendar day (pinned: REJECT)
	} {
		t, err := time.Parse(time.RFC3339Nano, s)
		if err != nil {
			fmt.Printf("  %-32q -> ERR %v\n", s, err)
			continue
		}
		fmt.Printf("  %-32q -> OK  reformat=%q\n", s, t.Format(time.RFC3339Nano))
	}

	// ---- date (date only) : no dedicated stdlib type, only time.Time ----
	fmt.Println("\n[date] NO date-only stdlib type. Reuse time.Time via layout \"2006-01-02\"")
	for _, s := range []string{"2020-02-29", "2021-02-29", "2021-13-01"} {
		t, err := time.Parse("2006-01-02", s)
		if err != nil {
			fmt.Printf("  %-12q -> ERR %v\n", s, err)
			continue
		}
		fmt.Printf("  %-12q -> OK  reformat=%q (carries a zero time-of-day + UTC)\n", s, t.Format("2006-01-02"))
	}

	// ---- time (time only) : no dedicated stdlib type ----
	fmt.Println("\n[time] NO time-only stdlib type. time.Time via layout \"15:04:05Z07:00\"")
	for _, s := range []string{"23:59:60Z", "12:00:00", "12:00:00.5+01:00"} {
		t, err := time.Parse("15:04:05Z07:00", s)
		if err != nil {
			fmt.Printf("  %-20q -> ERR %v\n", s, err)
			continue
		}
		fmt.Printf("  %-20q -> OK  reformat=%q (carries a zero date 0000-01-01)\n", s, t.Format("15:04:05Z07:00"))
	}

	// ---- uuid : NO stdlib type at all ----
	fmt.Println("\n[uuid] NO stdlib uuid type and NO stdlib parser. Would need [16]byte + hand-rolled parse, or github.com/google/uuid (DEP).")

	// ---- ipv4 / ipv6 : net.IP and net/netip.Addr ----
	fmt.Println("\n[ipv4/ipv6] type=net/netip.Addr (or legacy net.IP)")
	for _, s := range []string{
		"192.168.0.1",
		"256.0.0.1",   // out of range
		"01.2.3.4",    // leading zero (pinned ipv4: REJECT)
		"1.2.3",       // short
		"::1",
		"2001:db8::1",
		"2001:DB8::1",              // uppercase hex
		"::ffff:192.168.0.1",       // v4-mapped
		"fe80::1%eth0",             // zone id
		"2001:0db8:0000:0000:0000:0000:0000:0001", // fully expanded
	} {
		a, err := netip.ParseAddr(s)
		if err != nil {
			fmt.Printf("  netip %-42q -> ERR %v\n", s, err)
			continue
		}
		fmt.Printf("  netip %-42q -> OK  String()=%q is4=%v is6=%v\n", s, a.String(), a.Is4(), a.Is6())
	}
	fmt.Println("  -- net.ParseIP (legacy, does NOT distinguish v4/v6 family cleanly) --")
	for _, s := range []string{"192.168.0.1", "01.2.3.4", "1.2.3", "::1", "2001:DB8::1"} {
		ip := net.ParseIP(s)
		if ip == nil {
			fmt.Printf("  net.ParseIP %-16q -> nil (reject)\n", s)
			continue
		}
		fmt.Printf("  net.ParseIP %-16q -> OK  String()=%q\n", s, ip.String())
	}
}
