package main

import (
	"encoding/json"
	"fmt"
)

type Quad struct {
	OptNN *string `json:"opt_nn,omitempty"`
	OptN  *string `json:"opt_n,omitempty"`
	ReqNN string  `json:"req_nn"`
	ReqN  *string `json:"req_n"`
}

func (q Quad) MarshalJSON() ([]byte, error) {
	type alias Quad
	return json.Marshal(alias(q))
}

func ptr(s string) *string { return &s }

func main() {
	empty := ""
	fmt.Println("== 1. omitempty *string: nil omits, ptr-to-\"\" emits ==")
	a, _ := json.Marshal(struct{ P *string `json:"p,omitempty"` }{P: nil})
	fmt.Println("  nil       ->", string(a))
	b, _ := json.Marshal(struct{ P *string `json:"p,omitempty"` }{P: &empty})
	fmt.Println("  ptr-to-\"\" ->", string(b))

	fmt.Println("\n== 2. NO omitempty *string: nil emits null ==")
	c, _ := json.Marshal(struct{ P *string `json:"p"` }{P: nil})
	fmt.Println("  nil ->", string(c))

	fmt.Println("\n== 3. quadrant struct via type-alias MarshalJSON ==")
	q := Quad{ReqNN: "x"}
	out, _ := json.Marshal(q)
	fmt.Println("  opts unset, req_n nil ->", string(out))
	q2 := Quad{OptNN: ptr("a"), OptN: ptr("b"), ReqNN: "x", ReqN: ptr("c")}
	out2, _ := json.Marshal(q2)
	fmt.Println("  all set               ->", string(out2))
}
