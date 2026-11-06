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
  core_clock_mhz: number | null;
  mem_clock_mhz: number | null;
  max_core_clock_mhz: number | null;
  power_w: number | null;
  power_limit_w: number | null;
  fan_pct: number | null;
  /** "nvml" when NVIDIA sensors are available, otherwise "windows". */
  source: string;
}
export interface DiskIo {
  disk: string;
  read_bps: number;
  write_bps: number;
  active_pct: number;
}
export interface HistoryPoint {
  ts: number;
  cpu: number;
  cpu_max_core: number;
  mem_pct: number;
  gpu: number | null;
  gpu_temp: number | null;
  gpu_power: number | null;
  disk_read: number;
  disk_write: number;
  net_rx: number;
  net_tx: number;
}
export interface DriveHealth {
  device_id: string;
  model: string;
  media_type: string | null;
  bus_type: string | null;
  size_bytes: number | null;
  health: string | null;
  temperature_c: number | null;
  temperature_max_c: number | null;
  wear_pct: number | null;
  power_on_hours: number | null;
  read_errors: number | null;
  write_errors: number | null;
}
export interface StorageHealth {
  drives: DriveHealth[];
  reliability_available: boolean;
  note: string | null;
}
export interface Alert {
  kind: string;
  title: string;
  detail: string;
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
  interval_ms: number;
  commit_bytes: number | null;
  commit_limit: number | null;
  pages_per_sec: number | null;
  disk_io: DiskIo[];
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
export type Priority = "idle" | "below_normal" | "normal" | "above_normal" | "high";
export interface ProcessRule {
  name: string;
  priority: Priority | null;
  affinity: string | null;
  enabled: boolean;
  note: string;
  created_at: string;
  updated_at: string;
  applied_count: number;
  last_applied_at: string | null;
  last_error: string | null;
}
export type ProcFlag = "high_cpu" | "high_memory" | "disk_heavy";
export interface ProcGroup {
  name: string;
  instances: number;
  cpu: number;
  memory: number;
  disk_bps: number;
  has_window: boolean;
  exe: string | null;
  protected: boolean;
  cpu_avg: number;
  cpu_peak: number;
  mem_peak: number;
  first_seen: number | null;
  flags: ProcFlag[];
  rule: ProcessRule | null;
}
export interface ProcOverview {
  groups: ProcGroup[];
  total_processes: number;
  logical_cpus: number;
  mem_total: number;
  sampled_at: number | null;
}
export interface ProcInstance {
  pid: number;
  cpu: number;
  memory: number;
  exe: string | null;
  has_window: boolean;
  run_time_secs: number;
  priority: string | null;
  affinity: string | null;
}
export interface ProcDetails {
  name: string;
  protected: boolean;
  instances: ProcInstance[];
  system_mask: string;
  logical_cpus: number;
  rule: ProcessRule | null;
}
export interface ChangeOutcome {
  pid: number;
  ok: boolean;
  error: string | null;
}
export interface CloseResult {
  name: string;
  ended: number;
  failed: number;
  error: string | null;
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
export interface StoredPing {
  tested_at: string;
  label: string;
  target: string;
  avg_ms: number | null;
  jitter_ms: number | null;
  loss_percent: number;
}
export interface Hop {
  ttl: number;
  address: string | null;
  rtts: (number | null)[];
  private: boolean;
  reached: boolean;
}
export interface TraceReport {
  target: string;
  address: string;
  hops: Hop[];
  reached: boolean;
  note: string | null;
}
export interface DnsProvider {
  id: string;
  name: string;
  v4: string[];
  v6: string[];
  note: string;
}
export interface DnsPreviewItem {
  target: string;
  current: string;
  proposed: string;
  will_change: boolean;
}
export interface PowerScheme {
  guid: string;
  name: string;
  active: boolean;
}


// ------------------------------------------------------------- maintenance

export type MaintenanceTaskId = "crash_check" | "driver_check" | "disk_space" | "temp_cleanup" | "network_check";
export interface MaintenanceTask {
  id: MaintenanceTaskId;
  title: string;
  description: string;
  changes_system: boolean;
  default_on: boolean;
  default_days: number;
  enabled: boolean;
  interval_days: number;
  last_run: string | null;
  next_due: string | null;
}
export interface MaintenanceRun {
  id: number;
  task: MaintenanceTaskId;
  started_at: string;
  finished_at: string;
  status: "ok" | "attention" | "failed" | "deferred";
  trigger: "scheduled" | "manual";
  summary: string;
}

// -------------------------------------------------------------- benchmarks

export type BenchSuite = "cpu" | "memory" | "storage" | "boot";
export interface BenchMetric {
  key: string;
  label: string;
  value: number;
  unit: string;
  higher_is_better: boolean;
}
export interface BenchRun {
  id: number;
  started_at: string;
  suite: BenchSuite;
  target: string | null;
  label: string;
  duration_ms: number;
  metrics: BenchMetric[];
  context: { power_plan: string | null; threads: number; cpu: string | null; notes: string[] };
}
export interface BenchProgress {
  stage: string;
  pct: number;
}
export interface BootEntry {
  start: string;
  logon: string | null;
  to_sign_in: number | null;
  boot_type: number | null;
}
export interface BootReport {
  boots: BootEntry[];
  firmware_s: number | null;
  median_to_sign_in: number | null;
  fast_startup: boolean | null;
}

// ------------------------------------------------------------ driver centre

export interface SupportLink {
  key: string;
  label: string;
}
export interface ProblemDevice {
  id: string;
  name: string;
  identified: boolean;
  maker: string | null;
  code: number;
  severity: "critical" | "warning" | "info";
  meaning: string;
  action: string;
  link: SupportLink | null;
}
export interface KeyDriver {
  group: string;
  device: string;
  provider: string | null;
  version: string | null;
  date: string | null;
  age_days: number | null;
  built_in: boolean;
  signed: boolean | null;
  note: string | null;
  link: SupportLink | null;
}
export interface DriverInstall {
  time: string;
  device: string;
  provider: string | null;
  version: string | null;
  inf: string;
  updated: boolean;
}
export interface DriverReport {
  problems: ProblemDevice[];
  key: KeyDriver[];
  installs: DriverInstall[];
  unsigned: number;
  board_link: SupportLink | null;
}

// ----------------------------------------------------------------- crashes

export type CrashArea = "graphics" | "driver" | "memory" | "storage" | "hardware" | "power" | "overlay" | "app" | "windows" | "unknown";
export interface CrashIncident {
  time: string;
  kind: "blue_screen" | "unexpected_shutdown" | "hardware_error";
  code: string | null;
  code_name: string | null;
  area: CrashArea;
  area_label: string;
  explanation: string;
  dump: string | null;
  evidence: string[];
}
export interface CrashCounted {
  name: string;
  count: number;
  detail: string | null;
}
export interface CrashAppGroup {
  app: string;
  path: string | null;
  version: string | null;
  crashes: number;
  hangs: number;
  last: string;
  modules: CrashCounted[];
  codes: CrashCounted[];
  area: CrashArea;
  area_label: string;
  explanation: string;
}
export interface CrashFinding {
  severity: "good" | "info" | "warning" | "critical";
  title: string;
  detail: string;
  steps: string[];
}
export interface CrashDay {
  day: string;
  blue_screens: number;
  shutdowns: number;
  app_crashes: number;
  hangs: number;
}
export interface DumpFile {
  path: string;
  size: number;
  modified: string | null;
}
export interface CrashReport {
  window_days: number;
  generated_at: string;
  blue_screens: number;
  unexpected_shutdowns: number;
  hardware_errors: number;
  app_crashes: number;
  app_hangs: number;
  clean_boots: number;
  last_system_crash: string | null;
  findings: CrashFinding[];
  incidents: CrashIncident[];
  apps: CrashAppGroup[];
  daily: CrashDay[];
  stability: [string, number][];
  dumps: { minidumps: DumpFile[] | null; memory_dmp: DumpFile | null };
}

// ---------------------------------------------------------------- services

export type StartType = "automatic" | "automatic_delayed" | "manual" | "disabled";
export type ServiceTier = "protected" | "windows" | "optional" | "third_party";
export interface ServiceView {
  name: string;
  display_name: string;
  description: string | null;
  state: string;
  pid: number | null;
  start: { start: number; delayed: boolean };
  start_kind: StartType | null;
  start_label: string;
  image: string | null;
  account: string | null;
  publisher: string | null;
  tier: ServiceTier;
  tier_reason: string;
  note: string | null;
  anti_cheat: boolean;
  depends_on: string[];
  dependents: string[];
  allowed: StartType[];
}
export interface ServicePreviewItem {
  name: string;
  display_name: string;
  current: string;
  proposed: string;
  will_change: boolean;
  running: boolean;
  warnings: string[];
  refused: string | null;
}

// --------------------------------------------------------------- customise

export type CustomSection = "taskbar" | "explorer" | "notifications";
export interface CustomChoice {
  id: string;
  label: string;
}
export interface CustomSetting {
  id: string;
  section: CustomSection;
  title: string;
  description: string;
  choices: CustomChoice[];
  /** Choice Windows behaves like when the value isn"t set; null = varies by build. */
  windows_default: string | null;
  takes: Takes;
  min_build: number | null;
  target: string;
  /** Choice ID, "default" when not set, or null for a value NOCTURN doesn"t offer. */
  current: string | null;
  current_label: string;
  available: boolean;
  error: string | null;
}
export interface CustomSelection {
  id: string;
  choice: string;
}
export interface CustomPreviewItem {
  id: string;
  title: string;
  target: string;
  current: string;
  proposed: string;
  will_change: boolean;
  takes: Takes;
}
/** Either explicit selections, a section to reset, or everything. */
export type CustomRequest = { selections: CustomSelection[] } | { reset: CustomSection } | { resetAll: true };

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
  /** "system" (Tweak page profiles) or "gaming_mode" (Gaming page modes). */
  kind: "system" | "gaming_mode";
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
  insight?: StartupInsight;
}
export type SignatureState = "valid" | "unsigned" | "invalid" | "unknown";
export interface StartupInsight {
  target: string | null;
  publisher: string | null;
  description: string | null;
  signature: SignatureState | null;
  windows_component: boolean;
  review: string[];
}
export interface StartupChange {
  tx_id: string;
  target: string;
  status: string;
  source: string;
  applied_at: string | null;
  rolled_back_at: string | null;
}

// ------------------------------------------------------------------ gaming

export interface Game {
  /** "steam:730", "epic:Fortnite", "gog:…", "custom:<exe path>" */
  id: string;
  name: string;
  launcher: string;
  install_dir: string | null;
  /** Likely game executables, most relevant first. */
  exes: string[];
}
export type GpuPreference = "default" | "power_saving" | "high_performance";
export interface GameProfile {
  game_id: string;
  exe: string;
  priority: Priority | null;
  /** Hex CPU mask; null = all CPUs. */
  affinity: string | null;
  gpu_preference: GpuPreference | null;
  disable_fso: boolean;
  updated_at: string;
}
export interface GameView extends Game {
  running: boolean;
  profile: GameProfile | null;
  sessions: number;
  /** Marked "not a game" by the user: listed, but never starts sessions. */
  not_game: boolean;
}
export interface ProfilePreviewItem {
  kind: "registry" | "rule";
  title: string;
  target: string;
  current: string;
  proposed: string;
  will_change: boolean;
  note: string | null;
}
export interface ProfileOutcome {
  report: ApplyReport | null;
  rule: ProcessRule | null;
  rule_error: string | null;
}
export interface SessionSummary {
  id: number | null;
  game_id: string;
  game_name: string;
  exe: string;
  /** Unix ms. */
  started_at: number;
  ended_at: number;
  samples: number;
  avg_cpu: number;
  avg_max_core: number;
  avg_gpu: number | null;
  peak_gpu_temp: number | null;
  avg_mem_pct: number;
  game_mem_peak: number;
  gpu_bound_pct: number;
  cpu_bound_pct: number;
  verdict: string;
}
export interface Overlay {
  process: string;
  label: string;
}
export interface GamingNow {
  live: SessionSummary | null;
  game_cpu: number | null;
  game_mem: number | null;
  overlays: Overlay[];
}

// ----------------------------------------------------------------- storage

export interface StorageRoot {
  path: string;
  label: string;
  total: number | null;
  free: number | null;
}
export interface ScanProgress {
  files: number;
  dirs: number;
  bytes: number;
  current: string;
}
export interface ScanSummary {
  root: string;
  started_at: string;
  duration_ms: number;
  total_size: number;
  total_files: number;
  folders: number;
  cloud_only: number;
  unreadable: number;
  skipped_links: number;
  cancelled: boolean;
  dup_candidates: number;
}
export interface FolderChild {
  id: number;
  name: string;
  size: number;
  files: number;
  has_children: boolean;
}
export interface FolderView {
  id: number;
  path: string;
  size: number;
  files: number;
  own_size: number;
  own_files: number;
  crumbs: [number, string][];
  children: FolderChild[];
  more_children: number;
}
export interface FileView {
  path: string;
  size: number;
  /** Unix ms. */
  modified: number | null;
  protected: string | null;
  app_data: boolean;
}
export interface DupGroupView {
  size: number;
  wasted: number;
  files: FileView[];
}
export interface DupProgress {
  hashed_bytes: number;
  total_bytes: number;
  groups: number;
}
export interface RecycleReport {
  results: { path: string; ok: boolean; error: string | null }[];
  freed: number;
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
export interface MonitorSettings {
  interval_ms: number;
  paused: boolean;
  alerts: boolean;
  cpu_alert_pct: number;
  ram_alert_pct: number;
  gpu_temp_alert_c: number;
}
export interface AppSettings {
  restore_point_default: boolean;
  confirm_advanced: boolean;
  reduce_motion: boolean;
  ai: AiSettings;
  monitor: MonitorSettings;
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
  close_apps?: { name: string; instances: number; memory_bytes: number }[];
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
  telemetryRange: (minutes: number) => invoke<HistoryPoint[]>("get_telemetry_range", { minutes }),
  storageHealth: () => invoke<StorageHealth>("get_storage_health"),
  processes: () => invoke<ProcessInfo[]>("get_processes"),
  endProcess: (pid: number, name: string) => invoke<void>("end_process", { pid, name }),
  closeApps: (names: string[], source: "user" | "ai" = "user") => invoke<CloseResult[]>("close_apps", { names, source }),
  processOverview: () => invoke<ProcOverview>("process_overview"),
  processDetails: (name: string) => invoke<ProcDetails>("process_details", { name }),
  setProcessPriority: (name: string, priority: Priority) => invoke<ChangeOutcome[]>("set_process_priority", { name, priority }),
  setProcessAffinity: (name: string, mask: string) => invoke<ChangeOutcome[]>("set_process_affinity", { name, mask }),
  processRules: () => invoke<ProcessRule[]>("list_process_rules"),
  saveProcessRule: (rule: Pick<ProcessRule, "name" | "priority" | "affinity" | "enabled" | "note">) => invoke<ProcessRule>("save_process_rule", { rule }),
  deleteProcessRule: (name: string) => invoke<void>("delete_process_rule", { name }),
  health: (force = false) => invoke<HealthReport>("get_health", { force }),
  events: () => invoke<EventEntry[]>("get_events"),
  crashReport: () => invoke<CrashReport>("get_crash_report"),
  driverCentre: () => invoke<DriverReport>("get_driver_centre"),
  maintenance: () => invoke<{ tasks: MaintenanceTask[]; history: MaintenanceRun[] }>("maintenance_overview"),
  setMaintenanceTask: (task: MaintenanceTaskId, enabled: boolean, intervalDays: number) => invoke<void>("set_maintenance_task", { task, enabled, intervalDays }),
  runMaintenanceTask: (task: MaintenanceTaskId) => invoke<MaintenanceRun>("run_maintenance_task", { task }),
  runBenchmark: (suite: BenchSuite, target: string | null, label: string, onProgress?: (p: BenchProgress) => void) => {
    const ch = new Channel<BenchProgress>();
    if (onProgress) ch.onmessage = onProgress;
    return invoke<BenchRun>("run_benchmark", { suite, target, label, onProgress: ch });
  },
  cancelBenchmark: () => invoke<void>("cancel_benchmark"),
  benchmarks: () => invoke<BenchRun[]>("list_benchmarks"),
  deleteBenchmark: (id: number) => invoke<void>("delete_benchmark", { id }),
  renameBenchmark: (id: number, label: string) => invoke<void>("rename_benchmark", { id, label }),
  bootTimes: () => invoke<BootReport>("boot_times"),
  openSupportLink: (key: string) => invoke<void>("open_support_link", { key }),
  drivers: () => invoke<DriverEntry[]>("get_drivers"),
  services: () => invoke<ServiceEntry[]>("get_services"),
  restoreStatus: () => invoke<RestoreStatus>("get_restore_status"),

  networkOverview: () => invoke<NetworkOverview>("network_overview"),
  networkTest: (count = 10) => invoke<StabilityReport>("network_test", { count }),
  networkHistory: (limit = 300) => invoke<StoredPing[]>("network_history", { limit }),
  traceroute: (host: string, onHop: (h: Hop) => void) => {
    const ch = new Channel<Hop>();
    ch.onmessage = onHop;
    return invoke<TraceReport>("traceroute", { host, onHop: ch });
  },
  dnsProviders: () => invoke<DnsProvider[]>("dns_providers"),
  previewDns: (index: number, alias: string, provider: string) => invoke<DnsPreviewItem[]>("preview_dns", { index, alias, provider }),
  setDns: (index: number, alias: string, provider: string, onProgress?: (p: Progress) => void) =>
    invoke<ApplyReport>("set_dns", { index, alias, provider, onProgress: progressChannel(onProgress) }),

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

  serviceList: () => invoke<ServiceView[]>("list_services"),
  previewServices: (selections: { name: string; start: StartType }[]) => invoke<ServicePreviewItem[]>("preview_services", { selections }),
  applyServices: (selections: { name: string; start: StartType }[], onProgress?: (p: Progress) => void) =>
    invoke<ApplyReport>("apply_services", { selections, onProgress: progressChannel(onProgress) }),
  serviceChanges: () => invoke<StartupChange[]>("list_service_changes"),

  customise: () => invoke<CustomSetting[]>("list_customise"),
  previewCustomise: (req: CustomRequest) => invoke<CustomPreviewItem[]>("preview_customise", req),
  applyCustomise: (req: CustomRequest, label: string, onProgress?: (p: Progress) => void) =>
    invoke<ApplyReport>("apply_customise", { ...req, label, onProgress: progressChannel(onProgress) }),
  restartExplorer: () => invoke<void>("restart_explorer"),

  startup: () => invoke<StartupItem[]>("list_startup"),
  startupChanges: () => invoke<StartupChange[]>("list_startup_changes"),
  setStartup: (ids: string[], enabled: boolean, source: "user" | "ai" = "user", onProgress?: (p: Progress) => void) =>
    invoke<ApplyReport>("set_startup_enabled", { ids, enabled, source, onProgress: progressChannel(onProgress) }),

  games: (refresh = false) => invoke<GameView[]>("list_games", { refresh }),
  addCustomGame: (exe: string) => invoke<void>("add_custom_game", { exe }),
  removeCustomGame: (exe: string) => invoke<void>("remove_custom_game", { exe }),
  setNotGame: (id: string, notGame: boolean) => invoke<void>("set_not_game", { id, notGame }),
  gamingNow: () => invoke<GamingNow>("gaming_now"),
  gameSessions: (gameId?: string) => invoke<SessionSummary[]>("list_game_sessions", { gameId: gameId ?? null }),
  previewGameProfile: (profile: GameProfile) => invoke<ProfilePreviewItem[]>("preview_game_profile", { profile }),
  applyGameProfile: (profile: GameProfile, gameName: string, onProgress?: (p: Progress) => void) =>
    invoke<ProfileOutcome>("apply_game_profile", { profile, gameName, onProgress: progressChannel(onProgress) }),

  storageRoots: () => invoke<StorageRoot[]>("storage_roots"),
  storageScan: (root: string, onProgress: (p: ScanProgress) => void) => {
    const ch = new Channel<ScanProgress>();
    ch.onmessage = onProgress;
    return invoke<ScanSummary>("storage_scan", { root, onProgress: ch });
  },
  storageCancel: () => invoke<void>("storage_cancel"),
  storageStatus: () => invoke<ScanSummary | null>("storage_status"),
  storageFolder: (id?: number) => invoke<FolderView>("storage_folder", { id: id ?? null }),
  storageLargeFiles: () => invoke<FileView[]>("storage_large_files"),
  storageFindDuplicates: (onProgress: (p: DupProgress) => void) => {
    const ch = new Channel<DupProgress>();
    ch.onmessage = onProgress;
    return invoke<DupGroupView[]>("storage_find_duplicates", { onProgress: ch });
  },
  storageDuplicates: () => invoke<DupGroupView[] | null>("storage_duplicates"),
  storageRecycle: (paths: string[]) => invoke<RecycleReport>("storage_recycle", { paths }),
  recycleBinInfo: () => invoke<{ size: number; items: number }>("recycle_bin_info"),
  emptyRecycleBin: () => invoke<void>("empty_recycle_bin"),

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
