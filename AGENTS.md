# Agent guidance for mono-os

## Commit hygiene

### Run `cargo fmt` and `cargo clippy` before every commit

Before staging anything for a commit, run `cargo fmt` and `cargo clippy`
across the workspace and fix what they flag. This keeps `git blame` clean
(formatting churn doesn't get mixed into substantive commits) and keeps
review focused on intent rather than lint noise.

If clippy raises something that's a deliberate exception, suppress it at
the narrowest scope possible with `#[allow(...)]` and leave a one-line
comment explaining why — don't broaden the suppression to a whole module
or crate.

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
