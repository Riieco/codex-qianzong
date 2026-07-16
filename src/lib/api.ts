import { invoke } from "@tauri-apps/api/core";
import { defaultSettings, mockSnapshot } from "./mock";
import type {
  AppSettings,
  AuthCredentialStatus,
  CodexConfigBackup,
  DetectionPaths,
  TaskBoard,
  UsageSnapshot,
  HistoryRestoreOutcome,
} from "../types/usage";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function call<T>(command: string, args?: Record<string, unknown>, fallback?: T): Promise<T> {
  if (!isTauriRuntime() && fallback !== undefined) {
    await new Promise((resolve) => window.setTimeout(resolve, 120));
    return fallback;
  }
  return invoke<T>(command, args);
}

export function getUsageSnapshot(): Promise<UsageSnapshot> {
  return call("get_usage_snapshot", undefined, mockSnapshot);
}

export function refreshTaskBoard(): Promise<TaskBoard> {
  return call("refresh_task_board", undefined, mockSnapshot.taskBoard!);
}

export function getAppSettings(): Promise<AppSettings> {
  return call("get_app_settings", undefined, defaultSettings);
}

const mockCredentialStatus: AuthCredentialStatus = {
  hasStoredOfficialAuth: true,
  hasStoredRelayApiKey: false,
  relayEndpoint: null,
};

export function getAuthCredentialStatus(): Promise<AuthCredentialStatus> {
  return call("get_auth_credential_status", undefined, mockCredentialStatus);
}

export function clearRelayApiKey(): Promise<AuthCredentialStatus> {
  return call("clear_relay_api_key", undefined, {
    ...mockCredentialStatus,
    hasStoredRelayApiKey: false,
    relayEndpoint: null,
  });
}

export function hasUnifiedHistoryBackup(): Promise<boolean> {
  return call("has_unified_history_backup", undefined, false);
}

export function restoreUnifiedHistory(): Promise<HistoryRestoreOutcome> {
  return call("restore_unified_history", undefined, {
    restoredJsonlFiles: 0,
    restoredStateRows: 0,
    skippedReason: "no_backup_ledger",
  });
}

export function saveAppSettings(settings: AppSettings): Promise<AppSettings> {
  return call("save_app_settings", { settings }, settings);
}

export function setAlwaysOnTop(enabled: boolean): Promise<boolean> {
  return call("set_always_on_top", { enabled }, enabled);
}

export function getDetectionPaths(): Promise<DetectionPaths> {
  return call("get_detection_paths", undefined, {
    codexBinaryPath: "codex",
    codexDataDir: "~/.codex",
    stateDbPath: "~/.codex/state_5.sqlite",
    appLogDir: "logs",
  });
}

const mockBackups: CodexConfigBackup[] = [
  {
    id: "default-initial",
    label: "首次启动默认配置",
    createdAt: new Date().toISOString(),
    isDefault: true,
    hasConfig: true,
    hasAuth: true,
  },
];

export function listCodexConfigBackups(): Promise<CodexConfigBackup[]> {
  return call("list_codex_config_backups", undefined, mockBackups);
}

export function createCodexConfigBackup(label?: string): Promise<CodexConfigBackup[]> {
  return call("create_codex_config_backup", { label }, mockBackups);
}

export function restoreCodexConfigBackup(id: string): Promise<CodexConfigBackup[]> {
  return call("restore_codex_config_backup", { id }, mockBackups);
}

export function deleteCodexConfigBackup(id: string): Promise<CodexConfigBackup[]> {
  return call("delete_codex_config_backup", { id }, mockBackups);
}

export function openLogFolder(): Promise<string> {
  return call("open_log_folder", undefined, "logs");
}
