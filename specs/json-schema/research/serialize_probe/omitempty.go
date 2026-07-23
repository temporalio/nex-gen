package main

import (
	"encoding/json"
	"fmt"
)

type Quad struct {
	// optional+non-nullable: omit when unset
	OptNN *string `json:"opt_nn,omitempty"`
	// optional+nullable: conservative omit
	OptN *string `json:"opt_n,omitempty"`
	// required+non-nullable: value type, always emit
	ReqNN string `json:"req_nn"`
	// required+nullable: pointer, NO omitempty -> nil emits null
	ReqN *string `json:"req_n"`
}

// validate-then-delegate via type alias: struct tags (incl. omitempty) apply,
// no infinite recursion because alias has no MarshalJSON.
func (q Quad) MarshalJSON() ([]byte, error) {
	// (Validate() would run here)
	type alias Quad
	return json.Marshal(alias(q))
}

func ptr(s string) *string { return &s }

func probeOmitEmpty() {
	empty := ""

	fmt.Println("== 1. omitempty on *string: nil omits, ptr-to-\"\" emits ==")
	a, _ := json.Marshal(struct {
		P *string `json:"p,omitempty"`
	}{P: nil})
	fmt.Println("  nil       ->", string(a))
	b, _ := json.Marshal(struct {
		P *string `json:"p,omitempty"`
	}{P: &empty})
	fmt.Println("  ptr-to-\"\" ->", string(b))

	fmt.Println("\n== 2. NO omitempty on *string: nil emits null ==")
	c, _ := json.Marshal(struct {
		P *string `json:"p"`
	}{P: nil})
	fmt.Println("  nil ->", string(c))

	fmt.Println("\n== 3. full quadrant struct via type-alias MarshalJSON ==")
	q := Quad{ReqNN: "x"} // OptNN, OptN, ReqN all nil
	out, _ := json.Marshal(q)
	fmt.Println("  all-optional-unset, req_n nil ->", string(out))

	q2 := Quad{OptNN: ptr("a"), OptN: ptr("b"), ReqNN: "x", ReqN: ptr("c")}
	out2, _ := json.Marshal(q2)
	fmt.Println("  all-set                       ->", string(out2))
}
