package main

import pb "github.com/acme/demo/genproto"

func main() {
	pb.RegisterShippingServiceServer(srv, &server{})
}
