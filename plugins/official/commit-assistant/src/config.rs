use openidea_plugin::{Error, HttpRequest, HttpResponse, host};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub const DEFAULT_PROMPT: &str = include_str!("../default-prompt.txt");

pub struct Config {
    pub base: String,
    pub model: String,
    pub chinese: bool,
    pub prompt: String,
    pub timeout: u32,
    pub tokens: u64,
    pub context: usize,
}

impl Config {
    pub fn load(arg: &Value) -> Self {
        let get = |key: &str| {
            arg.get("settings")
                .and_then(|s| s.get(key))
                .cloned()
                .or_else(|| host::setting(key).and_then(|v| serde_json::from_str(&v).ok()))
        };
        let string = |key: &str, default: &str| {
            get(key).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| default.into())
        };
        let number = |key, default, min, max| {
            get(key).and_then(|v| v.as_i64()).unwrap_or(default as i64).clamp(min as i64, max as i64) as u64
        };
        let chinese = string("language", "zh") != "en";
        let prompt = string("promptTemplate", DEFAULT_PROMPT);
        Self {
            base: string("endpoint", "https://api.openai.com/v1"),
            model: string("model", "").trim().to_owned(),
            chinese,
            prompt: if prompt.trim().is_empty() { DEFAULT_PROMPT.into() } else { prompt },
            timeout: number("timeoutSeconds", 30, 5, 45) as u32 * 1000,
            tokens: number("maxTokens", 512, 64, 2048),
            context: number("contextCharacters", 24_000, 1000, 64_000) as usize,
        }
    }

    pub fn url(&self, resource: &str) -> Result<String, Error> {
        api_url(&self.base, resource)
    }

    pub fn system_prompt(&self) -> String {
        self.prompt.replace("{{language}}", if self.chinese { "Chinese" } else { "English" })
    }

    pub fn fetch(&self, resource: &str, body: Option<Value>) -> Result<HttpResponse, Error> {
        let url = self.url(resource)?;
        let mut request = match body {
            Some(body) => HttpRequest::post(&url, body.to_string()).header("Content-Type", "application/json"),
            None => HttpRequest::get(&url),
        };
        if host::secret_is_set("api-key") {
            request = request.header("Authorization", "Bearer {{secret:api-key}}");
        }
        request.timeout_ms = Some(self.timeout);
        host::fetch(&request)
    }

    pub fn complete(&self, messages: Value) -> Result<HttpResponse, Error> {
        let started = Instant::now();
        let mut body = json!({"model":self.model,"messages":messages,"max_tokens":self.tokens,"stream":false});
        let first = self.fetch("chat/completions", Some(body.clone()))?;
        if first.status == 400 && first.body.len() <= 512 * 1024 {
            let text = first.text();
            let normalized = text.to_ascii_lowercase();
            if normalized.contains("max_tokens") && normalized.contains("max_completion_tokens") {
                body.as_object_mut().unwrap().remove("max_tokens");
                body["max_completion_tokens"] = json!(self.tokens);
                let remaining = Duration::from_millis(self.timeout as u64).saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return Err("request timed out".into());
                }
                let mut request = HttpRequest::post(&self.url("chat/completions")?, body.to_string())
                    .header("Content-Type", "application/json");
                if host::secret_is_set("api-key") {
                    request = request.header("Authorization", "Bearer {{secret:api-key}}");
                }
                request.timeout_ms = Some(remaining.as_millis().max(1) as u32);
                return host::fetch(&request);
            }
        }
        Ok(first)
    }
}

pub fn diagnostic(id: &str, arg: &Value) -> Value {
    let task = host::begin_task("Testing API", true);
    let started = Instant::now();
    let c = Config::load(arg);
    let mut result = if c.url("models").is_err() {
        json!({"ok":false,"code":"invalidEndpoint"})
    } else if id == "test-model" && c.model.is_empty() {
        json!({"ok":false,"code":"missingModel"})
    } else {
        let response = if id == "test-model" {
            c.complete(json!([{"role":"user","content":"Reply only OK. This is an API connectivity test."}]))
        } else {
            c.fetch("models", None)
        };
        match response {
            Ok(r) if !(200..300).contains(&r.status) => {
                json!({"ok":false,"code":if id == "test-model" && matches!(r.status,404|405) {"chatEndpointMissing"} else {"http"},"status":r.status})
            }
            Ok(r) => {
                let value = serde_json::from_str::<Value>(&r.text()).ok();
                if id == "test-model" {
                    let valid = value
                        .as_ref()
                        .and_then(|v| v["choices"][0]["message"]["content"].as_str())
                        .is_some_and(|s| !s.trim().is_empty());
                    json!({"ok":valid,"code":if valid {"success"} else {"emptyCompletion"},"status":r.status})
                } else {
                    let list = value
                        .as_ref()
                        .and_then(|v| v.get("data").or_else(|| v.get("models")))
                        .and_then(Value::as_array);
                    match list {
                        Some(list) => {
                            let mut models: Vec<String> = list
                                .iter()
                                .filter_map(|v| v.get("id").or_else(|| v.get("name")).and_then(Value::as_str))
                                .filter(|id| !id.trim().is_empty() && id.len() <= 256)
                                .map(str::to_owned)
                                .collect();
                            models.sort();
                            models.dedup();
                            models.truncate(500);
                            json!({"ok":true,"status":r.status,"models":models})
                        }
                        None => json!({"ok":false,"code":"invalidResponse","status":r.status}),
                    }
                }
            }
            Err(e) => {
                let lower = e.to_string().to_ascii_lowercase();
                let code = match e {
                    Error::Denied(_) => "permissionDenied",
                    Error::Cancelled => "cancelled",
                    _ if lower.contains("timed out") || lower.contains("timeout") => "timeout",
                    _ if lower.contains("certificate") || lower.contains("tls") || lower.contains("ssl") => "tls",
                    _ if lower.contains("dns") || lower.contains("resolve") => "dns",
                    _ => "network",
                };
                json!({"ok":false,"code":code})
            }
        }
    };
    if task.is_cancelled() {
        result = json!({"ok":false,"code":"cancelled"});
    }
    result["elapsedMs"] = json!(started.elapsed().as_millis() as u64);
    result
}

pub fn api_url(base: &str, resource: &str) -> Result<String, Error> {
    let mut url = url::Url::parse(base.trim()).map_err(|_| "Invalid API base URL")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || url.scheme() == "http" && local)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Use an HTTPS API base URL without credentials, query or fragment (HTTP allowed for localhost)".into()
        );
    }
    let path = url.path().trim_end_matches('/');
    let base = path.strip_suffix("/chat/completions").unwrap_or(path);
    let base = if base.is_empty() { "/v1" } else { base };
    url.set_path(&format!("{base}/{resource}"));
    Ok(url.into())
}
