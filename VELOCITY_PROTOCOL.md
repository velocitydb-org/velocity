# Velocity Protocol Specification v1.0

## Overview
The Velocity Protocol is a binary protocol for secure, high-performance database operations over TCP/TLS connections. It implements SSH-style fingerprint authentication with username/password credentials.

## Connection Flow

### 1. Initial Handshake
```
Client -> Server: VELOCITY_HELLO
Server -> Client: VELOCITY_SERVER_INFO + SERVER_FINGERPRINT
Client -> Server: VELOCITY_AUTH_REQUEST + USERNAME + PASSWORD
Server -> Client: VELOCITY_AUTH_RESPONSE (SUCCESS/FAILURE)
```

### 2. Command Execution
```
Client -> Server: VELOCITY_COMMAND + SQL_QUERY
Server -> Client: VELOCITY_RESPONSE + RESULT_DATA
```

## Message Format

All messages use little-endian byte order:

```
[MAGIC: 4 bytes] [VERSION: 1 byte] [TYPE: 1 byte] [LENGTH: 4 bytes] [PAYLOAD: LENGTH bytes] [CHECKSUM: 4 bytes]
```

- **MAGIC**: `0x56454C4F` ("VELO")
- **VERSION**: Protocol version (0x01)
- **TYPE**: Message type (see below)
- **LENGTH**: Payload length in bytes
- **PAYLOAD**: Message-specific data
- **CHECKSUM**: CRC32 of MAGIC + VERSION + TYPE + LENGTH + PAYLOAD

## Message Types

### Connection Messages
- `0x01` - VELOCITY_HELLO
- `0x02` - VELOCITY_SERVER_INFO
- `0x03` - VELOCITY_AUTH_REQUEST
- `0x04` - VELOCITY_AUTH_RESPONSE
- `0x05` - VELOCITY_DISCONNECT

### Command Messages
- `0x10` - VELOCITY_COMMAND
- `0x11` - VELOCITY_RESPONSE
- `0x12` - VELOCITY_ERROR

### Control Messages
- `0x20` - VELOCITY_PING
- `0x21` - VELOCITY_PONG
- `0x22` - VELOCITY_STATS

## Authentication

### Server Fingerprint
- SHA-256 hash of server's public key
- Clients cache fingerprints for trust-on-first-use
- Prevents MITM attacks

### Credentials
- Username: UTF-8 string (max 64 bytes)
- Password: UTF-8 string (max 128 bytes)
- Passwords are hashed with Argon2id before transmission

## SQL Command Support

### Supported Operations
```sql
-- Key-Value Operations
SELECT value FROM kv WHERE key = 'user:1';
INSERT INTO kv (key, value) VALUES ('user:1', 'alice');
UPDATE kv SET value = 'alice_updated' WHERE key = 'user:1';
DELETE FROM kv WHERE key = 'user:1';

-- Batch Operations
INSERT INTO kv (key, value) VALUES 
  ('user:1', 'alice'),
  ('user:2', 'bob'),
  ('user:3', 'charlie');

-- Range Queries
SELECT key, value FROM kv WHERE key LIKE 'user:%' LIMIT 100;
SELECT key, value FROM kv WHERE key >= 'user:1' AND key <= 'user:9';

-- Statistics
SELECT COUNT(*) FROM kv;
SHOW STATS;
SHOW STATUS;
```

## Security Features

### Encryption
- TLS 1.3 for transport encryption
- Optional AES-256-GCM for data-at-rest encryption
- Argon2id for password hashing

### Rate Limiting
- Per-connection command rate limiting
- Global server rate limiting
- Configurable limits per user

### Audit Logging
- All commands logged with timestamps
- User identification in logs
- No plaintext credentials in logs

## Error Codes

- `0x0000` - SUCCESS
- `0x0001` - INVALID_CREDENTIALS
- `0x0002` - RATE_LIMITED
- `0x0003` - INVALID_COMMAND
- `0x0004` - KEY_NOT_FOUND
- `0x0005` - STORAGE_ERROR
- `0x0006` - PROTOCOL_ERROR
- `0x0007` - SERVER_OVERLOADED

## Performance Targets

- **Latency**: < 1ms for cached reads
- **Throughput**: 100K+ ops/sec per connection
- **Concurrent Connections**: 10,000+
- **Memory Usage**: < 100MB base + 1KB per connection

## Client Libraries

Reference implementations will be provided for:
- Rust (official)
- Python
- JavaScript/Node.js
- Go
- Java