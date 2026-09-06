package main

import "os"

func main() {
	mongo := os.Getenv("MONGO_URL")
	if mongo == "" {
		mongo = "mongodb://mongodb:27017/catalogue"
	}
}
