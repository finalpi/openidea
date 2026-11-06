import { Fragment, useEffect, useRef, useState } from "react";
import { tx } from "@/platform/i18n";
import { Button, Checkbox, Input, Select, T } from "@/ui";
import { useSettingsForm } from "@/workbench/contributions";
import { SecretsDetails, type SecretDecl } from "./secrets";
import { usePlugins } from "./store";
import { itemsOf } from "./projection";
import { diagnosticMessage, runSettingsCommand, type SettingsCommandResult } from "./settingsCommands";
import styles from "./PluginsPage.module.css";

export interface SettingSchema {
  key: string;
  type: "boolean" | "string" | "number" | "integer";
  title: string;
  description?: string | null;
  default: unknown;
  options?: string[] | null;
  multiline?: boolean;
  choicesCommand?: string | null;
  choicesDependencies?: string[] | null;
  order?: number;
}

export interface SettingsDecl {
  title: string;
  properties: SettingSchema[];
  actions?: { command: string; title: string }[];
}

type PluginSettings = Record<string, Record<string, unknown>>;

export function withPluginSetting(
  settings: PluginSettings | undefined,
  pluginId: string,
  key: string,
  value: unknown,
): PluginSettings {
  const own = { ...(settings?.[pluginId] ?? {}) };
  if (value === null) delete own[key];
  else own[key] = value;
  const next = { ...(settings ?? {}) };
  if (Object.keys(own).length) next[pluginId] = own;
  else delete next[pluginId];
  return next;
}

interface CommandState {
  busy: boolean;
  models?: string[];
  run(command: string): Promise<SettingsCommandResult | undefined>;
}

function SettingControl({
  pluginId,
  s,
  command,
  values,
  secretRevision,
}: {
  pluginId: string;
  s: SettingSchema;
  command: CommandState;
  values: Record<string, unknown>;
  secretRevision: number;
}) {
  const form = useSettingsForm();
  const all = form.useValue("plugins") as PluginSettings | undefined;
  const value = all?.[pluginId]?.[s.key] ?? s.default;
  const set = (v: unknown) =>
    form.set("plugins", withPluginSetting(all, pluginId, s.key, v === s.default && form.scope === "global" ? null : v));
  const id = `plugin-setting-${pluginId}-${s.key}`;
  const [models, setModels] = useState<string[]>([]);
  const [loaded, setLoaded] = useState(false);
  const dependencies =
    secretRevision +
    JSON.stringify(
      Object.fromEntries(
        Object.entries(values).filter(([key]) =>
          s.choicesDependencies ? s.choicesDependencies.includes(key) : key !== s.key,
        ),
      ),
    );
  useEffect(() => {
    setModels([]);
    setLoaded(false);
  }, [dependencies]);
  useEffect(() => {
    if (Array.isArray(command.models)) {
      setModels(
        [...new Set(command.models.filter((model) => typeof model === "string" && model.length <= 256))].slice(0, 500),
      );
      setLoaded(true);
    }
  }, [command.models]);
  const description = s.description ? <div className={styles.muted}>{tx(s.description)}</div> : null;
  if (s.type === "boolean")
    return (
      <div className={styles.settingRow}>
        <Checkbox checked={value === true} label={s.title} onChange={set} />
        {description}
      </div>
    );
  return (
    <div className={styles.settingRow}>
      <label htmlFor={id}>{tx(s.title)}</label>
      {s.choicesCommand && (
        <div className={styles.settingControls}>
          <Select
            aria-label={tx("Available API models")}
            searchable
            value={String(value)}
            options={models.map((model) => ({ value: model, label: model }))}
            placeholder={
              loaded
                ? models.length
                  ? "Choose a model"
                  : "No models returned; enter a model below"
                : "Refresh models to load choices"
            }
            disabled={command.busy || !models.length}
            onChange={set}
          />
          <Button
            size="small"
            icon="refresh"
            disabled={command.busy}
            onClick={async () => {
              const result = await command.run(s.choicesCommand!);
              if (result?.ok && Array.isArray(result.models)) {
                setModels(
                  [...new Set(result.models.filter((model) => typeof model === "string" && model.length <= 256))].slice(
                    0,
                    500,
                  ),
                );
                setLoaded(true);
              }
            }}
          >
            <T>Refresh models</T>
          </Button>
        </div>
      )}
      {s.options ? (
        <Select
          aria-label={tx(s.title)}
          value={String(value)}
          options={s.options.map((o) => ({ value: o, label: o }))}
          onChange={set}
        />
      ) : s.multiline ? (
        <>
          <textarea
            id={id}
            className={styles.settingText}
            value={String(value ?? "")}
            rows={8}
            onChange={(e) => set(e.target.value)}
            spellCheck={false}
          />
          <Button
            size="small"
            variant="ghost"
            onClick={() => form.set("plugins", withPluginSetting(all, pluginId, s.key, s.default))}
          >
            <T>Restore default template</T>
          </Button>
        </>
      ) : (
        <Input
          id={id}
          type={s.type === "string" ? "text" : "number"}
          value={String(value ?? "")}
          onChange={(e) => {
            const raw = e.target.value;
            if (s.type === "string") set(raw);
            else if (raw.trim() !== "" && Number.isFinite(Number(raw)))
              set(s.type === "integer" ? Math.trunc(Number(raw)) : Number(raw));
          }}
        />
      )}
      {s.choicesCommand && (
        <div className={styles.muted}>
          <T>You can also enter a model ID manually.</T>
        </div>
      )}
      {description}
    </div>
  );
}

export function settingsPage(pluginId: string, decl: SettingsDecl) {
  return function PluginSettingsPage() {
    const form = useSettingsForm();
    const all = form.useValue("plugins") as PluginSettings | undefined;
    const values = Object.fromEntries(decl.properties.map((s) => [s.key, all?.[pluginId]?.[s.key] ?? s.default]));
    const [secretRevision, setSecretRevision] = useState(0);
    const signature = JSON.stringify({ values, secretRevision });
    const current = useRef(signature);
    const alive = useRef(true);
    const busyRef = useRef(false);
    const [busy, setBusy] = useState(false);
    const [result, setResult] = useState<SettingsCommandResult | null>(null);
    const [error, setError] = useState<string | null>(null);
    useEffect(() => {
      current.current = signature;
      setResult(null);
      setError(null);
    }, [signature]);
    useEffect(() => {
      alive.current = true;
      return () => {
        alive.current = false;
      };
    }, []);
    const plugins = usePlugins((s) => s.plugins);
    const ui = usePlugins((s) => s.ui);
    const plugin = plugins.find((p) => p.id === pluginId);
    const secrets = itemsOf<SecretDecl>(ui, "secrets", pluginId);
    const run = async (command: string) => {
      if (busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      setError(null);
      setResult(null);
      try {
        const next = await runSettingsCommand(pluginId, command, values);
        if (!alive.current || current.current !== signature) return;
        setResult(next);
        return next;
      } catch (e) {
        if (alive.current && current.current === signature)
          setError(e instanceof Error ? e.message : String((e as { message?: unknown })?.message ?? e));
      } finally {
        busyRef.current = false;
        if (alive.current) setBusy(false);
      }
    };
    const controls = (
      <>
        {" "}
        {!!decl.actions?.length && (
          <>
            <div className={styles.settingControls}>
              {decl.actions.map((action) => (
                <Button key={action.command} disabled={busy} onClick={() => void run(action.command)}>
                  {action.title}
                </Button>
              ))}
            </div>
            <div className={styles.muted}>
              <T>
                Tests use the values above, including unapplied edits. Only the small model test sends a sample prompt;
                no project code is sent.
              </T>
            </div>
          </>
        )}
        {busy && (
          <div role="status">
            <T>Testing API…</T>
          </div>
        )}
        {error && (
          <div className={styles.error} role="alert">
            {error}
          </div>
        )}
        {result && (
          <div className={result.ok ? styles.muted : styles.error} role={result.ok ? "status" : "alert"}>
            {diagnosticMessage(result)}
            {result.status ? ` · HTTP ${result.status}` : ""}
            {Number.isFinite(result.elapsedMs) ? ` · ${Math.round(result.elapsedMs!)} ms` : ""}
            {result.ok && Array.isArray(result.models) && ` · ${result.models.length}`}
            {result.code === "promptPreview" && typeof result.prompt === "string" && (
              <pre className={styles.promptPreview}>{result.prompt.slice(0, 65536)}</pre>
            )}
          </div>
        )}
      </>
    );
    const actionAfter = Math.max(
      0,
      decl.properties.findLastIndex((s) => !!s.choicesCommand),
    );
    return (
      <div className={styles.settingsPage}>
        <p className={styles.muted}>
          <T>Settings of the plugin; projects can override them.</T>
        </p>
        {plugin && secrets.length > 0 && (
          <div className={styles.settingsSecrets}>
            <SecretsDetails plugin={plugin} items={secrets} onChanged={() => setSecretRevision((v) => v + 1)} />
            <div className={styles.muted}>
              <T>API keys are shared across projects and saved immediately. Other settings use Apply.</T>
            </div>
          </div>
        )}
        {decl.properties.map((s, i) => (
          <Fragment key={s.key}>
            <SettingControl
              pluginId={pluginId}
              s={s}
              command={{ busy, run, models: result?.ok ? result.models : undefined }}
              values={values}
              secretRevision={secretRevision}
            />
            {i === actionAfter ? controls : null}
          </Fragment>
        ))}
        {decl.properties.length === 0 ? controls : null}
      </div>
    );
  };
}
