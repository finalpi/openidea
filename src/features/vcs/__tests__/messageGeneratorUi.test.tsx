import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { registerAction } from "@/platform/actions";
import { registerCommitMessageGenerator } from "@/workbench/contributions";
import { CommitMessageField } from "../CommitView";
import { useCommit } from "../commitStore";
import { generateCommitMessage, useMessageGeneration } from "../generateMessage";

const disposers: (() => void)[] = [];
afterEach(() => disposers.splice(0).forEach((dispose) => dispose()));

it("offers installed generators and shows loading until the message draft is ready", async () => {
  let finish!: (message: string) => void;
  useMessageGeneration.setState({ busy: false });
  useCommit.setState({ message: "", focusTick: 0 });
  disposers.push(
    registerCommitMessageGenerator({
      id: "offline",
      title: "Offline generator",
      generate: () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    }),
  );
  disposers.push(
    registerAction({ id: "vcs.generateCommitMessage", title: "Generate", run: () => generateCommitMessage("offline") }),
  );
  render(
    <CommitMessageField
      included={1}
      runner={{ busy: false, emptyError: false, clearEmptyError: () => {}, canCommit: true, run: async () => {} }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Generate from staged changes" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Offline generator" }));
  expect(screen.getByRole("button", { name: "Generating commit message…" })).toBeDisabled();
  await act(async () => finish("chore: update files"));
  await waitFor(() =>
    expect(screen.getByRole("textbox", { name: "Commit message" })).toHaveValue("chore: update files"),
  );
  expect(screen.getByRole("button", { name: "Generate from staged changes" })).toBeEnabled();
});
