import { selectedGenerationContext } from "./generationContext";
import { create } from "zustand";
import { useProject } from "@/platform/project";
import { notifications, useCommitMessageGenerators } from "@/workbench/contributions";
import { rememberMessage, useCommit } from "./commitStore";
import { currentRepo, useVcs } from "./store";

const ERROR_KEY = "vcs.messageGeneration";

export const useMessageGeneration = create(() => ({ busy: false }));

/** A generator only fills the draft; staging and committing remain explicit actions. */
export async function generateCommitMessage(id?: string, source?: "selected" | "staged"): Promise<void> {
  if (useMessageGeneration.getState().busy) return;
  notifications().expire?.(ERROR_KEY);
  const generators = useCommitMessageGenerators.getState().generators;
  const generator = id ? generators.get(id) : generators.values().next().value;
  if (!generator) {
    notifications().notify({
      group: "vcs",
      key: ERROR_KEY,
      level: "warning",
      title: "Install a commit message generator plugin first",
    });
    return;
  }
  if (source === "selected" && !generator.supportsSelection) {
    notifications().notify({
      group: "vcs",
      key: ERROR_KEY,
      level: "warning",
      title: "Update the generator plugin to use selected changes",
    });
    return;
  }
  const project = useProject.getState().project;
  const repo = currentRepo();
  const version = useVcs.getState().version;
  const message = useCommit.getState().message;
  useMessageGeneration.setState({ busy: true });
  try {
    const selected = source !== "staged" && generator.supportsSelection;
    const selection = selected ? await selectedGenerationContext() : undefined;
    const signature = selection ? JSON.stringify(selection) : null;
    const generated = selection ? await generator.generate(repo, selection) : await generator.generate(repo);
    if (!generated?.trim()) return;
    if (
      useProject.getState().project !== project ||
      currentRepo() !== repo ||
      (!selected && useVcs.getState().version !== version) ||
      useCommit.getState().message !== message
    ) {
      notifications().notify({
        group: "vcs",
        key: ERROR_KEY,
        level: "info",
        title: "The draft or repository changed; generate the message again",
      });
      return;
    }
    if (signature !== null && JSON.stringify(await selectedGenerationContext()) !== signature) {
      notifications().notify({
        group: "vcs",
        key: ERROR_KEY,
        level: "info",
        title: "The selected changes changed; generate the message again",
      });
      return;
    }
    if (useCommit.getState().message !== message || useProject.getState().project !== project || currentRepo() !== repo)
      return;
    if (message.trim()) rememberMessage(message);
    useCommit.setState((s) => ({ message: generated.trim(), focusTick: s.focusTick + 1 }));
  } catch (error) {
    if ((error as { kind?: string })?.kind !== "cancelled") {
      const body = error instanceof Error ? error.message : String((error as { message?: unknown })?.message ?? error);
      notifications().notify({
        group: "vcs",
        key: ERROR_KEY,
        level: "error",
        title: "Could not generate commit message",
        body,
      });
    }
  } finally {
    useMessageGeneration.setState({ busy: false });
  }
}
