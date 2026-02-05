# Error Handling

## Critical Rule

**ALL Tauri commands MUST return `IpcResult<T>`, NEVER plain `Result<T, E>`.**

Plain `Result` serializes as `null`, causing frontend crash: `Cannot read properties of null (reading 'ok')`

## Pattern

```rust
use crate::error::{AppResult, IpcResult};

// Command wrapper (always returns IpcResult)
#[tauri::command]
pub fn my_command(args: String) -> IpcResult<ReturnType> {
    my_command_inner(&args).into()
}

// Business logic (uses AppResult)
fn my_command_inner(args: &str) -> AppResult<ReturnType> {
    let value = some_operation()?;
    Ok(value)
}
```

## Types

```rust
// Internal errors
pub enum AppError {
    Io(std::io::Error),
    ModNotFound(String),
    ValidationFailed(String),
    // ...
}

// Internal result (business logic)
pub type AppResult<T> = Result<T, AppError>;

// IPC result (commands only)
pub enum IpcResult<T> {
    Ok { value: T },
    Err { error: AppErrorResponse },
}
```

Serializes to:

- Success: `{ "ok": true, "value": T }`
- Error: `{ "ok": false, "error": { code, message, context } }`

## Common Mistakes

### ❌ Wrong: Plain Result

```rust
#[tauri::command]
pub fn command() -> Result<(), String> {
    Ok(())  // Serializes as null → crash
}
```

### ✅ Correct: IpcResult

```rust
#[tauri::command]
pub fn command() -> IpcResult<()> {
    command_inner().into()
}

fn command_inner() -> AppResult<()> {
    Ok(())
}
```

### ❌ Wrong: Missing .into()

```rust
#[tauri::command]
pub fn command() -> IpcResult<String> {
    command_inner()  // Type mismatch
}
```

### ✅ Correct: Add .into()

```rust
#[tauri::command]
pub fn command() -> IpcResult<String> {
    command_inner().into()  // Converts AppResult → IpcResult
}
```

## Template

```rust
use crate::error::{AppError, AppResult, IpcResult};

#[tauri::command]
pub fn my_command(arg: String, state: State<SomeState>) -> IpcResult<ReturnType> {
    my_command_inner(&arg, &state).into()
}

fn my_command_inner(arg: &str, state: &State<SomeState>) -> AppResult<ReturnType> {
    // Validate
    if arg.trim().is_empty() {
        return Err(AppError::ValidationFailed("Empty argument".to_string()));
    }

    // Business logic
    let value = do_something(arg)?;
    Ok(value)
}
```

## Remember

1. Commands → `IpcResult<T>`
2. Internal logic → `AppResult<T>`
3. Convert with `.into()`
4. Use specific `AppError` variants
