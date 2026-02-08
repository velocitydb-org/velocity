use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use bytes::{Buf, BufMut, BytesMut};
use sha2::{Sha256, Digest};
use serde_json;

use crate::server::{VelocityMessage, MessageType};
use crate::sql::{QueryResult, SqlValue};
use crate::{VeloResult, VeloError};

/// VelocityDB Client
pub struct VelocityClient {
    stream: TcpStream,
    server_fingerprint: Option<String>,
    authenticated: bool,
    #[allow(dead_code)]
    cached_fingerprints: HashMap<SocketAddr, String>,
}

impl VelocityClient {
    /// Connect to VelocityDB server
    pub async fn connect(address: &str) -> VeloResult<Self> {
        let stream = TcpStream::connect(address).await?;
        let _server_addr = stream.peer_addr()?;
        
        let mut client = Self {
            stream,
            server_fingerprint: None,
            authenticated: false,
            cached_fingerprints: HashMap::new(),
        };

        // Perform handshake
        client.handshake().await?;
        
        Ok(client)
    }

    /// Connect with cached fingerprint verification
    pub async fn connect_with_fingerprint(address: &str, expected_fingerprint: &str) -> VeloResult<Self> {
        let client = Self::connect(address).await?;
        
        if let Some(ref fingerprint) = client.server_fingerprint {
            if fingerprint != expected_fingerprint {
                return Err(VeloError::InvalidOperation(
                    "Server fingerprint mismatch - possible MITM attack".to_string()
                ));
            }
        }
        
        Ok(client)
    }

    /// Authenticate with username and password
    pub async fn authenticate(&mut self, username: &str, password: &str) -> VeloResult<()> {
        let auth_payload = format!("{}\0{}", username, password);
        let message = VelocityMessage::new(MessageType::AuthRequest, auth_payload.into_bytes());
        
        self.send_message(&message).await?;
        
        let response = self.receive_message().await?;
        match response.msg_type {
            MessageType::AuthResponse => {
                let response_text = String::from_utf8_lossy(&response.payload);
                if response_text == "OK" {
                    self.authenticated = true;
                    Ok(())
                } else {
                    Err(VeloError::InvalidOperation(format!("Authentication failed: {}", response_text)))
                }
            }
            MessageType::Error => {
                let error_text = String::from_utf8_lossy(&response.payload);
                Err(VeloError::InvalidOperation(format!("Auth error: {}", error_text)))
            }
            _ => Err(VeloError::InvalidOperation("Unexpected response to auth request".to_string()))
        }
    }

    /// Execute SQL query
    pub async fn query(&mut self, sql: &str) -> VeloResult<QueryResult> {
        if !self.authenticated {
            return Err(VeloError::InvalidOperation("Not authenticated".to_string()));
        }

        let message = VelocityMessage::new(MessageType::Command, sql.as_bytes().to_vec());
        self.send_message(&message).await?;
        
        let response = self.receive_message().await?;
        match response.msg_type {
            MessageType::Response => {
                let result: QueryResult = serde_json::from_slice(&response.payload)
                    .map_err(|e| VeloError::CorruptedData(format!("Failed to parse response: {}", e)))?;
                Ok(result)
            }
            MessageType::Error => {
                let error_text = String::from_utf8_lossy(&response.payload);
                Err(VeloError::InvalidOperation(error_text.to_string()))
            }
            _ => Err(VeloError::InvalidOperation("Unexpected response to query".to_string()))
        }
    }

    /// Convenience method for SELECT queries
    pub async fn select(&mut self, key: &str) -> VeloResult<Option<String>> {
        let sql = format!("SELECT value FROM kv WHERE key = '{}'", key);
        let result = self.query(&sql).await?;
        
        if result.data.is_empty() {
            Ok(None)
        } else {
            match &result.data[0].values[1] {
                SqlValue::String(s) => Ok(Some(s.clone())),
                SqlValue::Binary(b) => Ok(Some(String::from_utf8_lossy(b).to_string())),
                _ => Ok(None),
            }
        }
    }

    /// Convenience method for INSERT
    pub async fn insert(&mut self, key: &str, value: &str) -> VeloResult<()> {
        let sql = format!("INSERT INTO kv (key, value) VALUES ('{}', '{}')", key, value);
        let result = self.query(&sql).await?;
        
        if result.success {
            Ok(())
        } else {
            Err(VeloError::InvalidOperation("Insert failed".to_string()))
        }
    }

    /// Convenience method for UPDATE
    pub async fn update(&mut self, key: &str, value: &str) -> VeloResult<bool> {
        let sql = format!("UPDATE kv SET value = '{}' WHERE key = '{}'", value, key);
        let result = self.query(&sql).await?;
        
        Ok(result.rows_affected > 0)
    }

    /// Convenience method for DELETE
    pub async fn delete(&mut self, key: &str) -> VeloResult<bool> {
        let sql = format!("DELETE FROM kv WHERE key = '{}'", key);
        let result = self.query(&sql).await?;
        
        Ok(result.rows_affected > 0)
    }

    /// Get server statistics
    pub async fn stats(&mut self) -> VeloResult<serde_json::Value> {
        let message = VelocityMessage::new(MessageType::Stats, Vec::new());
        self.send_message(&message).await?;
        
        let response = self.receive_message().await?;
        match response.msg_type {
            MessageType::Response => {
                let stats: serde_json::Value = serde_json::from_slice(&response.payload)
                    .map_err(|e| VeloError::CorruptedData(format!("Failed to parse stats: {}", e)))?;
                Ok(stats)
            }
            MessageType::Error => {
                let error_text = String::from_utf8_lossy(&response.payload);
                Err(VeloError::InvalidOperation(error_text.to_string()))
            }
            _ => Err(VeloError::InvalidOperation("Unexpected response to stats request".to_string()))
        }
    }

    /// Ping server
    pub async fn ping(&mut self) -> VeloResult<Duration> {
        let start = std::time::Instant::now();
        
        let message = VelocityMessage::new(MessageType::Ping, Vec::new());
        self.send_message(&message).await?;
        
        let response = self.receive_message().await?;
        let duration = start.elapsed();
        
        match response.msg_type {
            MessageType::Pong => Ok(duration),
            _ => Err(VeloError::InvalidOperation("Unexpected response to ping".to_string()))
        }
    }

    /// Get server fingerprint
    pub fn server_fingerprint(&self) -> Option<&String> {
        self.server_fingerprint.as_ref()
    }

    /// Check if authenticated
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    // Private methods
    async fn handshake(&mut self) -> VeloResult<()> {
        // Send hello
        let hello = VelocityMessage::new(MessageType::Hello, Vec::new());
        self.send_message(&hello).await?;
        
        // Receive server info
        let response = self.receive_message().await?;
        match response.msg_type {
            MessageType::ServerInfo => {
                let server_info = String::from_utf8_lossy(&response.payload);
                
                // Extract fingerprint from server info
                for line in server_info.lines() {
                    if line.starts_with("Fingerprint: ") {
                        self.server_fingerprint = Some(line[13..].to_string());
                        break;
                    }
                }
                
                Ok(())
            }
            _ => Err(VeloError::InvalidOperation("Unexpected response to hello".to_string()))
        }
    }

    async fn send_message(&mut self, message: &VelocityMessage) -> VeloResult<()> {
        let data = message.encode();
        self.stream.write_all(&data).await?;
        Ok(())
    }

    async fn receive_message(&mut self) -> VeloResult<VelocityMessage> {
        let mut buffer = BytesMut::with_capacity(8192);
        
        // Read at least the header
        while buffer.len() < 14 {
            let n = self.stream.read_buf(&mut buffer).await?;
            if n == 0 {
                return Err(VeloError::InvalidOperation("Connection closed".to_string()));
            }
        }

        // Parse message length
        let payload_len = {
            let mut temp = &buffer[8..12];
            temp.get_u32_le() as usize
        };

        // Read remaining data
        let total_len = 14 + payload_len;
        while buffer.len() < total_len {
            let n = self.stream.read_buf(&mut buffer).await?;
            if n == 0 {
                return Err(VeloError::InvalidOperation("Connection closed".to_string()));
            }
        }

        // Decode message
        let message = VelocityMessage::decode(&buffer[..total_len])?;
        buffer.advance(total_len);
        
        Ok(message)
    }
}

/// Connection pool for multiple clients
pub struct VelocityPool {
    address: String,
    username: String,
    password: String,
    #[allow(dead_code)]
    max_connections: usize,
    available: Arc<tokio::sync::Mutex<Vec<VelocityClient>>>,
    semaphore: tokio::sync::Semaphore,
}

impl VelocityPool {
    pub fn new(address: String, username: String, password: String, max_connections: usize) -> Self {
        Self {
            address,
            username,
            password,
            max_connections,
            available: tokio::sync::Mutex::new(Vec::new()).into(),
            semaphore: tokio::sync::Semaphore::new(max_connections),
        }
    }

    pub async fn get_connection(&self) -> VeloResult<PooledConnection<'_>> {
        let _permit = self.semaphore.acquire().await.unwrap();
        
        let mut client = {
            let mut available = self.available.lock().await;
            if let Some(client) = available.pop() {
                client
            } else {
                let mut new_client = VelocityClient::connect(&self.address).await?;
                new_client.authenticate(&self.username, &self.password).await?;
                new_client
            }
        };

        // Verify connection is still alive
        if client.ping().await.is_err() {
            // Connection is dead, create a new one
            client = VelocityClient::connect(&self.address).await?;
            client.authenticate(&self.username, &self.password).await?;
        }

        Ok(PooledConnection {
            client: Some(client),
            pool: self,
            _permit,
        })
    }
}

pub struct PooledConnection<'a> {
    client: Option<VelocityClient>,
    pool: &'a VelocityPool,
    _permit: tokio::sync::SemaphorePermit<'a>,
}

impl<'a> PooledConnection<'a> {
    pub async fn query(&mut self, sql: &str) -> VeloResult<QueryResult> {
        self.client.as_mut().unwrap().query(sql).await
    }

    pub async fn select(&mut self, key: &str) -> VeloResult<Option<String>> {
        self.client.as_mut().unwrap().select(key).await
    }

    pub async fn insert(&mut self, key: &str, value: &str) -> VeloResult<()> {
        self.client.as_mut().unwrap().insert(key, value).await
    }

    pub async fn update(&mut self, key: &str, value: &str) -> VeloResult<bool> {
        self.client.as_mut().unwrap().update(key, value).await
    }

    pub async fn delete(&mut self, key: &str) -> VeloResult<bool> {
        self.client.as_mut().unwrap().delete(key).await
    }
}

impl<'a> Drop for PooledConnection<'a> {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            let pool_available = Arc::clone(&self.pool.available);
            tokio::spawn(async move {
                let mut available = pool_available.lock().await;
                available.push(client);
            });
        }
    }
}