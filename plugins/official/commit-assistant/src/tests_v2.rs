use super::*;
use openidea_plugin::mock::MockHost;
use openidea_plugin::{HttpResponse, ProcessOutput};

fn response(status: u16, body: &str) -> Result<HttpResponse, Error> {
    Ok(HttpResponse { status, headers: vec![], body: body.as_bytes().to_vec() })
}

fn run(host: &MockHost, command: &str, arg: Value) -> Value {
    let value = CommitAssistant.run_command(command, &host.invocation(), Some(&arg.to_string())).unwrap().unwrap();
    serde_json::from_str(&value).unwrap()
}

#[test]
fn diagnostics_use_draft_settings_without_project_or_code_access() {
    let host = MockHost::install().with_capabilities(&["net:api.example.com", "secrets"]);
    host.set_setting("endpoint", "\"https://old.example.com/v1\"");
    host.set_secret("api-key", "fake-test-key");
    host.on_fetch(|request| {
        assert_eq!(request.url, "https://api.example.com/custom/v1/models");
        assert_eq!(request.method, "GET");
        assert!(request.body.is_none());
        assert!(request.headers.iter().any(|(_, value)| value == "Bearer fake-test-key"));
        response(200, r#"{"data":[{"id":"z-model"},{"id":"a-model"},{"id":"z-model"}]}"#)
    });
    let result = run(&host, "list-models", json!({"settings":{"endpoint":"https://api.example.com/custom/v1"}}));
    assert_eq!(result["models"], json!(["a-model", "z-model"]));
    assert_eq!(result["ok"], true);
    assert!(result["elapsedMs"].is_number());
    assert!(host.process_runs().is_empty());
}

#[test]
fn errors_are_classified_without_echoing_server_bodies_or_keys() {
    let host = MockHost::install().with_capabilities(&["net:api.example.com"]);
    for status in [401, 403, 404, 429, 500] {
        host.on_fetch(move |_| response(status, "secret-error-response-must-not-appear"));
        let result = run(&host, "test-api", json!({"settings":{"endpoint":"https://api.example.com/v1"}}));
        assert_eq!(result["status"], status);
        assert_eq!(result["ok"], false);
        assert!(!result.to_string().contains("secret-error"));
    }
    for (message, code) in
        [("request timed out", "timeout"), ("DNS resolve error", "dns"), ("certificate failure", "tls")]
    {
        host.on_fetch(move |_| Err(Error::Failed(message.into())));
        assert_eq!(run(&host, "test-api", json!({"settings":{"endpoint":"https://api.example.com"}}))["code"], code);
    }
}

#[test]
fn model_probe_sends_a_small_non_project_prompt_and_supports_new_token_parameter() {
    let host = MockHost::install().with_capabilities(&["net:localhost"]);
    let calls = std::cell::Cell::new(0);
    host.on_fetch(move |request| {
        let body: Value = serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["model"], "draft-model");
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("connectivity test"));
        let call = calls.get();
        calls.set(call + 1);
        if call == 0 {
            assert_eq!(body["max_tokens"], 512);
            response(400, r#"{"error":{"message":"Use max_completion_tokens instead of max_tokens"}}"#)
        } else {
            assert!(body.get("max_tokens").is_none());
            assert_eq!(body["max_completion_tokens"], 512);
            response(200, r#"{"choices":[{"message":{"content":"OK"}}]}"#)
        }
    });
    let result =
        run(&host, "test-model", json!({"settings":{"endpoint":"http://localhost:11434/v1","model":"draft-model"}}));
    assert_eq!(result["ok"], true);
    assert_eq!(host.requests().len(), 2);
    assert!(host.process_runs().is_empty());
}

#[test]
fn prompt_defaults_preview_and_limits_are_consistent() {
    let host = MockHost::install();
    let schema: Value = serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    assert_eq!(schema["properties"]["promptTemplate"]["default"], config::DEFAULT_PROMPT);
    let result = run(
        &host,
        "preview-prompt",
        json!({"settings":{"promptTemplate":"Use {{language}}. Keep it short.","language":"en"}}),
    );
    assert_eq!(result["prompt"], "Use English. Keep it short.");
    assert!(host.requests().is_empty());
    let c =
        config::Config::load(&json!({"settings":{"maxTokens":999999,"timeoutSeconds":1,"contextCharacters":999999}}));
    assert_eq!((c.tokens, c.timeout, c.context), (2048, 5000, 64000));
}

#[test]
fn selected_untracked_files_work_without_staging_or_head() {
    let host = MockHost::install().with_capabilities(&["project.read", "process:git", "net:api.example.com"]);
    host.on_process("git", |args, _| {
        assert!(!args.iter().any(|s| ["add", "commit", "reset", "push"].contains(&s.as_str())));
        let missing = args.iter().any(|s| s == "rev-parse" || s == "cat-file");
        Ok(ProcessOutput { status: Some(if missing { 1 } else { 0 }), stdout: String::new(), stderr: String::new() })
    });
    host.on_fetch(|request| {
        let body: Value = serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("+export function added()"));
        assert!(!content.contains("never-send-this"));
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("Chinese"));
        response(200, r#"{"choices":[{"message":{"content":"feat: add function"}}]}"#)
    });
    let argument = json!({"settings":{"endpoint":"https://api.example.com","model":"model"},"selection":{"files":[
        {"repo":"/project","path":"added.ts","text":"export function added() {}\n"},
        {"repo":"/project","path":".env","text":"never-send-this"}
    ]}});
    let message = CommitAssistant.run_command("generate-ai", &host.invocation(), Some(&argument.to_string())).unwrap();
    assert_eq!(message.as_deref(), Some("feat: add function"));
}

#[test]
fn selected_partial_hunks_match_editor_utf16_keys_with_crlf_and_emoji() {
    let old = "begin\n旧😀\nanchor\nold2\nend\n";
    let new = "begin\r\n新😀\r\nanchor\r\nnew2\r\nend\r\n";
    // Golden key from src/features/vcs/hunks.ts (normalized old text, raw current text).
    let patch = selection::patch("file.ts", old, new, &["1jgs149:10:0"]);
    assert!(!patch.contains("+新😀"));
    assert!(patch.contains("+new2"));
}

#[test]
fn invalid_urls_missing_models_and_empty_responses_do_not_look_successful() {
    let host = MockHost::install().with_capabilities(&["net:localhost"]);
    assert_eq!(
        run(&host, "test-api", json!({"settings":{"endpoint":"http://remote.example/v1"}}))["code"],
        "invalidEndpoint"
    );
    assert_eq!(
        run(&host, "test-model", json!({"settings":{"endpoint":"http://localhost:11434"}}))["code"],
        "missingModel"
    );
    assert!(host.requests().is_empty());
    host.on_fetch(|_| response(200, "<html>not an API</html>"));
    assert_eq!(
        run(&host, "list-models", json!({"settings":{"endpoint":"http://localhost:11434"}}))["code"],
        "invalidResponse"
    );
    host.on_fetch(|_| response(200, r#"{"choices":[{"message":{"content":""}}]}"#));
    assert_eq!(
        run(&host, "test-model", json!({"settings":{"endpoint":"http://localhost:11434","model":"model"}}))["code"],
        "emptyCompletion"
    );
}
