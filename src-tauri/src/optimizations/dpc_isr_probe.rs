//! Manual, elevated-only probe for whether a LatencyMon-style "which driver
//! is causing DPC/ISR stalls" feature is viable. Not wired into the app -
//! this only exists to answer, on a real machine, the two open questions
//! from research: (1) can we actually start the NT Kernel Logger with
//! DPC/ISR flags and get real events via `ferrisetw`, and (2) can we parse
//! useful fields (routine address, duration) out of them, since classic
//! MOF-based kernel events are known-fragile even in Microsoft's own tools
//! (tracerpt's `-report` DPC/ISR breakdown produced zero decoded events on
//! this machine - see the session notes). Run with:
//!   cargo test --lib dpc_isr_probe -- --ignored --nocapture
//! from an elevated shell (NT Kernel Logger needs SeSystemProfilePrivilege).

#[cfg(test)]
mod tests {
    use ferrisetw::parser::Parser;
    use ferrisetw::provider::{kernel_providers, Provider};
    use ferrisetw::schema_locator::SchemaLocator;
    use ferrisetw::trace::KernelTrace;
    use ferrisetw::EventRecord;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Loaded kernel drivers, sorted by base address ascending. No exact
    /// size per driver (EnumDeviceDrivers doesn't give one) - attribution
    /// below uses "nearest preceding base address", the same heuristic
    /// reverse-engineering/diagnostic tools use when only base addresses
    /// are available, not exact module sizes.
    fn enumerate_drivers() -> Vec<(u64, String)> {
        use windows::Win32::System::ProcessStatus::{EnumDeviceDrivers, GetDeviceDriverBaseNameW};

        let mut bases: Vec<*mut c_void> = vec![std::ptr::null_mut(); 2048];
        let mut needed: u32 = 0;
        let buffer_bytes = (bases.len() * std::mem::size_of::<*mut c_void>()) as u32;
        unsafe {
            let _ = EnumDeviceDrivers(bases.as_mut_ptr(), buffer_bytes, &mut needed);
        }
        let count = ((needed as usize) / std::mem::size_of::<*mut c_void>()).min(bases.len());

        let mut result = Vec::new();
        for base in &bases[..count] {
            let mut name_buf = [0u16; 260];
            let len = unsafe { GetDeviceDriverBaseNameW(*base, &mut name_buf) };
            if len > 0 {
                let name = String::from_utf16_lossy(&name_buf[..len as usize]);
                result.push((*base as u64, name));
            }
        }
        result.sort_by_key(|(base, _)| *base);
        result
    }

    /// Nearest driver whose base address is <= `routine`, i.e. the driver
    /// most likely to own that address given only base addresses (no size).
    fn attribute_to_driver(drivers: &[(u64, String)], routine: u64) -> Option<&str> {
        drivers
            .iter()
            .filter(|(base, _)| *base <= routine)
            .max_by_key(|(base, _)| *base)
            .map(|(_, name)| name.as_str())
    }

    #[test]
    #[ignore]
    fn live_driver_enumeration_on_this_machine() {
        let drivers = enumerate_drivers();
        println!("Found {} loaded drivers", drivers.len());
        for (base, name) in drivers.iter().take(10) {
            println!("  {base:#018x}  {name}");
        }
        assert!(!drivers.is_empty(), "expected at least one loaded driver");
    }

    #[test]
    #[ignore]
    fn live_dpc_isr_capture_on_this_machine() {
        let dpc_seen = Arc::new(AtomicUsize::new(0));
        let isr_seen = Arc::new(AtomicUsize::new(0));
        let dpc_seen_cb = dpc_seen.clone();
        let isr_seen_cb = isr_seen.clone();

        let dump_event = |label: &str, record: &EventRecord, schema_locator: &SchemaLocator| {
            match schema_locator.event_schema(record) {
                Ok(schema) => {
                    let parser = Parser::create(record, &schema);
                    println!(
                        "[{label}] provider={} opcode_name={} opcode={} timestamp={}",
                        schema.provider_name(),
                        schema.opcode_name(),
                        record.opcode(),
                        record.raw_timestamp(),
                    );
                    // Field names are a guess based on the classic
                    // PerfInfo MOF layout (Microsoft's own "Example 15"
                    // doc names InitialTime/Routine) - try a handful and
                    // print whatever actually resolves, since this is
                    // exactly the unknown this probe exists to answer.
                    for field in ["Routine", "InitialTime", "Vector", "ReturnValue", "USecs"] {
                        match parser.try_parse::<u64>(field) {
                            Ok(value) => println!("    {field} (u64) = {value}"),
                            Err(_) => match parser.try_parse::<u32>(field) {
                                Ok(value) => println!("    {field} (u32) = {value}"),
                                Err(_) => {}
                            },
                        }
                    }
                }
                Err(error) => println!("[{label}] schema lookup failed: {error:?}"),
            }
        };

        let dpc_callback = move |record: &EventRecord, schema_locator: &SchemaLocator| {
            dpc_seen_cb.fetch_add(1, Ordering::Relaxed);
            if dpc_seen_cb.load(Ordering::Relaxed) <= 5 {
                dump_event("DPC", record, schema_locator);
            }
        };
        let isr_callback = move |record: &EventRecord, schema_locator: &SchemaLocator| {
            isr_seen_cb.fetch_add(1, Ordering::Relaxed);
            if isr_seen_cb.load(Ordering::Relaxed) <= 5 {
                dump_event("ISR", record, schema_locator);
            }
        };

        let dpc_provider = Provider::kernel(&kernel_providers::DPC_PROVIDER)
            .add_callback(dpc_callback)
            .build();
        let isr_provider = Provider::kernel(&kernel_providers::INTERRUPT_PROVIDER)
            .add_callback(isr_callback)
            .build();

        let trace = KernelTrace::new()
            .named(String::from("AnalystBlazeDpcIsrProbe"))
            .enable(dpc_provider)
            .enable(isr_provider)
            .start_and_process()
            .expect("starting the NT Kernel Logger requires an elevated shell");

        println!("Capturing DPC/ISR events for 15s...");
        std::thread::sleep(Duration::new(15, 0));
        trace.stop().ok();

        println!(
            "Total DPC events: {}, total ISR events: {}",
            dpc_seen.load(Ordering::Relaxed),
            isr_seen.load(Ordering::Relaxed)
        );
        assert!(
            dpc_seen.load(Ordering::Relaxed) > 0,
            "expected at least one DPC event in 15s - none arrived, capture mechanism itself may be broken"
        );
    }

    /// The real end-to-end question: capture live DPC/ISR events, resolve
    /// each one's `Routine` address to the driver that owns it, and rank
    /// drivers by event count - i.e. "who is actually causing this?", the
    /// one thing tracerpt's report couldn't answer (see the module doc).
    /// Both DPC_PROVIDER and INTERRUPT_PROVIDER callbacks saw the exact
    /// same merged event stream in the earlier probe (same opcode/values on
    /// both), so this uses one callback and splits by `opcode_name()`
    /// instead of trusting provider-based separation.
    #[test]
    #[ignore]
    fn live_dpc_isr_driver_attribution_on_this_machine() {
        let drivers = enumerate_drivers();
        println!("Loaded {} drivers for attribution", drivers.len());

        let routines: Arc<Mutex<Vec<(String, u64)>>> = Arc::new(Mutex::new(Vec::new()));
        let routines_cb = routines.clone();
        let total = Arc::new(AtomicUsize::new(0));
        let total_cb = total.clone();

        let callback = move |record: &EventRecord, schema_locator: &SchemaLocator| {
            let Ok(schema) = schema_locator.event_schema(record) else {
                return;
            };
            let opcode_name = schema.opcode_name();
            // Everything observed so far is either "DPC" or starts with
            // "ISR" (e.g. "ISR-MSI") - anything else (image load, process,
            // etc, if this session ever picks any up) is ignored here.
            let kind = if opcode_name == "DPC" {
                "DPC"
            } else if opcode_name.starts_with("ISR") {
                "ISR"
            } else {
                return;
            };
            total_cb.fetch_add(1, Ordering::Relaxed);
            let parser = Parser::create(record, &schema);
            if let Ok(routine) = parser.try_parse::<u64>("Routine") {
                routines_cb.lock().unwrap().push((kind.to_string(), routine));
            }
        };

        let dpc_provider = Provider::kernel(&kernel_providers::DPC_PROVIDER)
            .add_callback(callback.clone())
            .build();
        let isr_provider = Provider::kernel(&kernel_providers::INTERRUPT_PROVIDER)
            .add_callback(callback)
            .build();

        let trace = KernelTrace::new()
            .named(String::from("AnalystBlazeDpcIsrAttrib"))
            .enable(dpc_provider)
            .enable(isr_provider)
            .start_and_process()
            .expect("starting the NT Kernel Logger requires an elevated shell");

        println!("Capturing for 15s - move the mouse / browse a bit for realistic driver activity...");
        std::thread::sleep(Duration::new(15, 0));
        trace.stop().ok();

        let captured = routines.lock().unwrap();
        println!("Captured {} DPC/ISR events with a resolved Routine (of {} total)", captured.len(), total.load(Ordering::Relaxed));

        use std::collections::HashMap;
        let mut per_driver: HashMap<String, (usize, usize)> = HashMap::new(); // name -> (dpc_count, isr_count)
        let mut unattributed = 0usize;
        for (kind, routine) in captured.iter() {
            match attribute_to_driver(&drivers, *routine) {
                Some(name) => {
                    let entry = per_driver.entry(name.to_string()).or_insert((0, 0));
                    if kind == "DPC" {
                        entry.0 += 1;
                    } else {
                        entry.1 += 1;
                    }
                }
                None => unattributed += 1,
            }
        }

        let mut ranked: Vec<_> = per_driver.into_iter().collect();
        ranked.sort_by_key(|(_, (dpc, isr))| std::cmp::Reverse(dpc + isr));

        println!("Top drivers by DPC+ISR event count:");
        for (name, (dpc, isr)) in ranked.iter().take(15) {
            println!("  {name:<28} DPC={dpc:<6} ISR={isr:<6} total={}", dpc + isr);
        }
        println!("Unattributed (no driver base <= routine found): {unattributed}");

        assert!(!captured.is_empty(), "no DPC/ISR event had a parseable Routine field");
        assert!(!ranked.is_empty(), "captured events but none resolved to a known driver");
    }
}
