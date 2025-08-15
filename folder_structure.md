# Project Folder Structure

```
jdb/
├── Cargo.lock
├── Cargo.toml
├── LICENSE
├── README.md
├── folder_structure.md
└── storage/
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        └── page/
            └── mod.rs
```

## Directory Descriptions

- **/** - Root directory of the jdb project
  - `Cargo.toml` - Main workspace configuration
  - `Cargo.lock` - Dependency lock file
  - `LICENSE` - Project license file
  - `README.md` - Project documentation
  - `folder_structure.md` - This file

- **/storage/** - Storage engine module
  - `Cargo.toml` - Storage crate configuration
  
- **/storage/src/** - Storage source code
  - `lib.rs` - Library entry point
  
- **/storage/src/page/** - Page management module
  - `mod.rs` - Page module implementation