mod config;
mod selection;

use openidea_plugin::host;
use openidea_plugin::{Error, Invocation, Plugin, register_plugin};
use serde_json::{Value, json};

#[cfg(test)]
const MAX_CONTEXT: usize = 24_000;
const MAX_FILES: usize = 80;

#[derive(Default)]
pub struct CommitAssistant;

fn head(text: &str, max: usize) -> &str {
    text.char_indices().nth(max).map_or(text, |(i, _)| &text[..i])
}

fn sensitive(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    name == ".env"
        || name.starts_with(".env.")
        || name.contains("credential")
        || name.contains("secret")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || [".pem", ".key", ".p12", ".pfx", ".keystore"].iter().any(|s| name.ends_with(s))
}

fn paths(raw: &str) -> Result<Vec<String>, Error> {
    let mut items = raw.split_terminator('\0');
    let mut out = Vec::new();
    while let Some(status) = items.next() {
        let path = items.next().ok_or("Invalid staged file list")?;
        if status.starts_with(['R', 'C']) {
            return Err("Unexpected rename in staged file list".into());
        }
        if !sensitive(path) {
            out.push(path.to_owned());
        }
    }
    if out.is_empty() {
        return Err("Stage some non-sensitive changes before generating a commit message".into());
    }
    if out.len() > MAX_FILES {
        return Err("Too many staged files; split the changes into smaller commits (maximum 80 files)".into());
    }
    Ok(out)
}

#[cfg(test)]
fn endpoint(base: &str) -> Result<String, Error> {
    config::api_url(base, "chat/completions")
}

fn offline(files: &[String], chinese: bool) -> String {
    let names = files.iter().take(3).map(|p| p.rsplit('/').next().unwrap_or(p)).collect::<Vec<_>>().join(", ");
    let summary = if files.len() > 3 { format!("{names}, …") } else { names };
    let title = if chinese { format!("chore: 更新 {summary}") } else { format!("chore: update {summary}") };
    format!("{}\n\n{}", head(&title, 72), files.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n"))
}

fn response_message(text: &str) -> Result<String, Error> {
    let value: Value = serde_json::from_str(text).map_err(|_| "Invalid response from commit message API")?;
    let content = value["choices"][0]["message"]["content"].as_str().unwrap_or("").trim();
    let content = if content.starts_with("```") {
        content.split_once('\n').map(|(_, s)| s.trim_end_matches('`').trim()).unwrap_or("")
    } else {
        content
    };
    if content.is_empty() || content.chars().count() > 4000 {
        return Err("The API returned an empty or excessively long commit message".into());
    }
    Ok(content.to_owned())
}

impl Plugin for CommitAssistant {
    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let argument: Value =
            arg.map(serde_json::from_str).transpose().map_err(|_| "Invalid generator argument")?.unwrap_or(Value::Null);
        if id == "preview-prompt" {
            let c = config::Config::load(&argument);
            return Ok(Some(json!({"ok":true,"code":"promptPreview","prompt":c.system_prompt()}).to_string()));
        }
        if matches!(id, "list-models" | "test-api" | "test-model") {
            let result = config::diagnostic(id, &argument);
            if arg.is_none() {
                host::notify(
                    openidea_plugin::NotificationLevel::Info,
                    "Commit Assistant",
                    &format!("API test: {} ({} ms)", result["code"].as_str().unwrap_or("success"), result["elapsedMs"]),
                );
            }
            return Ok(Some(result.to_string()));
        }
        if !matches!(id, "generate-ai" | "generate-offline") {
            return Err(Error::NotFound(format!("command {id}")));
        }
        let c = config::Config::load(&argument);
        if id == "generate-ai" {
            if c.model.is_empty() {
                return Err("Set the model ID in Settings > Plugins > Commit Assistant".into());
            }
            c.url("chat/completions")?;
        }
        let project = ctx.project().ok_or("Open a trusted Git project first")?;
        let root = argument["repo"].as_str().map(str::to_owned).unwrap_or(project.root()?);
        let git = |args: &[&str]| -> Result<String, Error> {
            let out = selection::git(&project, &root, args)?;
            if out.status != Some(0) {
                return Err("Unable to read staged Git changes".into());
            }
            Ok(out.stdout)
        };
        let task = host::begin_task("Generating commit message", true);
        let selection = selection::context(&project, &argument, c.context, id == "generate-ai")?;
        let before = if selection.is_none() {
            Some(git(&["diff", "--cached", "--raw", "--no-abbrev", "--no-renames", "--no-ext-diff", "--no-textconv"])?)
        } else {
            None
        };
        let files = match &selection {
            Some(selection) => selection.files.clone(),
            None => paths(&git(&[
                "diff",
                "--cached",
                "--name-status",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
            ])?)?,
        };
        let message = if id == "generate-offline" {
            offline(&files, c.chinese)
        } else {
            let diff = if let Some(selection) = &selection {
                selection.diff.clone()
            } else {
                let mut args = vec![
                    "diff",
                    "--cached",
                    "--no-color",
                    "--no-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=3",
                    "--",
                ];
                args.extend(files.iter().map(String::as_str));
                git(&args)?
            };
            let truncated = diff.chars().count() >= c.context;
            let messages = json!([
                {"role":"system","content":c.system_prompt()},
                {"role":"user","content":format!("{} files:\n{}\n\nDiff{}:\n{}", if selection.is_some() {"Selected"} else {"Staged"}, files.join("\n"), if truncated {" (truncated)"} else {""}, head(&diff,c.context))}
            ]);
            if task.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let response = c.complete(messages)?;
            if !(200..300).contains(&response.status) {
                return Err(format!("Commit message API returned HTTP {}", response.status).into());
            }
            if response.body.len() > 2 * 1024 * 1024 {
                return Err("The API response exceeded the size limit".into());
            }
            response_message(&response.text())?
        };
        if task.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if let Some(selection) = &selection {
            selection.validate(&project)?;
        }
        if let Some(before) = before {
            let after =
                git(&["diff", "--cached", "--raw", "--no-abbrev", "--no-renames", "--no-ext-diff", "--no-textconv"])?;
            if before != after || after.is_empty() {
                return Err("Staged changes changed while generating; try again".into());
            }
        }
        if arg.is_none() {
            host::copy_to_clipboard(&message)?;
            host::notify(
                openidea_plugin::NotificationLevel::Info,
                "Commit Assistant",
                "Commit message copied to clipboard. Review it before committing.",
            );
        }
        Ok(Some(message))
    }
}

register_plugin!(CommitAssistant);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{HttpResponse, ProcessOutput};

    fn setup() -> MockHost {
        let host = MockHost::install().with_capabilities(&[
            "project.read",
            "process:git",
            "net:api.example.com",
            "secrets",
            "clipboard",
        ]);
        host.on_process("git", |args, _| {
            assert!(args.iter().any(|v| v == "--cached"));
            assert!(args.iter().all(|v| !["add", "commit", "reset", "push"].contains(&v.as_str())));
            let stdout = if args.iter().any(|v| v == "--name-status") {
                "M\0src/main.rs\0A\0.env\0".into()
            } else if args.iter().any(|v| v == "--raw") {
                ":100644 100644 old new M\tsrc/main.rs".into()
            } else {
                assert!(args.ends_with(&["--".into(), "src/main.rs".into()]));
                "+fn new_feature() {}\n".repeat(2000)
            };
            Ok(ProcessOutput { status: Some(0), stdout, stderr: String::new() })
        });
        host
    }

    #[test]
    fn offline_never_calls_a_model_and_skips_common_secret_files() {
        let host = setup();
        let message = CommitAssistant.run_command("generate-offline", &host.invocation(), Some("{}")).unwrap().unwrap();
        assert!(message.contains("src/main.rs"));
        assert!(!message.contains(".env"));
        assert!(host.requests().is_empty());
        assert_eq!(host.process_runs().len(), 3);
        assert!(paths("A\0secrets.json\0").is_err());
        assert_eq!(paths("M\0src/file with spaces.rs\0").unwrap(), ["src/file with spaces.rs"]);
    }

    #[test]
    fn endpoint_normalizes_the_api_base_and_rejects_unsafe_urls() {
        assert_eq!(endpoint("https://api.example.com/").unwrap(), "https://api.example.com/v1/chat/completions");
        assert_eq!(
            endpoint("https://api.example.com/custom/v1/").unwrap(),
            "https://api.example.com/custom/v1/chat/completions"
        );
        assert_eq!(
            endpoint("http://localhost:11434/v1/chat/completions").unwrap(),
            "http://localhost:11434/v1/chat/completions"
        );
        for url in
            ["http://example.com/v1", "https://key@example.com", "file:///tmp/a", "https://example.com/?key=secret"]
        {
            assert!(endpoint(url).is_err(), "{url}");
        }
    }

    #[test]
    fn ai_uses_the_configured_endpoint_key_and_bounded_staged_context() {
        let host = setup();
        host.set_setting("endpoint", "\"https://api.example.com/custom/v1\"");
        host.set_setting("model", "\"my-model\"");
        host.set_secret("api-key", "test-only");
        host.on_fetch(|r| {
            assert_eq!(r.url, "https://api.example.com/custom/v1/chat/completions");
            assert!(r.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer test-only"));
            let body: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert_eq!(body["model"], "my-model");
            assert!(body["messages"][1]["content"].as_str().unwrap().chars().count() < MAX_CONTEXT + 200);
            assert!(body["messages"][1]["content"].as_str().unwrap().contains("truncated"));
            assert_eq!(body["stream"], false);
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: br#"{"choices":[{"message":{"content":"feat: add feature"}}]}"#.to_vec(),
            })
        });
        let message = CommitAssistant.run_command("generate-ai", &host.invocation(), Some("{}")).unwrap();
        assert_eq!(message.as_deref(), Some("feat: add feature"));
        assert_eq!(host.requests().len(), 1);
    }

    #[test]
    fn ai_does_not_send_anything_until_configured_and_keeps_errors_redacted() {
        let host = setup();
        assert!(CommitAssistant.run_command("generate-ai", &host.invocation(), Some("{}")).is_err());
        assert!(host.requests().is_empty());
        host.set_setting("model", "\"model\"");
        host.set_setting("endpoint", "\"https://api.example.com\"");
        host.set_secret("api-key", "test-only");
        host.on_fetch(|_| {
            Ok(HttpResponse { status: 401, headers: vec![], body: b"never echo secret response".to_vec() })
        });
        let err = CommitAssistant.run_command("generate-ai", &host.invocation(), Some("{}")).unwrap_err();
        assert_eq!(err.to_string(), "Commit message API returned HTTP 401");
    }

    #[test]
    fn staging_changes_during_the_request_discard_the_answer() {
        let host = setup();
        let counter = std::cell::Cell::new(0);
        host.on_process("git", move |args, _| {
            let stdout = if args.iter().any(|v| v == "--name-status") {
                "M\0a.rs\0".into()
            } else {
                counter.set(counter.get() + 1);
                counter.get().to_string()
            };
            Ok(ProcessOutput { status: Some(0), stdout, stderr: String::new() })
        });
        assert!(
            CommitAssistant
                .run_command("generate-offline", &host.invocation(), Some("{}"))
                .unwrap_err()
                .to_string()
                .contains("changed")
        );
    }
}

#[cfg(test)]
mod tests_v2;
