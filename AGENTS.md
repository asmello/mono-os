# Agent guidance for mono-os

## Rust style

### `.expect()` messages describe invariants, not errors

`.expect("msg")` should encode the invariant that makes unwrapping safe — the
reason the caller believes this `Option`/`Result` cannot be `None`/`Err` at this
point. It is not a place to describe what would have gone wrong.

If you can't state the invariant, that's a signal to handle the error instead
of unwrapping.

Good (describes why unwrapping is safe):

```rust
boot_info.physical_memory_offset.into_option().expect("enabled in config")
env::var("HOME").expect("HOME is always set on unix")
mutex.lock().expect("not poisoned")
```

Bad (describes the failure mode):

```rust
something.parse::<u32>().expect("failed to parse number")
Command::new("foo").status().expect("failed to spawn foo")
```
