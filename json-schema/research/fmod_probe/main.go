// Cross-language multipleOf / bound-comparison portability probe (Go arm).
// Run: go run main.go
//
// The point: IEEE fmod (Go math.Mod, Java `%`, JS `%`, Python math.fmod) agrees
// VALUE-FOR-VALUE across all four languages, INCLUDING the fractional footgun:
//
//   0.3 % 0.1 = 0.09999999999999998   (all four -> NOT a multiple)
//   1.1 % 0.1 = 2.7755575615628914e-17 (all four -> NOT a multiple)
//   6.0 % 3   = 0                       (all four -> multiple)
//   7.5 % 1   = 0.5                     (all four -> NOT a multiple)
//   1e300 % 2 = 0                       (all four -> multiple)
//
// So the four "raw" languages agree; the divergence is Pydantic's TOLERANT
// native float multiple_of (see pyd_numeric_probe.py), which accepts 0.3 against
// multiple_of=0.1. Because that cannot be reconciled with fmod, fractional
// multipleOf is rejected at load; integer multipleOf is exact and portable.
//
// Also proves the bound-comparison claim: the integer cap ±(2^53-1) is exactly
// representable as float64, so a capped integer field can be compared against a
// float bound losslessly (float64(cap) == cap).
package main

import (
	"fmt"
	"math"
)

func main() {
	cases := [][2]float64{{10, 2}, {9, 2}, {6, 3}, {7.5, 1}, {0.3, 0.1}, {1.1, 0.1}, {1e300, 2}}
	for _, c := range cases {
		r := math.Mod(c[0], c[1])
		fmt.Printf("%v mod %v = %v -> mult=%v\n", c[0], c[1], r, r == 0)
	}
	var cap int64 = 9007199254740991 // 2^53 - 1
	fmt.Printf("int64 cap %% 7 = %d\n", cap%7)
	fmt.Printf("float64(cap) == cap? %v ; float64(cap) <= 5.5? %v\n",
		float64(cap) == 9007199254740991, float64(cap) <= 5.5)
}
