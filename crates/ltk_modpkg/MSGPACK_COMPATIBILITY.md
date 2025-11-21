# MessagePack Metadata Cross-Language Compatibility Guide

## Overview

The `ModpkgMetadata` structure is encoded using MessagePack and stored as a special chunk at `_meta_/metadata.msgpack` within the modpkg file. This provides excellent cross-language compatibility and makes it easy to add new metadata fields in future versions.

## How Metadata is Stored

- **Location**: Metadata is stored as a regular chunk at path `_meta_/metadata.msgpack`
- **Layer**: No layer (NO_LAYER_INDEX)
- **Compression**: No compression
- **Format**: MessagePack with named fields (maps) and internally tagged enums

## Current Encoding Format

### Metadata Structure (Rust)
```rust
pub struct ModpkgMetadata {
    pub schema_version: u32, // Default: 1
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub version: String,
    pub distributor: Option<DistributorInfo>,
    pub authors: Vec<ModpkgAuthor>,
    pub license: ModpkgLicense,
    pub layers: Vec<ModpkgLayerMetadata>,
}

pub struct DistributorInfo {
    pub site_id: String,
    pub site_name: String,
    pub site_url: String,
    pub mod_id: String,
}

pub struct ModpkgAuthor {
    pub name: String,
    pub role: Option<String>,
}

pub struct ModpkgLayerMetadata {
    pub name: String,
    pub priority: i32,
    pub description: Option<String>,
}

pub enum ModpkgLicense {
    None,
    Spdx { spdx_id: String },
    Custom { name: String, url: String },
}
```

### MessagePack Encoding Details

**Structs** are encoded as **MessagePack maps** (named fields):
- `ModpkgMetadata` → Map with keys: `{"schema_version": ..., "name": ..., "display_name": ..., "description": ..., "version": ..., "distributor": ..., "authors": ..., "license": ..., "layers": ...}`
- `DistributorInfo` → Map with keys: `{"site_id": ..., "site_name": ..., "site_url": ..., "mod_id": ...}`
- `ModpkgAuthor` → Map with keys: `{"name": ..., "role": ...}`
- Field names use `snake_case`

**Enums** use **internally tagged** format:
- `None` → `{"type": "none"}`
- `Spdx { spdx_id: "MIT" }` → `{"type": "spdx", "spdx_id": "MIT"}`
- `Custom { name: "X", url: "Y" }` → `{"type": "custom", "name": "X", "url": "Y"}`

**Option<T>** encodes as:
- `None` → MessagePack `nil`
- `Some(value)` → The value directly

This format is much more cross-language friendly than positional arrays!

## C# Implementation Example

### Using MessagePack-CSharp

```csharp
using MessagePack;
using System.Collections.Generic;

[MessagePackObject]
public class ModpkgMetadata
{
    [Key(0)]
    public uint SchemaVersion { get; set; } = 1;

    [Key(1)]
    public string Name { get; set; }
    
    [Key(2)]
    public string DisplayName { get; set; }
    
    [Key(3)]
    public string? Description { get; set; }
    
    [Key(4)]
    public string Version { get; set; }
    
    [Key(5)]
    public DistributorInfo? Distributor { get; set; }
}

[MessagePackObject]
public class DistributorInfo
{
    [Key(0)]
    public string SiteId { get; set; }
    
    [Key(1)]
    public string SiteName { get; set; }
    
    [Key(2)]
    public string SiteUrl { get; set; }
    
    [Key(3)]
    public string ModId { get; set; }
}

[MessagePackObject]
public class ModpkgAuthor
{
    [Key(0)]
    public string Name { get; set; }
    
    [Key(1)]
    public string? Role { get; set; }
}

// For enums, you need custom handling or use a union type
[Union(0, typeof(LicenseNone))]
[Union(1, typeof(LicenseSpdx))]
[Union(2, typeof(LicenseCustom))]
public interface ModpkgLicense { }

[MessagePackObject]
public class LicenseNone : ModpkgLicense
{
    // Matches "None" string encoding
}

[MessagePackObject]
public class LicenseSpdx : ModpkgLicense
{
    [Key(0)]
    public string SpdxId { get; set; }
}

[MessagePackObject]
public class LicenseCustom : ModpkgLicense
{
    [Key(0)]
    public string Name { get; set; }
    
    [Key(1)]
    public string Url { get; set; }
}

// Usage:
using (var stream = File.OpenRead("metadata.msgpack"))
{
    var metadata = MessagePackSerializer.Deserialize<ModpkgMetadata>(stream);
    Console.WriteLine($"Mod Name: {metadata.Name}");
}
```

## Python Implementation Example

```python
import msgpack
from typing import Optional, List
from dataclasses import dataclass

@dataclass
class ModpkgAuthor:
    name: str
    role: Optional[str]
    
    @staticmethod
    def from_msgpack(data):
        return ModpkgAuthor(name=data[0], role=data[1])

@dataclass
class ModpkgLicense:
    pass

@dataclass
class LicenseNone(ModpkgLicense):
    pass

@dataclass
class LicenseSpdx(ModpkgLicense):
    spdx_id: str

@dataclass
class LicenseCustom(ModpkgLicense):
    name: str
    url: str

@dataclass
class DistributorInfo:
    site_id: str
    site_name: str
    site_url: str
    mod_id: str

@dataclass
class ModpkgMetadata:
    schema_version: int
    name: str
    display_name: str
    description: Optional[str]
    version: str
    distributor: Optional[DistributorInfo]
    authors: List[ModpkgAuthor]
    license: ModpkgLicense
    
    @staticmethod
    def from_msgpack(data):
        # Note: Real implementation would access by name since we use named fields
        # This simplified example assumes positional for brevity but production
        # code should use named access.
        
        return ModpkgMetadata(
            schema_version=data.get("schema_version", 1),
            name=data["name"],
            # ... etc
        )

# Usage:
with open('metadata.msgpack', 'rb') as f:
    data = msgpack.unpackb(f.read(), raw=False)
    metadata = ModpkgMetadata.from_msgpack(data)
    print(f"Mod Name: {metadata.name}")
```
