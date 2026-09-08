const { Kafka } = require('kafkajs');
await producer.send({ topic: 'order-created', messages: [{ value: 'hi' }] });
await producer.send({ topic: 'audit-log', messages: [] });
res.send('ok');
