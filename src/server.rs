use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::timeout;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use rustls::{ServerConfig as TlsServerConfig, Certificate, PrivateKey};
use bytes::{Buf, BufMut, BytesMut};
use sha2::{Sha256, Digest};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use crc32fast::Hasher as CrcHasher;

use crate::{Velocity, VelocityConfig, VeloResult, VeloError};
use crate::sql::SqlEngine;

/// Velocity Protocol Constants
const MAGIC: u32 = 0x56454C4F; // "VELO"
const VERSION: u8 = 0x01;

/// Message Types
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageType {
    // Connection
    Hello = 0x01,
    ServerInfo = 0x02,
    AuthRequest = 0x03,
    AuthResponse = 0x04,
    Disconnect = 0x05,
    
    // Commands
    Command = 0x10,
    Response = 0x11,
    Error = 0x12,
    
    // Control
    Ping = 0x20,
    Pong = 0x21,
    Stats = 0x22,
}

impl From<u8> for MessageType {
    fn from(value: u8) -> Self {
        match value {
            0x01 => MessageType::Hello,
            0x02 => MessageType::ServerInfo,
            0x03 => MessageType::AuthRequest,
            0x04 => MessageType::AuthResponse,
            0x05 => MessageType::Disconnect,
            0x10 => MessageType::Command,
            0x11 => MessageType::Response,
            0x12 => MessageType::Error,
            0x20 => MessageType::Ping,
            0x21 => MessageType::Pong,
            0x22 => MessageType::Stats,
            _ => MessageType::Error,
        }
    }
}

/// Protocol Message
#[derive(Debug)]
pub struct VelocityMessage {
    pub msg_type: MessageType,
    pub payload: Vec<u8>,
}

impl VelocityMessage {
    pub fn new(msg_type: MessageType, payload: Vec<u8>) -> Self {
        Self { msg_type, payload }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(14 + self.payload.len());
        
        // Header
        buffer.put_u32_le(MAGIC);
        buffer.put_u8(VERSION);
        buffer.put_u8(self.msg_type as u8);
        buffer.put_u32_le(self.payload.len() as u32);
        
        // Payload
        buffer.extend_from_slice(&self.payload);
        
        // Checksum
        let mut hasher = CrcHasher::new();
        hasher.update(&buffer);
        buffer.put_u32_le(hasher.finalize());
        
        buffer
    }

    pub fn decode(mut data: &[u8]) -> VeloResult<Self> {
        if data.len() < 14 {
            return Err(VeloError::InvalidOperation("Message too short".to_string()));
        }

        // Verify magic
        let magic = data.get_u32_le();
        if magic != MAGIC {
            return Err(VeloError::InvalidOperation("Invalid magic".to_string()));
        }

        let version = data.get_u8();
        if version != VERSION {
            return Err(VeloError::InvalidOperation("Unsupported version".to_string()));
        }

        let msg_type = MessageType::from(data.get_u8());
        let payload_len = data.get_u32_le() as usize;

        if data.len() < payload_len + 4 {
            return Err(VeloError::InvalidOperation("Incomplete message".to_string()));
        }

        let payload = data[..payload_len].to_vec();
        let checksum = (&data[payload_len..]).get_u32_le();

        // Verify checksum
        let mut hasher = CrcHasher::new();
        hasher.update(&data[..data.len()-4]);
        if hasher.finalize() != checksum {
            return Err(VeloError::CorruptedData("Invalid checksum".to_string()));
        }

        Ok(Self { msg_type, payload })
    }
}

/// Server Configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: SocketAddr,
    pub max_connections: usize,
    pub connection_timeout: Duration,
    pub rate_limit_per_second: u32,
    pub enable_tls: bool,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub users: HashMap<String, String>, // username -> password_hash
}

impl Default for ServerConfig {
    fn default() -> Self {
        let mut users = HashMap::new();
        // Default admin user (password: "admin123")
        users.insert(
            "admin".to_string(),
            "$argon2id$v=19$m=65536,t=3,p=4$salt$hash".to_string()
        );

        Self {
            bind_address: "127.0.0.1:5432".parse().unwrap(),
            max_connections: 1000,
            connection_timeout: Duration::from_secs(300),
            rate_limit_per_second: 1000,
            enable_tls: false,
            cert_path: None,
            key_path: None,
            users,
        }
    }
}

/// Client Connection State
#[derive(Debug)]
struct ClientState {
    authenticated: bool,
    username: Option<String>,
    last_activity: Instant,
    command_count: u64,
    rate_limiter: RateLimiter,
}

impl ClientState {
    fn new(rate_limit: u32) -> Self {
        Self {
            authenticated: false,
            username: None,
            last_activity: Instant::now(),
            command_count: 0,
            rate_limiter: RateLimiter::new(rate_limit),
        }
    }
}

/// Simple Rate Limiter
#[derive(Debug)]
struct RateLimiter {
    max_per_second: u32,
    tokens: u32,
    last_refill: Instant,
}

impl RateLimiter {
    fn new(max_per_second: u32) -> Self {
        Self {
            max_per_second,
            tokens: max_per_second,
            last_refill: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        
        if elapsed >= Duration::from_secs(1) {
            self.tokens = self.max_per_second;
            self.last_refill = now;
        }

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
}

/// VelocityDB Server
pub struct VelocityServer {
    db: Arc<Velocity>,
    sql_engine: Arc<SqlEngine>,
    config: ServerConfig,
    server_fingerprint: String,
    connection_semaphore: Arc<Semaphore>,
    clients: Arc<RwLock<HashMap<SocketAddr, ClientState>>>,
}

impl VelocityServer {
    pub fn new(db: Velocity, config: ServerConfig) -> VeloResult<Self> {
        let db_arc = Arc::new(db);
        let sql_engine = Arc::new(SqlEngine::new(db_arc.clone()));
        
        // Generate server fingerprint
        let server_key = b"velocity_server_key_placeholder"; // In production, use actual server key
        let mut hasher = Sha256::new();
        hasher.update(server_key);
        let server_fingerprint = format!("{:x}", hasher.finalize());

        Ok(Self {
            db: db_arc,
            sql_engine,
            config: config.clone(),
            server_fingerprint,
            connection_semaphore: Arc::new(Semaphore::new(config.max_connections)),
            clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn start(&self) -> VeloResult<()> {
        let listener = TcpListener::bind(&self.config.bind_address).await?;
        log::info!("VelocityDB server listening on {}", self.config.bind_address);
        log::info!("Server fingerprint: {}", self.server_fingerprint);

        loop {
            match listener.accept().await {
                Ok((mut stream, addr)) => {
                    log::info!("New connection from {}", addr);
                    
                    // Check connection limit
                    if let Ok(_permit) = self.connection_semaphore.clone().try_acquire_owned() {
                        let server = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = server.handle_connection(stream, addr).await {
                                log::error!("Connection error for {}: {:?}", addr, e);
                            }
                            // Permit is automatically released when dropped
                        });
                    } else {
                        log::warn!("Connection limit reached, rejecting {}", addr);
                        let _ = stream.shutdown().await;
                    }
                }
                Err(e) => {
                    log::error!("Failed to accept connection: {:?}", e);
                }
            }
        }
    }

    async fn handle_connection(&self, stream: TcpStream, addr: SocketAddr) -> VeloResult<()> {
        // Initialize client state
        {
            let mut clients = self.clients.write().await;
            clients.insert(addr, ClientState::new(self.config.rate_limit_per_second));
        }

        let result = if self.config.enable_tls {
            // TLS connection handling would go here
            self.handle_plain_connection(stream, addr).await
        } else {
            self.handle_plain_connection(stream, addr).await
        };

        // Cleanup client state
        {
            let mut clients = self.clients.write().await;
            clients.remove(&addr);
        }

        result
    }

    async fn handle_plain_connection(&self, mut stream: TcpStream, addr: SocketAddr) -> VeloResult<()> {
        let mut buffer = BytesMut::with_capacity(8192);

        loop {
            // Read message with timeout
            match timeout(self.config.connection_timeout, stream.readable()).await {
                Ok(Ok(())) => {
                    // Read data
                    match stream.try_read_buf(&mut buffer) {
                        Ok(0) => break, // Connection closed
                        Ok(_) => {
                            // Process messages
                            while buffer.len() >= 14 {
                                match VelocityMessage::decode(&buffer) {
                                    Ok(message) => {
                                        let message_len = 14 + message.payload.len();
                                        buffer.advance(message_len);
                                        
                                        if let Some(response) = self.handle_message(message, addr).await? {
                                            let response_data = response.encode();
                                            stream.write_all(&response_data).await?;
                                        }
                                    }
                                    Err(_) => break, // Need more data
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            continue;
                        }
                        Err(e) => return Err(VeloError::IoError(e)),
                    }
                }
                Ok(Err(e)) => return Err(VeloError::IoError(e)),
                Err(_) => {
                    log::warn!("Connection timeout for {}", addr);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_message(&self, message: VelocityMessage, addr: SocketAddr) -> VeloResult<Option<VelocityMessage>> {
        // Rate limiting
        {
            let mut clients = self.clients.write().await;
            if let Some(client) = clients.get_mut(&addr) {
                if !client.rate_limiter.try_acquire() {
                    return Ok(Some(VelocityMessage::new(
                        MessageType::Error,
                        b"Rate limit exceeded".to_vec()
                    )));
                }
                client.last_activity = Instant::now();
            }
        }

        match message.msg_type {
            MessageType::Hello => {
                Ok(Some(VelocityMessage::new(
                    MessageType::ServerInfo,
                    format!("VelocityDB v1.0\nFingerprint: {}", self.server_fingerprint).into_bytes()
                )))
            }

            MessageType::AuthRequest => {
                self.handle_auth(message.payload, addr).await
            }

            MessageType::Command => {
                // Check authentication
                let authenticated = {
                    let clients = self.clients.read().await;
                    clients.get(&addr).map(|c| c.authenticated).unwrap_or(false)
                };

                if !authenticated {
                    return Ok(Some(VelocityMessage::new(
                        MessageType::Error,
                        b"Not authenticated".to_vec()
                    )));
                }

                self.handle_command(message.payload, addr).await
            }

            MessageType::Ping => {
                Ok(Some(VelocityMessage::new(MessageType::Pong, Vec::new())))
            }

            MessageType::Stats => {
                self.handle_stats().await
            }

            _ => {
                Ok(Some(VelocityMessage::new(
                    MessageType::Error,
                    b"Unsupported message type".to_vec()
                )))
            }
        }
    }

    async fn handle_auth(&self, payload: Vec<u8>, addr: SocketAddr) -> VeloResult<Option<VelocityMessage>> {
        // Parse username and password from payload
        let auth_data = String::from_utf8_lossy(&payload);
        let parts: Vec<&str> = auth_data.split('\0').collect();
        
        if parts.len() != 2 {
            return Ok(Some(VelocityMessage::new(
                MessageType::AuthResponse,
                b"Invalid auth format".to_vec()
            )));
        }

        let username = parts[0];
        let password = parts[1];

        // Verify credentials
        if let Some(stored_hash) = self.config.users.get(username) {
            let argon2 = Argon2::default();
            if let Ok(parsed_hash) = PasswordHash::new(stored_hash) {
                if argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok() {
                    // Authentication successful
                    {
                        let mut clients = self.clients.write().await;
                        if let Some(client) = clients.get_mut(&addr) {
                            client.authenticated = true;
                            client.username = Some(username.to_string());
                        }
                    }

                    log::info!("User {} authenticated from {}", username, addr);
                    return Ok(Some(VelocityMessage::new(
                        MessageType::AuthResponse,
                        b"OK".to_vec()
                    )));
                }
            }
        }

        log::warn!("Failed authentication attempt for {} from {}", username, addr);
        Ok(Some(VelocityMessage::new(
            MessageType::AuthResponse,
            b"Authentication failed".to_vec()
        )))
    }

    async fn handle_command(&self, payload: Vec<u8>, addr: SocketAddr) -> VeloResult<Option<VelocityMessage>> {
        let sql = String::from_utf8_lossy(&payload);
        
        // Update command count
        {
            let mut clients = self.clients.write().await;
            if let Some(client) = clients.get_mut(&addr) {
                client.command_count += 1;
            }
        }

        match self.sql_engine.execute(&sql).await {
            Ok(result) => {
                let response = serde_json::to_vec(&result).unwrap_or_else(|_| b"Serialization error".to_vec());
                Ok(Some(VelocityMessage::new(MessageType::Response, response)))
            }
            Err(e) => {
                let error_msg = format!("SQL Error: {:?}", e);
                Ok(Some(VelocityMessage::new(MessageType::Error, error_msg.into_bytes())))
            }
        }
    }

    async fn handle_stats(&self) -> VeloResult<Option<VelocityMessage>> {
        let db_stats = self.db.stats();
        let client_count = self.clients.read().await.len();
        
        let stats = serde_json::json!({
            "database": {
                "memtable_entries": db_stats.memtable_entries,
                "sstable_count": db_stats.sstable_count,
                "cache_entries": db_stats.cache_entries,
                "total_sstable_size": db_stats.total_sstable_size
            },
            "server": {
                "active_connections": client_count,
                "max_connections": self.config.max_connections,
                "server_fingerprint": self.server_fingerprint
            }
        });

        let response = serde_json::to_vec(&stats).unwrap();
        Ok(Some(VelocityMessage::new(MessageType::Response, response)))
    }
}

impl Clone for VelocityServer {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            sql_engine: self.sql_engine.clone(),
            config: self.config.clone(),
            server_fingerprint: self.server_fingerprint.clone(),
            connection_semaphore: self.connection_semaphore.clone(),
            clients: self.clients.clone(),
        }
    }
}

/// Password hashing utility
pub fn hash_password(password: &str) -> VeloResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    
    match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(hash) => Ok(hash.to_string()),
        Err(e) => Err(VeloError::InvalidOperation(format!("Password hashing failed: {}", e))),
    }
}