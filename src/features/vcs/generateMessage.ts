import { create } from "zustand";
import { useProject } from "@/platform/project";
import { notifications, useCommitMessageGenerators } from "@/workbench/contributions";
import { rememberMessage, useCommit } from "./commitStore";
import { currentRepo, useVcs } from "./store";

export const useMessageGeneration = create(() => ({ busy: false }));

/** A generator only fills the draft; staging and committing remain explicit actions. */
export async function generateCommitMessage(id?: string): Promise<void> {
  if (useMessageGeneration.getState().busy) return;
  const generators = useCommitMessageGenerators.getState().generators;
  const generator = id ? generators.get(id) : generators.values().next().value;
  if (!generator) {
    notifications().notify({ level: "warning", title: "Install a commit message generator plugin first" });
    return;
  }
  const project = useProject.getState().project;
  const repo = currentRepo();
  const version = useVcs.getState().version;
  const message = useCommit.getState().message;
  useMessageGeneration.setState({ busy: true });
  try {
    const generated = await generator.generate(repo);
    if (!generated?.trim()) return;
    if (
      useProject.getState().project !== project ||
      currentRepo() !== repo ||
      useVcs.getState().version !== version ||
      useCommit.getState().message !== message
    ) {
      notifications().notify({ level: "info", title: "The draft or repository changed; generate the message again" });
      return;
    }
    if (message.trim()) rememberMessage(message);
    useCommit.setState((s) => ({ message: generated.trim(), focusTick: s.focusTick + 1 }));
  } catch (error) {
    notifications().notify({ level: "error", title: "Could not generate commit message", body: String(error) });
  } finally {
    useMessageGeneration.setState({ busy: false });
  }
}
