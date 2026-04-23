package main

import (
	"fmt"
	"log"

	"github.com/Endgame-Labs/kesha/tiktoken"
)

func main() {
	text := "hello from another Go app"

	count, err := tiktoken.Count("gpt-4o", text)
	if err != nil {
		log.Fatal(err)
	}

	tokens, err := tiktoken.Encode("gpt-4o", text)
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("%q is %d tokens: %v\n", text, count, tokens)
}
