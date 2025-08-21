use std::sync::Once;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

static INIT: Once = Once::new();

/// Initialize tracing - safe to call multiple times
pub fn init_tracing(env: &str, service: &str) {
    INIT.call_once(|| {
        let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

        if env == "production" {
            tracing_subscriber::registry()
                .with(tracing_subscriber::fmt::layer().json())
                .with(env_filter)
                .init();
        } else {
            tracing_subscriber::registry()
                .with(tracing_subscriber::fmt::layer().pretty())
                .with(env_filter)
                .init();
        }

        tracing::info!(service = %service, "Tracing initialized");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_idempotent() {
        // Should not panic when called multiple times
        init_tracing("test", "glimpser-test");
        init_tracing("test", "glimpser-test");
    }
}
