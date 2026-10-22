fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&opentrack_api::openapi()).expect("OpenAPI serializes")
    );
}
