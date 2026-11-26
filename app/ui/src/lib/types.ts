export type Health = 'Good' | 'Caution' | 'Bad' | 'Unknown';
export type DiskKind = 'Ata' | 'Nvme' | 'Other';

export interface Attribute {
  id: number;
  name: string;
  current: number | null;
  worst: number | null;
  threshold: number | null;
  raw: number;
  raw_hex: string;
  status: Health;
}

export interface NvmeHealth {
  critical_warning: number;
  composite_temp_k: number;
  available_spare: number;
  available_spare_threshold: number;
  percentage_used: number;
  temp_sensors_k: number[];
}

export interface Disk {
  id: string;
  dev_path: string;
  kind: DiskKind | null;
  model: string;
  serial: string;
  firmware: string;
  size_bytes: number;
  rotation_rate: number | null;
  interface: string;
  transfer_mode: string | null;
  standard: string | null;
  features: string[];
  mount_points: string[];
  removable: boolean;
  smart_available: boolean;
  smart_error: string | null;
  attributes: Attribute[];
  nvme: NvmeHealth | null;
  temperature_c: number | null;
  power_on_hours: number | null;
  power_on_count: number | null;
  host_reads_gb: number | null;
  host_writes_gb: number | null;
  nand_writes_gb: number | null;
  health: Health;
  health_percent: number | null;
  health_reasons: string[];
}

export interface HealthConfig {
  reallocated: number;
  pending: number;
  uncorrectable: number;
  life_caution: number;
}

export interface Settings {
  autoRefresh: number;
  fahrenheit: boolean;
  rawDecimal: boolean;
  hideSerial: boolean;
  elevateOnStart: boolean;
  language: string;
  window: unknown;
  alarms: Record<string, HealthConfig>;
}

export type Mode = 'demo' | 'root' | 'native' | 'elevated' | 'readonly';

export interface Snapshot {
  disks: Disk[];
  mode: Mode;
  dataDir: string;
  settings: Settings;
  lastRefresh: string | null;
  error: string | null;
  version: string;
  initialDisk: string | null;
  initialGraph: boolean;
}

export interface Point {
  t: number;
  v: number;
}
