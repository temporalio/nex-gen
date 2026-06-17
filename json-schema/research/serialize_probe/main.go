package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
)

const IntegerCap = 1<<53 - 1

type ValidationError struct {
	Path   string
	Reason string
}

func (e *ValidationError) Error() string { return e.Path + ": " + e.Reason }

// ---- SHARED predicate layer: pure functions over DECODED values.
// Called by BOTH MarshalJSON (serialize) and UnmarshalJSON (deserialize).

func checkMinLen(path, v string, n int, errs *[]error) {
	if len(v) < n {
		*errs = append(*errs, &ValidationError{path, fmt.Sprintf("minLength %d, got %d", n, len(v))})
	}
}
func checkConst(path, v, want string, errs *[]error) {
	if v != want {
		*errs = append(*errs, &ValidationError{path, fmt.Sprintf("const %q, got %q", want, v)})
	}
}
func checkCap(path string, v int64, errs *[]error) {
	if v < -IntegerCap || v > IntegerCap {
		*errs = append(*errs, &ValidationError{path, "exceeds ±(2^53-1) cap"})
	}
}

// ---- model ----

type User struct {
	Name     string  // required, minLength 3
	Nickname *string // optional, default "anon" (NOT stored; surfaced on read)
	Version  string  // const "v1", auto-managed
	Count    *int64  // optional, capped
}

// SHARED Validate over the decoded model. Identical predicates both directions.
func (u User) Validate() error {
	var errs []error
	checkMinLen("name", u.Name, 3, &errs)
	checkConst("version", u.Version, "v1", &errs)
	if u.Count != nil {
		checkCap("count", *u.Count, &errs)
	}
	return errors.Join(errs...)
}

// default materialized on READ, not stored at deserialize. No deep-equals anywhere.
func (u User) GetNickname() string {
	if u.Nickname != nil {
		return *u.Nickname
	}
	return "anon"
}

func (u User) MarshalJSON() ([]byte, error) {
	if err := u.Validate(); err != nil { // <-- shared layer
		return nil, err
	}
	m := map[string]any{
		"name":    u.Name,
		"version": "v1", // const: AUTO-EMITTED, never omitted
	}
	if u.Nickname != nil { // omit-unset; no comparison against default
		m["nickname"] = *u.Nickname
	}
	if u.Count != nil {
		m["count"] = *u.Count
	}
	return json.Marshal(m)
}

func (u *User) UnmarshalJSON(data []byte) error {
	var s struct {
		Name     *string      `json:"name"`
		Nickname *string      `json:"nickname"`
		Version  *string      `json:"version"`
		Count    *json.Number `json:"count"`
	}
	if err := json.Unmarshal(data, &s); err != nil {
		return err
	}
	var errs []error
	// PARSE LAYER (deserialize-only, direction-specific): wire-absence -> required
	if s.Name == nil {
		errs = append(errs, &ValidationError{"name", "required, absent"})
	} else {
		u.Name = *s.Name
	}
	if s.Version == nil {
		errs = append(errs, &ValidationError{"version", "required, absent"})
	} else {
		u.Version = *s.Version
	}
	u.Nickname = s.Nickname // NOT populated with default
	if s.Count != nil {
		// PARSE LAYER: spec-integer parse (1.0 ok, 1.5 reject) -- cannot live in shared
		f, err := s.Count.Float64()
		if err != nil {
			errs = append(errs, &ValidationError{"count", "not a number"})
		} else if f != math.Trunc(f) {
			errs = append(errs, &ValidationError{"count", "fractional, not an integer"})
		} else {
			i, _ := s.Count.Int64()
			u.Count = &i
		}
	}
	// SHARED LAYER: the SAME Validate() that MarshalJSON runs.
	if err := u.Validate(); err != nil {
		errs = append(errs, err)
	}
	return errors.Join(errs...)
}

func main() {
	bignum := int64(IntegerCap + 1)

	fmt.Println("== A. invalid in-memory model -> serialize aggregates (shared Validate) ==")
	bad := User{Name: "ab", Version: "v1", Count: &bignum}
	if _, err := json.Marshal(bad); err != nil {
		fmt.Println(err)
	}

	fmt.Println("\n== B. valid, nickname unset -> serialize omits nickname, auto-emits const ==")
	good := User{Name: "alice", Version: "v1"}
	b, _ := json.Marshal(good)
	fmt.Println(string(b))

	fmt.Println("\n== C. deserialize wire w/o nickname -> not stored; default on read ==")
	var u User
	_ = json.Unmarshal([]byte(`{"name":"alice","version":"v1"}`), &u)
	fmt.Printf("Nickname stored = %v (nil=%v); GetNickname() = %q\n", u.Nickname, u.Nickname == nil, u.GetNickname())

	fmt.Println("\n== D. deserialize invalid (short name) -> SAME shared Validate error ==")
	var u2 User
	if err := json.Unmarshal([]byte(`{"name":"ab","version":"v1"}`), &u2); err != nil {
		fmt.Println(err)
	}

	fmt.Println("\n== E. deserialize count 1.5 -> PARSE-layer error (not in shared) ==")
	var u3 User
	if err := json.Unmarshal([]byte(`{"name":"alice","version":"v1","count":1.5}`), &u3); err != nil {
		fmt.Println(err)
	}

	fmt.Println("\n== F. round-trip: deserialize then serialize is byte-identical (no default echo) ==")
	in := `{"name":"alice","version":"v1"}`
	var rt User
	_ = json.Unmarshal([]byte(in), &rt)
	out, _ := json.Marshal(rt)
	fmt.Printf("in : %s\nout: %s\nidentical-shape: %v\n", in, string(out), string(out) == in)
}
