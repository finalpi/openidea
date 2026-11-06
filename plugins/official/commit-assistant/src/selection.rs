use gix_imara_diff::{Algorithm, Diff, InternedInput};
use openidea_plugin::{Error, Project};
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_FILE: usize = 128 * 1024;
const MAX_TOTAL: usize = 256 * 1024;

pub struct Context {
    pub files: Vec<String>,
    pub diff: String,
    signatures: BTreeMap<String, String>,
}

pub fn git(project: &Project, root: &str, args: &[&str]) -> Result<openidea_plugin::ProcessOutput, Error> {
    let mut command = vec!["--no-optional-locks".into(), "--literal-pathspecs".into(), "-C".into(), root.into()];
    command.extend(args.iter().map(|s| s.to_string()));
    project.run_process("git", &command, None)
}

fn signature(project: &Project, root: &str) -> Result<String, Error> {
    let raw = git(
        project,
        root,
        &["diff", "--cached", "--raw", "--no-abbrev", "--no-renames", "--no-ext-diff", "--no-textconv"],
    )?;
    if raw.status != Some(0) {
        return Err("Unable to read Git repository state".into());
    }
    let head = git(project, root, &["rev-parse", "--verify", "HEAD"])?;
    Ok(format!("{}\n{}", if head.status == Some(0) { head.stdout } else { String::new() }, raw.stdout))
}

fn blob(project: &Project, root: &str, revision: &str) -> Result<Option<String>, Error> {
    let size = git(project, root, &["cat-file", "-s", revision])?;
    if size.status != Some(0) {
        return Ok(Some(String::new()));
    }
    if size.stdout.trim().parse::<usize>().unwrap_or(usize::MAX) > MAX_FILE {
        return Ok(None);
    }
    let content = git(project, root, &["show", "--no-ext-diff", "--no-textconv", revision])?;
    if content.status != Some(0) {
        return Err("A selected revision could not be read".into());
    }
    if content.stdout.len() > MAX_FILE || content.stdout.contains('\0') {
        return Ok(None);
    }
    Ok(Some(content.stdout))
}

fn base36(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut chars = Vec::new();
    while n > 0 {
        chars.push(char::from_digit(n % 36, 36).unwrap());
        n /= 36;
    }
    chars.into_iter().rev().collect()
}

/// Uses the same Histogram diff and UTF-16 content keys as the commit hunk selector.
pub fn patch(path: &str, old: &str, new: &str, excluded: &[&str]) -> String {
    let old_normal = old.replace("\r\n", "\n").replace('\r', "\n");
    let new_normal = new.replace("\r\n", "\n").replace('\r', "\n");
    let old_lines: Vec<_> = old_normal.split_inclusive('\n').collect();
    let new_lines: Vec<_> = new_normal.split_inclusive('\n').collect();
    let raw_new: Vec<_> = new.split_inclusive('\n').collect();
    let mut input: InternedInput<&str> = InternedInput::default();
    input.update_before(old_lines.iter().copied());
    input.update_after(new_lines.iter().copied());
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    let mut body = String::new();
    for hunk in diff.hunks() {
        let before = &old_lines[hunk.before.start as usize..hunk.before.end as usize];
        let after = &new_lines[hunk.after.start as usize..hunk.after.end as usize];
        let raw_after = raw_new.get(hunk.after.start as usize..hunk.after.end as usize).unwrap_or(after);
        let key_text = format!("{}\0{}", before.concat(), raw_after.concat());
        let mut hash = 0x811c9dc5u32;
        for unit in key_text.encode_utf16() {
            hash = (hash ^ unit as u32).wrapping_mul(0x01000193);
        }
        let base = format!("{}:{}", base36(hash), key_text.encode_utf16().count());
        let occurrence = seen.entry(base.clone()).or_default();
        let key = format!("{base}:{occurrence}");
        *occurrence += 1;
        if excluded.contains(&key.as_str()) {
            continue;
        }
        body.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.before.start + 1,
            before.len(),
            hunk.after.start + 1,
            after.len()
        ));
        for (prefix, lines) in [('-', before), ('+', after)] {
            for line in lines {
                body.push(prefix);
                body.push_str(line);
                if !line.ends_with('\n') {
                    body.push_str("\n\\ No newline at end of file\n");
                }
            }
        }
    }
    if body.is_empty() {
        return body;
    }
    format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n{body}")
}

pub fn context(project: &Project, arg: &Value, limit: usize, with_diff: bool) -> Result<Option<Context>, Error> {
    let Some(selection) = arg.get("selection") else {
        return Ok(None);
    };
    if selection.is_null() {
        return Ok(None);
    }
    let files = selection["files"].as_array().ok_or("Invalid selected change list")?;
    if files.is_empty() || files.len() > 80 {
        return Err("Select between 1 and 80 files before generating a message".into());
    }
    let mut context = Context { files: Vec::new(), diff: String::new(), signatures: BTreeMap::new() };
    let mut total = 0;
    for file in files {
        let path = file["path"].as_str().ok_or("Invalid selected file path")?;
        if path.is_empty() || path.len() > 2048 || path.split(['/', '\\']).any(|p| p == "..") {
            return Err("Invalid selected file path".into());
        }
        if super::sensitive(path) || file["oldPath"].as_str().is_some_and(super::sensitive) {
            continue;
        }
        let root = file["repo"].as_str().filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or(project.root()?);
        if !context.signatures.contains_key(&root) {
            context.signatures.insert(root.clone(), signature(project, &root)?);
        }
        context.files.push(path.to_owned());
        if !with_diff || context.diff.chars().count() >= limit {
            continue;
        }
        let staged = file["staged"].as_bool().unwrap_or(false);
        let new = if file["omitted"].as_bool().unwrap_or(false) {
            None
        } else if staged {
            blob(project, &root, &format!(":{path}"))?
        } else if file["deleted"].as_bool().unwrap_or(false) {
            Some(String::new())
        } else {
            file["text"].as_str().map(str::to_owned)
        };
        if new.as_ref().is_some_and(|s| s.len() > MAX_FILE) {
            return Err("A selected file exceeds the context limit".into());
        }
        total += new.as_ref().map_or(0, String::len);
        if total > MAX_TOTAL {
            return Err("Selected file snapshots exceed the context budget".into());
        }
        let old_path = file["oldPath"].as_str().unwrap_or(path);
        let old = if new.is_some() { blob(project, &root, &format!("HEAD:{old_path}"))? } else { None };
        let patch = match (old, new) {
            (Some(old), Some(new)) => {
                let excluded: Vec<_> = file["excludedHunks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .take(2048)
                    .collect();
                patch(path, &old, &new, &excluded)
            }
            _ => format!("Contents omitted (symlink, binary, large, or outside the snapshot budget): {path}\n"),
        };
        let remaining = limit.saturating_sub(context.diff.chars().count());
        context.diff.push_str(super::head(&patch, remaining));
    }
    if context.files.is_empty() {
        return Err("Select some non-sensitive changes before generating a message".into());
    }
    Ok(Some(context))
}

impl Context {
    pub fn validate(&self, project: &Project) -> Result<(), Error> {
        for (root, before) in &self.signatures {
            if signature(project, root)? != *before {
                return Err("The repository changed while generating; try again".into());
            }
        }
        Ok(())
    }
}
