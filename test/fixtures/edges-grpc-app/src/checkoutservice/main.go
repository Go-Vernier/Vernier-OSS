package main

import (
	"os"

	pb "github.com/acme/demo/genproto"
)

func main() {
	email := pb.NewEmailServiceClient(dial(os.Getenv("EMAIL_SERVICE_ADDR")))
	_ = email
}
