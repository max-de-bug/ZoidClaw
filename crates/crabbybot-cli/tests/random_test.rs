fn main() {
    let _signer = polymarket_client_sdk::auth::LocalSigner::random();
    println!("Random signer generated");
}
