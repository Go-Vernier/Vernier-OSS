package main

import "github.com/streadway/amqp"

func main() {
	ch.ExchangeDeclare("robot-shop", "direct", true, false, false, false, nil)
	ch.QueueDeclare("orders", true, false, false, false, nil)
	ch.QueueBind("orders", "orders", "robot-shop", false, nil)
	msgs, _ := ch.Consume("orders", "", true, false, false, false, nil)
	_ = msgs
}
