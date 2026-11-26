import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import type { HealthConfig, Point, Settings, Snapshot } from './types';

export const api = {
  snapshot: () => invoke<Snapshot>('get_snapshot'),
  refresh: () => invoke<Snapshot>('refresh'),
  elevate: () => invoke<Snapshot>('elevate'),
  setSettings: (settings: Settings) => invoke<Snapshot>('set_settings', { settings }),
  setAlarm: (key: string, alarm: HealthConfig | null) => invoke<Snapshot>('set_alarm', { key, alarm }),
  historySeries: (key: string) => invoke<string[]>('history_series', { key }),
  history: (key: string, series: string) => invoke<Point[]>('history', { key, series }),
  copy: (text: string) => writeText(text),
  exit: () => getCurrentWindow().close(),
  onSnapshot: (cb: (s: Snapshot) => void): Promise<UnlistenFn> => listen<Snapshot>('snapshot', (e) => cb(e.payload)),
};
