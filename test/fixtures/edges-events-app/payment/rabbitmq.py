import pika


class Publisher:
    EXCHANGE = 'robot-shop'
    ROUTING_KEY = 'orders'

    def publish(self, body):
        self._channel.exchange_declare(exchange=self.EXCHANGE, exchange_type='direct', durable=True)
        self._channel.basic_publish(exchange=self.EXCHANGE,
                                    routing_key=self.ROUTING_KEY,
                                    body=body)

    def notify(self, body):
        self._channel.basic_publish(exchange='', routing_key='email', body=body)
