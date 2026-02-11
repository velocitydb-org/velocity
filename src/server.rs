use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use bytes::{Buf, BufMut, BytesMut};
use crc32fast::Hasher as CrcHasher;
use rustls::{Certificate, PrivateKey, ServerConfig as TlsServerConfig};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::timeout;
use tokio_rustls::{server::TlsStream, TlsAcceptor};

use crate::sql::SqlEngine;
use crate::{VeloError, VeloResult, Velocity, VelocityConfig};


const MAGIC: u32 = 0x56454C4F;
const VERSION: u8 = 0x01;


#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageType {

    Hello = 0x01,
    ServerInfo = 0x02,
    AuthRequest = 0x03,
    AuthResponse = 0x04,
    Disconnect = 0x05,


    Command = 0x10,
    Response = 0x11,
    Error = 0x12,


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


        buffer.put_u32_le(MAGIC);
        buffer.put_u8(VERSION);
        buffer.put_u8(self.msg_type as u8);
        buffer.put_u32_le(self.payload.len() as u32);


        buffer.extend_from_slice(&self.payload);


        let mut hasher = CrcHasher::new();
        hasher.update(&buffer);
        buffer.put_u32_le(hasher.finalize());

        buffer
    }

    pub fn decode(data: &[u8]) -> VeloResult<Self> {
        if data.len() < 14 {
            return Err(VeloError::InvalidOperation("Message too short".to_string()));
        }


        let original_data = data;


        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != MAGIC {
            return Err(VeloError::InvalidOperation(format!(
                "Invalid magic: {:08x}",
                magic
            )));
        }

        let version = data[4];
        if version != VERSION {
            return Err(VeloError::InvalidOperation(format!(
                "Unsupported version: {}",
                version
            )));
        }

        let msg_type = MessageType::from(data[5]);
        let payload_len = u32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;

        if data.len() < 10 + payload_len + 4 {
            return Err(VeloError::InvalidOperation(
                "Incomplete message".to_string(),
            ));
        }

        let payload = data[10..10 + payload_len].to_vec();
        let checksum = u32::from_le_bytes([
            data[10 + payload_len],
            data[10 + payload_len + 1],
            data[10 + payload_len + 2],
            data[10 + payload_len + 3],
        ]);


        let mut hasher = CrcHasher::new();
        hasher.update(&original_data[..10 + payload_len]);
        let calculated_checksum = hasher.finalize();

        if calculated_checksum != checksum {
            return Err(VeloError::CorruptedData(format!(
                "Invalid checksum: expected {:08x}, got {:08x}",
                calculated_checksum, checksum
            )));
        }

        Ok(Self { msg_type, payload })
    }
}


#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: SocketAddr,
    pub max_connections: usize,
    pub connection_timeout: Duration,
    pub rate_limit_per_second: u32,
    pub enable_tls: bool,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub users: HashMap<String, String>,
    pub audit_log_path: String,
    pub audit_logging: bool,
    pub auth_ban_duration: Duration,
    pub max_auth_failures: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let mut users = HashMap::new();
        users.insert(
            "admin".to_string(),
            "$argon2id$v=19$m=19456,t=2,p=1$GDWQpkPCnz9uM5W2SBpCmw$RNLHaiBA1s5wdbQSKJ28JzwD30wohA5KoB+W8MZOxic".to_string(),
        );

        Self {
            bind_address: "127.0.0.1:2005".parse().unwrap(),
            max_connections: 1000,
            connection_timeout: Duration::from_secs(300),
            rate_limit_per_second: 1000,
            enable_tls: false,
            cert_path: None,
            key_path: None,
            users,
            audit_log_path: "./velocitydb_audit.log".to_string(),
            audit_logging: true,
            auth_ban_duration: Duration::from_secs(300),
            max_auth_failures: 5,
        }
    }
}

#[derive(Debug)]
struct ClientState {
    authenticated: bool,
    username: Option<String>,
    last_activity: Instant,
    command_count: u64,
    rate_limiter: RateLimiter,
    current_db: String,
}

impl ClientState {
    fn new(rate_limit: u32) -> Self {
        Self {
            authenticated: false,
            username: None,
            last_activity: Instant::now(),
            command_count: 0,
            rate_limiter: RateLimiter::new(rate_limit),
            current_db: "default".to_string(),
        }
    }
}


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

use crate::addon::DatabaseManager;


pub struct VelocityServer {
    db_manager: Arc<DatabaseManager>,

    config: ServerConfig,
    server_fingerprint: String,
    connection_semaphore: Arc<Semaphore>,
    clients: Arc<RwLock<HashMap<SocketAddr, ClientState>>>,
}

impl VelocityServer {
    pub fn new(db_manager: Arc<DatabaseManager>, config: ServerConfig) -> VeloResult<Self> {

        let server_key = b"velocity_server_key_placeholder";
        let mut hasher = Sha256::new();
        hasher.update(server_key);
        let server_fingerprint = format!("{:x}", hasher.finalize());

        Ok(Self {
            db_manager,
            config: config.clone(),
            server_fingerprint,
            connection_semaphore: Arc::new(Semaphore::new(config.max_connections)),
            clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn start(&self) -> VeloResult<()> {
        let listener = TcpListener::bind(&self.config.bind_address).await?;
        log::info!(
            "VelocityDB server listening on {}",
            self.config.bind_address
        );
        log::info!("Server fingerprint: {}", self.server_fingerprint);

        loop {
            match listener.accept().await {
                Ok((mut stream, addr)) => {
                    log::info!("New connection from {}", addr);


                    if let Ok(_permit) = self.connection_semaphore.clone().try_acquire_owned() {
                        let server = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = server.handle_connection(stream, addr).await {
                                log::error!("Connection error for {}: {:?}", addr, e);
                            }

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

        {
            let mut clients = self.clients.write().await;
            clients.insert(addr, ClientState::new(self.config.rate_limit_per_second));
        }

        let result = if self.config.enable_tls {

            self.handle_plain_connection(stream, addr).await
        } else {
            self.handle_plain_connection(stream, addr).await
        };


        {
            let mut clients = self.clients.write().await;
            clients.remove(&addr);
        }

        result
    }

    async fn handle_plain_connection(
        &self,
        mut stream: TcpStream,
        addr: SocketAddr,
    ) -> VeloResult<()> {
        let mut buffer = BytesMut::with_capacity(8192);

        loop {

            match timeout(self.config.connection_timeout, stream.readable()).await {
                Ok(Ok(())) => {

                    match stream.try_read_buf(&mut buffer) {
                        Ok(0) => break,
                        Ok(_) => {

                            while buffer.len() >= 14 {
                                match VelocityMessage::decode(&buffer) {
                                    Ok(message) => {
                                        let message_len = 14 + message.payload.len();
                                        buffer.advance(message_len);

                                        match self.handle_message(message, addr).await {
                                            Ok(Some(response)) => {
                                                let response_data = response.encode();
                                                if let Err(e) =
                                                    stream.write_all(&response_data).await
                                                {
                                                    log::error!(
                                                        "Failed to send response to {}: {:?}",
                                                        addr,
                                                        e
                                                    );
                                                    return Err(VeloError::IoError(e));
                                                }
                                            }
                                            Ok(None) => {

                                            }
                                            Err(e) => {
                                                log::error!(
                                                    "Error handling message from {}: {:?}",
                                                    addr,
                                                    e
                                                );

                                                let error_msg = format!("{:?}", e);
                                                let error_response = VelocityMessage::new(
                                                    MessageType::Error,
                                                    error_msg.into_bytes(),
                                                );
                                                let _ = stream
                                                    .write_all(&error_response.encode())
                                                    .await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to decode message from {}: {:?}",
                                            addr,
                                            e
                                        );
                                        log::error!(
                                            "Buffer length: {}, hex: {}",
                                            buffer.len(),
                                            buffer
                                                .iter()
                                                .take(32)
                                                .map(|b| format!("{:02x}", b))
                                                .collect::<String>()
                                        );
                                        break;
                                    }
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

    async fn handle_message(
        &self,
        message: VelocityMessage,
        addr: SocketAddr,
    ) -> VeloResult<Option<VelocityMessage>> {

        {
            let mut clients = self.clients.write().await;
            if let Some(client) = clients.get_mut(&addr) {
                if !client.rate_limiter.try_acquire() {
                    return Ok(Some(VelocityMessage::new(
                        MessageType::Error,
                        b"Rate limit exceeded".to_vec(),
                    )));
                }
                client.last_activity = Instant::now();
            }
        }

        match message.msg_type {
            MessageType::Hello => Ok(Some(VelocityMessage::new(
                MessageType::ServerInfo,
                format!("VelocityDB v1.0\nFingerprint: {}", self.server_fingerprint).into_bytes(),
            ))),

            MessageType::AuthRequest => self.handle_auth(message.payload, addr).await,

            MessageType::Command => {

                let (authenticated, current_db) = {
                    let clients = self.clients.read().await;
                    if let Some(c) = clients.get(&addr) {
                        (c.authenticated, c.current_db.clone())
                    } else {
                        (false, "default".to_string())
                    }
                };

                if !authenticated {
                    return Ok(Some(VelocityMessage::new(
                        MessageType::Error,
                        b"Not authenticated".to_vec(),
                    )));
                }

                self.handle_command(message.payload, addr, &current_db)
                    .await
            }

            MessageType::Ping => Ok(Some(VelocityMessage::new(MessageType::Pong, Vec::new()))),

            MessageType::Stats => self.handle_stats().await,

            _ => Ok(Some(VelocityMessage::new(
                MessageType::Error,
                b"Unsupported message type".to_vec(),
            ))),
        }
    }

    async fn handle_auth(
        &self,
        payload: Vec<u8>,
        addr: SocketAddr,
    ) -> VeloResult<Option<VelocityMessage>> {

        let auth_data = String::from_utf8_lossy(&payload);
        let parts: Vec<&str> = auth_data.split('\0').collect();

        if parts.len() != 2 {
            return Ok(Some(VelocityMessage::new(
                MessageType::AuthResponse,
                b"Invalid auth format".to_vec(),
            )));
        }

        let username = parts[0];
        let password = parts[1];


        if username == "apikey" && password.starts_with("vdb_") {
            if let Some(default_db) = self.db_manager.get_database("default") {
                let auth_key = format!("auth:keys:{}", password);
                if let Ok(Some(db_name_bytes)) = default_db.get(&auth_key) {
                    let db_name = String::from_utf8_lossy(&db_name_bytes).to_string();

                    {
                        let mut clients = self.clients.write().await;
                        if let Some(client) = clients.get_mut(&addr) {
                            client.authenticated = true;
                            client.username = Some(username.to_string());
                            client.current_db = db_name.clone();
                        }
                    }
                    log::info!(
                        "Dynamic API Key validated. Scoped to database '{}' from {}",
                        db_name,
                        addr
                    );
                    return Ok(Some(VelocityMessage::new(
                        MessageType::AuthResponse,
                        b"OK".to_vec(),
                    )));
                }
            }
        }


        if let Some(stored_hash) = self.config.users.get(username) {
            let argon2 = Argon2::default();
            if let Ok(parsed_hash) = PasswordHash::new(stored_hash) {
                if argon2
                    .verify_password(password.as_bytes(), &parsed_hash)
                    .is_ok()
                {

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
                        b"OK".to_vec(),
                    )));
                }
            }
        }

        log::warn!(
            "Failed authentication attempt for {} from {}",
            username,
            addr
        );
        Ok(Some(VelocityMessage::new(
            MessageType::AuthResponse,
            b"Authentication failed".to_vec(),
        )))
    }

    async fn handle_command(
        &self,
        payload: Vec<u8>,
        addr: SocketAddr,
        current_db: &str,
    ) -> VeloResult<Option<VelocityMessage>> {
        let sql = String::from_utf8_lossy(&payload);


        {
            let mut clients = self.clients.write().await;
            if let Some(client) = clients.get_mut(&addr) {
                client.command_count += 1;
            }
        }


        let sql_upper = sql.trim().to_uppercase();
        if sql_upper.starts_with("CREATE DATABASE") {
            let parts: Vec<&str> = sql.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                let db_name = parts[2];
                match self.db_manager.create_database(db_name, None) {
                    Ok(_) => {
                        let msg = format!("Database '{}' created successfully", db_name);
                        return Ok(Some(VelocityMessage::new(
                            MessageType::Response,
                            msg.into_bytes(),
                        )));
                    }
                    Err(e) => {
                        let msg = format!("Failed to create database: {}", e);
                        return Ok(Some(VelocityMessage::new(
                            MessageType::Error,
                            msg.into_bytes(),
                        )));
                    }
                }
            }
        } else if sql_upper.starts_with("DROP DATABASE") {
            let parts: Vec<&str> = sql.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                let db_name = parts[2];
                match self.db_manager.drop_database(db_name) {
                    Ok(_) => {
                        let msg = format!("Database '{}' dropped successfully", db_name);
                        return Ok(Some(VelocityMessage::new(
                            MessageType::Response,
                            msg.into_bytes(),
                        )));
                    }
                    Err(e) => {
                        let msg = format!("Failed to drop database: {}", e);
                        return Ok(Some(VelocityMessage::new(
                            MessageType::Error,
                            msg.into_bytes(),
                        )));
                    }
                }
            }
        } else if sql_upper == "SHOW DATABASES" {
            let list = self.db_manager.list_databases();
            let response = serde_json::to_vec(&list).unwrap();
            return Ok(Some(VelocityMessage::new(MessageType::Response, response)));
        } else if sql_upper == "SHOW DATABASE DEFAULT MAX DISK SIZE" {
            let response = serde_json::to_vec(&serde_json::json!({
                "default_max_disk_size_bytes": self.db_manager.get_default_database_max_disk_size_bytes()
            }))
            .unwrap();
            return Ok(Some(VelocityMessage::new(MessageType::Response, response)));
        } else if sql_upper.starts_with("SET DATABASE DEFAULT MAX DISK SIZE") {
            let parts: Vec<&str> = sql.trim().split_whitespace().collect();
            if parts.len() >= 7 {
                let raw_value = parts[6].trim_end_matches(';');
                let normalized = raw_value.to_uppercase();
                let parsed = if normalized == "UNLIMITED"
                    || normalized == "NONE"
                    || normalized == "NULL"
                {
                    None
                } else {
                    Some(raw_value.parse::<u64>().map_err(|_| {
                        VeloError::InvalidOperation(
                            "Disk limit must be a positive integer (bytes) or UNLIMITED"
                                .to_string(),
                        )
                    })?)
                };

                self.db_manager
                    .set_default_database_max_disk_size_bytes(parsed)?;

                let msg = if let Some(limit) = parsed {
                    format!(
                        "Default database disk limit set to {} bytes for new databases",
                        limit
                    )
                } else {
                    "Default database disk limit removed (unlimited)".to_string()
                };
                return Ok(Some(VelocityMessage::new(
                    MessageType::Response,
                    msg.into_bytes(),
                )));
            }
        } else if sql_upper.starts_with("DATABASE STATS") {
            let parts: Vec<&str> = sql.trim().split_whitespace().collect();
            let db_name = if parts.len() >= 3 {
                parts[2]
            } else {
                current_db
            };

            if let Some(db) = self.db_manager.get_database(db_name) {
                let s = db.stats();
                let stats = serde_json::json!({
                    "name": db_name,
                    "memtable_entries": s.memtable_entries,
                    "sstable_count": s.sstable_count,
                    "cache_entries": s.cache_entries,
                    "total_sstable_size": s.total_sstable_size,
                    "record_count": s.total_records,
                    "size_bytes": s.total_size_bytes
                });
                let response = serde_json::to_vec(&stats).unwrap();
                return Ok(Some(VelocityMessage::new(MessageType::Response, response)));
            } else {
                return Ok(Some(VelocityMessage::new(
                    MessageType::Error,
                    format!("Database '{}' not found", db_name).into_bytes(),
                )));
            }
        } else if sql_upper.starts_with("USE") {
            let parts: Vec<&str> = sql.trim().split_whitespace().collect();
            if parts.len() >= 2 {
                let db_name = parts[1];
                if self.db_manager.get_database(db_name).is_some() {

                    let mut clients = self.clients.write().await;
                    if let Some(client) = clients.get_mut(&addr) {
                        client.current_db = db_name.to_string();
                    }
                    let msg = format!("Switched to database '{}'", db_name);
                    return Ok(Some(VelocityMessage::new(
                        MessageType::Response,
                        msg.into_bytes(),
                    )));
                } else {
                    return Ok(Some(VelocityMessage::new(
                        MessageType::Error,
                        format!("Database '{}' not found", db_name).into_bytes(),
                    )));
                }
            }
        }


        if let Some(db) = self.db_manager.get_database(current_db) {
            if Self::is_write_sql(&sql) {
                if let Err(e) = self.db_manager.can_accept_write(current_db) {
                    return Ok(Some(VelocityMessage::new(
                        MessageType::Error,
                        format!("SQL Error: {}", e).into_bytes(),
                    )));
                }
            }
            let engine = SqlEngine::new(db);
            match engine.execute(&sql).await {
                Ok(result) => {
                    let response = serde_json::to_vec(&result)
                        .unwrap_or_else(|_| b"Serialization error".to_vec());
                    Ok(Some(VelocityMessage::new(MessageType::Response, response)))
                }
                Err(e) => {
                    let error_msg = format!("SQL Error: {:?}", e);
                    Ok(Some(VelocityMessage::new(
                        MessageType::Error,
                        error_msg.into_bytes(),
                    )))
                }
            }
        } else {
            Ok(Some(VelocityMessage::new(
                MessageType::Error,
                b"Current database not found".to_vec(),
            )))
        }
    }

    fn is_write_sql(sql: &str) -> bool {
        let upper = sql.trim_start().to_uppercase();
        upper.starts_with("INSERT") || upper.starts_with("UPDATE") || upper.starts_with("DELETE")
    }

    async fn handle_stats(&self) -> VeloResult<Option<VelocityMessage>> {
        let db_stats = self.db_manager.stats();
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
            db_manager: self.db_manager.clone(),
            config: self.config.clone(),
            server_fingerprint: self.server_fingerprint.clone(),
            connection_semaphore: self.connection_semaphore.clone(),
            clients: self.clients.clone(),
        }
    }
}


pub fn hash_password(password: &str) -> VeloResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(hash) => Ok(hash.to_string()),
        Err(e) => Err(VeloError::InvalidOperation(format!(
            "Password hashing failed: {}",
            e
        ))),
    }
}
