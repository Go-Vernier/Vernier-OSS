<?php
$url = getenv('CATALOGUE_URL') ?: 'http://catalogue:8080';
$pdo = new PDO(getenv('PDO_URL'));
