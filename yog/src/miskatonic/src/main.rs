#![feature(exit_status_error)]

/// A safe experimental space for trying out code and ideas.
///
/// This is a minimal test crate that serves as a playground for experimenting
/// with new concepts, testing code snippets, or running temporary code without
/// affecting the main codebase.
///
/// # Purpose
///
/// - Quick prototyping and experimentation
/// - Testing small code snippets
/// - Learning and exploration
/// - Temporary code execution
///
/// # Examples
///
/// Running the experimental code:
/// ```bash
/// miskatonic
/// ```
///
/// The tool currently just prints a friendly greeting and exits successfully.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hi :)");
    Ok(())
}
