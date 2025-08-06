# JDB - PostgreSQL-like Database in Rust

An open source PostgreSQL-compatible relational database implemented in Rust, designed for performance, safety, and modern development practices.

## Architecture

JDB is organized as a Cargo workspace with the following components:

### Core Libraries

- **`core/`** - Database core functionality
  - Fundamental data types and structures
  - Common traits and interfaces
  - Error handling and result types
  - Configuration management
  - Shared utilities and helpers

- **`storage/`** - Storage engine
  - B+ tree implementations for indexes
  - Page management and buffer pool
  - Write-Ahead Logging (WAL) system
  - Transaction management and MVCC
  - Disk I/O operations and file management

- **`sql/`** - SQL parsing and execution
  - SQL parser and lexer
  - Abstract Syntax Tree (AST) definitions
  - Query planner and optimizer
  - Execution engine and operators
  - Catalog management (tables, schemas, etc.)

### Binaries

- **`server/`** - Database server
  - Network protocol implementation (PostgreSQL wire protocol)
  - Connection handling and session management
  - Authentication and authorization
  - Query processing coordination
  - Background processes (checkpointer, vacuum, etc.)

- **`cli/`** - Command line interface
  - Interactive SQL shell (psql-like)
  - Database administration tools
  - Import/export utilities
  - Backup and restore functionality

- **`bench/`** - Benchmarking and testing
  - Performance benchmarks
  - Load testing tools
  - Comparison with other databases
  - Stress testing utilities

## Development

```bash
# Build all components
cargo build

# Run tests
cargo test

# Start the database server
cargo run --bin jdb-server

# Launch the CLI client
cargo run --bin jdb-cli
```
