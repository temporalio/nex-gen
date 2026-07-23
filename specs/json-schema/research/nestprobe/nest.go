package main
type UserEvent struct {
	type Kind = string   // attempt to nest a type decl inside a struct
	Kind Kind
}
func main() {}
