const redis = require('redis');
const catalogueHost = process.env.CATALOGUE_HOST || 'catalogue';
const redisHost = process.env.REDIS_HOST || 'redis';
fetch(`http://${catalogueHost}:8080/product/${sku}`);
