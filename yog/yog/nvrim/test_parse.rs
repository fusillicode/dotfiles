use chrono::DateTime;

fn main() {
    let input = "25-12-2023,14:30:45Z";
    let format = "%d-%m-%Y,%H:%M:%S%Z";
    match DateTime::parse_from_str(input, format) {
        Ok(dt) => println!("Parsed: {}", dt),
        Err(e) => println!("Error: {}", e),
    }
}
