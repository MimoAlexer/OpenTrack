mod cli;
mod schema;
pub use schema::{json_schema, reference_markdown};
#[cfg(test)]
mod tests;
pub use cli::{Action, Cli};
use schema::{FIELDS, Kind, field};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use toml::Value;
use url::Url;

type Result<T> = std::result::Result<T, String>;
const DEFAULTS: &str = r#"schema_version = 1
[server]
environment = "production"
local_container = false
listen = "127.0.0.1:8080"
public_url = ""
allowed_origins = []
static_dir = "web/dist"
[database]
api_max_connections = 20
[auth]
hash_concurrency = 2
recovery_ttl_minutes = 30
[operator]
[storage]
backend = "local"
max_upload_bytes = 26214400
workspace_quota_bytes = 5368709120
quarantine_ttl = 86400
antivirus_required = false
[worker]
max_connections = 5
lease_seconds = 60
poll_interval_ms = 2000
"#;

// Intentionally no Debug/Serialize: these values coexist with secret inputs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub schema_version: u32,
    pub server: Server,
    pub database: Database,
    pub auth: Auth,
    pub operator: Operator,
    pub worker: Worker,
    pub storage: Storage,
    #[serde(skip)]
    secrets: BTreeMap<String, SecretInput>,
    #[serde(skip)]
    provenance: BTreeMap<String, Source>,
    #[serde(skip, default = "empty_table")]
    effective: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub environment: String,
    pub local_container: bool,
    pub listen: SocketAddr,
    pub public_url: String,
    pub allowed_origins: Vec<String>,
    pub static_dir: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Database {
    pub url_file: Option<PathBuf>,
    pub migration_url_file: Option<PathBuf>,
    pub api_max_connections: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    pub setup_token_file: Option<PathBuf>,
    pub hash_concurrency: usize,
    pub recovery_ttl_minutes: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operator {
    pub database_url_file: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worker {
    pub database_url_file: Option<PathBuf>,
    pub max_connections: u32,
    pub lease_seconds: u32,
    pub poll_interval_ms: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub backend: String,
    pub local_root: Option<PathBuf>,
    pub max_upload_bytes: i64,
    pub workspace_quota_bytes: i64,
    pub quarantine_ttl: u32,
    pub antivirus_required: bool,
    pub scanner_socket: Option<PathBuf>,
}
enum SecretInput {
    Literal(String),
    File,
}
fn empty_table() -> Value {
    Value::Table(toml::Table::new())
}
#[derive(Clone)]
struct Source {
    kind: &'static str,
    location: String,
    line: Option<usize>,
    column: Option<usize>,
}
impl Source {
    fn default() -> Self {
        Self {
            kind: "default",
            location: "compiled defaults".into(),
            line: None,
            column: None,
        }
    }
    fn env(name: &str) -> Self {
        Self {
            kind: "environment",
            location: name.into(),
            line: None,
            column: None,
        }
    }
    fn cli(name: &str) -> Self {
        Self {
            kind: "command_line",
            location: name.into(),
            line: None,
            column: None,
        }
    }
    fn describe(&self) -> String {
        let line = self
            .line
            .map(|l| format!(":{l}:{}", self.column.unwrap_or(1)))
            .unwrap_or_default();
        format!("{} {}{line}", self.kind, safe_location(&self.location))
    }
}
fn safe_location(text: &str) -> String {
    text.chars()
        .flat_map(char::escape_default)
        .take(512)
        .collect()
}
fn error(path: &str, source: &Source, message: &str) -> String {
    format!("{path}: {message} (from {})", source.describe())
}

impl Settings {
    pub fn load(cli: &Cli) -> Result<Self> {
        Self::from_sources(cli, &env::vars().collect())
    }
    fn from_sources(cli: &Cli, environment: &BTreeMap<String, String>) -> Result<Self> {
        let mut merged: Value =
            toml::from_str(DEFAULTS).map_err(|_| "Compiled configuration defaults are invalid")?;
        let mut provenance = BTreeMap::new();
        for f in FIELDS {
            if get(&merged, f.path).is_some() {
                provenance.insert(f.path.into(), Source::default());
            }
        }
        let base = cli
            .config
            .clone()
            .or_else(|| environment.get("OPENTRACK_CONFIG").map(PathBuf::from));
        if let Some(path) = base {
            merge_file(&mut merged, &mut provenance, &path)?;
        }
        if let Some(path) = &cli.environment {
            merge_file(&mut merged, &mut provenance, path)?;
        }
        for (name, path) in LEGACY {
            if let Some(raw) = environment.get(*name) {
                let source = Source::env(name);
                let value = if *name == "OPENTRACK_ALLOWED_ORIGINS" {
                    Value::Array(
                        raw.split(',')
                            .map(str::trim)
                            .filter(|x| !x.is_empty())
                            .map(|x| Value::String(x.into()))
                            .collect(),
                    )
                } else {
                    parse_override(path, raw, &source)?
                };
                put(&mut merged, path, value);
                provenance.insert((*path).into(), source);
            }
        }
        for (name, raw) in environment
            .iter()
            .filter(|(name, _)| name.starts_with("OPENTRACK__"))
        {
            let path = name
                .trim_start_matches("OPENTRACK__")
                .to_ascii_lowercase()
                .replace("__", ".");
            let source = Source::env(name);
            if field(&path).is_none() {
                return Err(error(&safe_field(&path), &source, "unknown setting"));
            }
            let value = parse_override(&path, raw, &source)?;
            put(&mut merged, &path, value);
            provenance.insert(path, source);
        }
        for (path, raw, flag) in &cli.overrides {
            let source = Source::cli(flag);
            let value = parse_override(path, raw, &source)?;
            put(&mut merged, path, value);
            provenance.insert(path.clone(), source);
        }
        let target = if cli.action == Action::CheckConfig {
            cli.check_target
        } else {
            cli.action
        };
        validate_values(&merged, &provenance, target)?;
        let mut secrets = BTreeMap::new();
        for (literal, file) in SECRET_PAIRS {
            let direct = get(&merged, literal).and_then(Value::as_str);
            let reference = get(&merged, file).and_then(Value::as_str);
            if direct.is_some() && reference.is_some() {
                return Err(error(
                    literal,
                    source_for(&provenance, literal),
                    &format!("choose one literal or {file} secret reference, not both"),
                ));
            }
            if let Some(raw) = direct {
                if raw.trim().is_empty() {
                    return Err(error(
                        literal,
                        source_for(&provenance, literal),
                        "secret must not be empty",
                    ));
                }
                secrets.insert((*literal).into(), SecretInput::Literal(raw.trim().into()));
            } else if let Some(raw) = reference {
                let path = PathBuf::from(raw);
                validate_secret_file(&path)
                    .map_err(|m| error(file, source_for(&provenance, file), &m))?;
                secrets.insert((*literal).into(), SecretInput::File);
            }
        }
        let mut public = merged.clone();
        for (literal, _) in SECRET_PAIRS {
            remove(&mut public, literal);
        }
        let mut settings: Self = public.try_into().map_err(
            |_| "Configuration types do not match the implemented schema (values redacted)",
        )?;
        settings.provenance = provenance;
        settings.effective = merged;
        settings.secrets = secrets;
        Ok(settings)
    }
    pub fn origins(&self) -> Vec<String> {
        let mut origins = self.server.allowed_origins.clone();
        origins.push(self.server.public_url.trim_end_matches('/').into());
        origins.sort();
        origins.dedup();
        origins
    }
    pub fn database_url(&self, migrate: bool) -> Result<String> {
        let key = if migrate {
            "database.migration_url"
        } else {
            "database.url"
        };
        self.postgres_secret(key)
    }
    pub fn operator_database_url(&self) -> Result<String> {
        self.postgres_secret("operator.database_url")
    }
    pub fn worker_database_url(&self) -> Result<String> {
        self.postgres_secret("worker.database_url")
    }
    fn postgres_secret(&self, key: &str) -> Result<String> {
        let value = self.secret(key)?;
        let url = Url::parse(&value)
            .map_err(|_| self.secret_error(key, "invalid PostgreSQL URL (redacted)"))?;
        if !matches!(url.scheme(), "postgres" | "postgresql")
            || url.host_str().is_none()
            || url.path().trim_matches('/').is_empty()
        {
            return Err(self.secret_error(
                key,
                "expected a PostgreSQL URL with host and database (redacted)",
            ));
        }
        Ok(value)
    }
    pub fn setup_token(&self) -> Result<String> {
        let value = self.secret("auth.setup_token")?;
        if value.len() < 32 {
            return Err(self.secret_error(
                "auth.setup_token",
                "setup token must contain at least 32 bytes",
            ));
        }
        Ok(value)
    }
    pub fn validate_secrets(&self, action: Action) -> Result<()> {
        match action {
            Action::Worker => {
                self.worker_database_url()?;
            }
            Action::Migrate => {
                self.database_url(true)?;
            }
            action if action.is_operator() => {
                self.operator_database_url()?;
            }
            _ => {
                self.database_url(false)?;
                self.setup_token()?;
            }
        }
        Ok(())
    }
    fn secret(&self, key: &str) -> Result<String> {
        match self.secrets.get(key) {
            Some(SecretInput::Literal(value)) => Ok(value.clone()),
            Some(SecretInput::File) => {
                let path = match key {
                    "database.url" => self.database.url_file.as_deref(),
                    "database.migration_url" => self.database.migration_url_file.as_deref(),
                    "auth.setup_token" => self.auth.setup_token_file.as_deref(),
                    "operator.database_url" => self.operator.database_url_file.as_deref(),
                    "worker.database_url" => self.worker.database_url_file.as_deref(),
                    _ => None,
                }
                .ok_or_else(|| self.secret_error(key, "secret reference is unavailable"))?;
                // Recheck immediately before opening; the handle is validated again below.
                read_secret_file(path).map_err(|m| self.secret_error(key, &m))
            }
            None => Err(self.secret_error(key, "required secret is not configured")),
        }
    }
    fn secret_error(&self, key: &str, message: &str) -> String {
        let file = SECRET_PAIRS
            .iter()
            .find(|(literal, _)| *literal == key)
            .map(|(_, file)| *file)
            .unwrap_or(key);
        let actual = if self.provenance.contains_key(key) {
            key
        } else {
            file
        };
        error(
            actual,
            self.provenance.get(actual).unwrap_or(&Source::default()),
            message,
        )
    }
    pub fn effective_toml(&self) -> Result<String> {
        let mut safe = self.effective.clone();
        for spec in FIELDS {
            if spec.secret && get(&safe, spec.path).is_some() {
                put(&mut safe, spec.path, Value::String("<redacted>".into()));
            }
        }
        let mut output = toml::Table::new();
        output.insert("settings".into(), safe);
        let mut origins = toml::Table::new();
        for spec in FIELDS {
            let mut item = toml::Table::new();
            if let Some(source) = self.provenance.get(spec.path) {
                item.insert("source".into(), Value::String(source.kind.into()));
                item.insert("location".into(), Value::String(source.location.clone()));
                if let Some(line) = source.line {
                    item.insert("line".into(), Value::Integer(line as i64));
                    item.insert(
                        "column".into(),
                        Value::Integer(source.column.unwrap_or(1) as i64),
                    );
                }
            } else {
                item.insert("source".into(), Value::String("unset".into()));
            }
            item.insert("redacted".into(), Value::Boolean(spec.secret));
            origins.insert(spec.path.into(), Value::Table(item));
        }
        output.insert("provenance".into(), Value::Table(origins));
        toml::to_string_pretty(&output).map_err(|_| "Cannot render effective configuration".into())
    }
}
fn source_for<'a>(sources: &'a BTreeMap<String, Source>, path: &str) -> &'a Source {
    sources
        .get(path)
        .expect("every resolved setting has provenance")
}
fn get<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |value, key| value.get(key))
}
fn put(root: &mut Value, path: &str, value: Value) {
    let mut parts = path.split('.').peekable();
    let mut current = root;
    while let Some(key) = parts.next() {
        let table = current.as_table_mut().expect("schema table");
        if parts.peek().is_none() {
            table.insert(key.into(), value);
            return;
        }
        current = table
            .entry(key)
            .or_insert_with(|| Value::Table(toml::Table::new()));
    }
}
fn remove(root: &mut Value, path: &str) {
    if let Some((section, key)) = path.split_once('.')
        && let Some(table) = root.get_mut(section).and_then(Value::as_table_mut)
    {
        table.remove(key);
    }
}
fn safe_field(path: &str) -> String {
    if path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
    {
        path.chars().take(128).collect()
    } else {
        "<unknown field>".into()
    }
}
fn line_column(contents: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(contents.len());
    let prefix = &contents[..contents.floor_char_boundary(offset)];
    (
        prefix.bytes().filter(|b| *b == b'\n').count() + 1,
        prefix
            .rsplit('\n')
            .next()
            .map(|s| s.chars().count() + 1)
            .unwrap_or(1),
    )
}
fn merge_file(root: &mut Value, sources: &mut BTreeMap<String, Source>, path: &Path) -> Result<()> {
    let contents = fs::read_to_string(path).map_err(|_| {
        format!(
            "Cannot read explicitly selected configuration {}",
            safe_location(&path.to_string_lossy())
        )
    })?;
    let source = Source {
        kind: "toml",
        location: path.to_string_lossy().into_owned(),
        line: Some(1),
        column: Some(1),
    };
    let parsed: Value = toml::from_str(&contents).map_err(|e| {
        let (line, column) = line_column(&contents, e.span().map(|r| r.start).unwrap_or(0));
        error(
            "TOML syntax",
            &Source {
                line: Some(line),
                column: Some(column),
                ..source.clone()
            },
            "invalid syntax (source excerpt redacted)",
        )
    })?;
    let table = parsed
        .as_table()
        .ok_or_else(|| error("configuration", &source, "expected TOML table"))?;
    let spans = toml::de::DeTable::parse(&contents)
        .map_err(|_| error("TOML syntax", &source, "cannot locate configuration fields"))?;
    let mut locations = BTreeMap::new();
    collect_locations(spans.get_ref(), "", &contents, &source, &mut locations);
    if table.get("schema_version").and_then(Value::as_integer) != Some(1) {
        return Err(error(
            "schema_version",
            locations.get("schema_version").unwrap_or(&source),
            "every selected TOML file requires schema_version = 1",
        ));
    }
    merge_table(root, sources, table, "", &source, &locations)?;
    Ok(())
}
fn collect_locations(
    table: &toml::de::DeTable<'_>,
    prefix: &str,
    contents: &str,
    source: &Source,
    locations: &mut BTreeMap<String, Source>,
) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.get_ref().to_string()
        } else {
            format!("{prefix}.{}", key.get_ref())
        };
        let (line, column) = line_column(contents, key.span().start);
        locations.insert(
            path.clone(),
            Source {
                line: Some(line),
                column: Some(column),
                ..source.clone()
            },
        );
        if let toml::de::DeValue::Table(child) = value.get_ref() {
            collect_locations(child, &path, contents, source, locations);
        }
    }
}
fn merge_table(
    root: &mut Value,
    sources: &mut BTreeMap<String, Source>,
    table: &toml::Table,
    prefix: &str,
    source: &Source,
    locations: &BTreeMap<String, Source>,
) -> Result<()> {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        let location = locations.get(&path).unwrap_or(source);
        if let Some(spec) = field(&path) {
            if spec.env_only {
                return Err(error(
                    &path,
                    location,
                    "literal secrets are accepted through environment only; use a secret-file reference in TOML",
                ));
            }
            validate_type(&path, value, location)?;
            put(root, &path, value.clone());
            sources.insert(path, location.clone());
        } else if FIELDS
            .iter()
            .any(|spec| spec.path.starts_with(&format!("{path}.")))
        {
            let child = value
                .as_table()
                .ok_or_else(|| error(&path, location, "expected a table"))?;
            merge_table(root, sources, child, &path, source, locations)?;
        } else {
            return Err(error(&safe_field(&path), location, "unknown setting"));
        }
    }
    Ok(())
}
fn parse_override(path: &str, raw: &str, source: &Source) -> Result<Value> {
    let spec = field(path).ok_or_else(|| error(&safe_field(path), source, "unknown setting"))?;
    let value = match spec.kind {
        Kind::String => Value::String(raw.into()),
        Kind::Boolean => Value::Boolean(
            raw.parse()
                .map_err(|_| error(path, source, "expected true or false"))?,
        ),
        Kind::Integer => Value::Integer(
            raw.parse()
                .map_err(|_| error(path, source, "expected an integer"))?,
        ),
        Kind::Strings => {
            let parsed: Value = toml::from_str(&format!("value = {raw}"))
                .map_err(|_| error(path, source, "expected a TOML array of strings"))?;
            if parsed.as_table().is_none_or(|table| table.len() != 1) {
                return Err(error(
                    path,
                    source,
                    "expected a single TOML array of strings",
                ));
            }
            parsed
                .get("value")
                .cloned()
                .ok_or_else(|| error(path, source, "expected a TOML array of strings"))?
        }
    };
    validate_type(path, &value, source)?;
    Ok(value)
}
fn validate_type(path: &str, value: &Value, source: &Source) -> Result<()> {
    let spec = field(path).ok_or_else(|| error(&safe_field(path), source, "unknown setting"))?;
    let valid = match spec.kind {
        Kind::String => value.is_str(),
        Kind::Boolean => value.is_bool(),
        Kind::Integer => value.is_integer(),
        Kind::Strings => value
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_str)),
    };
    if !valid {
        return Err(error(
            path,
            source,
            match spec.kind {
                Kind::String => "expected a string",
                Kind::Boolean => "expected a boolean",
                Kind::Integer => "expected an integer",
                Kind::Strings => "expected an array of strings",
            },
        ));
    }
    Ok(())
}
fn validate_values(root: &Value, sources: &BTreeMap<String, Source>, target: Action) -> Result<()> {
    for spec in FIELDS {
        let Some(value) = get(root, spec.path) else {
            continue;
        };
        let source = source_for(sources, spec.path);
        validate_type(spec.path, value, source)?;
        if let Some(n) = value.as_integer()
            && (spec.min.is_some_and(|min| n < min) || spec.max.is_some_and(|max| n > max))
        {
            return Err(error(
                spec.path,
                source,
                &format!(
                    "must be between {} and {}",
                    spec.min.unwrap_or(i64::MIN),
                    spec.max.unwrap_or(i64::MAX)
                ),
            ));
        }
    }
    let mode = get(root, "server.environment")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(mode, "production" | "development" | "test") {
        return Err(error(
            "server.environment",
            source_for(sources, "server.environment"),
            "expected production, development, or test",
        ));
    }
    let listen = get(root, "server.listen")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .parse::<SocketAddr>()
        .map_err(|_| {
            error(
                "server.listen",
                source_for(sources, "server.listen"),
                "expected a socket address",
            )
        })?;
    let local_container = get(root, "server.local_container")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if local_container && mode != "development" {
        return Err(error(
            "server.local_container",
            source_for(sources, "server.local_container"),
            "local containers require development mode; production HTTPS rules remain unchanged",
        ));
    }
    if mode != "production"
        && !listen.ip().is_loopback()
        && !(local_container && listen.ip().is_unspecified())
    {
        return Err(error(
            "server.listen",
            source_for(sources, "server.listen"),
            "development and test listeners must use loopback; a local container may explicitly use a wildcard listener",
        ));
    }
    for path in ["server.public_url", "server.allowed_origins"] {
        let value = get(root, path).expect("default provided");
        let values = if let Some(items) = value.as_array() {
            items.iter().filter_map(Value::as_str).collect()
        } else {
            vec![value.as_str().unwrap_or_default()]
        };
        for value in values {
            if path == "server.public_url"
                && value.is_empty()
                && (target.is_operator() || target == Action::Worker)
            {
                continue;
            }
            validate_origin(value, mode == "production")
                .map_err(|message| error(path, source_for(sources, path), message))?;
            if local_container {
                let origin = Url::parse(value).expect("origin validated above");
                if origin.scheme() != "http"
                    || !matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                {
                    return Err(error(
                        path,
                        source_for(sources, path),
                        "local container origins must be HTTP loopback; publish its host port only on loopback",
                    ));
                }
            }
        }
    }
    let static_dir = get(root, "server.static_dir")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if static_dir.is_empty() {
        return Err(error(
            "server.static_dir",
            source_for(sources, "server.static_dir"),
            "must not be empty",
        ));
    }
    if mode == "production"
        && target != Action::Migrate
        && target != Action::Worker
        && !target.is_operator()
        && !Path::new(static_dir).is_absolute()
    {
        return Err(error(
            "server.static_dir",
            source_for(sources, "server.static_dir"),
            "production static directory must be an absolute path",
        ));
    }
    if get(root, "storage.backend").and_then(Value::as_str) != Some("local") {
        return Err(error(
            "storage.backend",
            source_for(sources, "storage.backend"),
            "only local storage is implemented",
        ));
    }
    if matches!(target, Action::Serve | Action::Worker) {
        let path = get(root, "storage.local_root").and_then(Value::as_str);
        if path.is_none_or(|p| !Path::new(p).is_absolute()) {
            return Err(error(
                "storage.local_root",
                sources
                    .get("storage.local_root")
                    .unwrap_or(&Source::default()),
                "an explicit absolute private storage directory is required",
            ));
        }
    }
    if let Some(path) = get(root, "storage.scanner_socket").and_then(Value::as_str) {
        if !Path::new(path).is_absolute() {
            return Err(error(
                "storage.scanner_socket",
                source_for(sources, "storage.scanner_socket"),
                "must be an absolute Unix socket path",
            ));
        }
    } else if get(root, "storage.antivirus_required").and_then(Value::as_bool) == Some(true) {
        return Err(error(
            "storage.antivirus_required",
            source_for(sources, "storage.antivirus_required"),
            "requires storage.scanner_socket",
        ));
    }
    Ok(())
}
fn validate_origin(value: &str, production: bool) -> std::result::Result<(), &'static str> {
    let u = Url::parse(value).map_err(|_| "expected an explicit absolute origin URL")?;
    let local = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
        || u.host_str().is_none()
        || !(u.scheme() == "https" || (!production && local && u.scheme() == "http"))
    {
        return Err(
            "origin must be HTTPS without credentials/path/query; loopback HTTP requires development or test mode",
        );
    }
    Ok(())
}
fn validate_secret_file(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err("secret-file path must be absolute".into());
    }
    let meta = fs::symlink_metadata(path).map_err(|_| "cannot inspect secret file")?;
    if !meta.is_file() {
        return Err("secret file must be a regular file, not a symlink".into());
    }
    #[cfg(windows)]
    opentrack_adapters::open_private_file(path)
        .map_err(|_| "secret file must have a private service-owned DACL and no links")?;
    validate_secret_metadata(&meta)
}
fn validate_secret_metadata(meta: &fs::Metadata) -> Result<()> {
    if !meta.is_file() {
        return Err("secret file must be a regular file, not a symlink".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        // geteuid has no memory effects and is available on supported Unix platforms.
        let uid = unsafe { geteuid() };
        if meta.uid() != uid || !matches!(meta.permissions().mode() & 0o7777, 0o400 | 0o600) {
            return Err(
                "secret file must be owned and readable only by the service user (0400 or 0600)"
                    .into(),
            );
        }
    }
    if meta.len() > 65536 {
        return Err("secret file exceeds 64 KiB".into());
    }
    Ok(())
}
fn read_secret_file(path: &Path) -> Result<String> {
    use std::io::Read;
    validate_secret_file(path)?;
    #[cfg(windows)]
    let file = opentrack_adapters::open_private_file(path)
        .map_err(|_| "cannot safely open private secret file")?;
    #[cfg(not(windows))]
    let file = {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        options.open(path).map_err(|_| "cannot open secret file")?
    };
    validate_secret_metadata(
        &file
            .metadata()
            .map_err(|_| "cannot inspect opened secret file")?,
    )?;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read secret file")?;
    if bytes.len() > 65536 {
        return Err("secret file exceeds 64 KiB".into());
    }
    let value = String::from_utf8(bytes).map_err(|_| "secret file must contain UTF-8 text")?;
    let value = value.trim();
    if value.is_empty() {
        return Err("secret file must not be empty".into());
    }
    Ok(value.into())
}
const SECRET_PAIRS: &[(&str, &str)] = &[
    ("database.url", "database.url_file"),
    ("database.migration_url", "database.migration_url_file"),
    ("auth.setup_token", "auth.setup_token_file"),
    ("operator.database_url", "operator.database_url_file"),
    ("worker.database_url", "worker.database_url_file"),
];
const LEGACY: &[(&str, &str)] = &[
    ("OPENTRACK_WORKER_DATABASE_URL", "worker.database_url"),
    (
        "OPENTRACK_WORKER_DATABASE_URL_FILE",
        "worker.database_url_file",
    ),
    ("OPENTRACK_ENVIRONMENT", "server.environment"),
    ("OPENTRACK_LISTEN", "server.listen"),
    ("OPENTRACK_PUBLIC_URL", "server.public_url"),
    ("OPENTRACK_STATIC_DIR", "server.static_dir"),
    ("OPENTRACK_ALLOWED_ORIGINS", "server.allowed_origins"),
    ("OPENTRACK_DATABASE_URL", "database.url"),
    ("OPENTRACK_DATABASE_URL_FILE", "database.url_file"),
    ("OPENTRACK_MIGRATION_DATABASE_URL", "database.migration_url"),
    (
        "OPENTRACK_MIGRATION_DATABASE_URL_FILE",
        "database.migration_url_file",
    ),
    ("OPENTRACK_SETUP_TOKEN", "auth.setup_token"),
    ("OPENTRACK_SETUP_TOKEN_FILE", "auth.setup_token_file"),
    ("OPENTRACK_OPERATOR_DATABASE_URL", "operator.database_url"),
    (
        "OPENTRACK_OPERATOR_DATABASE_URL_FILE",
        "operator.database_url_file",
    ),
];
