// Probe: Rust STANDARD LIBRARY typed reps for the 6 formats.
// std only (NO chrono/uuid/time crates). Run: rustc typed.rs -o /tmp/typed_rs && /tmp/typed_rs
// Backs features/format typed-repr research. Rust is the generator's own gate engine,
// but this documents what std can and cannot construct.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

fn main() {
    println!("=== Rust std typed representations ===");

    // ---- date-time / date / time ----
    // std has NO calendar/civil date-time type. std::time::SystemTime / Instant are
    // opaque monotonic/wall clocks with NO string parsing and NO field access.
    // There is no std parser for RFC 3339. A crate (chrono / time) is REQUIRED.
    println!("\n[date-time] std has NO civil date-time type and NO RFC3339 parser.");
    println!("            std::time::SystemTime is opaque (no fields, no parse). -> needs chrono/time CRATE.");
    println!("[date]      NO std date type. -> needs a CRATE.");
    println!("[time]      NO std time-of-day type. -> needs a CRATE.");

    // ---- uuid ----
    // std has NO uuid type or parser. -> needs the `uuid` CRATE.
    println!("\n[uuid] std has NO Uuid type and NO parser. -> needs the `uuid` CRATE.");

    // ---- ipv4 / ipv6 : std::net::IpAddr / Ipv4Addr / Ipv6Addr (FromStr) ----
    println!("\n[ipv4] type=std::net::Ipv4Addr  ctor=Ipv4Addr::from_str(s) (also IpAddr::from_str)");
    for s in ["192.168.0.1", "256.0.0.1", "01.2.3.4", "1.2.3", "1.2.3.4.5"] {
        match Ipv4Addr::from_str(s) {
            Ok(a) => println!("  Ipv4  {:42} -> OK  to_string={}", format!("{:?}", s), a),
            Err(e) => println!("  Ipv4  {:42} -> ERR {}", format!("{:?}", s), e),
        }
    }

    println!("\n[ipv6] type=std::net::Ipv6Addr  ctor=Ipv6Addr::from_str(s)");
    for s in ["::1", "2001:db8::1", "2001:DB8::1", "::ffff:192.168.0.1",
              "fe80::1%eth0", "2001:0db8:0000:0000:0000:0000:0000:0001"] {
        match Ipv6Addr::from_str(s) {
            Ok(a) => println!("  Ipv6  {:42} -> OK  to_string={}", format!("{:?}", s), a),
            Err(e) => println!("  Ipv6  {:42} -> ERR {}", format!("{:?}", s), e),
        }
    }

    println!("\n[IpAddr::from_str] (family-agnostic)");
    for s in ["192.168.0.1", "01.2.3.4", "::1", "2001:DB8::1"] {
        match IpAddr::from_str(s) {
            Ok(a) => println!("  IpAddr {:20} -> OK is_ipv4={} to_string={}", format!("{:?}", s), a.is_ipv4(), a),
            Err(e) => println!("  IpAddr {:20} -> ERR {}", format!("{:?}", s), e),
        }
    }
}
