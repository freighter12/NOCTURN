// Typed bridge to the Rust backend. Every type here mirrors a Rust struct's
// serde output; every function maps 1:1 to a #[tauri::command].

import { Channel, invoke } from "@tauri-apps/api/core";

// ------------------------------------------------------------------ errors

export interface AppError {
  kind: string;
  message: string;
}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as AppError).message);
  return "Something went wrong";
}

// ------------------------------------------------------------------ system

export interface OsInfo {
  name: string;
  edition: string | null;
  display_version: string | null;
  version: string;
  build: string;
  ubr: number | null;
  architecture: string | null;
  install_date: string | null;
  last_boot: string | null;
  hostname: string | null;
  is_windows_11: boolean;
}
export interface CpuInfo {
  name: string;
  manufacturer: string | null;
  cores: number | null;
  threads: number | null;
  max_clock_mhz: number | null;
  l2_kb: number | null;
  l3_kb: number | null;
  virtualization_enabled: boolean | null;
}
export interface GpuInfo {
  name: string;
  vendor: string | null;
  driver_version: string | null;
  driver_date: string | null;
  vram_bytes: number | null;
  resolution: string | null;
  refresh_hz: number | null;
}
export interface MemoryModule {
  slot: string | null;
  capacity_bytes: number | null;
  speed_mts: number | null;
  configured_mts: number | null;
  manufacturer: string | null;
  part_number: string | null;
}
export interface BoardInfo {
  system_manufacturer: string | null;
  system_model: string | null;
  board_manufacturer: string | null;
  board_product: string | null;
  bios_vendor: string | null;
  bios_version: string | null;
  bios_date: string | null;
}
export interface PhysicalDisk {
  model: string;
  media_type: string | null;
  bus_type: string | null;
  size_bytes: number | null;
  health: string | null;
}
export interface SystemInfo {
  os: OsInfo;
  cpu: CpuInfo;
  gpus: GpuInfo[];
  memory_total_bytes: number | null;
  memory_modules: MemoryModule[];
  board: BoardInfo;
  disks: PhysicalDisk[];
  is_laptop: boolean;
  has_battery: boolean;
  is_elevated: boolean;
  collected_at: string;
  warnings: string[];
}

export interface GpuSample {
  name: string;
  usage: number | null;
  vram_used: number | null;
  vram_total: number | null;
  temperature_c: number | null;
}
export interface DiskSample {
  mount: string;
  label: string;
  file_system: string;
  kind: string;
  total: number;
  available: number;
  removable: boolean;
}
export interface Sample {
  ts: number;
  cpu_usage: number;
  cpu_per_core: number[];
  cpu_freq_mhz: number;
  mem_total: number;
  mem_used: number;
  mem_available: number;
  swap_total: number;
  swap_used: number;
  gpus: GpuSample[];
  net_rx_bps: number;
  net_tx_bps: number;
  disks: DiskSample[];
  process_count: number;
  uptime_secs: number;
}
export interface ProcessInfo {
  pid: number;
  parent: number | null;
  name: string;
  exe: string | null;
  cpu: number;
  memory: number;
  disk_read_bps: number;
  disk_write_bps: number;
  run_time_secs: number;
}

export type Severity = "info" | "warning" | "critical";
export type FindingAction =
  | { kind: "tweaks"; ids: string[]; label: string }
  | { kind: "page"; page: string; label: string }
  | { kind: "ask"; prompt: string; label: string };
export interface Finding {
  id: string;
  severity: Severity;
  title: string;
  detail: string;
  evidence: string;
  action: FindingAction | null;
}
export interface HealthReport {
  score: number;
  condition: string;
  findings: Finding[];
  checked_at: string;
  skipped: string[];
}

export interface EventEntry {
  log: string;
  id: number;
  provider: string;
  level: number;
  time: string;
  message: string;
}
export interface DriverEntry {
  device: string;
  class: string | null;
  provider: string | null;
  version: string | null;
  date: string | null;
  signed: boolean | null;
}
export interface ServiceEntry {
  name: string;
  display_name: string;
  start_mode: string;
  state: string;
}
export interface RestoreStatus {
  queryable: boolean;
  points: { Description?: string; SequenceNumber?: number; Created?: string }[];
}

export interface Adapter {
  name: string;
  description: string;
  index: number;
  status: string;
  link_speed: string | null;
  media: string | null;
  mac: string | null;
  is_virtual: boolean;
  driver_version: string | null;
  dns_servers: string[];
  gateway: string | null;
}
export interface WifiInfo {
  connected: boolean;
  ssid: string | null;
  signal_percent: number | null;
  radio: string | null;
  band: string | null;
  channel: string | null;
  receive_mbps: number | null;
  transmit_mbps: number | null;
  raw: [string, string][];
}
export interface NetworkOverview {
  adapters: Adapter[];
  wifi: WifiInfo | null;
  warnings: string[];
}
export interface PingResult {
  target: string;
  label: string;
  sent: number;
  received: number;
  loss_percent: number;
  min_ms: number | null;
  avg_ms: number | null;
  max_ms: number | null;
  jitter_ms: number | null;
}
export interface StabilityReport {
  pings: PingResult[];
  dns: { host: string; ms: number | null; error: string | null }[];
  tested_at: string;
}
export interface PowerScheme {
  guid: string;
  name: string;
  active: boolean;
}

// ------------------------------------------------------------------ tweaks

export type Category = "gaming" | "performance" | "network" | "privacy";
export type Risk = "safe" | "moderate" | "advanced";
export type Takes = "immediately" | "app_restart" | "sign_out" | "restart";
export type TweakState = "applied" | "not_applied" | "partial" | "action" | "unavailable" | "unknown";

export interface TweakView {
  id: string;
  category: Category;
  title: string;
  summary: string;
  details: string;
  caveat: string | null;
  risk: Risk;
  takes: Takes;
  requires_admin: boolean;
  reversible: boolean;
  one_shot: boolean;
  status: { state: TweakState; detail: string | null };
}
export interface Profile {
  id: string;
  name: string;
  description: string;
  tweaks: string[];
}
export interface PreviewItem {
  tweak_id: string;
  tweak_title: string;
  target: string;
  current: string;
  proposed: string;
  will_change: boolean;
  requires_admin: boolean;
  reversible: boolean;
  note: string | null;
}
export interface Preview {
  items: PreviewItem[];
  changes: number;
  requires_admin: boolean;
  irreversible: string[];
  unavailable: string[];
  takes: Takes;
  is_elevated: boolean;
}
export interface ChangeResult {
  change_id: number;
  tweak_id: string;
  target: string;
  status: string;
  before: string;
  after: string;
  error: string | null;
}
export interface ApplyReport {
  tx_id: string;
  status: string;
  results: ChangeResult[];
  restore_point: { ok: boolean; message: string } | null;
  notes: string[];
  takes: Takes;
}
export interface RollbackReport {
  tx_id: string;
  status: string;
  results: ChangeResult[];
  notes: string[];
}
export interface Progress {
  txId: string;
  done: number;
  total: number;
  message: string;
}
export interface TxSummary {
  id: string;
  created_at: string;
  finished_at: string | null;
  source: string;
  label: string;
  status: string;
  restore_point: string | null;
  backup_path: string | null;
  total: number;
  applied: number;
  failed: number;
  rolled_back: number;
}
export interface ChangeRow {
  id: number;
  tx_id: string;
  tweak_id: string;
  target: string;
  op: Record<string, unknown>;
  original: Record<string, unknown> | null;
  status: string;
  error: string | null;
  applied_at: string | null;
  rolled_back_at: string | null;
}

export type StartupLocation = "run_user" | "run_machine" | "run_machine32" | "folder_user" | "folder_common";
export interface StartupItem {
  id: string;
  name: string;
  command: string;
  location: StartupLocation;
  location_label: string;
  enabled: boolean;
  file_path: string | null;
  file_exists: boolean | null;
  requires_admin: boolean;
}

// ---------------------------------------------------------------- settings

export interface AiSettings {
  provider: "anthropic" | "openai_compatible";
  model: string;
  base_url: string;
  effort: string;
  max_tokens: number;
  allow_system_context: boolean;
  custom_instructions: string;
}
export interface AppSettings {
  restore_point_default: boolean;
  confirm_advanced: boolean;
  reduce_motion: boolean;
  ai: AiSettings;
}
export interface AppInfo {
  version: string;
  data_dir: string;
  log_dir: string;
  is_elevated: boolean;
}
export interface AuditEntry {
  ts: string;
  level: string;
  actor: string;
  event: string;
  detail: unknown;
}

// ---------------------------------------------------------------------- AI

export type Block =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tool_use"; id: string; name: string; input: Record<string, unknown> }
  | { type: "tool_result"; tool_use_id: string; content: string; is_error: boolean };

export interface PlanCard {
  plan_id: string;
  title: string;
  summary: string;
  confidence: string;
  expected_effect: string;
  verification: string | null;
  tweaks: { id: string; title: string; risk: Risk; requires_admin: boolean; reversible: boolean; caveat: string | null }[];
  startup: { id: string; name: string; requires_admin: boolean }[];
  preview: Preview | null;
}
export interface StoredMessage {
  id: number;
  created_at: string;
  role: "user" | "assistant";
  blocks: Block[];
  meta?: {
    hidden?: boolean;
    plans?: PlanCard[];
    error?: string;
    model?: string;
    usage?: Usage;
    stop_reason?: string;
    stop_detail?: string | null;
    kind?: string;
    plan_id?: string;
  } | null;
}
export interface Usage {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
}
export interface ConversationSummary {
  id: string;
  title: string;
  provider: string;
  model: string;
  created_at: string;
  updated_at: string;
}
export type ChatEvent =
  | { type: "started"; conversation_id: string }
  | { type: "model"; model: string }
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; text: string }
  | { type: "tool_start"; id: string; name: string }
  | { type: "tool_result"; id: string; name: string; is_error: boolean }
  | { type: "plan"; plan: PlanCard }
  | { type: "turn_end"; stop_reason: string; detail: string | null; usage: Usage }
  | { type: "error"; message: string }
  | { type: "done"; conversation_id: string; usage: Usage };
export interface ConnectionTest {
  ok: boolean;
  message: string;
  models: string[];
}

// ---------------------------------------------------------------- commands

function progressChannel(onProgress?: (p: Progress) => void): Channel<Progress> {
  const ch = new Channel<Progress>();
  ch.onmessage = (p) => onProgress?.(p);
  return ch;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (value: AppSettings) => invoke<AppSettings>("save_settings", { value }),
  listAudit: (limit = 200) => invoke<AuditEntry[]>("list_audit", { limit }),

  systemInfo: (refresh = false) => invoke<SystemInfo>("get_system_info", { refresh }),
  telemetryHistory: () => invoke<Sample[]>("get_telemetry_history"),
  processes: () => invoke<ProcessInfo[]>("get_processes"),
  endProcess: (pid: number, name: string) => invoke<void>("end_process", { pid, name }),
  health: (force = false) => invoke<HealthReport>("get_health", { force }),
  events: () => invoke<EventEntry[]>("get_events"),
  drivers: () => invoke<DriverEntry[]>("get_drivers"),
  services: () => invoke<ServiceEntry[]>("get_services"),
  restoreStatus: () => invoke<RestoreStatus>("get_restore_status"),

  networkOverview: () => invoke<NetworkOverview>("network_overview"),
  networkTest: (count = 10) => invoke<StabilityReport>("network_test", { count }),

  powerPlans: () => invoke<PowerScheme[]>("power_plans"),
  setPowerPlan: (guid: string, onProgress?: (p: Progress) => void) =>
    invoke<ApplyReport>("set_power_plan", { guid, onProgress: progressChannel(onProgress) }),

  tweaks: () => invoke<TweakView[]>("list_tweaks"),
  profiles: () => invoke<Profile[]>("list_profiles"),
  preview: (ids: string[]) => invoke<Preview>("preview_tweaks", { ids }),
  apply: (
    request: { tweak_ids: string[]; source: string; label: string; create_restore_point: boolean },
    onProgress?: (p: Progress) => void,
  ) => invoke<ApplyReport>("apply_tweaks", { request, onProgress: progressChannel(onProgress) }),
  rollback: (txId: string, onProgress?: (p: Progress) => void) =>
    invoke<RollbackReport>("rollback_transaction", { txId, onProgress: progressChannel(onProgress) }),
  history: (limit = 100) => invoke<TxSummary[]>("list_history", { limit }),
  transaction: (txId: string) => invoke<{ summary: TxSummary; changes: ChangeRow[] }>("get_transaction", { txId }),

  startup: () => invoke<StartupItem[]>("list_startup"),
  setStartup: (ids: string[], enabled: boolean, source: "user" | "ai" = "user", onProgress?: (p: Progress) => void) =>
    invoke<ApplyReport>("set_startup_enabled", { ids, enabled, source, onProgress: progressChannel(onProgress) }),

  aiKeyStatus: () => invoke<{ anthropic: boolean; openai_compatible: boolean }>("ai_key_status"),
  aiSetKey: (provider: string, key: string | null) => invoke<void>("ai_set_key", { provider, key }),
  aiTest: () => invoke<ConnectionTest>("ai_test_connection"),
  aiSend: (conversationId: string | null, text: string, onEvent: (e: ChatEvent) => void, meta?: unknown) => {
    const ch = new Channel<ChatEvent>();
    ch.onmessage = onEvent;
    return invoke<string>("ai_send", { conversationId, text, meta: meta ?? null, onEvent: ch });
  },
  aiCancel: (conversationId: string) => invoke<boolean>("ai_cancel", { conversationId }),
  aiConversations: () => invoke<ConversationSummary[]>("ai_list_conversations"),
  aiConversation: (id: string) => invoke<StoredMessage[]>("ai_get_conversation", { id }),
  aiRename: (id: string, title: string) => invoke<void>("ai_rename_conversation", { id, title }),
  aiDelete: (id: string) => invoke<void>("ai_delete_conversation", { id }),
};
