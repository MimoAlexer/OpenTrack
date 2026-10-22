use serde_json::{Value, json};
#[derive(Clone, Copy)]
pub(super) enum Kind {
    String,
    Boolean,
    Integer,
    Strings,
}
pub(super) struct Field {
    pub path: &'static str,
    pub kind: Kind,
    pub secret: bool,
    pub env_only: bool,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub description: &'static str,
}
macro_rules! f {
    ($path:literal,$kind:ident,$description:literal) => {
        Field {
            path: $path,
            kind: Kind::$kind,
            secret: false,
            env_only: false,
            min: None,
            max: None,
            description: $description,
        }
    };
    ($path:literal,$kind:ident,$description:literal,$min:literal,$max:literal) => {
        Field {
            min: Some($min),
            max: Some($max),
            ..f!($path, $kind, $description)
        }
    };
    ($path:literal,secret,$env:literal,$description:literal) => {
        Field {
            secret: true,
            env_only: $env,
            ..f!($path, String, $description)
        }
    };
}
pub(super) const FIELDS: &[Field] = &[
    f!(
        "storage.backend",
        String,
        "Only local immutable blob storage is implemented."
    ),
    f!(
        "storage.local_root",
        String,
        "Explicit absolute existing service-owned mode-0700 directory, outside webroot and Git checkouts. Required for API and worker."
    ),
    f!(
        "storage.max_upload_bytes",
        Integer,
        "Maximum bytes per attachment.",
        1048576,
        104857600
    ),
    f!(
        "storage.workspace_quota_bytes",
        Integer,
        "Workspace quota including pending, quarantine and not-yet-collected deleted bytes.",
        1,
        9223372036854775807
    ),
    f!(
        "storage.quarantine_ttl",
        Integer,
        "Unlinked upload lifetime in seconds; expired bytes become eligible for fenced worker cleanup.",
        60,
        604800
    ),
    f!(
        "storage.antivirus_required",
        Boolean,
        "Require a configured ClamAV scanner before any attachment becomes ready; no scanner reports not_scanned when optional."
    ),
    f!(
        "storage.scanner_socket",
        String,
        "Absolute ClamAV Unix socket; configured scanner failures retain quarantine for retry."
    ),
    f!(
        "worker.database_url",
        secret,
        true,
        "Separate worker PostgreSQL URL; never required for serving and exclusive with worker.database_url_file."
    ),
    f!(
        "worker.database_url_file",
        secret,
        false,
        "Absolute service-owned 0400/0600 file with the restricted worker PostgreSQL URL; mount only in the worker."
    ),
    f!(
        "worker.max_connections",
        Integer,
        "Maximum worker pool connections; one batch executes at a time in this implementation.",
        1,
        50
    ),
    f!(
        "worker.lease_seconds",
        Integer,
        "Lease per bounded batch; renewed by a new claim between batches, with a fencing token.",
        10,
        300
    ),
    f!(
        "worker.poll_interval_ms",
        Integer,
        "Idle discovery interval in milliseconds. No transaction is held while waiting.",
        100,
        60000
    ),
    f!(
        "schema_version",
        Integer,
        "Required in every selected TOML file. Only schema version 1 is implemented.",
        1,
        1
    ),
    f!(
        "server.environment",
        String,
        "production, development, or test. Development/test listen only on loopback unless local_container is explicitly enabled for development."
    ),
    f!(
        "server.local_container",
        Boolean,
        "Default false. Development-only wildcard container listener with exclusively HTTP loopback origins. Publish host ports only on loopback; never enable for a directly exposed host service. Production HTTPS mode rejects this flag."
    ),
    f!(
        "server.listen",
        String,
        "Listener IP and port; default 127.0.0.1:8080."
    ),
    f!(
        "server.public_url",
        String,
        "Explicit HTTPS origin; loopback HTTP only in development/test. No credentials, path, query, or fragment."
    ),
    f!(
        "server.allowed_origins",
        Strings,
        "Additional exact origins. The public origin is always included. Arrays replace rather than concatenate."
    ),
    f!(
        "server.static_dir",
        String,
        "Built frontend directory; production serving requires an absolute path. Development default is web/dist; migrations do not use it."
    ),
    f!(
        "database.api_max_connections",
        Integer,
        "Maximum connections in the API database pool.",
        2,
        100
    ),
    f!(
        "database.url",
        secret,
        true,
        "Runtime PostgreSQL URL supplied by environment only. Exclusive with database.url_file."
    ),
    f!(
        "database.url_file",
        secret,
        false,
        "Absolute service-owned 0400/0600 UTF-8 file containing the runtime PostgreSQL URL."
    ),
    f!(
        "database.migration_url",
        secret,
        true,
        "Migration PostgreSQL URL supplied by environment only. Exclusive with database.migration_url_file."
    ),
    f!(
        "database.migration_url_file",
        secret,
        false,
        "Absolute service-owned 0400/0600 UTF-8 file containing the separate migrator PostgreSQL URL."
    ),
    f!(
        "auth.hash_concurrency",
        Integer,
        "Maximum concurrent password hashing tasks.",
        1,
        8
    ),
    f!(
        "auth.setup_token",
        secret,
        true,
        "Setup token supplied by environment only; minimum 32 bytes. Exclusive with auth.setup_token_file."
    ),
    f!(
        "auth.setup_token_file",
        secret,
        false,
        "Absolute service-owned 0400/0600 UTF-8 file containing the setup token."
    ),
    f!(
        "auth.recovery_ttl_minutes",
        Integer,
        "Lifetime in minutes of newly issued local-operator recovery codes.",
        5,
        60
    ),
    f!(
        "operator.database_url",
        secret,
        true,
        "Separate local-operator PostgreSQL URL supplied by environment only. Exclusive with operator.database_url_file; never required for serving."
    ),
    f!(
        "operator.database_url_file",
        secret,
        false,
        "Absolute service-owned 0400/0600 file with the separate local-operator PostgreSQL URL; mount only for operator commands."
    ),
];
pub(super) fn field(path: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.path == path)
}
pub fn json_schema() -> Value {
    let defaults: toml::Value = toml::from_str(super::DEFAULTS).expect("compiled defaults");
    let mut props = serde_json::Map::new();
    for field in FIELDS.iter().filter(|f| !f.env_only) {
        let mut schema = json!({"type":match field.kind{Kind::String=>"string",Kind::Boolean=>"boolean",Kind::Integer=>"integer",Kind::Strings=>"array"},"description":field.description});
        if let Kind::Strings = field.kind {
            schema["items"] = json!({"type":"string"});
        }
        if let Some(min) = field.min {
            schema["minimum"] = min.into();
        }
        if let Some(max) = field.max {
            schema["maximum"] = max.into();
        }
        if field.path == "server.environment" {
            schema["enum"] = json!(["production", "development", "test"]);
        }
        if field.secret {
            schema["writeOnly"] = true.into();
        }
        if let Some(value) = super::get(&defaults, field.path)
            && field.path != "server.public_url"
        {
            schema["default"] = serde_json::to_value(value).expect("TOML to JSON");
        }
        if let Some((section, key)) = field.path.split_once('.') {
            let parent = props.entry(section).or_insert_with(
                || json!({"type":"object","additionalProperties":false,"properties":{}}),
            );
            parent["properties"][key] = schema;
        } else {
            props.insert(field.path.into(), schema);
        }
    }
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema","title":"OpenTrack implemented configuration schema v1","description":"Only implemented fields; runtime additionally validates origins, cross-field constraints, secret ownership and command-specific required secrets. Future catalog keys are rejected until implemented.","type":"object","additionalProperties":false,"required":["schema_version"],"properties":props})
}

pub fn reference_markdown() -> String {
    let defaults: toml::Value = toml::from_str(super::DEFAULTS).expect("compiled defaults");
    let mut output = String::from(
        "# Implemented settings reference\n\nGenerated by `opentrack-server config-reference` from the same field metadata as validation and [schema.json](schema.json). Do not edit this table by hand. [Usage and precedence](../docs/configuration/IMPLEMENTED_SETTINGS.md).\n\nOnly these settings are implemented. Every explicitly selected TOML file requires `schema_version = 1`; compiled defaults provide it when no file is selected. Secret literals are accepted only through environment variables. File references also accept named CLI flags.\n\n| Key | Type | Default | Bounds | Secret | Meaning |\n| --- | --- | --- | --- | --- | --- |\n",
    );
    for field in FIELDS {
        let kind = match field.kind {
            Kind::String => "string",
            Kind::Boolean => "boolean",
            Kind::Integer => "integer",
            Kind::Strings => "string array",
        };
        let default = super::get(&defaults, field.path)
            .filter(|_| field.path != "server.public_url")
            .map(|value| format!("`{value}`"))
            .unwrap_or_else(|| "unset".into());
        let bounds = match (field.min, field.max) {
            (Some(min), Some(max)) if min == max => format!("exactly {min}"),
            (Some(min), Some(max)) => format!("{min}–{max}"),
            _ => "—".into(),
        };
        let secret = if field.secret { "yes; redacted" } else { "no" };
        output.push_str(&format!(
            "| `{}` | {kind} | {default} | {bounds} | {secret} | {} |\n",
            field.path, field.description
        ));
    }
    output
}
