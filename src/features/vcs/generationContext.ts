import { editorsFor } from "@/editor/api";
import { call, commands } from "@/platform/ipc";
import { tx } from "@/platform/i18n";
import type { FileEntry } from "@/bindings";
import { dirName } from "./model";
import type { CommitMessageSelection } from "@/workbench/contributions";
import { checkOf, planCommit } from "./changes";
import { useChangelists, withListDefaults } from "./changelists";
import { useCommit } from "./commitStore";
import { readWorkingText } from "./ops";
import { currentRepoRoot, repoOf, useVcs } from "./store";

const FILE_BYTES = 128 * 1024;
const TOTAL_BYTES = 256 * 1024;

export function sensitiveCommitPath(path: string): boolean {
  const name = path.toLowerCase().split(/[/\\]/).at(-1) ?? "";
  return (
    name === ".env" ||
    name.startsWith(".env.") ||
    name.includes("credential") ||
    name.includes("secret") ||
    name.startsWith("id_rsa") ||
    name.startsWith("id_ed25519") ||
    /\.(pem|key|p12|pfx|keystore)$/.test(name)
  );
}

/** Bounded snapshots include unsaved buffers and never stage or save files. */
export async function selectedGenerationContext(): Promise<CommitMessageSelection> {
  const vcs = useVcs.getState();
  const commit = useCommit.getState();
  const checks = withListDefaults(vcs.status, vcs.root ?? "", useChangelists.getState().state, commit.checks);
  const included = planCommit(vcs.status, checks).included.filter(
    (f) => !sensitiveCommitPath(f.relPath) && !sensitiveCommitPath(f.renamedFrom ?? ""),
  );
  if (!included.length) throw new Error(tx("Select some non-sensitive changes before generating a message."));
  if (included.length > 80) throw new Error(tx("Select at most 80 files for one commit message."));
  const files: CommitMessageSelection["files"] = [];
  let budget = TOTAL_BYTES;
  const directories = new Map<string, FileEntry[]>();
  for (const file of included) {
    const staged = checkOf(file, checks) === "staged";
    const partial = checkOf(file, checks) === "hunks";
    const deleted = (staged ? file.staged : (file.unstaged ?? file.staged)) === "deleted";
    let text: string | undefined;
    let omitted = false;
    if (!staged && !deleted) {
      const directory = dirName(file.path);
      if (!directories.has(directory)) directories.set(directory, await call(commands.fsReadDir(directory)));
      const entry = directories.get(directory)!.find((e) => e.path === file.path);
      const editor = editorsFor(file.path)[0];
      if (entry?.kind === "symlink") omitted = true;
      else if (editor) {
        if (editor.view.state.doc.length > FILE_BYTES || budget <= 0) omitted = true;
        else text = editor.view.state.doc.toString();
      } else {
        const probe = await call(commands.fsProbe(file.path));
        if (probe.binary || probe.size > FILE_BYTES || probe.size > budget) omitted = true;
        else {
          const content = await readWorkingText(file.path);
          if (content === null)
            throw new Error(tx("A selected file could not be read. Refresh the changes and try again."));
          text = content;
        }
      }
      if (text !== undefined) {
        const size = new TextEncoder().encode(text).byteLength;
        if (size > FILE_BYTES || size > budget || text.includes("\0")) {
          text = undefined;
          omitted = true;
        } else budget -= size;
      }
    }
    const excluded = commit.excludedHunks.get(file.path) ?? commit.excludedHunks.get(`${file.path}/`);
    files.push({
      path: file.relPath,
      repo: repoOf(file.path) ?? currentRepoRoot() ?? "",
      staged,
      deleted,
      omitted,
      ...(text !== undefined ? { text } : {}),
      ...(file.renamedFrom ? { oldPath: file.renamedFrom } : {}),
      ...(partial ? { excludedHunks: [...(excluded ?? [])].sort() } : {}),
    });
  }
  return { files };
}
