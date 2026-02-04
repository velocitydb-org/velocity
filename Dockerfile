# VelocityDB Docker Image
FROM rust:1.70 as builder

# Set working directory
WORKDIR /app

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY examples ./examples

# Build the application
RUN cargo build --release

# Runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create velocitydb user
RUN useradd -r -s /bin/false velocitydb

# Create data directory
RUN mkdir -p /var/lib/velocitydb && \
    chown velocitydb:velocitydb /var/lib/velocitydb

# Copy binary from builder
COPY --from=builder /app/target/release/velocity /usr/local/bin/velocitydb

# Copy default configuration
COPY velocity.toml /etc/velocitydb/velocity.toml

# Set permissions
RUN chown velocitydb:velocitydb /usr/local/bin/velocitydb && \
    chmod +x /usr/local/bin/velocitydb

# Switch to velocitydb user
USER velocitydb

# Expose port
EXPOSE 5432

# Set data directory
VOLUME ["/var/lib/velocitydb"]

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD /usr/local/bin/velocitydb ping || exit 1

# Default command
CMD ["/usr/local/bin/velocitydb", "server", \
     "--config", "/etc/velocitydb/velocity.toml", \
     "--data-dir", "/var/lib/velocitydb", \
     "--bind", "0.0.0.0:5432"]