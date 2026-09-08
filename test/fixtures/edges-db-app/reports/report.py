import os
conn = os.getenv("DB_CONNECTION_STRING")
legacy = os.getenv("LEGACY_DB_URL", "mysql://mysql:3306/shop")
