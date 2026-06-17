package main

type User struct {
	Nickname        *string `json:"nickname,omitempty"`
	NicknameOrDefault string `json:"nicknameOrDefault"` // a DECLARED field whose name equals the accessor
}

// generated accessor for the default-bearing Nickname field
func (u User) NicknameOrDefault() string {
	if u.Nickname != nil {
		return *u.Nickname
	}
	return "anon"
}

func main() {}
