package main

import (
	"os"

	pb "github.com/acme/demo/genproto"
)

func main() {
	cartAddr := os.Getenv("CART_SERVICE_ADDR")
	cart := pb.NewCartServiceClient(dial(cartAddr))
	shipping := pb.NewShippingServiceClient(dial(os.Getenv("SHIPPING_SERVICE_ADDR")))
	_ = cart
	_ = shipping
}
