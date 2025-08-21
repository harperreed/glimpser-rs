use gl_core::telemetry;

fn main() {
    telemetry::init_tracing("development", "glimpser");
    tracing::info!("glimpser starting");
    println!("Hello, world!");
}
