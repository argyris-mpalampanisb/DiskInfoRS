import type { Attribute, Disk, Health, Settings } from './types';

export const historyKey = (d: Disk) =>
  `${d.model.trim()}${d.serial.trim()}`.replace(/[\/\\\0:*?"<>|]/g, '_');

export const size = (bytes: number) => (bytes > 0 ? `${(bytes / 1e9).toFixed(1)} GB` : '----');

export const temp = (c: number | null | undefined, f: boolean) => {
  if (c == null || c <= 0) return '-- °C';
  return f ? `${Math.round((c * 9) / 5 + 32)} °F` : `${c} °C`;
};

export const rotation = (r: number | null) => (r == null ? '----' : r === 1 ? '---- (SSD)' : `${r} RPM`);

export const num = (n: number | null | undefined, unit: string) =>
  n == null ? '----' : `${n.toLocaleString('en-US')} ${unit}`;

export const serial = (d: Disk, hide: boolean) => (hide ? '*'.repeat(Math.min(d.serial.length || 8, 20)) : d.serial || '----');

export const raw = (a: Attribute, decimal: boolean) => (decimal ? a.raw.toLocaleString('en-US') : a.raw_hex);

export const isSsd = (d: Disk) => d.rotation_rate === 1 || d.kind === 'Nvme';

/** Temperature alarm level by drive type (°C). */
export function tempLevel(d: Disk): Health {
  const t = d.temperature_c;
  if (t == null || t <= 0) return 'Unknown';
  const [warn, bad] = d.kind === 'Nvme' ? [70, 80] : isSsd(d) ? [60, 70] : [50, 55];
  return t >= bad ? 'Bad' : t >= warn ? 'Caution' : 'Good';
}

export const healthLabel = (h: Health) => h;

export function mounts(d: Disk) {
  return d.mount_points.length ? d.mount_points.join('  ') : '----';
}

export function shortMount(d: Disk) {
  const m = d.mount_points.find((p) => p !== '[SWAP]') ?? d.mount_points[0];
  return m ?? d.id;
}

/** Plain-text report of a disk, in the spirit of CrystalDiskInfo's "Copy". */
export function report(d: Disk, s: Settings): string {
  const L = (k: string, v: string) => `${k.padStart(22)} : ${v}`;
  const out: string[] = [];
  out.push('-'.repeat(76));
  out.push(` ${d.model} (${d.dev_path})`);
  out.push('-'.repeat(76));
  out.push(L('Model', d.model));
  out.push(L('Firmware', d.firmware || '----'));
  out.push(L('Serial Number', serial(d, s.hideSerial)));
  out.push(L('Disk Size', size(d.size_bytes)));
  out.push(L('Interface', d.interface || '----'));
  out.push(L('Standard', d.standard ?? '----'));
  out.push(L('Transfer Mode', d.transfer_mode ?? '----'));
  out.push(L('Power On Hours', num(d.power_on_hours, 'hours')));
  out.push(L('Power On Count', num(d.power_on_count, 'count')));
  if (d.host_reads_gb != null) out.push(L('Host Reads', num(d.host_reads_gb, 'GB')));
  if (d.host_writes_gb != null) out.push(L('Host Writes', num(d.host_writes_gb, 'GB')));
  if (d.nand_writes_gb != null) out.push(L('NAND Writes', num(d.nand_writes_gb, 'GB')));
  out.push(L('Rotation Rate', rotation(d.rotation_rate)));
  out.push(L('Mount Points', mounts(d)));
  out.push(L('Features', d.features.join(', ') || '----'));
  out.push(L('Temperature', temp(d.temperature_c, s.fahrenheit)));
  out.push(L('Health Status', `${d.health}${d.health_percent != null ? ` (${d.health_percent} %)` : ''}`));
  if (d.smart_error) out.push(L('Note', d.smart_error));
  if (d.attributes.length) {
    out.push('');
    const nvme = d.kind === 'Nvme';
    out.push(nvme ? 'ID RawValues(6) Attribute Name' : 'ID Cur Wor Thr RawValues(6) Attribute Name');
    for (const a of d.attributes) {
      const id = a.id.toString(16).toUpperCase().padStart(2, '0');
      const p = (n: number | null) => (n == null ? '---' : n.toString().padStart(3, '0'));
      out.push(nvme ? `${id} ${a.raw_hex.padStart(12)} ${a.name}` : `${id} ${p(a.current)} ${p(a.worst)} ${p(a.threshold)} ${a.raw_hex} ${a.name}`);
    }
  }
  return out.join('\n');
}
