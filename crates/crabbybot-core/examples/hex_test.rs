use alloy::primitives::U256;
use std::str::FromStr;

fn main() {
    let hex_id = "0xa6d8e1b575e4a2c8610d036d5b3a969358a3334fb60ea6e8a0e22c1aba891b22";
    let u = U256::from_str(hex_id).unwrap();
    println!("Hex: {}", hex_id);
    println!("Decimal: {}", u.to_string());
}
