import { pluginsBackend } from "./backend";
import { tx } from "@/platform/i18n";

export interface SettingsCommandResult {
  ok: boolean;
  code?: string;
  status?: number;
  elapsedMs?: number;
  models?: string[];
  prompt?: string;
}

/** Configuration commands receive draft non-secret settings, never an API key. */
export async function runSettingsCommand(pluginId: string, command: string, settings: Record<string, unknown>) {
  const raw = await pluginsBackend().runCommand(pluginId, command, JSON.stringify({ settings }), null);
  const value: unknown = raw ? JSON.parse(raw) : null;
  if (!value || typeof value !== "object" || typeof (value as SettingsCommandResult).ok !== "boolean") {
    throw new Error(tx("The plugin returned an invalid diagnostic result"));
  }
  return value as SettingsCommandResult;
}

export function diagnosticMessage(result: SettingsCommandResult): string {
  if (result.code === "promptPreview") return tx("Effective prompt preview");
  if (result.ok) return tx("Request succeeded");
  if (result.code === "invalidEndpoint") return tx("Check the API URL. Use HTTPS, or HTTP for localhost.");
  if (result.code === "missingModel") return tx("Choose or enter a model first.");
  if (result.code === "responseTooLarge")
    return tx("The API response is too large. Enter the model manually or reduce the response size.");
  if (result.code === "chatEndpointMissing")
    return tx("The chat endpoint is unavailable. Check the API base URL and Chat Completions compatibility.");
  if (result.code === "emptyCompletion")
    return tx("The API accepted the request but returned no text. Increase the output limit or choose a chat model.");
  if (result.code === "invalidResponse") return tx("The API returned an unexpected response format.");
  if (result.code === "permissionDenied") return tx("Allow this plugin's network and secrets permissions in Plugins.");
  if (result.code === "timeout") return tx("The request timed out. Check the service or increase the timeout.");
  if (result.code === "tls") return tx("TLS or certificate validation failed.");
  if (result.code === "dns") return tx("The API hostname could not be resolved.");
  if (result.code === "cancelled") return tx("Request cancelled");
  if (result.status === 401) return tx("Authentication failed. Save a valid API key.");
  if (result.status === 403) return tx("Access denied. Check the account and model permissions.");
  if (result.status === 404)
    return tx("This endpoint is unavailable. Enter the model manually and test the selected model.");
  if (result.status === 429) return tx("The API rate limit or quota was exceeded.");
  if (result.status && result.status >= 500) return tx("The API service returned a server error.");
  if (result.status) return tx("The API rejected the request. Check the endpoint and model settings.");
  return tx("Could not connect to the API. Check the address, network and proxy.");
}
