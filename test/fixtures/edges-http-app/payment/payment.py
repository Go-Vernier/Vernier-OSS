import os, requests
USER = os.getenv('USER_HOST', 'user')
CART = os.getenv('CART_HOST', 'cart')
AMQP = os.getenv('AMQP_HOST', 'rabbitmq')
requests.get('http://' + USER + ':8080/check/' + id)
requests.get('http://{}:8080/cart/{}'.format(CART, id))
requests.post('https://paypal.com/pay')
