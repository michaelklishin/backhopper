# backhopper-driver

This is a statically typed Rust driver for the `backhopper` CLI that
simplifies embedding or driving `backhopper` from an agent. It owns
the subprocess, parses the JSON envelope into deserialised payloads,
and exposes a type-state builder pattern that turns "required
argument missing" into a compile-time error.

```rust,no_run
use backhopper_driver::{Backhopper, ExecutedInvocation};
use backhopper_driver::types::SeriesName;
use std::str::FromStr;

fn drive() -> Result<(), Box<dyn std::error::Error>> {
    let driver = Backhopper::auto_discover()?;
    let diff = std::fs::read("V-29.patch")?;
    let series = SeriesName::from_str("rabbitmq-4.2")?;

    let (evaluation, executed): (_, ExecutedInvocation) = driver.check()
        .patch()
        .series(series)
        .patch_bytes(diff)
        .run_with_diagnostics()?;

    println!("ran {:?} in {:?}", executed.argv, executed.duration);
    println!("verdict: {:?}", evaluation.worst_verdict());
    Ok(())
}
```

See `docs/012_backhopper_driver_crate_design.md` for the full design.
