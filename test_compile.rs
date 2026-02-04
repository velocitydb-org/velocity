// Simple compilation test
use velocity::{Velocity, VelocityConfig};

fn main() {
    println!("VelocityDB compilation test");
    
    let config = VelocityConfig::default();
    match Velocity::open_with_config("./test_compile_db", config) {
        Ok(db) => {
            println!("Database opened successfully");
            let _ = db.close();
        }
        Err(e) => {
            println!("Error opening database: {}", e);
        }
    }
}