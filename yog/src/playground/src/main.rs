#![feature(exit_status_error)]

/// A safe space where to try stuff :)
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hi :)");
    Ok(())
}
